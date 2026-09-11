//! Minimal ASCII DXF parser — purpose-built for the r14 output of `dwg2dxf`.
//!
//! Why this exists: the `dxf 0.6` crate overflows on r14 true-color group codes
//! (420–429, 310) and hits ENDSEC parse errors on real-world r14 files.  The
//! ASCII DXF format is just alternating `(group-code int / value)` lines — a
//! linear scan extracts 2D coordinates cheaply and robustly.
//!
//! Scope: extract geometry from a flat ENTITIES section.  This is a *geometry
//! extractor* not a full DXF codec — enough to tessellate hairline wireframes
//! for the Rocktier viewer.

use crate::aci::aci_to_rgb;
use crate::model::{LayerInfo, LayoutMeta, RangeSpec, SceneMeta, TextItem};

/// Parse ASCII DXF text into `(SceneMeta, geometry_blob)`.
///
/// `file_time_ms` is included in meta so the caller can surface parse time in
/// the UI; `was_dwg` flags whether dwg2dxf was invoked.
pub fn parse_and_build(
    input: &str,
    parse_time_ms: u64,
    was_dwg: bool,
    convert_ms: u64,
) -> Result<(SceneMeta, Vec<u8>), String> {
    let mut st = State::default();
    st.parse(input);
    st.finalize(parse_time_ms, was_dwg, convert_ms)
}

// ------------------------------------------------------------------------ internals

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
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    max_valid: f64,
    texts: Vec<TextItem>,
    current_layer: usize,
    model_space: usize,
}

impl State {
    fn layer_idx(&mut self, name: &str) -> usize {
        if let Some(&i) = self.layer_index.get(name) {
            return i;
        }
        let i = self.layers.len();
        self.layer_index.insert(name.to_string(), i);
        let (r, g, b) = aci_to_rgb(7);
        self.layers.push(LayerRec {
            name: name.to_string(),
            r, g, b, auto: true, off: false,
            lines: Vec::new(), points: Vec::new(), tris: Vec::new(),
        });
        i
    }

