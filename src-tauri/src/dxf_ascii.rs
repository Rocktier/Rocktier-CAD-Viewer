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
    parse_and_build_with_progress(input, parse_time_ms, was_dwg, convert_ms, &|_| {})
}

/// Same as `parse_and_build`, but reports parse progress as `0.0..=1.0`.
///
/// A 20 MB DXF is ~2.5 M lines and several seconds of work; without this the
/// UI can only spin.  The callback is invoked at most every `PROGRESS_LINES`
/// lines so it stays free on the hot path.
pub fn parse_and_build_with_progress(
    input: &str,
    parse_time_ms: u64,
    was_dwg: bool,
    convert_ms: u64,
    on_progress: &dyn Fn(f32),
) -> Result<(SceneMeta, Vec<u8>), String> {
    let mut st = State::new();
    st.parse(input, on_progress);
    on_progress(1.0);
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
}

/// Geometry sinks for one layout, indexed by layer.  Layout 0 is model space;
/// paper-space layouts get their own buffers (and their own extents) so the
/// fit-to-view box of a sheet never leaks into the plan.
#[derive(Default)]
struct LayoutBuf {
    name: String,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    lines: Vec<Vec<u8>>,
    points: Vec<Vec<u8>>,
    tris: Vec<Vec<u8>>,
    /// WIPEOUT masks: filled *after* everything else, in the background
    /// colour, so they actually hide what they cover.
    masks: Vec<Vec<u8>>,
}

impl LayoutBuf {
    fn new(name: impl Into<String>) -> Self {
        LayoutBuf {
            name: name.into(),
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
            lines: Vec::new(),
            points: Vec::new(),
            tris: Vec::new(),
            masks: Vec::new(),
        }
    }

    /// Grow the per-layer sinks so index `li` is addressable.
    fn reserve(&mut self, li: usize) {
        if self.lines.len() <= li {
            self.lines.resize(li + 1, Vec::new());
            self.points.resize(li + 1, Vec::new());
            self.tris.resize(li + 1, Vec::new());
            self.masks.resize(li + 1, Vec::new());
        }
    }
}

/// A linetype dash pattern: `dashes` alternates on/off lengths in drawing
/// units (group 49 of the LTYPE record; negative = gap), `scale` already folds
/// in $LTSCALE and the entity's own code 48.
struct Dash {
    pattern: Vec<f64>,
    scale: f64,
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
    /// LAYER table records: name → (ACI colour, is-off, linetype).
    layer_table: std::collections::HashMap<String, (u8, bool, String)>,
    /// LAYER table names in file order (a HashMap would shuffle the panel).
    layer_names: Vec<String>,
    /// LTYPE table: name → dash pattern (group 49 values).
    ltype_defs: std::collections::HashMap<String, Vec<f64>>,
    /// LAYER record linetype (group 6): layer name → linetype name.
    layer_ltype: std::collections::HashMap<String, String>,
    /// Global linetype scale ($LTSCALE); 1.0 when the header omits it.
    ltscale: f64,
    /// STYLE table: text style name → width factor (group 41).  Chinese
    /// drawings lean on condensed styles (0.5…0.8) for labels and dimension
    /// text; drawing them at 1.0 makes every such note visibly too wide.
    style_width: std::collections::HashMap<String, f64>,
    /// BLOCKS section: block name → [(entity type, group-code pairs)].
    blocks: std::collections::HashMap<String, Vec<(String, Vec<(i32, String)>)>>,
    /// Active block-insert transform: (tx, ty, scale, cos θ, sin θ).
    xf: (f64, f64, f64, f64, f64),
    xf_depth: u32,
    /// Geometry per layout (0 = model space, 1.. = paper-space sheets).
    layouts: Vec<LayoutBuf>,
    cur_layout: usize,
    max_valid: f64,
    texts: Vec<TextItem>,
    current_layer: usize,
    /// Entity-level colour override (group code 62) as RGBA, when present;
    /// `(0,0,0,0)` means "background colour" (ACI 7 / auto).
    cur_color: Option<(u8, u8, u8, u8)>,
    /// Dashes in effect for the entity being parsed (None = continuous).
    cur_dash: Option<Dash>,
    /// Arc-length consumed by dashes so far, in pre-transform drawing units.
    dash_phase: f64,
    /// Active block-insert context: the INSERT's layer index and its resolved
    /// colour.  Per ObjectARX semantics, block content on the special layer
    /// "0" inherits the insert's layer, and BYBLOCK (ACI 0) content inherits
    /// the insert's colour.
    blk_layer: Option<usize>,
    blk_color: Option<(u8, u8, u8, u8)>,
    poly: Option<PolyAcc>,
    /// Raw entities, kept so paper-space viewports can re-project model space.
    /// Only filled when `keep_raw` is set — see `State::new` / `parse`.
    model_ents: Vec<(String, Vec<(i32, String)>)>,
    /// True when some paper-space block has a VIEWPORT, i.e. when replaying
    /// model space is actually needed.
    keep_raw: bool,
    /// Entities tagged group 67=1 in ENTITIES (the active paper layout).
    paper_ents: Vec<(String, Vec<(i32, String)>)>,
    /// Viewport clip rectangle (paper space) applied to every emitted segment.
    clip: Option<(f64, f64, f64, f64)>,
    /// Strided reserve of coordinates, for the robust "core" extents.
    samples: Vec<(f32, f32)>,
    sample_seen: u64,
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
            ltype_defs: std::collections::HashMap::new(),
            layer_ltype: std::collections::HashMap::new(),
            ltscale: 1.0,
            style_width: std::collections::HashMap::new(),
            blocks: std::collections::HashMap::new(),
            xf: (0.0, 0.0, 1.0, 1.0, 0.0),
            xf_depth: 0,
            layouts: vec![LayoutBuf::new("Model")],
            cur_layout: 0,
            max_valid: 0.0,
            texts: Vec::new(),
            current_layer: 0,
            cur_color: None,
            cur_dash: None,
            dash_phase: 0.0,
            blk_layer: None,
            blk_color: None,
            poly: None,
            model_ents: Vec::new(),
            keep_raw: false,
            paper_ents: Vec::new(),
            clip: None,
            samples: Vec::new(),
            sample_seen: 0,
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
        let (aci, off, _) = self
            .layer_table
            .get(name)
            .cloned()
            .unwrap_or((7, false, String::new()));
        let (r, g, b) = aci_to_rgb(aci);
        self.layers.push(LayerRec {
            name: name.to_string(),
            r, g, b, auto: aci == 7, off,
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
            let li = self.cur_layout;
            let l = &mut self.layouts[li];
            if x < l.min_x { l.min_x = x; }
            if y < l.min_y { l.min_y = y; }
            if x > l.max_x { l.max_x = x; }
            if y > l.max_y { l.max_y = y; }
            let m = x.abs().max(y.abs());
            if m > self.max_valid && m < 1.0e12 { self.max_valid = m; }
            // Reservoir sample for the outlier-resistant "core" extents: the
            // 1-in-16 stride keeps this bounded on huge drawings.  Model space
            // only — a sheet frame is not an outlier.
            if self.cur_layout == 0 {
                self.sample_seen += 1;
                if self.sample_seen % 16 == 0 && self.samples.len() < 400_000 {
                    self.samples.push((x as f32, y as f32));
                }
            }
        }
    }

    /// Emit one segment: transform, clip to the active viewport, record
    /// extents, append the vertices.
    fn emit_seg(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        let (x1, y1) = self.apply_xf(x1, y1);
        let (x2, y2) = self.apply_xf(x2, y2);
        let (x1, y1, x2, y2) = match self.clip {
            Some(rect) => match clip_segment(x1, y1, x2, y2, rect) {
                Some(s) => s,
                None => return,
            },
            None => (x1, y1, x2, y2),
        };
        self.set_extents(x1, y1);
        self.set_extents(x2, y2);
        let li = self.current_layer;
        let (r, g, b, a) = self.cur_rgba();
        let lay = &mut self.layouts[self.cur_layout];
        lay.reserve(li);
        append_line(&mut lay.lines[li], x1, y1, x2, y2, r, g, b, a);
    }

