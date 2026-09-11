//! DWG → GeoJSON geometry builder.
//!
//! Spawns `dwgread -OGeoJSON` (from the LibreDWG package — `brew install
//! libredwg`), drains its stdout as a feature collection, and produces the
//! same `(SceneMeta, Vec<u8>)` pair the ASCII DXF parser does.
//!
//! This path is mandatory for pre-2010 DWG files (AC1021..AC1023): those
//! crash LibreDWG's ASCII DXF writer ("Invalid num_pages 0"), but the
//! GeoJSON format sidesteps the bad code path entirely.

use std::path::Path;
use std::process::Command;

use crate::aci::aci_to_rgb;
use crate::model::{LayerInfo, LayoutMeta, RangeSpec, SceneMeta, TextItem};

// Reuse the same vertex encoders the ASCII parser uses.
use crate::dxf_ascii::{append_line, append_point};

/// Build geometry by reading DWG via the system `dwgread` CLI (LibreDWG).
///
/// `parse_time_ms`, `convert_ms`, and `was_dwg` bubble through to the
/// returned SceneMeta so the UI shows correct diagnostics.
pub fn read_dwg_geojson(
    path: &Path,
    parse_time_ms: u64,
    convert_ms: u64,
) -> Result<(SceneMeta, Vec<u8>), String> {
    if !path.is_file() {
        return Err(format!("文件不存在: {}", path.display()));
    }

    let output = Command::new("dwgread")
        .args(["-OGeoJSON"])
        .arg(path)
        .output()
        .map_err(|e| format!("启动 dwgread 失败: {e}（请先 brew install libredwg）"))?;

    if !output.status.success() {
        let err_text = String::from_utf8_lossy(&output.stderr);
        let first_err = err_text.lines().next().unwrap_or("(no message)");
        return Err(format!("dwgread 退出码 {:?}: {}", output.status.code(), first_err));
    }

    parse_geojson_bytes(&output.stdout, parse_time_ms, convert_ms)
}

fn parse_geojson_bytes(
    bytes: &[u8],
    parse_time_ms: u64,
    convert_ms: u64,
) -> Result<(SceneMeta, Vec<u8>), String> {
    let v: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| format!("GeoJSON 解析失败: {e}"))?;

    let features = v.get("features")
        .and_then(|f| f.as_array())
        .ok_or_else(|| "GeoJSON 无 features 数组".to_string())?;

    let mut st = State::default();

    for feat in features {
        st.push_feature(feat);
    }

    st.finalize(parse_time_ms, true, convert_ms)
}

// ------------------------------------------------------------------------ geometry state

#[derive(Default)]
struct LayerRec {
    name: String,
    r: u8,
    g: u8,
    b: u8,
    auto: bool,
    off: bool,
    lines: Vec<u8>,
    points: Vec<u8>,
    tris: Vec<u8>,
}

#[derive(Default)]
struct State {
    layers: Vec<LayerRec>,
    layer_index: std::collections::HashMap<String, usize>,
    texts: Vec<TextItem>,
    segments_total: u64,
}

impl State {
    fn layer_idx(&mut self, name: &str) -> usize {
        if let Some(&i) = self.layer_index.get(name) {
            return i;
        }
        let i = self.layers.len();
        self.layer_index.insert(name.to_string(), i);
        let (r, g, b) = aci_to_rgb(7); // default auto color (ACI 7 → theme)
        self.layers.push(LayerRec {
            name: name.to_string(),
            r, g, b,
            auto: true,
            off: false,
            lines: Vec::new(),
            points: Vec::new(),
            tris: Vec::new(),
        });
        i
    }

