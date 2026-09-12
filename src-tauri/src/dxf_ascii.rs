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
    let mut st = State::new();
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

/// A POLYLINE accumulates its VERTEX children until SEQEND arrives.
struct PolyAcc {
    layer: usize,
    color: Option<(u8, u8, u8, u8)>,
    closed: bool,
    pts: Vec<(f64, f64)>,
    bulges: Vec<f64>,
}

struct State {
    layers: Vec<LayerRec>,
    layer_index: std::collections::HashMap<String, usize>,
    /// LAYER table records: name → (ACI colour, is-off).
    layer_table: std::collections::HashMap<String, (u8, bool)>,
    /// LAYER table names in file order (a HashMap would shuffle the panel).
    layer_names: Vec<String>,
    /// BLOCKS section: block name → [(entity type, group-code pairs)].
    blocks: std::collections::HashMap<String, Vec<(String, Vec<(i32, String)>)>>,
    /// Active block-insert transform: (tx, ty, scale, cos θ, sin θ).
    xf: (f64, f64, f64, f64, f64),
    xf_depth: u32,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    max_valid: f64,
    texts: Vec<TextItem>,
    current_layer: usize,
    /// Entity-level colour override (group code 62) as RGBA, when present;
    /// `(0,0,0,0)` means "background colour" (ACI 7 / auto).
    cur_color: Option<(u8, u8, u8, u8)>,
    poly: Option<PolyAcc>,
    /// Count of recognised-but-unsupported entity types (HATCH / DIMENSION /
    /// unresolved INSERT).  Bubbles through to the UI.
    skipped: u64,
}

impl State {
    fn new() -> Self {
        // Extents start at ±infinity (not 0): drawings offset from the origin
        // (survey/UTM coordinates) would otherwise clamp min_x/min_y to 0 and
        // skew the fit-to-view bounding box.
        State {
            layers: Vec::new(),
            layer_index: std::collections::HashMap::new(),
            layer_table: std::collections::HashMap::new(),
            layer_names: Vec::new(),
            blocks: std::collections::HashMap::new(),
            xf: (0.0, 0.0, 1.0, 1.0, 0.0),
            xf_depth: 0,
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
            max_valid: 0.0,
            texts: Vec::new(),
            current_layer: 0,
            cur_color: None,
            poly: None,
            skipped: 0,
        }
    }

    fn layer_idx(&mut self, name: &str) -> usize {
        if let Some(&i) = self.layer_index.get(name) {
            return i;
        }
        let i = self.layers.len();
        self.layer_index.insert(name.to_string(), i);
        // Colour from the TABLES/LAYER record when present (ACI 7 = "auto"),
        // otherwise ACI 7 from the start.
        let (aci, off) = self.layer_table.get(name).copied().unwrap_or((7, false));
        let (r, g, b) = aci_to_rgb(aci);
        self.layers.push(LayerRec {
            name: name.to_string(),
            r, g, b, auto: aci == 7, off,
            lines: Vec::new(), points: Vec::new(), tris: Vec::new(),
        });
        i
    }

    /// Apply the active block-insert transform to a block-space coordinate.
    fn apply_xf(&self, x: f64, y: f64) -> (f64, f64) {
        let (tx, ty, s, c, sn) = self.xf;
        (
            s * (c * x - sn * y) + tx,
            s * (sn * x + c * y) + ty,
        )
    }

    /// Colour for the entity being parsed: explicit code 62 wins, else layer.
    fn cur_rgba(&self) -> (u8, u8, u8, u8) {
        self.cur_color
            .unwrap_or_else(|| layer_color(&self.layers[self.current_layer]))
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
        let (x1, y1) = self.apply_xf(x1, y1);
        let (x2, y2) = self.apply_xf(x2, y2);
        self.set_extents(x1, y1);
        self.set_extents(x2, y2);
        let li = self.current_layer;
        let (r, g, b, a) = self.cur_rgba();
        append_line(&mut self.layers[li].lines, x1, y1, x2, y2, r, g, b, a);
    }

    fn push_point(&mut self, x: f64, y: f64) {
        let (x, y) = self.apply_xf(x, y);
        self.set_extents(x, y);
        let li = self.current_layer;
        let (r, g, b, a) = self.cur_rgba();
        append_point(&mut self.layers[li].points, x, y, r, g, b, a);
    }