    /// Dash-aware line: splits the segment according to the active linetype.
    ///
    /// Dashes are measured in pre-transform units, so a scaled block reference
    /// scales its dashes too — the same thing AutoCAD does, and the reason
    /// layer HIDDEN / 轴线 styles read correctly in a block.
    fn push_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        let Some(dash) = self.cur_dash.as_ref() else {
            self.emit_seg(x1, y1, x2, y2);
            return;
        };
        let pattern: Vec<f64> = {
            // Group 49 writes gaps as negative numbers; a trailing gap may be
            // omitted, which would flip the parity of every later entry.
            let mut p: Vec<f64> = dash.pattern.iter().map(|v| v.abs() * dash.scale).collect();
            p.retain(|v| *v > 1e-9);
            if p.len() % 2 == 1 { p.push(0.0); }
            p
        };
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len = (dx * dx + dy * dy).sqrt();
        let period: f64 = pattern.iter().sum();
        if len < 1e-12 || period <= 1e-9 || len / period > 800.0 {
            // Continuous, or a pattern so fine it would emit millions of
            // dashes (unset $LTSCALE against a paper-unit ISO pattern).
            self.emit_seg(x1, y1, x2, y2);
            return;
        }
        let ux = dx / len;
        let uy = dy / len;
        let mut pos = self.dash_phase % period;
        let mut idx = 0usize;
        while idx < pattern.len() && pos >= pattern[idx] {
            pos -= pattern[idx];
            idx = (idx + 1) % pattern.len();
        }
        let mut travelled = 0.0f64;
        while travelled < len - 1e-12 {
            let step = (pattern[idx] - pos).max(0.0).min(len - travelled);
            if idx % 2 == 0 && step > 0.0 {
                let t0 = travelled;
                let t1 = travelled + step;
                self.emit_seg(x1 + ux * t0, y1 + uy * t0, x1 + ux * t1, y1 + uy * t1);
            }
            if step <= 0.0 { break; }
            travelled += step;
            pos = 0.0;
            idx = (idx + 1) % pattern.len();
        }
        self.dash_phase += len;
    }

    fn push_point(&mut self, x: f64, y: f64) {
        let (x, y) = self.apply_xf(x, y);
        if let Some((cx0, cy0, cx1, cy1)) = self.clip {
            if x < cx0 || x > cx1 || y < cy0 || y > cy1 {
                return;
            }
        }
        self.set_extents(x, y);
        let li = self.current_layer;
        let (r, g, b, a) = self.cur_rgba();
        let lay = &mut self.layouts[self.cur_layout];
        lay.reserve(li);
        append_point(&mut lay.points[li], x, y, r, g, b, a);
    }

    /// Mask triangle (WIPEOUT).  Stored in its own stream because it must be
    /// painted last; the renderer draws it with the background colour.
    fn push_mask(&mut self, a: (f64, f64), b: (f64, f64), c: (f64, f64)) {
        let a = self.apply_xf(a.0, a.1);
        let b = self.apply_xf(b.0, b.1);
        let c = self.apply_xf(c.0, c.1);
        if let Some((cx0, cy0, cx1, cy1)) = self.clip {
            let inside = |p: (f64, f64)| p.0 >= cx0 && p.0 <= cx1 && p.1 >= cy0 && p.1 <= cy1;
            if !(inside(a) && inside(b) && inside(c)) {
                return;
            }
        }
        let li = self.current_layer;
        let lay = &mut self.layouts[self.cur_layout];
        lay.reserve(li);
        append_tri(&mut lay.masks[li], a, b, c, 0, 0, 0, 0);
    }

    /// Filled triangle (HATCH / SOLID / TRACE / 3DFACE).
    fn push_tri(&mut self, a: (f64, f64), b: (f64, f64), c: (f64, f64)) {
        let a = self.apply_xf(a.0, a.1);
        let b = self.apply_xf(b.0, b.1);
        let c = self.apply_xf(c.0, c.1);
        if let Some((cx0, cy0, cx1, cy1)) = self.clip {
            // Triangles are dropped rather than clipped when they straddle the
            // viewport edge: a hole at the sheet border beats a wrong shape.
            let inside = |p: (f64, f64)| p.0 >= cx0 && p.0 <= cx1 && p.1 >= cy0 && p.1 <= cy1;
            if !(inside(a) && inside(b) && inside(c)) {
                return;
            }
        }
        self.set_extents(a.0, a.1);
        self.set_extents(b.0, b.1);
        self.set_extents(c.0, c.1);
        let li = self.current_layer;
        let (cr, cg, cb, ca) = self.cur_rgba();
        let lay = &mut self.layouts[self.cur_layout];
        lay.reserve(li);
        append_tri(&mut lay.tris[li], a, b, c, cr, cg, cb, ca);
    }

    fn parse(&mut self, input: &str, on_progress: &dyn Fn(f32)) {
        // TABLES/LAYER, LTYPE and BLOCKS are needed up front: layer colours and
        // linetypes when the first entity referencing a layer is parsed, blocks
        // on every INSERT, dashes for every dashed entity.
        let (layer_table, layer_names, layer_ltype) = scan_layer_table(input);
        self.layer_table = layer_table;
        self.layer_names = layer_names;
        self.layer_ltype = layer_ltype;
        self.ltype_defs = scan_ltype_table(input);
        self.ltscale = scan_header_var(input, "LTSCALE", 1.0);
        self.style_width = scan_style_table(input);
        self.blocks = scan_blocks(input);

        // Register every LAYER record up front, so the panel lists the drawing's
        // layers in file order even when one currently holds no geometry.
        for name in std::mem::take(&mut self.layer_names) {
            self.layer_idx(&name);
        }

        // Set up layer "0" by default.
        self.current_layer = self.layer_idx("0");

        // Replaying model space into viewports costs a second copy of every raw
        // entity, so it is only paid for when a sheet actually uses one.
        // 判据还要覆盖「ENTITIES 里带 67=1 的 VIEWPORT」这种情况：那时没有纸空间块，
        // 旧判据下 model_ents 恒为空，视口只能画出空框。用一次文本粗扫兜住，
        // 代价仅是这类图纸多保留一份原始实体。
        self.keep_raw = self
            .blocks
            .iter()
            .any(|(n, e)| is_paper_block(n) && e.iter().any(|(t, _)| is_viewport(t)))
            || input.contains("\nVIEWPORT\n");

        let lines: Vec<&str> = input.lines().collect();
        let total = lines.len().max(1) as f32;
        // Report at most every PROGRESS_LINES lines — a 2.5 M-line DXF would
        // otherwise spend more time calling back than parsing.
        const PROGRESS_LINES: usize = 100_000;
        let mut next_report = PROGRESS_LINES;
        let mut i = 0usize;
        let mut in_entities = false;
        let mut current_entity: Option<&str> = None;
        let mut buf: Vec<(i32, String)> = Vec::new();
        on_progress(0.02);

        while i + 1 < lines.len() {
            if i >= next_report {
                on_progress((i as f32 / total).min(1.0));
                next_report = i + PROGRESS_LINES;
            }
            let code_line = lines[i].trim();
            let value_line = lines[i + 1].trim_end_matches('\r').trim_start();
            i += 2;

            let code: i32 = match code_line.parse() {
                Ok(c) => c,
                Err(_) => {
                    if code_line.eq_ignore_ascii_case("EOF") {
                        // Flush the pending entity, then stop the scan (the
                        // paper-layout pass below still has to run).
                        if let Some(etype) = current_entity {
                            self.dispatch_model(etype, &buf);
                        }
                        break;
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
        self.build_paper_layouts();
    }

    /// Top-level entity dispatch.  Model space fills layout 0; entities tagged
    /// group 67 = 1 belong to the *active* paper layout and are kept aside
    /// until `build_paper_layouts` knows which sheet that is.
    fn dispatch_model(&mut self, entity_type: &str, buf: &[(i32, String)]) {
        if buf.iter().any(|(c, v)| *c == 67 && v.trim() == "1") {
            self.paper_ents.push((entity_type.to_string(), buf.to_vec()));
            return;
        }
        if self.keep_raw {
            // Only kept when a paper layout actually has a VIEWPORT to project
            // model space into; otherwise this doubles the parse memory.
            self.model_ents.push((entity_type.to_string(), buf.to_vec()));
        }
        self.dispatch(entity_type, buf);
    }

    /// One layout per non-empty `*Paper_Space*` block, with model space
    /// projected through every VIEWPORT found on it.
    ///
    /// ponytail: the sheet's *name* comes from the block name (`*Paper_Space`
    /// → "Layout 1"), not from the LAYOUT objects in the OBJECTS section — the
    /// object → block-record → block chain needs handle resolution that buys
    /// nothing a viewer shows.  Swap in the LAYOUT name if a user ever cares.
    fn build_paper_layouts(&mut self) {
        let mut names: Vec<String> = self
            .blocks
            .iter()
            .filter(|(n, e)| is_paper_block(n) && !e.is_empty())
            .map(|(n, _)| n.clone())
            .collect();
        names.sort_by_key(|n| paper_block_rank(n));

        let model_ents = std::mem::take(&mut self.model_ents);
        let paper_ents = std::mem::take(&mut self.paper_ents);
        let n_sheets = names.len();

        for name in names {
            let Some(ents) = self.blocks.get(&name).cloned() else { continue };
            self.begin_layout(paper_layout_name(&name));
            for (etype, pairs) in &ents {
                if is_viewport(etype) {
                    self.emit_viewport(pairs, &model_ents);
                } else {
                    self.dispatch(etype, pairs);
                }
            }
        }

        // No sheet carried geometry, yet ENTITIES held group 67 = 1 rows (the
        // active layout written outside its block): give them a sheet of their
        // own rather than dropping them.  Doing both would double the drawing,
        // hence the `n_sheets == 0` guard.
        if n_sheets == 0 && !paper_ents.is_empty() {
            self.begin_layout("Layout 1".to_string());
            for (etype, pairs) in &paper_ents {
                if is_viewport(etype) {
                    self.emit_viewport(pairs, &model_ents);
                } else {
                    self.dispatch(etype, pairs);
                }
            }
        }
        self.cur_layout = 0;
    }

    fn begin_layout(&mut self, name: String) {
        self.layouts.push(LayoutBuf::new(name));
        self.cur_layout = self.layouts.len() - 1;
        self.clip = None;
        self.dash_phase = 0.0;
    }

    /// Project model space into one paper-space viewport.
    ///
    /// VIEWPORT geometry: 10/20 = centre in paper units, 40/41 = window size,
    /// 45 = view height in model units, 12/22 = view centre in model space.
    /// Id 1 is the "overall" viewport that *is* the sheet — skipping it keeps a
    /// second, differently-scaled copy of the plan off the paper.
    fn emit_viewport(&mut self, buf: &[(i32, String)], model_ents: &[(String, Vec<(i32, String)>)]) {
        if i(buf, 69, 0) == 1 {
            return;
        }
        let (cx, cy) = (f(buf, 10, f64::NAN), f(buf, 20, f64::NAN));
        let (w, h) = (f(buf, 40, 0.0), f(buf, 41, 0.0));
        let view_h = f(buf, 45, 0.0);
        let (mx, my) = (f(buf, 12, f64::NAN), f(buf, 22, f64::NAN));
        if !(cx.is_finite() && cy.is_finite() && mx.is_finite() && my.is_finite()) {
            return;
        }
        if !(w > 0.0 && h > 0.0 && view_h > 0.0) {
            return;
        }
        let s = h / view_h;
        if !s.is_finite() || s <= 0.0 {
            return;
        }
        let saved = (self.xf, self.clip);
        self.xf = (cx - mx * s, cy - my * s, s, 1.0, 0.0);
        self.clip = Some((cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0));
        for (etype, pairs) in model_ents {
            self.dispatch(etype, pairs);
        }
        self.xf = saved.0;
        self.clip = saved.1;
    }

    fn dispatch(&mut self, entity_type: &str, buf: &[(i32, String)]) {
        // Group 60 = 1 marks an entity invisible.  Dynamic blocks keep every
        // visibility-state variant in the block definition and flag the
        // inactive ones this way — LibreDWG's DXF output carries 2 700+ such
        // entities on a single Chinese residential plan, and drawing them
        // superimposes several sizes of the same fixture into an unreadable
        // tangle.  AutoCAD never shows them, so neither do we.
        if i(buf, 60, 0) == 1 {
            return;
        }
        // Dash phase is per entity: without this reset every dashed line would
        // resume wherever the previous one stopped.
        self.dash_phase = 0.0;
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
            // An attribute *value* (what a user reads) — world coordinates, so
            // it needs no insert transform.  Justification lives in 72/74 here,
            // where TEXT uses 72/73.
            "ATTRIB" => self.parse_attrib(buf),
            // The attribute *template* inside a block definition: drawing it
            // would print the placeholder on top of the real value.
            "ATTDEF" => {}
            "POINT" => self.parse_point(buf),
            "INSERT" => self.expand_insert(buf),
            "HATCH" => self.parse_hatch(buf),
            "SOLID" | "TRACE" | "3DFACE" => self.parse_solid(buf),
            "WIPEOUT" => self.parse_wipeout(buf),
            "LEADER" => self.parse_leader(buf),
            "DIMENSION" => self.expand_dimension(buf),
            "RAY" | "XLINE" => {
                // An infinite construction line: its two definition points are
                // all a viewer has, so draw that much.
                self.skipped += 1;
                self.parse_dimlike(buf);
            }
            "VIEWPORT" => {} // only meaningful while building a paper layout
            _ => {} // silently ignore unsupported entities (MULTILEADER, IMAGE, …)
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
            None => {
                // `_Dot` is the dot marker TianZheng / AutoCAD Architecture
                // sprinkles along dimension and furniture symbols.  Its block
                // never travels with the DXF, and all it draws is a single
                // point — emitting one beats counting thousands of "undrawn
                // entities" in the status bar.
                if name.eq_ignore_ascii_case("_Dot") {
                    self.push_point(ix, iy);
                    return;
                }
                self.skipped += 1; // unresolved / xref block
                return;
            }
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
        let saved_blk = (self.blk_layer, self.blk_color);
        // Establish the insert context that BYBLOCK content and layer-"0"
        // content inherit from (see `set_layer`).
        self.blk_layer = Some(self.current_layer);
        self.blk_color = Some(self.cur_rgba());
        for (etype, pairs) in &ents {
            self.dispatch(etype, pairs);
        }
        self.xf_depth -= 1;
        self.xf = (tx, ty, os, oc, osn);
        self.current_layer = saved_layer;
        self.cur_color = saved_color;
        self.blk_layer = saved_blk.0;
        self.blk_color = saved_blk.1;
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
            // Not counted as skipped — the dimension lines *are* drawn, and
            // counting every one of them made the status bar claim tens of
            // thousands of undrawn entities on a plan where none were missing.
            self.parse_dimlike(buf);
            return;
        }
        self.set_layer(buf);
        self.expand_block(&name, 0.0, 0.0, 1.0, 0.0);
    }

    fn sample_arc_bulge(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, bulge: f64) {
        for w in arc_points(x1, y1, x2, y2, bulge).windows(2) {
            self.push_line(w[0].0, w[0].1, w[1].0, w[1].1);
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
        // 组码 71 的 degree 未校验：负/超大值会让 knots[k] 越界 panic，
        // 而 release profile 是 panic="abort"，等于整个应用闪退。这里钳到合法区间。
        let degree = buf.iter().find(|(c, _)| *c == 71)
            .and_then(|(_, v)| v.parse::<i64>().ok())
            .unwrap_or(3) as i32;
        let ctrl: Vec<(f64, f64)> = extract_pairs(buf, 10, 20).collect();
        let knots: Vec<f64> = buf.iter().filter(|(c, _)| *c == 40)
            .filter_map(|(_, v)| v.parse().ok()).collect();
        if ctrl.len() < 2 { return; }
        self.set_layer(buf);
        let degree = degree.clamp(1, 32);
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

    /// Effective horizontal scale for a text entity.
    ///
    /// TEXT/ATTRIB carry the width factor in group 41; MTEXT uses group 41 for
    /// the reference column width instead, so there only the style's factor
    /// applies.
    fn width_factor(&self, buf: &[(i32, String)], is_mtext: bool) -> f64 {
        if !is_mtext {
            let own = f(buf, 41, f64::NAN);
            if own.is_finite() && own > 1e-6 {
                return own;
            }
        }
        match self.style_width.get(s(buf, 7)) {
            Some(&w) if w.is_finite() && w > 1e-6 => w,
            _ => 1.0,
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
        if let Some((cx0, cy0, cx1, cy1)) = self.clip {
            if x < cx0 || x > cx1 || y < cy0 || y > cy1 {
                return;
            }
        }
        let wf = self.width_factor(buf, is_mtext);
        self.texts.push(TextItem {
            // Layout index (0 = model space), not a layer index.
            layout: self.cur_layout as u32,
            layer: li as u32,
            x: x as f32,
            y: y as f32,
            h: (f(buf, 40, 1.0) * sx) as f32,
            wf: wf as f32,
            rot: (rot + sxn.atan2(cx).to_degrees()) as f32,
            r, g, b, a,
            ha: ha as u8,
            va: va as u8,
            text: display,
        });
    }

    /// ATTRIB — the attribute *value* that follows an INSERT carrying
    /// attributes (group 66 = 1).  Group 1 is the value, 72/74 the
    /// justification (plain TEXT uses 72/73), and the position is the
    /// attribute's own: DXF writes it already placed in the enclosing space,
    /// so this is why the value must not go through the INSERT transform.
    fn parse_attrib(&mut self, buf: &[(i32, String)]) {
        let display = decode_mtext(s(buf, 1));
        if display.trim().is_empty() {
            return;
        }
        self.set_layer(buf);
        let li = self.current_layer;
        let (r, g, b, a) = self.cur_rgba();
        let (x, y) = self.apply_xf(f(buf, 10, 0.0), f(buf, 20, 0.0));
        if let Some((cx0, cy0, cx1, cy1)) = self.clip {
            if x < cx0 || x > cx1 || y < cy0 || y > cy1 {
                return;
            }
        }
        let (_, _, sx, cx, sxn) = self.xf;
        let wf = self.width_factor(buf, false);
        self.texts.push(TextItem {
            layout: self.cur_layout as u32,
            layer: li as u32,
            x: x as f32,
            y: y as f32,
            h: (f(buf, 40, 1.0) * sx) as f32,
            wf: wf as f32,
            rot: (f(buf, 50, 0.0) + sxn.atan2(cx).to_degrees()) as f32,
            r, g, b, a,
            ha: i(buf, 72, 0) as u8,
            va: i(buf, 74, 0) as u8,
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

    // --------------------------------------------------------------- HATCH

    /// HATCH: boundary paths, plus a solid fill when group 70 bit 1 is set.
    ///
    /// The pattern definition that follows the paths is skipped, so a pattern
    /// hatch draws as its boundary — the honest subset (AutoCAD's own default
    /// for "regen without patterns" looks the same).  Islands are not
    /// subtracted: a solid hatch with a hole fills the hole.  Both are visible
    /// decisions, not missing geometry.
    fn parse_hatch(&mut self, buf: &[(i32, String)]) {
        let solid = i(buf, 70, 0) & 1 != 0;
        let name = s(buf, 2);
        self.set_layer(buf);

        // The outline is always drawn — that is what a drawing sees when a
        // pattern cannot be regenerated, and it keeps unmapped patterns useful.
        let mut loops: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut cur = Cursor::new(buf);
        while let Some((code, val)) = cur.next() {
            if code != 91 {
                continue;
            }
            // 与环内其它计数一样要限流：损坏文件里 91 可能是 4e9
            let paths = val.trim().parse::<usize>().unwrap_or(0).min(4096);
            for _ in 0..paths {
                if let Some(loop_pts) = cur.boundary_path() {
                    loops.push(loop_pts);
                }
            }
            break;
        }
        for l in &loops {
            self.emit_loop_outline(l);
        }

        if solid {
            self.fill_loops(&loops);
            return;
        }
        // Patterned hatch: regenerate the pattern lines defined by the entity
        // (53/43/44/45/46/79/49), scaled by 41 and rotated by 52.
        let lines = cur.pattern_lines();
        if lines.is_empty() {
            let _ = name; // pattern definition absent (rare) — outline only
            return;
        }
        let angle = cur.angle_deg;
        let scale = cur.pattern_scale;
        for pl in lines {
            self.emit_pattern_line(&loops, &pl, angle, scale);
        }
    }

    fn emit_loop_outline(&mut self, pts: &[(f64, f64)]) {
        if pts.len() < 2 {
            return;
        }
        for w in pts.windows(2) {
            self.push_line(w[0].0, w[0].1, w[1].0, w[1].1);
        }
        // A path is closed by construction; draw the closing edge explicitly.
        let (a, b) = (pts[pts.len() - 1], pts[0]);
        if (a.0 - b.0).abs() > 1e-9 || (a.1 - b.1).abs() > 1e-9 {
            self.push_line(a.0, a.1, b.0, b.1);
        }
    }

    /// Solid fill of a hatch, honouring island paths (holes).
    ///
    /// One loop → ear clipping (crisp, few triangles).  Several loops → an
    /// even-odd scanline fill, because the polygon-with-holes triangulation is
    /// where hand-rolled code usually goes wrong; a scanline cannot get the
    /// winding rules wrong, it only costs more triangles.
    fn fill_loops(&mut self, loops: &[Vec<(f64, f64)>]) {
        if loops.is_empty() {
            return;
        }
        if loops.len() == 1 {
            let pts = &loops[0];
            // 耳切三角化是 O(n²)（凹多边形更差），2 万点会让图纸几分钟打不开
            if pts.len() < 3 || pts.len() > 2_000 {
                return;
            }
            let tris = triangulate(pts);
            if tris.is_empty() {
                for k in 1..pts.len() - 1 {
                    self.push_tri(pts[0], pts[k], pts[k + 1]);
                }
                return;
            }
            for [i, j, k] in tris {
                self.push_tri(pts[i], pts[j], pts[k]);
            }
            return;
        }
        let (min_x, min_y, max_x, max_y) = loops_bounds(loops);
        let height = max_y - min_y;
        if !(height > 0.0) {
            return;
        }
        let steps = ((height / 1.0).min(192.0)).max(4.0) as usize;
        let dy = height / steps as f64;
        for s in 0..=steps {
            let y = min_y + dy * s as f64;
            for (t0, t1) in even_odd_spans(loops, (0.0, y), (1.0, 0.0), min_x, max_x) {
                if t1 - t0 <= 1e-9 {
                    continue;
                }
                self.push_tri((t0, y), (t1, y), (t0, y + dy));
                self.push_tri((t1, y), (t1, y + dy), (t0, y + dy));
            }
        }
    }

    /// One pattern definition line, clipped to the boundary by the even-odd
    /// rule — which is also what makes island paths leave holes in the hatch.
    fn emit_pattern_line(
        &mut self,
        loops: &[Vec<(f64, f64)>],
        pl: &PatternLine,
        hatch_angle: f64,
        hatch_scale: f64,
    ) {
        let declared = if hatch_scale.is_finite() && hatch_scale > 0.0 { hatch_scale } else { 1.0 };
        let rot = hatch_angle.to_radians();
        let (cr, sr) = (rot.cos(), rot.sin());
        let rot_pt = |p: (f64, f64)| (p.0 * cr - p.1 * sr, p.0 * sr + p.1 * cr);

        let dir = rot_pt((pl.angle.to_radians().cos(), pl.angle.to_radians().sin()));
        // The pattern line's offset vector separates adjacent parallel lines.
        let off_raw = rot_pt(pl.offset);
        let spacing_raw = (off_raw.0 * off_raw.0 + off_raw.1 * off_raw.1).sqrt();
        if spacing_raw < 1e-9 {
            return;
        }
        let (min_x, min_y, max_x, max_y) = loops_bounds(loops);
        // Pattern scale (group 41): AutoCAD expects the definition to be
        // unscaled, but some writers — LibreDWG among them — bake the scale
        // into it.  Honouring the declared scale then leaves one or two lines
        // on the hatch (3.6 m spacing instead of 90 mm), so fall back to the
        // definition as written whenever it is the only reading that produces
        // an actual texture.
        let scale = {
            let nx0 = off_raw.0 / spacing_raw;
            let ny0 = off_raw.1 / spacing_raw;
            let spread = [(min_x, min_y), (max_x, min_y), (max_x, max_y), (min_x, max_y)]
                .iter()
                .map(|c| c.0 * nx0 + c.1 * ny0)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
            let width = spread.1 - spread.0;
            let lines_at = |sc: f64| width / (spacing_raw * sc);
            if lines_at(declared) < 2.0 && lines_at(1.0) >= 2.0 {
                1.0
            } else {
                declared
            }
        };
        let off = (off_raw.0 * scale, off_raw.1 * scale);
        let spacing = spacing_raw * scale;
        let base = rot_pt((pl.base.0 * scale, pl.base.1 * scale));
        // Project the boundary box onto the *offset* direction: by definition a
        // hatch pattern's offset vector is perpendicular to its lines, so this
        // is the normal to measure along.  (Rotating `off` by 90° here picks a
        // completely different family of lines that misses the boundary.)
        let nx = off.0 / spacing;
        let ny = off.1 / spacing;
        let corners = [
            (min_x, min_y),
            (max_x, min_y),
            (max_x, max_y),
            (min_x, max_y),
        ];
        let (mut n_min, mut n_max) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in corners {
            let d = (c.0 - base.0) * nx + (c.1 - base.1) * ny;
            n_min = n_min.min(d);
            n_max = n_max.max(d);
        }
        let k0 = (n_min / spacing).floor() as i64;
        let k1 = (n_max / spacing).ceil() as i64;
        if k1 - k0 > 2_000 {
            return; // a pattern finer than the drawing is not readable anyway
        }
        let dashes: Vec<f64> = pl.dashes.iter().map(|d| d.abs() * scale).collect();
        for k in k0..=k1 {
            let origin = (
                base.0 + off.0 * k as f64,
                base.1 + off.1 * k as f64,
            );
            // The parameter range must be measured from *this* line's origin —
            // using the family base would slide the window along the direction
            // and silently cut every span away.
            let (mut t_min, mut t_max) = (f64::INFINITY, f64::NEG_INFINITY);
            for c in corners {
                let t = (c.0 - origin.0) * dir.0 + (c.1 - origin.1) * dir.1;
                t_min = t_min.min(t);
                t_max = t_max.max(t);
            }
            for (t0, t1) in even_odd_spans(loops, origin, dir, t_min, t_max) {
                if t1 - t0 <= 1e-9 {
                    continue;
                }
                if dashes.iter().all(|d| *d <= 1e-9) {
                    self.push_line(
                        origin.0 + dir.0 * t0,
                        origin.1 + dir.1 * t0,
                        origin.0 + dir.0 * t1,
                        origin.1 + dir.1 * t1,
                    );
                    continue;
                }
                // Dashed pattern lines (ANSI31 is solid, but custom ones dash).
                let mut t = t0;
                let mut idx = 0usize;
                let period: f64 = dashes.iter().sum();
                if period <= 1e-9 {
                    continue;
                }
                let mut phase = (t0 - t_min).rem_euclid(period);
                while t < t1 - 1e-9 {
                    let step = (dashes[idx] - phase).max(0.0).min(t1 - t);
                    if idx % 2 == 0 && step > 0.0 {
                        self.push_line(
                            origin.0 + dir.0 * t,
                            origin.1 + dir.1 * t,
                            origin.0 + dir.0 * (t + step),
                            origin.1 + dir.1 * (t + step),
                        );
                    }
                    if step <= 0.0 {
                        break;
                    }
                    t += step;
                    phase = 0.0;
                    idx = (idx + 1) % dashes.len();
                }
            }
        }
    }

    /// WIPEOUT: a mask that hides whatever was drawn before it, so it renders
    /// as a background-coloured polygon in its own pass, above the geometry.
    fn parse_wipeout(&mut self, buf: &[(i32, String)]) {
        // A WIPEOUT is a raster image with no pixels: 10/20 is the insertion
        // point, 11/21 and 12/22 are the u/v vectors *of a single pixel*, and
        // the clip boundary (14/24) is in pixel coordinates — so the world
        // polygon is insertion + u·x + v·y.
        let base = (f(buf, 10, 0.0), f(buf, 20, 0.0));
        let u = (f(buf, 11, 1.0), f(buf, 21, 0.0));
        let v = (f(buf, 12, 0.0), f(buf, 22, 1.0));
        let pts: Vec<(f64, f64)> = extract_pairs(buf, 14, 24)
            .map(|(cx, cy)| (base.0 + u.0 * cx + v.0 * cy, base.1 + u.1 * cx + v.1 * cy))
            .collect();
        if pts.len() < 3 {
            return;
        }
        self.set_layer(buf);
        let tris = triangulate(&pts);
        if tris.is_empty() {
            for k in 1..pts.len() - 1 {
                self.push_mask(pts[0], pts[k], pts[k + 1]);
            }
            return;
        }
        for [i, j, k] in tris {
            self.push_mask(pts[i], pts[j], pts[k]);
        }
    }

    /// LEADER: a polyline through its vertices (group 76 counts them), usually
    /// with an arrow drawn as a separate SOLID at the first point.
    fn parse_leader(&mut self, buf: &[(i32, String)]) {
        let pts: Vec<(f64, f64)> = extract_pairs(buf, 10, 20).collect();
        if pts.len() < 2 {
            self.skipped += 1;
            return;
        }
        self.set_layer(buf);
        for w in pts.windows(2) {
            self.push_line(w[0].0, w[0].1, w[1].0, w[1].1);
        }
    }

    // ------------------------------------------------------- SOLID / 3DFACE

    /// SOLID / TRACE / 3DFACE: a filled triangle or quad.  The corners are
    /// listed in "Z" order (the 3rd and 4th are swapped), so the outline runs
    /// 1-2-4-3.  These are the arrows on dimensions, so filling them matters.
    fn parse_solid(&mut self, buf: &[(i32, String)]) {
        let p = |xc: i32, yc: i32| (f(buf, xc, f64::NAN), f(buf, yc, f64::NAN));
        let p1 = p(10, 20);
        let p2 = p(11, 21);
        let p3 = p(12, 22);
        let p4 = p(13, 23);
        if !(p1.0.is_finite() && p2.0.is_finite()) {
            return;
        }
        self.set_layer(buf);
        let mut poly = vec![p1, p2];
        if p4.0.is_finite() {
            poly.push(p4);
        } else if p3.0.is_finite() {
            // 三角形 SOLID 只写 10/11/12 三组坐标：没有 p4 时整个实体会被丢弃
            poly.push(p3);
        }
        // A triangle-shaped SOLID repeats the last corner; skip the duplicate so
        // the fan does not emit a degenerate triangle.
        let p3_distinct =
            p3.0.is_finite() && ((p3.0 - p4.0).abs() > 1e-9 || (p3.1 - p4.1).abs() > 1e-9);
        if p3_distinct {
            poly.push(p3);
        }
        if poly.len() < 3 {
            return;
        }
        for k in 1..poly.len() - 1 {
            self.push_tri(poly[0], poly[k], poly[k + 1]);
        }
    }

    fn set_layer(&mut self, buf: &[(i32, String)]) {
        // Group 8 = layer.  Block content sitting on the special layer "0"
        // inherits the *insert's* layer (ObjectARX semantics): dimension ticks
        // and anonymous-block geometry then toggle visibility with the
        // dimension/insert layer instead of piling up on layer "0".
        if let Some(name) = buf.iter().find(|(c, _)| *c == 8).map(|(_, v)| v.as_str()) {
            let li = self.layer_idx(name);
            self.current_layer = if name == "0" {
                self.blk_layer.unwrap_or(li)
            } else {
                li
            };
        }
        // Group code 62: 1..=255 is an explicit ACI colour, 0 = BYBLOCK
        // (inherit the inserting entity's colour), 256/absent = BYLAYER.
        // ACI 7 is the *background* colour (white on dark, black on light) —
        // painting it as literal white would make the drawing vanish on a
        // light canvas.
        self.cur_color = match buf
            .iter()
            .find(|(c, _)| *c == 62)
            .and_then(|(_, v)| v.parse::<i32>().ok())
        {
            Some(c) if (1..=255).contains(&c) => {
                if c == 7 {
                    Some((0, 0, 0, 0))
                } else {
                    let (r, g, b) = aci_to_rgb(c as u8);
                    Some((r, g, b, 255))
                }
            }
            Some(0) => self.blk_color, // BYBLOCK → insert's resolved colour
            _ => None,                 // absent / BYLAYER → layer colour
        };
        self.cur_dash = self.resolve_dash(buf);
    }

    /// Linetype in force for the entity being parsed.
    ///
    /// Group 6 names the style ("ByLayer" and an absent code both mean the
    /// layer's own), group 48 is the entity linetype scale, and $LTSCALE
    /// multiplies globally.  Returns None for a solid line.
    fn resolve_dash(&self, buf: &[(i32, String)]) -> Option<Dash> {
        let layer_ltype = || {
            self.layer_ltype
                .get(&self.layers[self.current_layer].name)
                .cloned()
                .unwrap_or_default()
        };
        let name = match buf.iter().find(|(c, _)| *c == 6).map(|(_, v)| v.trim()) {
            None => layer_ltype(),
            Some(n) if n.is_empty() || n.eq_ignore_ascii_case("ByLayer") => layer_ltype(),
            // BYBLOCK: the insert's linetype, which we do not track across the
            // block boundary — solid is the safe reading.
            Some(n) if n.eq_ignore_ascii_case("ByBlock") => return None,
            Some(n) => n.to_string(),
        };
        if name.is_empty() || name.eq_ignore_ascii_case("Continuous") {
            return None;
        }
        let pattern = self.ltype_defs.get(&name)?;
        if pattern.is_empty() {
            return None;
        }
        let e = f(buf, 48, 1.0);
        let entity_scale = if e.is_finite() && e > 0.0 { e } else { 1.0 };
        let global = if self.ltscale.is_finite() && self.ltscale > 0.0 {
            self.ltscale
        } else {
            1.0
        };
        Some(Dash { pattern: pattern.clone(), scale: entity_scale * global })
    }

    fn finalize(mut self, parse_time_ms: u64, was_dwg: bool, convert_ms: u64) -> Result<(SceneMeta, Vec<u8>), String> {
        // Pad every layout's box by 5% so the drawing is not glued to the
        // window edge; a layout whose geometry collapsed to a point gets a
        // small box around it rather than the origin (survey coordinates).
        for l in &mut self.layouts {
            if l.min_x.is_finite() && l.max_x >= l.min_x {
                let sx = (l.max_x - l.min_x).max(0.0);
                let sy = (l.max_y - l.min_y).max(0.0);
                let px = if sx > 0.0 { sx * 0.05 } else { 1.0 };
                let py = if sy > 0.0 { sy * 0.05 } else { 1.0 };
                l.min_x -= px;
                l.min_y -= py;
                l.max_x += px;
                l.max_y += py;
            } else {
                l.min_x = -1.0;
                l.min_y = -1.0;
                l.max_x = 1.0;
                l.max_y = 1.0;
            }
        }

        let core = self.core_box();
        let segments_total: u64 = self
            .layouts
            .iter()
            .map(|l| l.lines.iter().map(|b| (b.len() / 24) as u64).sum::<u64>())
            .sum();
        let text_count = self.texts.len() as u64;

        let mut geometry: Vec<u8> = Vec::new();
        let mut layouts: Vec<LayoutMeta> = Vec::new();
        for (i, buf) in self.layouts.iter().enumerate() {
            let mut m = Self::meta_for_layout(buf, &mut geometry);
            // Only model space has a meaningful "core" box: a sheet is padded
            // by its own frame and has no outliers to trim.
            if i == 0 {
                m.core_min_x = core.0;
                m.core_min_y = core.1;
                m.core_max_x = core.2;
                m.core_max_y = core.3;
            } else {
                m.core_min_x = m.min_x;
                m.core_min_y = m.min_y;
                m.core_max_x = m.max_x;
                m.core_max_y = m.max_y;
            }
            layouts.push(m);
        }

        let meta = SceneMeta {
            layers: self.layers.iter().map(|l| LayerInfo {
                name: l.name.clone(), r: l.r, g: l.g, b: l.b,
                auto: l.auto,
                // "Defpoints" is AutoCAD's non-plotting internals layer —
                // revision clouds get parked there, and showing them reads as
                // mystery arcs around the plan.  Start hidden (the layer stays
                // in the panel, one click reveals it), matching AutoCAD's
                // never-plot convention.
                off: l.off || l.name.eq_ignore_ascii_case("defpoints"),
            }).collect(),
            layouts,
            texts: self.texts,
            segments: segments_total,
            text_count,
            parse_ms: parse_time_ms,
            convert_ms,
            was_dwg,
            skipped: self.skipped,
        };
        Ok((meta, geometry))
    }

    /// Outlier-resistant model extent: the 0.5 %..99.5 % of sampled
    /// coordinates.  A handful of stray vertices (a legend parked 300 000
    /// units away, a coordinate blip) otherwise shrink the whole plan to a
    /// speck.  Offered to the user as "fit content" — never applied silently.
    fn core_box(&self) -> (f64, f64, f64, f64) {
        let model = &self.layouts[0];
        let full = (model.min_x, model.min_y, model.max_x, model.max_y);
        if self.samples.len() < 64 {
            return full;
        }
        let mut xs: Vec<f32> = self.samples.iter().map(|s| s.0).collect();
        let mut ys: Vec<f32> = self.samples.iter().map(|s| s.1).collect();
        let cmp = |a: &f32, b: &f32| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal);
        xs.sort_by(cmp);
        ys.sort_by(cmp);
        let q = |v: &[f32], t: f64| -> f64 {
            let i = (((v.len() - 1) as f64) * t).round().clamp(0.0, (v.len() - 1) as f64) as usize;
            v[i] as f64
        };
        let (x0, y0, x1, y1) = (
            q(&xs, 0.005),
            q(&ys, 0.005),
            q(&xs, 0.995),
            q(&ys, 0.995),
        );
        if x1 > x0 && y1 > y0 {
            (x0, y0, x1, y1)
        } else {
            full
        }
    }

    fn meta_for_layout(buf: &LayoutBuf, geometry: &mut Vec<u8>) -> LayoutMeta {
        let lines_start = geometry.len();
        let mut line_ranges: Vec<RangeSpec> = Vec::new();
        for (li, l) in buf.lines.iter().enumerate() {
            if l.is_empty() { continue; }
            line_ranges.push(RangeSpec {
                layer: li as u32,
                offset: (geometry.len() - lines_start) as u32,
                len: l.len() as u32,
            });
            geometry.extend_from_slice(l);
        }
        let lines_len = (geometry.len() - lines_start) as u32;

        let points_start = geometry.len();
        let mut point_ranges: Vec<RangeSpec> = Vec::new();
        for (li, p) in buf.points.iter().enumerate() {
            if p.is_empty() { continue; }
            point_ranges.push(RangeSpec {
                layer: li as u32,
                offset: (geometry.len() - points_start) as u32,
                len: p.len() as u32,
            });
            geometry.extend_from_slice(p);
        }
        let points_len = (geometry.len() - points_start) as u32;

        let masks_start = geometry.len();
        let mut mask_ranges: Vec<RangeSpec> = Vec::new();
        for (li, m) in buf.masks.iter().enumerate() {
            if m.is_empty() { continue; }
            mask_ranges.push(RangeSpec {
                layer: li as u32,
                offset: (geometry.len() - masks_start) as u32,
                len: m.len() as u32,
            });
            geometry.extend_from_slice(m);
        }
        let masks_len = (geometry.len() - masks_start) as u32;

        let tris_start = geometry.len();
        let mut tri_ranges: Vec<RangeSpec> = Vec::new();
        for (li, t) in buf.tris.iter().enumerate() {
            if t.is_empty() { continue; }
            tri_ranges.push(RangeSpec {
                layer: li as u32,
                offset: (geometry.len() - tris_start) as u32,
                len: t.len() as u32,
            });
            geometry.extend_from_slice(t);
        }
        let tris_len = (geometry.len() - tris_start) as u32;

        LayoutMeta {
            name: buf.name.clone(),
            min_x: buf.min_x,
            min_y: buf.min_y,
            max_x: buf.max_x,
            max_y: buf.max_y,
            core_min_x: buf.min_x,
            core_min_y: buf.min_y,
            core_max_x: buf.max_x,
            core_max_y: buf.max_y,
            lines_offset: lines_start as u32,
            lines_len,
            points_offset: points_start as u32,
            points_len,
            masks_offset: masks_start as u32,
            masks_len,
            tris_offset: tris_start as u32,
            tris_len,
            line_ranges,
            point_ranges,
            tri_ranges,
            mask_ranges,
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

// ------------------------------------------------------------ paper space

/// `*Paper_Space`, `*Paper_Space0`, … are the layout blocks.
fn is_paper_block(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("*paper_space")
}

/// Order of a paper block: `*Paper_Space` first, then `*Paper_Space0`, `1`, …
fn paper_block_rank(name: &str) -> u32 {
    let lower = name.to_ascii_lowercase();
    let suffix = lower.strip_prefix("*paper_space").unwrap_or("");
    if suffix.is_empty() {
        0
    } else {
        suffix.trim_start_matches('0').parse::<u32>().map(|n| n + 1).unwrap_or(1)
    }
}

fn paper_layout_name(block: &str) -> String {
    format!("Layout {}", paper_block_rank(block) + 1)
}

fn is_viewport(entity_type: &str) -> bool {
    entity_type.eq_ignore_ascii_case("VIEWPORT")
}

/// Liang–Barsky clip of a segment against an axis-aligned rectangle.
fn clip_segment(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    rect: (f64, f64, f64, f64),
) -> Option<(f64, f64, f64, f64)> {
    let (xmin, ymin, xmax, ymax) = rect;
    let (dx, dy) = (x2 - x1, y2 - y1);
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    for (p, q) in [
        (-dx, x1 - xmin),
        (dx, xmax - x1),
        (-dy, y1 - ymin),
        (dy, ymax - y1),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return None;
            }
            if r > t0 {
                t0 = r;
            }
        } else {
            if r < t0 {
                return None;
            }
            if r < t1 {
                t1 = r;
            }
        }
    }
    Some((x1 + t0 * dx, y1 + t0 * dy, x1 + t1 * dx, y1 + t1 * dy))
}

// ------------------------------------------------------------- HATCH helper

/// Sequential reader over a HATCH's group-code pairs.  HATCH is the one DXF
/// entity where group codes repeat in a structured, nested way, so index-free
/// `f(buf, code, …)` lookups cannot express it.
/// One pattern definition line of a HATCH: direction, origin and spacing of
/// a family of parallel lines, plus the dash pattern along them.
struct PatternLine {
    angle: f64,
    base: (f64, f64),
    offset: (f64, f64),
    dashes: Vec<f64>,
}

struct Cursor<'a> {
    buf: &'a [(i32, String)],
    i: usize,
    /// Hatch pattern angle (group 52) / scale (group 41), read while scanning
    /// for the pattern definition.
    angle_deg: f64,
    pattern_scale: f64,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [(i32, String)]) -> Self {
        Cursor { buf, i: 0, angle_deg: 0.0, pattern_scale: 1.0 }
    }

    /// Pattern definition lines (group 78 counts them), read after the boundary
    /// paths.  Also captures the hatch-level pattern angle (52) and scale (41),
    /// which transform the whole pattern.
    fn pattern_lines(&mut self) -> Vec<PatternLine> {
        let mut n_lines = 0i64;
        while let Some((code, val)) = self.next() {
            match code {
                52 => self.angle_deg = val.trim().parse().unwrap_or(0.0),
                41 => self.pattern_scale = val.trim().parse().unwrap_or(1.0),
                78 => {
                    n_lines = val.trim().parse().unwrap_or(0);
                    break;
                }
                // 98 starts the seed points: no pattern data follows.
                98 => return Vec::new(),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for _ in 0..n_lines.clamp(0, 512) {
            let angle = self.f(53, 0.0);
            let bx = self.f(43, 0.0);
            let by = self.f(44, 0.0);
            let ox = self.f(45, 0.0);
            let oy = self.f(46, 0.0);
            let n_dash = self.integer(79, 0).clamp(0, 256);
            let mut dashes = Vec::new();
            for _ in 0..n_dash {
                dashes.push(self.f(49, 0.0));
            }
            out.push(PatternLine { angle, base: (bx, by), offset: (ox, oy), dashes });
        }
        out
    }

    fn next(&mut self) -> Option<(i32, &'a str)> {
        let e = self.buf.get(self.i)?;
        self.i += 1;
        Some((e.0, e.1.as_str()))
    }

    /// Consume the next pair only when its code matches.
    fn take(&mut self, code: i32) -> Option<&'a str> {
        if self.buf.get(self.i).map(|e| e.0) == Some(code) {
            self.i += 1;
            return Some(self.buf[self.i - 1].1.as_str());
        }
        None
    }

    fn f(&mut self, code: i32, default: f64) -> f64 {
        self.take(code).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    fn integer(&mut self, code: i32, default: i64) -> i64 {
        self.f(code, default as f64) as i64
    }

    /// Read one boundary path — the caller has already consumed group 91 and
    /// this consumes group 92 plus everything the path needs.
    fn boundary_path(&mut self) -> Option<Vec<(f64, f64)>> {
        let flag = self.integer(92, 0);
        let mut pts: Vec<(f64, f64)> = Vec::new();

        if flag & 2 != 0 {
            // Polyline form: 72 = has bulge, 73 = closed, 93 = vertex count.
            let has_bulge = self.integer(72, 0) != 0;
            let closed = self.integer(73, 0) != 0;
            let n = self.integer(93, 0).clamp(0, 200_000) as usize;
            let mut vs: Vec<(f64, f64)> = Vec::with_capacity(n);
            let mut bs: Vec<f64> = Vec::with_capacity(n);
            for _ in 0..n {
                let x = self.f(10, f64::NAN);
                let y = self.f(20, f64::NAN);
                let bulge = if has_bulge { self.f(42, 0.0) } else { 0.0 };
                if x.is_finite() && y.is_finite() {
                    vs.push((x, y));
                    bs.push(bulge);
                }
            }
            let m = vs.len();
            let edges = if closed { m } else { m.saturating_sub(1) };
            for e in 0..edges {
                let (p, q) = (vs[e], vs[(e + 1) % m]);
                if bs.get(e).copied().unwrap_or(0.0).abs() >= 1e-9 {
                    push_points(&mut pts, &arc_points(p.0, p.1, q.0, q.1, bs[e]));
                } else {
                    push_pt(&mut pts, p);
                    push_pt(&mut pts, q);
                }
            }
        } else {
            // Edge list: 93 = number of edges, each starting with its type (72).
            let n = self.integer(93, 0).clamp(0, 200_000) as usize;
            for _ in 0..n {
                match self.integer(72, 0) {
                    1 => {
                        let (x1, y1) = (self.f(10, f64::NAN), self.f(20, f64::NAN));
                        let (x2, y2) = (self.f(11, f64::NAN), self.f(21, f64::NAN));
                        push_pt(&mut pts, (x1, y1));
                        push_pt(&mut pts, (x2, y2));
                    }
                    2 => {
                        let (cx, cy) = (self.f(10, f64::NAN), self.f(20, f64::NAN));
                        let r = self.f(40, 0.0);
                        let a0 = self.f(50, 0.0).to_radians();
                        let a1 = self.f(51, 360.0).to_radians();
                        self.integer(73, 1); // ccw flag: sweep direction only
                        if cx.is_finite() && r > 0.0 {
                            push_points(&mut pts, &arc_pts_angle(cx, cy, r, r, 0.0, a0, a1));
                        }
                    }
                    3 => {
                        let (cx, cy) = (self.f(10, f64::NAN), self.f(20, f64::NAN));
                        let (ax, ay) = (self.f(11, 0.0), self.f(21, 0.0));
                        let ratio = self.f(40, 1.0).clamp(1e-6, 1.0);
                        let a0 = self.f(50, 0.0).to_radians();
                        let a1 = self.f(51, 360.0).to_radians();
                        self.integer(73, 1);
                        let major = (ax * ax + ay * ay).sqrt();
                        if cx.is_finite() && major > 0.0 {
                            let rot = ay.atan2(ax);
                            push_points(
                                &mut pts,
                                &arc_pts_angle(cx, cy, major, major * ratio, rot, a0, a1),
                            );
                        }
                    }
                    4 => {
                        // Spline edge: the control polygon is a serviceable
                        // boundary for a filled region.
                        let n_ctrl = self.integer(96, 0).clamp(0, 100_000) as usize;
                        let n_knots = self.integer(95, 0).clamp(0, 100_000) as usize;
                        self.integer(94, 3);
                        self.integer(73, 0);
                        self.integer(74, 0);
                        for _ in 0..n_knots {
                            self.f(40, 0.0);
                        }
                        for _ in 0..n_ctrl {
                            let x = self.f(10, f64::NAN);
                            let y = self.f(20, f64::NAN);
                            push_pt(&mut pts, (x, y));
                        }
                    }
                    _ => {}
                }
            }
        }

        // 97 = number of source boundary objects; their 330 handles must not be
        // mistaken for the next path's coordinates.
        let src = self.integer(97, 0).clamp(0, 100_000) as usize;
        for _ in 0..src {
            self.take(330);
        }

        if pts.len() >= 2 { Some(pts) } else { None }
    }
}

fn loops_bounds(loops: &[Vec<(f64, f64)>]) -> (f64, f64, f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for l in loops {
        for p in l {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
    }
    if !x0.is_finite() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    (x0, y0, x1, y1)
}

/// Spans of the line `base + t * dir` that lie inside the boundary loops,
/// under the even-odd rule — the same rule AutoCAD uses, and the reason island
/// paths punch holes into a hatch for free.
///
/// Returns (t0, t1) pairs with t0 < t1, or an empty list when the line misses.
fn even_odd_spans(
    loops: &[Vec<(f64, f64)>],
    base: (f64, f64),
    dir: (f64, f64),
    t_min: f64,
    t_max: f64,
) -> Vec<(f64, f64)> {
    let len2 = dir.0 * dir.0 + dir.1 * dir.1;
    if len2 < 1e-18 {
        return Vec::new();
    }
    // Normal of the line: which side of it a point falls on.
    let nx = -dir.1;
    let ny = dir.0;
    let mut ts: Vec<f64> = Vec::new();
    for l in loops {
        let n = l.len();
        if n < 3 {
            continue;
        }
        for k in 0..n {
            let a = l[k];
            let b = l[(k + 1) % n];
            let sa = (a.0 - base.0) * nx + (a.1 - base.1) * ny;
            let sb = (b.0 - base.0) * nx + (b.1 - base.1) * ny;
            // Half-open rule (strict > 0): a vertex sitting exactly on the line
            // counts once, not twice, so spans stay paired.
            if (sa > 0.0) == (sb > 0.0) {
                continue;
            }
            let t = sa / (sa - sb);
            let px = a.0 + (b.0 - a.0) * t;
            let py = a.1 + (b.1 - a.1) * t;
            let tp = ((px - base.0) * dir.0 + (py - base.1) * dir.1) / len2;
            if tp >= t_min - 1e-9 && tp <= t_max + 1e-9 {
                ts.push(tp.clamp(t_min, t_max));
            }
        }
    }
    if ts.len() < 2 {
        return Vec::new();
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::with_capacity(ts.len() / 2);
    let mut i = 0;
    while i + 1 < ts.len() {
        if ts[i + 1] - ts[i] > 1e-9 {
            out.push((ts[i], ts[i + 1]));
        }
        i += 2;
    }
    out
}

fn push_pt(out: &mut Vec<(f64, f64)>, p: (f64, f64)) {
    if !(p.0.is_finite() && p.1.is_finite()) {
        return;
    }
    if let Some(last) = out.last() {
        if (last.0 - p.0).abs() < 1e-9 && (last.1 - p.1).abs() < 1e-9 {
            return;
        }
    }
    out.push(p);
}

fn push_points(out: &mut Vec<(f64, f64)>, pts: &[(f64, f64)]) {
    for p in pts {
        push_pt(out, *p);
    }
}

/// Bulge (DXF group 42) → arc polyline points, including both endpoints.
fn arc_points(x1: f64, y1: f64, x2: f64, y2: f64, bulge: f64) -> Vec<(f64, f64)> {
    let mut out = vec![(x1, y1)];
    let (dx, dy) = (x2 - x1, y2 - y1);
    let chord = (dx * dx + dy * dy).sqrt();
    if chord < 1e-12 {
        out.push((x2, y2));
        return out;
    }
    let sagitta = bulge * chord / 2.0;
    let included = 4.0 * bulge.abs().atan();
    let n = ((included / (2.0 * std::f64::consts::PI) * 64.0).ceil() as u32).max(4);
    let (mx, my) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
    let (px, py) = (-dy / chord, dx / chord);
    let sign = if bulge >= 0.0 { 1.0 } else { -1.0 };
    let (cx, cy) = (mx + sign * px * sagitta, my + sign * py * sagitta);
    let r = ((x1 - cx).powi(2) + (y1 - cy).powi(2)).sqrt();
    let a0 = (y1 - cy).atan2(x1 - cx);
    let dir = if bulge >= 0.0 { 1.0 } else { -1.0 };
    for i in 1..=n {
        let a = a0 + dir * included * (i as f64 / n as f64);
        out.push((cx + r * a.cos(), cy + r * a.sin()));
    }
    out
}

/// Elliptical arc sampler (a circle is the `rx == ry`, `rot == 0` case).
fn arc_pts_angle(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    rot: f64,
    a0: f64,
    mut a1: f64,
) -> Vec<(f64, f64)> {
    while a1 <= a0 {
        a1 += 2.0 * std::f64::consts::PI;
    }
    let sweep = a1 - a0;
    let steps = ((sweep / (2.0 * std::f64::consts::PI) * 64.0).ceil() as usize).max(6);
    (0..=steps)
        .map(|i| {
            let t = a0 + sweep * (i as f64 / steps as f64);
            ellipse_pt(cx, cy, rx, ry, rot, t)
        })
        .collect()
}

// ------------------------------------------------------------ triangulation

fn signed_area(poly: &[(f64, f64)]) -> f64 {
    let mut a = 0.0;
    for i in 0..poly.len() {
        let p = poly[i];
        let q = poly[(i + 1) % poly.len()];
        a += p.0 * q.1 - q.0 * p.1;
    }
    a / 2.0
}

fn cross(o: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

fn point_in_tri(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

/// Ear-clipping triangulation of a simple polygon, returned as index triples.
///
/// An empty result means the boundary was degenerate or self-intersecting
/// (hatch boundaries in the wild are usually neither); the caller then falls
/// back to a triangle fan so something is still filled.
fn triangulate(poly: &[(f64, f64)]) -> Vec<[usize; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    // 规模护栏：耳切对每个候选角都要扫剩余顶点，n 超过阈值时直接走调用方的
    // 三角扇回退，避免大环把解析卡死。
    if n > 2_000 {
        return Vec::new();
    }
    let mut idx: Vec<usize> = (0..n).collect();
    if signed_area(poly) < 0.0 {
        idx.reverse(); // work counter-clockwise
    }
    let mut out: Vec<[usize; 3]> = Vec::with_capacity(n.saturating_sub(2));
    let mut guard = 0usize;
    while idx.len() > 3 && guard <= n * n + 16 {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross(a, b, c) <= 0.0 {
                continue; // reflex corner
            }
            let mut blocked = false;
            for &j in &idx {
                if j == ia || j == ib || j == ic {
                    continue;
                }
                if point_in_tri(poly[j], a, b, c) {
                    blocked = true;
                    break;
                }
            }
            if blocked {
                continue;
            }
            out.push([ia, ib, ic]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            return Vec::new();
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

// --------------------------------------------------------------- pre-scans

/// $NAME / value out of the HEADER section (group 9 names it, the next pair
/// carries the value).
fn scan_header_var(input: &str, name: &str, default: f64) -> f64 {
    let lines: Vec<&str> = input.lines().collect();
    let needle = format!("${name}");
    let mut i = 0usize;
    while i + 3 < lines.len() {
        if lines[i].trim() == "9" && lines[i + 1].trim_end_matches('\r').trim() == needle {
            if let Ok(v) = lines[i + 3].trim().parse::<f64>() {
                return v;
            }
        }
        i += 2;
    }
    default
}

/// LTYPE records: name → dash pattern (group 49 entries, gaps negative).
fn scan_ltype_table(input: &str) -> std::collections::HashMap<String, Vec<f64>> {
    use std::collections::HashMap;
    let lines: Vec<&str> = input.lines().collect();
    let mut out: HashMap<String, Vec<f64>> = HashMap::new();
    let mut in_tables = false;
    let mut is_ltype = false;
    let mut name: Option<String> = None;
    let mut dashes: Vec<f64> = Vec::new();

    let mut i = 0usize;
    while i + 1 < lines.len() {
        let code: i32 = match lines[i].trim().parse() {
            Ok(c) => c,
            Err(_) => {
                i += 2;
                continue;
            }
        };
        let value = lines[i + 1].trim_end_matches('\r').trim_start();
        i += 2;

        if !in_tables {
            if code == 2 && value.eq_ignore_ascii_case("TABLES") {
                in_tables = true;
            }
            continue;
        }
        if code == 0 {
            if let Some(n) = name.take() {
                if !dashes.is_empty() {
                    out.insert(n, std::mem::take(&mut dashes));
                }
            }
            if value.eq_ignore_ascii_case("ENDSEC") {
                break;
            }
            is_ltype = value.eq_ignore_ascii_case("LTYPE");
            continue;
        }
        if is_ltype {
            if code == 2 {
                name = Some(value.to_string());
            }
            if code == 49 {
                if let Ok(v) = value.parse::<f64>() {
                    dashes.push(v);
                }
            }
        }
    }
    if let Some(n) = name {
        if !dashes.is_empty() {
            out.insert(n, dashes);
        }
    }
    out
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

/// Pre-scan the TABLES section for LAYER records: name → (ACI colour, off,
/// linetype), plus the names in file order, plus name → linetype.  Colour is
/// stored positive; a negative code 62 means the layer is off.
#[allow(clippy::type_complexity)]
fn scan_layer_table(
    input: &str,
) -> (
    std::collections::HashMap<String, (u8, bool, String)>,
    Vec<String>,
    std::collections::HashMap<String, String>,
) {
    use std::collections::HashMap;
    let lines: Vec<&str> = input.lines().collect();
    let mut out: HashMap<String, (u8, bool, String)> = HashMap::new();
    let mut ltypes: HashMap<String, String> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut in_tables = false;
    let mut is_layer = false;
    let mut name: Option<String> = None;
    let mut color = 7i32;
    let mut ltype = String::new();

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
                if out.insert(n.clone(), (aci, color < 0, ltype.clone())).is_none() {
                    order.push(n.clone());
                }
                if !ltype.is_empty() {
                    ltypes.insert(n, ltype.clone());
                }
            }
            if value.eq_ignore_ascii_case("ENDSEC") { break; }
            is_layer = value.eq_ignore_ascii_case("LAYER");
            color = 7;
            ltype.clear();
            continue;
        }
        if is_layer {
            if code == 2 { name = Some(value.to_string()); }
            if code == 62 { if let Ok(c) = value.parse::<i32>() { color = c; } }
            if code == 6 { ltype = value.to_string(); }
        }
    }
    (out, order, ltypes)
}

/// Pre-scan the TABLES section for STYLE records: style name → width factor
/// (group 41).  Height and font file live there too, but the width factor is
/// the only one a viewer that substitutes system fonts can act on: it is what
/// makes a condensed Chinese style read condensed.
fn scan_style_table(input: &str) -> std::collections::HashMap<String, f64> {
    use std::collections::HashMap;
    let lines: Vec<&str> = input.lines().collect();
    let mut out: HashMap<String, f64> = HashMap::new();
    let mut in_tables = false;
    let mut in_style = false;
    let mut name: Option<String> = None;
    let mut width = 1.0f64;

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
            if let Some(n) = name.take() {
                out.insert(n, width);
            }
            if value.eq_ignore_ascii_case("ENDSEC") { break; }
            in_style = value.eq_ignore_ascii_case("STYLE");
            width = 1.0;
            continue;
        }
        if in_style {
            if code == 2 { name = Some(value.to_string()); }
            if code == 41 {
                if let Ok(w) = value.parse::<f64>() {
                    if w.is_finite() && w > 1e-6 { width = w; }
                }
            }
        }
    }
    out
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

pub(crate) fn append_tri(
    out: &mut Vec<u8>,
    a: (f64, f64),
    b: (f64, f64),
    c: (f64, f64),
    r: u8,
    g: u8,
    bl: u8,
    al: u8,
) {
    for p in [a, b, c] {
        out.extend_from_slice(&(p.0 as f32).to_le_bytes());
        out.extend_from_slice(&(p.1 as f32).to_le_bytes());
        out.push(r); out.push(g); out.push(bl); out.push(al);
    }
}

// ------------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    /// Block content on layer "0" with BYBLOCK colour inherits the INSERT's
    /// layer and colour (ObjectARX semantics).
    #[test]
    fn byblock_and_layer0_inherit_from_insert() {
        let dxf = "\
0\nSECTION\n2\nTABLES\n0\nLAYER\n2\nDIM\n62\n8\n0\nENDTAB\n0\nENDSEC\n\
0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nTICK\n0\nLWPOLYLINE\n8\n0\n62\n0\n90\n2\n10\n0.0\n20\n0.0\n10\n1.0\n20\n1.0\n0\nENDBLK\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nTICK\n8\nDIM\n62\n8\n10\n0.0\n20\n0.0\n0\nENDSEC\n0\nEOF\n";
        let (meta, geo) = parse_and_build(dxf, 0, false, 0).unwrap();
        let dim = meta.layers.iter().position(|l| l.name == "DIM").unwrap();
        // The polyline must land on the insert's layer "DIM", not "0".
        let range = meta.layouts[0].line_ranges.iter().find(|r| r.layer as usize == dim).expect("geometry on DIM");
        let bytes = &geo[meta.layouts[0].lines_offset as usize..][range.offset as usize..][..range.len as usize];
        // ACI 8 = 0x808080 grey, inherited via BYBLOCK from the INSERT.
        assert_eq!(&bytes[8..12], &[0x80, 0x80, 0x80, 255]);
        // Nothing may land on layer "0" — it was absorbed by the insert's layer.
        assert!(!meta.layouts[0].line_ranges.iter().any(|r| meta.layers[r.layer as usize].name == "0"));
    }

    /// Explicit entity colours are kept literal (ACI 2 yellow stays yellow);
    /// ACI 7 resolves to the adaptive "auto" colour (alpha 0).
    #[test]
    fn explicit_aci_and_auto() {
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nL1\n62\n2\n10\n0\n20\n0\n11\n1\n21\n0\n0\nLINE\n8\nL1\n62\n7\n10\n0\n20\n0\n11\n2\n21\n0\n0\nENDSEC\n0\nEOF\n";
        let (meta, geo) = parse_and_build(dxf, 0, false, 0).unwrap();
        let bytes = &geo[meta.layouts[0].lines_offset as usize..][..meta.layouts[0].lines_len as usize];
        assert_eq!(&bytes[8..12], &[255, 255, 0, 255]); // ACI 2 literal
        assert_eq!(&bytes[8 + 24..8 + 28], &[0, 0, 0, 0]); // ACI 7 → auto
    }

    /// Group code 71 (degree) is clamped and a mismatched knot vector falls
    /// back to the control polygon, so a malformed SPLINE must degrade
    /// gracefully instead of indexing `knots[k]` out of bounds — release is
    /// panic=abort, which would take the whole app down.
    #[test]
    fn malformed_spline_does_not_panic() {
        // Negative degree, too few knots.
        let neg_degree = "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n8\nL1\n71\n-5\n10\n0\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nEOF\n";
        // Absurdly large degree.
        let huge_degree = "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n8\nL1\n71\n99999\n10\n0\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nEOF\n";
        // Legal degree but the knot vector does not match the control count.
        let knot_mismatch = "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n8\nL1\n71\n3\n10\n0\n20\n0\n10\n1\n20\n1\n40\n0\n40\n1\n0\nENDSEC\n0\nEOF\n";
        for dxf in [neg_degree, huge_degree, knot_mismatch] {
            assert!(
                parse_and_build(dxf, 0, false, 0).is_ok(),
                "malformed SPLINE must not panic or error"
            );
        }
    }
}
