//! DWG → GeoJSON geometry builder.
//!
//! Spawns `dwgread -OGeoJSON` (from the LibreDWG package — `brew install
//! libredwg`), drains its stdout as a feature collection, and produces the
//! same `(SceneMeta, Vec<u8>)` pair the ASCII DXF parser does.
//!
//! This path is mandatory for pre-2010 DWG files (AC1021..AC1023): those
//! crash LibreDWG's ASCII DXF writer ("Invalid num_pages 0"), but the
//! GeoJSON format sidesteps the bad code path entirely.
//!
//! ## Layer name problem
//!
//! LibreDWG 0.14's `out_geojson.c` re-emits layer.name (stored as raw
//! UTF-16LE bytes) as if it were Latin1, producing irreversible mojibake.
//! We work around this by spawning the `dwg_layer_names` helper binary
//! (compiled at build time via `build.rs`, source in `src/bin/`): it decodes
//! the layer table through the C API to correct UTF-8, and returns an
//! entity-handle → layer-idx map so we can assign each GeoJSON feature to
//! the right layer bucket by reference to the helper's canonical table.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::aci::aci_to_rgb;
use crate::model::{LayerInfo, LayoutMeta, RangeSpec, SceneMeta, TextItem};

// Reuse the same vertex encoders the ASCII parser uses.
use crate::dxf_ascii::{append_line, append_point};

/// Layer metadata as reported by the C helper.
#[derive(Debug, serde::Deserialize)]
struct HelperLayer {
    #[allow(dead_code)] // present for forward-compat; we iterate positionally
    idx: usize,
    name: String,
    aci: u16, // LibreDWG uses 256 as BYLAYER sentinel — exceeds u8 range
    off: u8,
}

/// Full JSON body emitted by the C helper.
#[derive(Debug, serde::Deserialize)]
struct HelperOutput {
    layers: Vec<HelperLayer>,
    entity_layer: HashMap<String, usize>,
}

/// Spawn the helper binary, feed it `path`, return the parsed table + map.
fn run_dwg_layer_helper(path: &Path) -> Result<HelperOutput, String> {
    let bin = env!("DWG_LAYER_HELPER").to_string();
    let output = Command::new(&bin)
        .arg(path)
        .output()
        .map_err(|e| format!("启动 dwg_layer_helper 失败: {e}"))?;

    if !output.status.success() {
        let err_text = String::from_utf8_lossy(&output.stderr);
        let first_err = err_text.lines().next().unwrap_or("(no message)");
        return Err(format!(
            "dwg_layer_helper 退出码 {:?}: {}",
            output.status.code(),
            first_err
        ));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("dwg_layer_helper JSON 解析失败: {e}"))
}

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

    // Step 1: run the C helper to get canonical layer names + entity map.
    let helper = run_dwg_layer_helper(path)?;

    // Step 2: run GeoJSON CLI as before.
    let output = Command::new("dwgread")
        .args(["-OGeoJSON"])
        .arg(path)
        .output()
        .map_err(|e| format!("启动 dwgread 失败: {e}（请先 brew install libredwg）"))?;

    if !output.status.success() {
        let err_text = String::from_utf8_lossy(&output.stderr);
        let first_err = err_text.lines().next().unwrap_or("(no message)");
        return Err(format!(
            "dwgread 退出码 {:?}: {}",
            output.status.code(),
            first_err
        ));
    }

    parse_geojson_bytes(&output.stdout, &helper, parse_time_ms, convert_ms)
}

fn parse_geojson_bytes(
    bytes: &[u8],
    helper: &HelperOutput,
    parse_time_ms: u64,
    convert_ms: u64,
) -> Result<(SceneMeta, Vec<u8>), String> {
    let v: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| format!("GeoJSON 解析失败: {e}"))?;

    let features = v
        .get("features")
        .and_then(|f| f.as_array())
        .ok_or_else(|| "GeoJSON 无 features 数组".to_string())?;

    let mut st = State::new(helper);

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

struct State {
    layers: Vec<LayerRec>,
    entity_layer: HashMap<String, usize>,
    texts: Vec<TextItem>,
    segments_total: u64,
}