    fn set_extents(&mut self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            if x < self.min_x { self.min_x = x; }
            if y < self.min_y { self.min_y = y; }
            if x > self.max_x { self.max_x = x; }
            if y > self.max_y { self.max_y = y; }
            let m = x.abs().max(y.abs());
            if m > self.max_valid && m < 1.0e12 { self.max_valid = m; }
        }
    }

    fn push_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        self.set_extents(x1, y1);
        self.set_extents(x2, y2);
        let li = self.current_layer;
        let (r, g, b, a) = layer_color(&self.layers[li]);
        append_line(&mut self.layers[li].lines, x1, y1, x2, y2, r, g, b, a);
    }

    fn push_point(&mut self, x: f64, y: f64) {
        self.set_extents(x, y);
        let li = self.current_layer;
        let (r, g, b, a) = layer_color(&self.layers[li]);
        append_point(&mut self.layers[li].points, x, y, r, g, b, a);
    }

    #[allow(dead_code)]
    fn push_tri(&mut self, p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) {
        self.set_extents(p1.0, p1.1);
        self.set_extents(p2.0, p2.1);
        self.set_extents(p3.0, p3.1);
        let li = self.current_layer;
        let (r, g, b, a) = layer_color(&self.layers[li]);
        append_tri(&mut self.layers[li].tris, p1, p2, p3, r, g, b, a);
    }

    fn parse(&mut self, input: &str) {
        // Set up layer "0" by default.
        let zero = self.layer_idx("0");
        self.current_layer = zero;
        self.model_space = zero;

        let lines: Vec<&str> = input.lines().collect();
        let mut i = 0usize;
        let mut in_entities = false;
        let mut current_entity: Option<&str> = None;
        let mut buf: Vec<(i32, String)> = Vec::new();

        while i + 1 < lines.len() {
            let code_line = lines[i].trim();
            let value_line = lines[i + 1].trim_end_matches('\r').trim_start();
            i += 2;

            let code: i32 = match code_line.parse() {
                Ok(c) => c,
                Err(_) => {
                    if code_line.eq_ignore_ascii_case("EOF") {
                        // Flush pending entity, then stop.
                        if let Some(etype) = current_entity {
                            self.dispatch(etype, &buf);
                        }
                        return;
                    }
                    continue;
                }
            };

            if !in_entities {
                if code == 2 && value_line.eq_ignore_ascii_case("ENTITIES") {
                    in_entities = true;
                }
                continue;
            }
            if code == 0 && value_line.eq_ignore_ascii_case("ENDSEC") {
                break;
            }
            if code == 0 && value_line.eq_ignore_ascii_case("EOF") {
                break;
            }

            if code == 0 {
                // Entity dispatch point.
                if let Some(etype) = current_entity {
                    self.dispatch(etype, &buf);
                }
                buf.clear();
                current_entity = Some(value_line);
                continue;
            }

            buf.push((code, value_line.to_string()));
        }
        if let Some(etype) = current_entity {
            self.dispatch(etype, &buf);
        }
    }

    fn dispatch(&mut self, entity_type: &str, buf: &[(i32, String)]) {
        match entity_type.to_ascii_uppercase().as_str() {
            "LINE" => self.parse_line(buf),
            "CIRCLE" => self.parse_circle(buf),
            "ARC" => self.parse_arc(buf),
            "LWPOLYLINE" => self.parse_lwpolyline(buf),
            "POLYLINE" => { /* header; real vertices follow as separate VERTEX entities */ }
            "VERTEX" => { /* handled via POLYLINE parent context — ignored for now */ }
            "SEQEND" => {}
            "ELLIPSE" => self.parse_ellipse(buf),
            "SPLINE" => self.parse_spline(buf),
            "TEXT" | "MTEXT" => self.parse_text(buf),
            "POINT" => self.parse_point(buf),
            "INSERT" => { /* block reference — skipped; requires block def traversal */ }
            "DIMENSION" | "LEADER" | "RAY" | "XLINE" | "SOLID" | "TRACE" | "3DFACE" => {
                // Fallback: try to pull point + endpoints.
                self.parse_dimlike(buf);
            }
            _ => {} // silently ignore unsupported entities
        }
    }

    fn parse_line(&mut self, buf: &[(i32, String)]) {
        let x1 = f(buf, 10, 0.0);
        let y1 = f(buf, 20, 0.0);
        let x2 = f(buf, 11, x1);
        let y2 = f(buf, 21, y1);
        if (x1, y1) != (x2, y2) {
            self.set_layer(buf);
            self.push_line(x1, y1, x2, y2);
        }
    }

    fn parse_circle(&mut self, buf: &[(i32, String)]) {
        let cx = f(buf, 10, 0.0);
        let cy = f(buf, 20, 0.0);
        let r = f(buf, 40, 0.0);
        if r <= 0.0 { return; }
        let n = 64u32;
        self.set_layer(buf);
        let mut prev = (cx + r, cy);
        for i in 1..=n {
            let a = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            let p = (cx + r * a.cos(), cy + r * a.sin());
            self.push_line(prev.0, prev.1, p.0, p.1);
            prev = p;
        }
    }

    fn parse_arc(&mut self, buf: &[(i32, String)]) {
        let cx = f(buf, 10, 0.0);
        let cy = f(buf, 20, 0.0);
        let r = f(buf, 40, 0.0);
        if r <= 0.0 { return; }
        let a0 = f(buf, 50, 0.0).to_radians();
        let mut a1 = f(buf, 51, 360.0).to_radians();
        if a1 <= a0 { a1 += 2.0 * std::f64::consts::PI; }
        let sweep = a1 - a0;
        let n = 64u32;
        self.set_layer(buf);
        let mut prev = (cx + r * a0.cos(), cy + r * a0.sin());
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let a = a0 + sweep * t;
            let p = (cx + r * a.cos(), cy + r * a.sin());
            self.push_line(prev.0, prev.1, p.0, p.1);
            prev = p;
        }
    }

    fn parse_lwpolyline(&mut self, buf: &[(i32, String)]) {
        let mut pts: Vec<(f64, f64)> = Vec::new();
        let mut bulges: Vec<f64> = Vec::new();
        for (c, v) in buf {
            match *c {
                10 => {
                    if let Ok(x) = v.parse::<f64>() {
                        pts.push((x, f64::NAN));
                    }
                }
                20 => {
                    if let Ok(y) = v.parse::<f64>() {
                        if let Some(last) = pts.last_mut() {
                            last.1 = y;
                        }
                    }
                }
                42 => {
                    let b = v.parse::<f64>().unwrap_or(0.0);
                    bulges.push(b);
                }
                70 => { /* closed flag parsed later */ }
                _ => {}
            }
        }

        // Re-pair bulges with their originating vertex.
        // In DXF, a bulge appears after its vertex's Y group-code; so
        // bulge[j] belongs between vertex[j] and vertex[j+1].
        let closed = buf.iter().find(|(c, _)| *c == 70)
            .and_then(|(_, v)| v.parse::<i64>().ok())
            .map(|v| v & 1 != 0)
            .unwrap_or(false);

        pts.retain(|p| p.1.is_finite());
        if pts.len() < 2 { return; }

        self.set_layer(buf);
        let n = pts.len();
        let edge_count = if closed { n } else { n - 1 };
        for i in 0..edge_count {
            let j = (i + 1) % n;
            let (x1, y1) = pts[i];
            let (x2, y2) = pts[j];
            let bulge = bulges.get(i).copied().unwrap_or(0.0);
            if bulge.abs() < 1e-9 {
                self.push_line(x1, y1, x2, y2);
            } else {
                self.sample_arc_bulge(x1, y1, x2, y2, bulge);
            }
        }
    }

    fn sample_arc_bulge(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, bulge: f64) {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let chord = (dx * dx + dy * dy).sqrt();
        if chord < 1e-12 {
            self.push_line(x1, y1, x2, y2);
            return;
        }
        let sagitta = bulge * chord / 2.0;
        let included = 2.0 * (bulge.abs()).atan() * 2.0; // 4*atan(|bulge|)
        let n = 32u32;
        let mx = (x1 + x2) / 2.0;
        let my = (y1 + y2) / 2.0;
        let perp_x = -dy / chord;
        let perp_y = dx / chord;
        let sign = if bulge >= 0.0 { 1.0 } else { -1.0 };
        let cx = mx + sign * perp_x * sagitta;
        let cy = my + sign * perp_y * sagitta;
        let a0 = (y1 - cy).atan2(x1 - cx);
        let dir = if bulge >= 0.0 { 1.0 } else { -1.0 };
        let mut prev = (x1, y1);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let a = a0 + dir * included * t;
            let r = ((x1 - cx).powi(2) + (y1 - cy).powi(2)).sqrt();
            let p = (cx + r * a.cos(), cy + r * a.sin());
            self.push_line(prev.0, prev.1, p.0, p.1);
            prev = p;
        }
    }

    fn parse_ellipse(&mut self, buf: &[(i32, String)]) {
        let cx = f(buf, 10, 0.0);
        let cy = f(buf, 20, 0.0);
        let ax = f(buf, 11, 0.0);
        let ay = f(buf, 21, 0.0);
        let ratio = f(buf, 40, 1.0);
        let a0 = f(buf, 41, 0.0);
        let a1 = f(buf, 42, std::f64::consts::PI * 2.0);
        let major = (ax * ax + ay * ay).sqrt();
        let minor = major * ratio.min(1.0).max(1e-12);
        let rot = ay.atan2(ax);
        let steps = 64u32;
        self.set_layer(buf);
        let mut prev = ellipse_pt(cx, cy, major, minor, rot, a0);
        for i in 1..=steps {
            let t = a0 + (a1 - a0) * (i as f64 / steps as f64);
            let p = ellipse_pt(cx, cy, major, minor, rot, t);
            self.push_line(prev.0, prev.1, p.0, p.1);
            prev = p;
        }
    }

    fn parse_spline(&mut self, buf: &[(i32, String)]) {
        let degree = buf.iter().find(|(c, _)| *c == 71)
            .and_then(|(_, v)| v.parse::<i64>().ok())
            .unwrap_or(3) as i32;
        let ctrl: Vec<(f64, f64)> = extract_pairs(buf, 10, 20).collect();
        let knots: Vec<f64> = buf.iter().filter(|(c, _)| *c == 40)
            .filter_map(|(_, v)| v.parse().ok()).collect();
        if ctrl.len() < 2 { return; }
        self.set_layer(buf);
        if knots.len() < ctrl.len() + degree as usize + 1 {
            // Fall back to control polygon.
            for w in ctrl.windows(2) {
                self.push_line(w[0].0, w[0].1, w[1].0, w[1].1);
            }
            return;
        }
        let u_min = knots[degree as usize];
        let u_max = knots[ctrl.len()];
        if u_max <= u_min { return; }
        let steps = 64usize;
        let mut prev = de_boor(degree, &knots, &ctrl, u_min);
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let u = u_min + (u_max - u_min) * t;
            let p = de_boor(degree, &knots, &ctrl, u);
            self.push_line(prev.0, prev.1, p.0, p.1);
            prev = p;
        }
    }

    fn parse_text(&mut self, buf: &[(i32, String)]) {
        let x = f(buf, 10, 0.0);
        let y = f(buf, 20, 0.0);
        let raw = s(buf, 1);
        let display = decode_mtext(raw);
        if display.trim().is_empty() { return; }
        let li = self.layer_idx(s(buf, 8));
        let (r, g, b) = layer_color_raw(&self.layers[li]);
        let a = if self.layers[li].auto { 0 } else { 255 };
        self.texts.push(TextItem {
            layout: self.model_space as u32,
            layer: li as u32,
            x: x as f32,
            y: y as f32,
            h: f(buf, 40, 1.0) as f32,
            rot: f(buf, 50, 0.0) as f32,
            r, g, b, a,
            ha: i(buf, 72, 0) as u8,
            va: i(buf, 73, 0) as u8,
            text: display,
        });
    }

    fn parse_point(&mut self, buf: &[(i32, String)]) {
        let x = f(buf, 10, 0.0);
        let y = f(buf, 20, 0.0);
        self.set_layer(buf);
        self.push_point(x, y);
    }

    fn parse_dimlike(&mut self, buf: &[(i32, String)]) {
        // Extract any (10,20)-(11,21)-(13,23) triples we can find.
        let x1 = f(buf, 10, f64::NAN);
        let y1 = f(buf, 20, f64::NAN);
        let x2 = f(buf, 11, f64::NAN);
        let y2 = f(buf, 21, f64::NAN);
        let x3 = f(buf, 13, f64::NAN);
        let y3 = f(buf, 23, f64::NAN);
        self.set_layer(buf);
        if x1.is_finite() && y1.is_finite() {
            if x2.is_finite() && y2.is_finite() {
                self.push_line(x1, y1, x2, y2);
            }
            if x3.is_finite() && y3.is_finite() {
                self.push_line(x1, y1, x3, y3);
            }
        }
    }

    fn set_layer(&mut self, buf: &[(i32, String)]) {
        if let Some(name) = buf.iter().find(|(c, _)| *c == 8).map(|(_, v)| v.as_str()) {
            self.current_layer = self.layer_idx(name);
        }
    }

    fn finalize(mut self, parse_time_ms: u64, was_dwg: bool, convert_ms: u64) -> Result<(SceneMeta, Vec<u8>), String> {
        // Setup default camera to cover all geometry.
        if self.max_valid.is_finite() && self.max_valid > 0.0 {
            let pad = self.max_valid * 0.05;
            self.min_x = -pad;
            self.min_y = -pad;
            self.max_x = self.max_valid + pad;
            self.max_y = self.max_valid + pad;
        } else {
            self.min_x = -1.0; self.min_y = -1.0; self.max_x = 1.0; self.max_y = 1.0;
        }

        let segments_total: u64 = self.layers.iter()
            .map(|l| (l.lines.len() / 24) as u64) // 2 verts × 12 bytes
            .sum();
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
            min_x: ls.min_x,
            min_y: ls.min_y,
            max_x: ls.max_x,
            max_y: ls.max_y,
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

fn s<'a>(buf: &'a [(i32, String)], code: i32) -> &'a str {
    buf.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str()).unwrap_or("")
}