    #[allow(dead_code)]
    fn push_tri(&mut self, p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) {
        let p1 = self.apply_xf(p1.0, p1.1);
        let p2 = self.apply_xf(p2.0, p2.1);
        let p3 = self.apply_xf(p3.0, p3.1);
        self.set_extents(p1.0, p1.1);
        self.set_extents(p2.0, p2.1);
        self.set_extents(p3.0, p3.1);
        let li = self.current_layer;
        let (r, g, b, a) = self.cur_rgba();
        append_tri(&mut self.layers[li].tris, p1, p2, p3, r, g, b, a);
    }

    fn parse(&mut self, input: &str) {
        // TABLES/LAYER and BLOCKS are needed up front: layer colours when the
        // first entity referencing a layer is parsed, blocks on every INSERT.
        let (layer_table, layer_names) = scan_layer_table(input);
        self.layer_table = layer_table;
        self.layer_names = layer_names;
        self.blocks = scan_blocks(input);

        // Register every LAYER record up front, so the panel lists the drawing's
        // layers in file order even when one currently holds no geometry.
        for name in std::mem::take(&mut self.layer_names) {
            self.layer_idx(&name);
        }

        // Set up layer "0" by default.
        self.current_layer = self.layer_idx("0");

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
                            self.dispatch_model(etype, &buf);
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
                    self.dispatch_model(etype, &buf);
                }
                buf.clear();
                current_entity = Some(value_line);
                continue;
            }

            buf.push((code, value_line.to_string()));
        }
        if let Some(etype) = current_entity {
            self.dispatch_model(etype, &buf);
        }
    }

    /// Top-level entity dispatch: drops paper-space geometry.
    ///
    /// Group 67 = 1 marks a paper-space entity (title blocks, viewport frames).
    /// The viewer renders model space, and overlaying a sheet-sized frame on the
    /// plan — and folding it into the fit-to-view extents — is worse than
    /// leaving it out.  Block contents still go through `dispatch` untouched.
    fn dispatch_model(&mut self, entity_type: &str, buf: &[(i32, String)]) {
        if buf.iter().any(|(c, v)| *c == 67 && v.trim() == "1") {
            return;
        }
        self.dispatch(entity_type, buf);
    }

    fn dispatch(&mut self, entity_type: &str, buf: &[(i32, String)]) {
        match entity_type.to_ascii_uppercase().as_str() {
            "LINE" => self.parse_line(buf),
            "CIRCLE" => self.parse_circle(buf),
            "ARC" => self.parse_arc(buf),
            "LWPOLYLINE" => self.parse_lwpolyline(buf),
            "POLYLINE" => self.poly_begin(buf),
            "VERTEX" => self.poly_vertex(buf),
            "SEQEND" => self.poly_flush(),
            "ELLIPSE" => self.parse_ellipse(buf),
            "SPLINE" => self.parse_spline(buf),
            "TEXT" => self.parse_text(buf, false),
            "MTEXT" => self.parse_text(buf, true),
            "POINT" => self.parse_point(buf),
            "INSERT" => self.expand_insert(buf),
            "HATCH" | "SOLID" | "TRACE" | "3DFACE" => { self.skipped += 1; /* unsupported; skip */ }
            "DIMENSION" => self.expand_dimension(buf),
            "LEADER" | "RAY" | "XLINE" => {
                self.skipped += 1;
                // Fallback: try to pull point + endpoints.
                self.parse_dimlike(buf);
            }
            _ => {} // silently ignore unsupported entities
        }
    }

    // ------------------------------------------------------------------------
    // Adaptive circular-segment count based on radius
    // ------------------------------------------------------------------------

    /// Decide how many segments a circle / arc should be tessellated into.
    /// Big radii need more segments to look round; tiny radii need fewer.
    /// Chord-height tolerance ≈ 0.5px at unit scale gives smooth display.
    fn adaptive_segments(r: f64) -> u32 {
        const SEG_PER_UNIT: f64 = 0.6;
        let estimate = (2.0 * std::f64::consts::PI * r.abs() * SEG_PER_UNIT).ceil() as u32;
        estimate.clamp(16, 512)
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
        let n = Self::adaptive_segments(r);
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
        let n = (Self::adaptive_segments(r) as f64 * (sweep / (2.0 * std::f64::consts::PI))).ceil() as u32;
        let n = n.max(8);
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

    /// Emit edges for a vertex list.  `bulges[i]` is the bulge of the edge
    /// leaving vertex `i` (DXF group code 42 semantics), 0.0 = straight.
    fn emit_polyline(&mut self, pts: &[(f64, f64)], bulges: &[f64], closed: bool) {
        let n = pts.len();
        if n < 2 { return; }
        let edge_count = if closed { n } else { n - 1 };
        for i in 0..edge_count {
            let j = (i + 1) % n;
            let (x1, y1) = pts[i];
            let (x2, y2) = pts[j];
            let bulge = bulges.get(i).copied().unwrap_or(0.0);
            if bulge.abs() >= 1e-9 {
                self.sample_arc_bulge(x1, y1, x2, y2, bulge);
            } else {
                self.push_line(x1, y1, x2, y2);
            }
        }
    }

    fn parse_lwpolyline(&mut self, buf: &[(i32, String)]) {
        let mut pts: Vec<(f64, f64)> = Vec::new();
        let mut bulges: std::collections::HashMap<usize, f64> = std::collections::HashMap::new();
        let mut last_vertex: Option<usize> = None;

        for (c, v) in buf {
            match *c {
                10 => {
                    if let Ok(x) = v.parse::<f64>() {
                        // Start a new vertex — leave y blank until code 20 arrives.
                        pts.push((x, f64::NAN));
                        last_vertex = Some(pts.len() - 1);
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
                    // This bulge belongs to the **most recently opened** vertex
                    // (whether or not its Y has been seen — DXF allows Y to come
                    // after the bulge in some writers).
                    if let Some(orig) = last_vertex {
                        let b = v.parse::<f64>().unwrap_or(0.0);
                        // If a vertex had two bulge codes (ill-formed DXF),
                        // the last one wins.
                        bulges.insert(orig, b);
                    }
                }
                _ => {}
            }
        }

        // Drop vertices with missing / NaN Y coords (guards truncated files),
        // carrying each bulge along with its *original* index so the surviving
        // list stays aligned.
        let mut kept: Vec<(f64, f64)> = Vec::new();
        let mut kept_bulges: Vec<f64> = Vec::new();
        for (i, p) in pts.iter().enumerate() {
            if p.1.is_finite() {
                kept.push(*p);
                kept_bulges.push(bulges.get(&i).copied().unwrap_or(0.0));
            }
        }
        if kept.len() < 2 { return; }

        self.set_layer(buf);
        self.emit_polyline(&kept, &kept_bulges, closed_flag(buf));
    }

    // ------------------------------------------------------------ POLYLINE

    fn poly_begin(&mut self, buf: &[(i32, String)]) {
        self.poly_flush(); // a previous POLYLINE without its SEQEND
        self.set_layer(buf);
        self.poly = Some(PolyAcc {
            layer: self.current_layer,
            color: self.cur_color,
            closed: closed_flag(buf),
            pts: Vec::new(),
            bulges: Vec::new(),
        });
    }

    fn poly_vertex(&mut self, buf: &[(i32, String)]) {
        let x = f(buf, 10, f64::NAN);
        let y = f(buf, 20, f64::NAN);
        if !(x.is_finite() && y.is_finite()) { return; }
        let bulge = f(buf, 42, 0.0);
        if let Some(p) = self.poly.as_mut() {
            p.pts.push((x, y));
            p.bulges.push(bulge);
        }
    }

    fn poly_flush(&mut self) {
        if let Some(p) = self.poly.take() {
            if p.pts.len() >= 2 {
                self.current_layer = p.layer;
                self.cur_color = p.color;
                self.emit_polyline(&p.pts, &p.bulges, p.closed);
            }
        }
    }

    // ------------------------------------------------------------ INSERT

    /// Expand a named block with a local placement, composing it with the
    /// transform already active (so nested references nest correctly).
    ///
    /// ponytail: uniform scale only (42 is ignored), block base point is
    /// assumed (0,0), and the definition is cloned per reference — split the
    /// transform on x/y and index into `blocks` if a real drawing needs it.
    fn expand_block(&mut self, name: &str, ix: f64, iy: f64, scale: f64, rot_deg: f64) {
        if self.xf_depth >= 8 { return; } // cyclic block references
        let ents = match self.blocks.get(name) {
            Some(e) => e.clone(),
            None => { self.skipped += 1; return; } // unresolved / xref block
        };

        let rot = rot_deg.to_radians();
        let (tx, ty, os, oc, osn) = self.xf;
        let (c, sn) = (rot.cos(), rot.sin());
        self.xf = (
            os * (oc * ix - osn * iy) + tx,
            os * (osn * ix + oc * iy) + ty,
            os * scale,
            oc * c - osn * sn,
            osn * c + oc * sn,
        );
        self.xf_depth += 1;
        let (saved_layer, saved_color) = (self.current_layer, self.cur_color);
        for (etype, pairs) in &ents {
            self.dispatch(etype, pairs);
        }
        self.xf_depth -= 1;
        self.xf = (tx, ty, os, oc, osn);
        self.current_layer = saved_layer;
        self.cur_color = saved_color;
    }

    fn expand_insert(&mut self, buf: &[(i32, String)]) {
        let name = s(buf, 2).to_string();
        let ix = f(buf, 10, 0.0);
        let iy = f(buf, 20, 0.0);
        let scale = match f(buf, 41, 1.0) {
            v if v.is_finite() && v != 0.0 => v,
            _ => 1.0,
        };
        let rot = f(buf, 50, 0.0);
        // The insert's own layer/colour is inherited by block entities that
        // don't set their own.
        self.set_layer(buf);
        self.expand_block(&name, ix, iy, scale, rot);
    }

    /// A DIMENSION's anonymous block (`*D…`) holds the drawn dimension — its
    /// lines, arrows and measurement text — already in world coordinates, so it
    /// expands with an identity placement: codes 10/11 are the definition and
    /// text points, not a block insertion point.
    fn expand_dimension(&mut self, buf: &[(i32, String)]) {
        let name = s(buf, 2).to_string();
        if name.is_empty() || !self.blocks.contains_key(&name) {
            // No anonymous block (e.g. minimal DXF): fall back to its endpoints.
            self.skipped += 1;
            self.parse_dimlike(buf);
            return;
        }
        self.set_layer(buf);
        self.expand_block(&name, 0.0, 0.0, 1.0, 0.0);
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
        let included = 4.0 * bulge.abs().atan();
        let n = (included / (2.0 * std::f64::consts::PI) * 64.0).ceil() as u32;
        let n = n.max(4);
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
        // Adaptive: scale with major axis length.
        let r_avg = (major + minor) * 0.5;
        let steps = Self::adaptive_segments(r_avg);
        self.set_layer(buf);
        let mut prev = ellipse_pt(cx, cy, major, minor, rot, a0);
        for i in 1..=steps {
            let t = a0 + (a1 - a0) * (i as f64 / steps as f64);
            let p = ellipse_pt(cx, cy, major, minor, rot, t);
            self.push_line(prev.0, prev.1, p.0, p.1);
            prev = p;
        }
    }

    /// Tessellate a B-spline by evaluating de Boor at uniform knot-parameter
    /// steps over the active domain [knots[degree], knots[ctrl.len()]].
    ///
    /// ponytail: uniform in parameter, not arc length — re-parameterise by
    /// chord length only if a drawing's splines ever look visibly uneven.
    fn parse_spline(&mut self, buf: &[(i32, String)]) {
        let degree = buf.iter().find(|(c, _)| *c == 71)
            .and_then(|(_, v)| v.parse::<i64>().ok())
            .unwrap_or(3) as i32;
        let ctrl: Vec<(f64, f64)> = extract_pairs(buf, 10, 20).collect();
        let knots: Vec<f64> = buf.iter().filter(|(c, _)| *c == 40)
            .filter_map(|(_, v)| v.parse().ok()).collect();
        if ctrl.len() < 2 { return; }
        self.set_layer(buf);
        let k = degree as usize;
        if knots.len() < ctrl.len() + k + 1 {
            // Knot vector doesn't match control count — fall back to the
            // control polygon (visual approximation).
            for w in ctrl.windows(2) {
                self.push_line(w[0].0, w[0].1, w[1].0, w[1].1);
            }
            return;
        }

        let u_lo = knots[k];
        let u_hi = knots[ctrl.len()];
        if u_hi <= u_lo { return; }

        // Step count scales with control-polygon length; at least 64.
        let chord: f64 = ctrl.windows(2)
            .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
            .sum();
        let steps = ((64.0 + chord * 0.02).ceil() as usize).clamp(32, 512);

        let mut prev = de_boor(u_lo, degree, &knots, &ctrl);
        for i in 1..=steps {
            let u = u_lo + (u_hi - u_lo) * (i as f64 / steps as f64);
            let p = de_boor(u, degree, &knots, &ctrl);
            if p != prev {
                self.push_line(prev.0, prev.1, p.0, p.1);
            }
            prev = p;
        }
    }

    fn parse_text(&mut self, buf: &[(i32, String)], is_mtext: bool) {
        let raw = s(buf, 1);
        let display = decode_mtext(raw);
        if display.trim().is_empty() { return; }
        self.set_layer(buf);
        let li = self.current_layer;
        let (r, g, b, a) = self.cur_rgba();
        // Text inside a block reference inherits the insert's placement.
        let (x, y) = self.apply_xf(f(buf, 10, 0.0), f(buf, 20, 0.0));
        let (_, _, sx, cx, sxn) = self.xf;
        // MTEXT carries no group 50: its rotation is the x-axis direction
        // vector (11/21) — that is how vertical dimension text is encoded.
        // Reading only 50 left those annotations lying flat across the plan.
        let rot = if is_mtext {
            let (dx, dy) = (f(buf, 11, f64::NAN), f(buf, 21, f64::NAN));
            if dx.is_finite() && dy.is_finite() && (dx != 0.0 || dy != 0.0) {
                dy.atan2(dx).to_degrees()
            } else {
                f(buf, 50, 0.0)
            }
        } else {
            f(buf, 50, 0.0)
        };
        // Alignment: TEXT uses 72/73 (justification); MTEXT uses the
        // attachment point 71 — 72 there is the *drawing direction*.
        let (ha, va) = if is_mtext {
            match i(buf, 71, 5) {
                1 => (0, 3), 2 => (1, 3), 3 => (2, 3),
                4 => (0, 2), 5 => (1, 2), 6 => (2, 2),
                7 => (0, 1), 8 => (1, 1), 9 => (2, 1),
                _ => (0, 1),
            }
        } else {
            (i(buf, 72, 0) as i64, i(buf, 73, 0) as i64)
        };
        self.texts.push(TextItem {
            // A single "Model" layout — this is a layout index, not a layer one.
            layout: 0,
            layer: li as u32,
            x: x as f32,
            y: y as f32,
            h: (f(buf, 40, 1.0) * sx) as f32,
            rot: (rot + sxn.atan2(cx).to_degrees()) as f32,
            r, g, b, a,
            ha: ha as u8,
            va: va as u8,
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
        // Group code 62 on an entity overrides the layer colour.  0 (BYBLOCK)
        // and 256 (BYLAYER) are not colours, and ACI 7 is the *background*
        // colour (white on dark, black on light) — neither falls back to the
        // layer, and painting ACI 7 as literal white makes the drawing vanish
        // on a light canvas.
        self.cur_color = buf
            .iter()
            .find(|(c, _)| *c == 62)
            .and_then(|(_, v)| v.parse::<i32>().ok())
            .filter(|c| (1..=255).contains(c))
            .map(|c| {
                if c == 7 {
                    (0, 0, 0, 0)
                } else {
                    let (r, g, b) = aci_to_rgb(c as u8);
                    (r, g, b, 255)
                }
            });
    }

    fn finalize(mut self, parse_time_ms: u64, was_dwg: bool, convert_ms: u64) -> Result<(SceneMeta, Vec<u8>), String> {
        // Setup default camera to cover all geometry. Use the real bounding
        // box padded by 5% — the old `max_valid`-symmetric range misplaced
        // drawings that are offset from the origin (e.g. survey coordinates).
        if self.max_valid.is_finite() && self.max_valid > 0.0 {
            let span_x = self.max_x - self.min_x;
            let span_y = self.max_y - self.min_y;
            let px = if span_x > 0.0 { span_x * 0.05 } else { self.max_valid * 0.05 };
            let py = if span_y > 0.0 { span_y * 0.05 } else { self.max_valid * 0.05 };
            self.min_x -= px;
            self.min_y -= py;
            self.max_x += px;
            self.max_y += py;
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
            skipped: self.skipped,
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

/// Decode MTEXT/TEXT content: drop the inline formatting codes AutoCAD writes
/// (`\A1;` vertical alignment, `\fSimSun|b0|i0|c134|p2;` font, `\H…;`, `\W…;`,
/// `{…}` grouping) and expand the legacy `%%d/c/p` escapes.
///
/// Without this, dimension text renders literally as "A1;8780" instead of
/// "8780".
pub(crate) fn decode_mtext(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '{' | '}' => i += 1,
            '\\' => {
                i += 1;
                if i >= chars.len() { break; }
                match chars[i] {
                    'P' | 'X' => { out.push('\n'); i += 1; }
                    '~' => { out.push(' '); i += 1; }
                    '\\' | '{' | '}' => { out.push(chars[i]); i += 1; }
                    'S' => {
                        // Stacked fraction: keep it inline as "a/b".
                        i += 1;
                        while i < chars.len() && chars[i] != ';' {
                            out.push(if chars[i] == '^' { '/' } else { chars[i] });
                            i += 1;
                        }
                        if i < chars.len() { i += 1; }
                    }
                    _ => {
                        // Any other code: skip its `;`-terminated parameter.
                        i += 1;
                        while i < chars.len()
                            && chars[i] != ';'
                            && chars[i] != '\\'
                            && chars[i] != '{'
                            && chars[i] != '}'
                        {
                            i += 1;
                        }
                        if i < chars.len() && chars[i] == ';' { i += 1; }
                    }
                }
            }
            c => { out.push(c); i += 1; }
        }
    }
    out.replace("%%D", "°")
        .replace("%%d", "°")
        .replace("%%C", "∅")
        .replace("%%c", "∅")
        .replace("%%P", "±")
        .replace("%%p", "±")
        .replace("%%O", "")
        .replace("%%o", "")
        .replace("%%U", "")
        .replace("%%u", "")
        .replace("%%%", "%")
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

/// Closed flag (group code 70, bit 1) of a LWPOLYLINE / POLYLINE header.
fn closed_flag(buf: &[(i32, String)]) -> bool {
    buf.iter().find(|(c, _)| *c == 70)
        .and_then(|(_, v)| v.parse::<i64>().ok())
        .map(|v| v & 1 != 0)
        .unwrap_or(false)
}

/// Pre-scan the TABLES section for LAYER records: name → (ACI colour, off),
/// plus the names in file order.  Colour is stored positive; a negative code
/// 62 means the layer is off.
#[allow(clippy::type_complexity)]
fn scan_layer_table(input: &str) -> (std::collections::HashMap<String, (u8, bool)>, Vec<String>) {
    use std::collections::HashMap;
    let lines: Vec<&str> = input.lines().collect();
    let mut out: HashMap<String, (u8, bool)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut in_tables = false;
    let mut is_layer = false;
    let mut name: Option<String> = None;
    let mut color = 7i32;

    let mut i = 0usize;
    while i + 1 < lines.len() {
        let code: i32 = match lines[i].trim().parse() {
            Ok(c) => c,
            Err(_) => { i += 2; continue; }
        };
        let value = lines[i + 1].trim_end_matches('\r').trim_start();
        i += 2;

        if !in_tables {
            if code == 2 && value.eq_ignore_ascii_case("TABLES") { in_tables = true; }
            continue;
        }
        if code == 0 {
            // Every code-0 line ends the pending LAYER record — including the
            // next `0 LAYER`, which is how consecutive records are separated.
            if let Some(n) = name.take() {
                // 0 is not a legal ACI for a layer record; fall back to 7.
                let aci = if color == 0 { 7 } else { color.abs().min(255) as u8 };
                if out.insert(n.clone(), (aci, color < 0)).is_none() {
                    order.push(n);
                }
            }
            if value.eq_ignore_ascii_case("ENDSEC") { break; }
            is_layer = value.eq_ignore_ascii_case("LAYER");
            color = 7;
            continue;
        }
        if is_layer {
            if code == 2 { name = Some(value.to_string()); }
            if code == 62 { if let Ok(c) = value.parse::<i32>() { color = c; } }
        }
    }
    (out, order)
}

/// Pre-scan the BLOCKS section into name → [(entity type, group-code pairs)].
///
/// ponytail: the BLOCK base point (10/20) is ignored — block definitions are
/// almost always written at the origin.
fn scan_blocks(input: &str) -> std::collections::HashMap<String, Vec<(String, Vec<(i32, String)>)>> {
    use std::collections::HashMap;
    let lines: Vec<&str> = input.lines().collect();
    let mut out: HashMap<String, Vec<(String, Vec<(i32, String)>)>> = HashMap::new();
    let mut in_blocks = false;
    let mut name: Option<String> = None;
    let mut ents: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    let mut cur: Option<(String, Vec<(i32, String)>)> = None;

    let mut i = 0usize;
    while i + 1 < lines.len() {
        let code: i32 = match lines[i].trim().parse() {
            Ok(c) => c,
            Err(_) => { i += 2; continue; }
        };
        let value = lines[i + 1].trim_end_matches('\r').trim_start();
        i += 2;

        if !in_blocks {
            if code == 2 && value.eq_ignore_ascii_case("BLOCKS") { in_blocks = true; }
            continue;
        }
        if code == 0 && value.eq_ignore_ascii_case("ENDSEC") { break; }

        if code == 0 {
            if let Some(e) = cur.take() { ents.push(e); }
            let upper = value.to_ascii_uppercase();
            match upper.as_str() {
                "BLOCK" => { name = None; ents.clear(); }
                "ENDBLK" => {
                    if let Some(n) = name.take() {
                        if !ents.is_empty() { out.insert(n, std::mem::take(&mut ents)); }
                    }
                }
                _ => cur = Some((upper, Vec::new())),
            }
            continue;
        }
        // Inside the BLOCK header (no entity open yet) code 2 is the block name.
        if cur.is_none() && code == 2 && name.is_none() {
            name = Some(value.to_string());
            continue;
        }
        if let Some((_, pairs)) = cur.as_mut() {
            pairs.push((code, value.to_string()));
        }
    }
    out
}

fn ellipse_pt(cx: f64, cy: f64, a: f64, b: f64, rot: f64, t: f64) -> (f64, f64) {
    let cosr = rot.cos();
    let sinr = rot.sin();
    let px = a * t.cos();
    let py = b * t.sin();
    (cx + px * cosr - py * sinr, cy + px * sinr + py * cosr)
}

/// Cox–de Boor evaluation.
///
/// `u` must already be clamped via `domain_clamp(u, u_lo, u_hi)` by the
/// caller so the active span lookup can never under-run past the knot tail.
/// Returns the tessellated point at parameter `u`.  Closed-periodic splines
/// (ctrl.len() != knots.len() - degree - 1) degrade gracefully; the caller
/// is responsible for avoiding double-wrapping on periodic cases.
fn de_boor(u: f64, degree: i32, knots: &[f64], ctrl: &[(f64, f64)]) -> (f64, f64) {
    let n = ctrl.len();
    let k = degree as usize;
    // Find the active knot span i such that knots[i] <= u <= knots[i+1].
    // Because u ∈ [knots[k], knots[n]] (already clamped), the loop is bounded.
    let mut s = k;
    for i in k..n {
        if i + 1 >= knots.len() { break; }
        if u >= knots[i] && u <= knots[i + 1] {
            s = i;
            break;
        }
        if u < knots[i] { s = i.saturating_sub(1).max(k); break; }
    }
    // Guard: `s` must allow `s - k..=s` to index ctrl safely.
    let s = s.clamp(k, n - 1);
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