impl State {
    fn new(helper: &HelperOutput) -> Self {
        let layers: Vec<LayerRec> = helper
            .layers
            .iter()
            .map(|hl| {
                // LibreDWG uses ACI index 256 as the BYLAYER sentinel; for
                // rendering purposes BYLAYER falls back to white (ACI 7).
                let aci_effective = if hl.aci >= 256 { 7u16 } else { hl.aci };
                let (r, g, b) = aci_to_rgb(aci_effective as u8);
                LayerRec {
                    name: hl.name.clone(),
                    r,
                    g,
                    b,
                    auto: hl.aci >= 256,
                    off: hl.off != 0,
                    ..Default::default()
                }
            })
            .collect();
        State {
            layers,
            entity_layer: helper.entity_layer.clone(),
            texts: Vec::new(),
            segments_total: 0,
        }
    }

    /// Resolve a GeoJSON feature to its layer idx using the entity handle map.
    fn layer_idx_for(&self, feat: &serde_json::Value) -> Option<usize> {
        let handle = feat
            .get("properties")?
            .as_object()?
            .get("EntityHandle")?
            .as_str()?;
        self.entity_layer.get(handle).copied()
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

        // Look up the canonical layer index via the helper map.
        let li = match self.layer_idx_for(feat) {
            Some(i) if i < self.layers.len() => i,
            _ => {
                // Features whose handle didn't resolve get dropped onto layer 0.
                0
            }
        };

        let props = feat.get("properties").and_then(|p| p.as_object());
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

        // If the entity has an explicit colour, the layer's "auto" flag flips false
        // and we adopt that explicit colour.
        let (r, g, b, auto) = if a == 255 {
            self.layers[li].auto = false;
            (r, g, b, false)
        } else {
            (
                self.layers[li].r,
                self.layers[li].g,
                self.layers[li].b,
                self.layers[li].auto,
            )
        };
        // Keep the stored auto-colour in sync for the final LayerInfo emission.
        if !auto {
            self.layers[li].r = r;
            self.layers[li].g = g;
            self.layers[li].b = b;
        }

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
                    append_line(
                        &mut self.layers[li].lines,
                        w[0].0,
                        w[0].1,
                        w[1].0,
                        w[1].1,
                        r,
                        g,
                        b,
                        a,
                    );
                    self.segments_total += 1;
                }
            }
            "Polygon" => {
                // Polygon coordinates: [outer_ring, opt_hole_1, opt_hole_2, ...]
                for ring in coords {
                    let ring_pts = ring
                        .as_array()
                        .map(|arr| arr.iter().filter_map(coord_2d).collect::<Vec<_>>())
                        .unwrap_or_default();
                    if ring_pts.len() >= 2 {
                        for i in 0..ring_pts.len() {
                            let j = (i + 1) % ring_pts.len();
                            if i == ring_pts.len() - 1 && ring_pts[j] == ring_pts[0] {
                                continue;
                            }
                            append_line(
                                &mut self.layers[li].lines,
                                ring_pts[i].0,
                                ring_pts[i].1,
                                ring_pts[j].0,
                                ring_pts[j].1,
                                r,
                                g,
                                b,
                                a,
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

    fn finalize(
        self,
        parse_time_ms: u64,
        was_dwg: bool,
        convert_ms: u64,
    ) -> Result<(SceneMeta, Vec<u8>), String> {
        let segments_total = self.segments_total;
        let text_count = self.texts.len() as u64;

        let mut geometry: Vec<u8> = Vec::new();
        let mut layouts: Vec<LayoutMeta> = Vec::new();
        let single_meta = Self::meta_for_single_layout(&self, &mut geometry);
        layouts.push(single_meta);

        let meta = SceneMeta {
            layers: self
                .layers
                .iter()
                .map(|l| LayerInfo {
                    name: l.name.clone(),
                    r: l.r,
                    g: l.g,
                    b: l.b,
                    auto: l.auto,
                    off: l.off,
                })
                .collect(),
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
            if l.lines.is_empty() {
                continue;
            }
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
            if l.tris.is_empty() {
                continue;
            }
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
            if l.points.is_empty() {
                continue;
            }
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
            min_x: 0.0,
            min_y: 0.0,
            max_x: 1.0,
            max_y: 1.0,
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