fn f(buf: &[(i32, String)], code: i32, default: f64) -> f64 {
    buf.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.parse().ok()).unwrap_or(default)
}

fn i(buf: &[(i32, String)], code: i32, default: i64) -> i64 {
    buf.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.parse().ok()).unwrap_or(default)
}

fn decode_mtext(s: &str) -> String {
    s.replace("\\P", "\n")
        .replace("%%D", "°")
        .replace("%%C", "∅")
        .replace("%%P", "±")
}

/// Pairs of (code_x, code_y) — collects tuples until both x and y are present.
fn extract_pairs<'a>(
    buf: &'a [(i32, String)],
    code_x: i32,
    code_y: i32,
) -> impl Iterator<Item = (f64, f64)> + 'a {
    #[derive(Default)]
    struct Acc { x: Option<f64>, pts: Vec<(f64, f64)> }
    let mut acc = Acc::default();
    for (c, v) in buf {
        if *c == code_x { acc.x = v.parse().ok(); }
        if *c == code_y {
            if let Ok(y) = v.parse::<f64>() {
                if let Some(x) = acc.x.take() {
                    acc.pts.push((x, y));
                }
            }
        }
    }
    acc.pts.into_iter()
}

fn layer_color(l: &LayerRec) -> (u8, u8, u8, u8) {
    if l.auto {
        (0, 0, 0, 0) // a==0 means "auto color" in renderer
    } else {
        (l.r, l.g, l.b, 255)
    }
}