    fn push_feature(&mut self, feat: &serde_json::Value) {
        let geom = match feat.get("geometry") {
            Some(g) => g,
            None => return,
        };
        let gtype = geom.get("type").and_then(|s| s.as_str());
        let coords = match geom.get("coordinates").and_then(|c| c.as_array()) {
            Some(c) => c,
            None => return,
        };

        let props = feat.get("properties").and_then(|p| p.as_object());
        let layer_name = props
            .and_then(|p| p.get("Layer"))
            .and_then(|l| l.as_str())
            .unwrap_or("0");

        // Color: LibreDWG emits ACI index under "Color" — fall back to layer auto.
        let (r, g, b, a) = props
            .and_then(|p| p.get("Color"))
            .and_then(|c| c.as_i64())
            .map(|aci| {
                if aci <= 0 || aci > 255 {
                    (0u8, 0u8, 0u8, 0u8) // auto = 0
                } else {
                    let (r, g, b) = aci_to_rgb(aci as u8);
                    (r, g, b, 255u8)
                }
            })
            .unwrap_or((0u8, 0u8, 0u8, 0u8));

        // Track this layer even if BYLAYER/auto.
        let li = self.layer_idx(layer_name);

        match gtype.unwrap_or("") {
            "Point" => {
                let (x, y) = match coords.first().and_then(coord_2d) {
                    Some(c) => c,
                    None => return,
                };
                append_point(&mut self.layers[li].points, x, y, r, g, b, a);
                self.segments_total += 1;
            }
            "LineString" => {
                let pts: Vec<(f64, f64)> = coords.iter().filter_map(coord_2d).collect();
                for w in pts.windows(2) {
                    append_line(&mut self.layers[li].lines, w[0].0, w[0].1, w[1].0, w[1].1, r, g, b, a);
                    self.segments_total += 1;
                }
            }
            "Polygon" => {
                // Polygon coordinates: [outer_ring, opt_hole_1, opt_hole_2, ...]
                for ring in coords {
                    let ring_pts = ring.as_array()
                        .map(|arr| arr.iter().filter_map(coord_2d).collect::<Vec<_>>())
                        .unwrap_or_default();
                    // Use line segments around the ring (closed loop).
                    if ring_pts.len() >= 2 {
                        for i in 0..ring_pts.len() {
                            let j = (i + 1) % ring_pts.len();
                            // Skip duplicate closing point identical to first.
                            if i == ring_pts.len() - 1 && ring_pts[j] == ring_pts[0] {
                                continue;
                            }
                            append_line(
                                &mut self.layers[li].lines,
                                ring_pts[i].0, ring_pts[i].1,
                                ring_pts[j].0, ring_pts[j].1,
                                r, g, b, a,
                            );
                            self.segments_total += 1;
                        }
                    }
                }
            }
            _ => {
                // MultiLineString / MultiPolygon / GeometryCollection — skip for now.
            }
        }
    }

    fn finalize(self, parse_time_ms: u64, was_dwg: bool, convert_ms: u64) -> Result<(SceneMeta, Vec<u8>), String> {
        let segments_total = self.segments_total;
        let text_count = self.texts.len() as u64;

        let mut geometry: Vec<u8> = Vec::new();
        let mut layouts: Vec<LayoutMeta> = Vec::new();
        let single_meta = Self::meta_for_single_layout(&self, &mut geometry);
        layouts.push(single_meta);

        let meta = SceneMeta {
            layers: self.layers.iter().map(|l| LayerInfo {
                name: l.name.clone(), r: l.r, g: l.g, b: l.b,
                auto: l.auto, off: l.off,
            }).collect(),
            layouts,
            texts: self.texts,
            segments: segments_total,
            text_count,
            parse_ms: parse_time_ms,
            tess_ms: 0,
            convert_ms,
            was_dwg,
            truncated: false,
        };
        Ok((meta, geometry))
    }

    fn meta_for_single_layout(ls: &State, geometry: &mut Vec<u8>) -> LayoutMeta {
        let lines_start = geometry.len();
        let mut line_ranges: Vec<RangeSpec> = Vec::new();
        for (li, l) in ls.layers.iter().enumerate() {
            if l.lines.is_empty() { continue; }
            line_ranges.push(RangeSpec {
                layer: li as u32,
                offset: (geometry.len() - lines_start) as u32,
                len: l.lines.len() as u32,
            });
            geometry.extend_from_slice(&l.lines);
        }
        let lines_len = (geometry.len() - lines_start) as u32;

        let tris_start = geometry.len();
        let mut tri_ranges: Vec<RangeSpec> = Vec::new();
        for (li, l) in ls.layers.iter().enumerate() {
            if l.tris.is_empty() { continue; }
            tri_ranges.push(RangeSpec {
                layer: li as u32,
                offset: (geometry.len() - tris_start) as u32,
                len: l.tris.len() as u32,
            });
            geometry.extend_from_slice(&l.tris);
        }
        let tris_len = (geometry.len() - tris_start) as u32;

        let points_start = geometry.len();
        let mut point_ranges: Vec<RangeSpec> = Vec::new();
        for (li, l) in ls.layers.iter().enumerate() {
            if l.points.is_empty() { continue; }
            point_ranges.push(RangeSpec {
                layer: li as u32,
                offset: (geometry.len() - points_start) as u32,
                len: l.points.len() as u32,
            });
            geometry.extend_from_slice(&l.points);
        }
        let points_len = (geometry.len() - points_start) as u32;

        LayoutMeta {
            name: "Model".to_owned(),
            min_x: 0.0, min_y: 0.0, max_x: 1.0, max_y: 1.0,
            lines_offset: lines_start as u32,
            lines_len,
            tris_offset: tris_start as u32,
            tris_len,
            points_offset: points_start as u32,
            points_len,
            line_ranges,
            tri_ranges,
            point_ranges,
        }
    }
}

// ------------------------------------------------------------------------ helpers

fn coord_2d(v: &serde_json::Value) -> Option<(f64, f64)> {
    let arr = v.as_array()?;
    let x = arr.first()?.as_f64()?;
    let y = arr.get(1)?.as_f64()?;
    Some((x, y))
}