fn layer_color_raw(l: &LayerRec) -> (u8, u8, u8) {
    (l.r, l.g, l.b)
}

fn ellipse_pt(cx: f64, cy: f64, a: f64, b: f64, rot: f64, t: f64) -> (f64, f64) {
    let cosr = rot.cos();
    let sinr = rot.sin();
    let px = a * t.cos();
    let py = b * t.sin();
    (cx + px * cosr - py * sinr, cy + px * sinr + py * cosr)
}

/// Cox–de Boor evaluation for a uniform-ish knot vector at parameter `u`.
fn de_boor(degree: i32, knots: &[f64], ctrl: &[(f64, f64)], u: f64) -> (f64, f64) {
    let n = ctrl.len();
    let k = degree as usize;
    // Find the active knot span i such that knots[i] <= u < knots[i+1].
    let mut s = k;
    for i in k..(n - 1) {
        if u >= knots[i] && u < knots[i + 1] {
            s = i;
            break;
        }
    }
    let mut d: Vec<(f64, f64)> = ctrl[s - k..=s].iter().copied().collect();
    for r in 1..=k {
        for j in (r..=k).rev() {
            let denom = knots[s + j + 1 - r] - knots[s + j - k];
            let a = if denom.abs() < 1e-12 { 0.5 } else {
                ((u - knots[s + j - k]) / denom).clamp(0.0, 1.0)
            };
            d[j].0 = (1.0 - a) * d[j - 1].0 + a * d[j].0;
            d[j].1 = (1.0 - a) * d[j - 1].1 + a * d[j].1;
        }
    }
    d[k]
}

// ------------------------------------------------------------------------ vertex encoders

pub(crate) fn append_line(out: &mut Vec<u8>, x1: f64, y1: f64, x2: f64, y2: f64, r: u8, g: u8, b: u8, a: u8) {
    out.extend_from_slice(&(x1 as f32).to_le_bytes());
    out.extend_from_slice(&(y1 as f32).to_le_bytes());
    out.push(r); out.push(g); out.push(b); out.push(a);
    out.extend_from_slice(&(x2 as f32).to_le_bytes());
    out.extend_from_slice(&(y2 as f32).to_le_bytes());
    out.push(r); out.push(g); out.push(b); out.push(a);
}

pub(crate) fn append_point(out: &mut Vec<u8>, x: f64, y: f64, r: u8, g: u8, b: u8, a: u8) {
    out.extend_from_slice(&(x as f32).to_le_bytes());
    out.extend_from_slice(&(y as f32).to_le_bytes());
    out.push(r); out.push(g); out.push(b); out.push(a);
}

pub(crate) fn append_tri(out: &mut Vec<u8>, p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), r: u8, g: u8, b: u8, a: u8) {
    append_point(out, p1.0, p1.1, r, g, b, a);
    append_point(out, p2.0, p2.1, r, g, b, a);
    append_point(out, p3.0, p3.1, r, g, b, a);
}
