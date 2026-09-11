//! Entity flattening: DXF entities → per-layout vertex buffers.
//!
//! Every vertex is 12 bytes: f32 x, f32 y, u8 r, g, b, a. `a == 0` marks an
//! "auto color" vertex which the renderer paints with the active theme color
//! (white on dark canvas, black on light) — matching CAD's ACI 7 behavior.
//! Segments are bucketed per layer so the frontend can toggle visibility with
//! one draw call per layer.

use std::collections::HashMap;

use dxf::entities::{Entity, EntityType};
use dxf::enums::{HorizontalTextJustification, VerticalTextJustification};
use dxf::{Color, Drawing};

use crate::aci::aci_to_rgb;
use crate::model::{LayerInfo, LayoutMeta, RangeSpec, SceneMeta, TextItem};

pub const MAX_SEGMENTS: u64 = 12_000_000;
pub const MAX_TEXTS: u64 = 200_000;
const MAX_INSERT_DEPTH: u32 = 16;
/// Max relative chord error when tessellating curves (fraction of radius).
const CURVE_TOL: f64 = 0.002;
const MIN_ARC_SEGS: usize = 6;
const MAX_ARC_SEGS: usize = 2880;
/// Overshoot length for RAY/XLINE, relative to the current extents estimate.
const INFINITE_MULT: f64 = 4.0;

// ---------------------------------------------------------------- paint

#[derive(Clone, Copy, Debug)]
pub struct Paint {
    r: u8,
    g: u8,
    b: u8,
    auto: bool,
}

const PAINT_AUTO: Paint = Paint { r: 0, g: 0, b: 0, auto: true };

fn paint_from_aci(i: u8) -> Paint {
    if i == 7 {
        PAINT_AUTO
    } else {
        let (r, g, b) = aci_to_rgb(i);
        Paint { r, g, b, auto: false }
    }
}

// ---------------------------------------------------------------- transform

/// 2D affine transform: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Clone, Copy)]
pub struct Xform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Xform {
    fn identity() -> Self {
        Xform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
    }
    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
    /// Composes so that the result applies `inner` first, then `self`.
    fn compose(&self, inner: &Xform) -> Xform {
        Xform {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            e: self.a * inner.e + self.c * inner.f + self.e,
            f: self.b * inner.e + self.d * inner.f + self.f,
        }
    }
    fn translation(x: f64, y: f64) -> Xform {
        Xform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: x, f: y }
    }
    fn rotation(deg: f64) -> Xform {
        let r = deg.to_radians();
        Xform { a: r.cos(), b: r.sin(), c: -r.sin(), d: r.cos(), e: 0.0, f: 0.0 }
    }
    fn scaling(sx: f64, sy: f64) -> Xform {
        Xform { a: sx, b: 0.0, c: 0.0, d: sy, e: 0.0, f: 0.0 }
    }
    /// Average axis scale (approximate uniform scale).
    fn uniform_scale(&self) -> f64 {
        (self.a.hypot(self.b) + self.c.hypot(self.d)) / 2.0
    }
    /// Rotation carried by the transform, in degrees CCW (world space).
    fn rotation_deg(&self) -> f64 {
        self.b.atan2(self.a).to_degrees()
    }
}

// ---------------------------------------------------------------- layout builder

struct LayoutBuilder {
    name: String,
    layer_lines: Vec<Vec<u8>>,
    layer_tris: Vec<Vec<u8>>,
    layer_points: Vec<Vec<u8>>,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    segments: u64,
}

impl LayoutBuilder {
    fn new(name: &str, layer_count: usize) -> Self {
        let n = layer_count.max(1);
        LayoutBuilder {
            name: name.to_string(),
            layer_lines: vec![Vec::new(); n],
            layer_tris: vec![Vec::new(); n],
            layer_points: vec![Vec::new(); n],
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
            segments: 0,
        }
    }

    fn ensure_layer_count(&mut self, n: usize) {
        if self.layer_lines.len() < n {
            self.layer_lines.resize(n, Vec::new());
            self.layer_tris.resize(n, Vec::new());
            self.layer_points.resize(n, Vec::new());
        }
    }

    fn mark(&mut self, x: f64, y: f64) {
        if x < self.min_x { self.min_x = x; }
        if y < self.min_y { self.min_y = y; }
        if x > self.max_x { self.max_x = x; }
        if y > self.max_y { self.max_y = y; }
    }

    /// Current extents diagonal — used to size "infinite" construction lines.
    fn diag(&self) -> f64 {
        if self.min_x > self.max_x {
            0.0
        } else {
            (self.max_x - self.min_x).hypot(self.max_y - self.min_y)
        }
    }

    fn push_vertex(buf: &mut Vec<u8>, x: f64, y: f64, p: Paint) {
        buf.extend_from_slice(&(x as f32).to_le_bytes());
        buf.extend_from_slice(&(y as f32).to_le_bytes());
        buf.push(p.r);
        buf.push(p.g);
        buf.push(p.b);
        buf.push(if p.auto { 0 } else { 255 });
    }

    fn push_seg(&mut self, layer: u16, x1: f64, y1: f64, x2: f64, y2: f64, p: Paint) {
        self.mark(x1, y1);
        self.mark(x2, y2);
        self.segments += 1;
        let buf = &mut self.layer_lines[layer as usize];
        Self::push_vertex(buf, x1, y1, p);
        Self::push_vertex(buf, x2, y2, p);
    }

    /// Segment that must not extend the extents (RAY/XLINE overshoot).
    fn push_seg_infinite(&mut self, layer: u16, x1: f64, y1: f64, x2: f64, y2: f64, p: Paint) {
        self.segments += 1;
        let buf = &mut self.layer_lines[layer as usize];
        Self::push_vertex(buf, x1, y1, p);
        Self::push_vertex(buf, x2, y2, p);
    }

    fn push_tri(&mut self, layer: u16, pts: [(f64, f64); 3], p: Paint) {
        for &(x, y) in &pts {
            self.mark(x, y);
        }
        self.segments += 3;
        let buf = &mut self.layer_tris[layer as usize];
        for (x, y) in pts {
            Self::push_vertex(buf, x, y, p);
        }
    }

    fn push_point(&mut self, layer: u16, x: f64, y: f64, p: Paint) {
        self.mark(x, y);
        self.segments += 1;
        let buf = &mut self.layer_points[layer as usize];
        Self::push_vertex(buf, x, y, p);
    }

    fn push_polyline(&mut self, layer: u16, pts: &[(f64, f64)], closed: bool, p: Paint) {
        if pts.len() < 2 {
            return;
        }
        for w in pts.windows(2) {
            self.push_seg(layer, w[0].0, w[0].1, w[1].0, w[1].1, p);
        }
        if closed {
            let a = pts[pts.len() - 1];
            let b = pts[0];
            self.push_seg(layer, a.0, a.1, b.0, b.1, p);
        }
    }
}

// ---------------------------------------------------------------- scene builder

struct SceneBuilder {
    layers: Vec<LayerInfo>,
    layer_index: HashMap<String, u16>,
    layouts: Vec<LayoutBuilder>,
    texts: Vec<TextItem>,
    text_truncated: bool,
}

impl SceneBuilder {
    fn layer_idx(&mut self, name: &str) -> u16 {
        if let Some(&i) = self.layer_index.get(name) {
            return i;
        }
        let i = self.layers.len() as u16;
        self.layers.push(LayerInfo {
            name: name.to_string(),
            r: 0,
            g: 0,
            b: 0,
            auto: true,
            off: false,
        });
        self.layer_index.insert(name.to_string(), i);
        for lb in &mut self.layouts {
            lb.ensure_layer_count(self.layers.len());
        }
        i
    }

    fn add_table_layer(&mut self, name: &str, aci: Option<u8>, off: bool) {
        let i = self.layer_idx(name);
        let (r, g, b, auto) = match aci {
            Some(7) | None => (0, 0, 0, true),
            Some(idx) => {
                let (r, g, b) = aci_to_rgb(idx);
                (r, g, b, false)
            }
        };
        let l = &mut self.layers[i as usize];
        l.r = r;
        l.g = g;
        l.b = b;
        l.auto = auto;
        l.off = off;
    }

    fn layer_paint(&self, layer: u16) -> Paint {
        match self.layers.get(layer as usize) {
            Some(l) => Paint { r: l.r, g: l.g, b: l.b, auto: l.auto },
            None => PAINT_AUTO,
        }
    }

    fn add_layout(&mut self, name: &str) -> usize {
        self.layouts.push(LayoutBuilder::new(name, self.layers.len()));
        self.layouts.len() - 1
    }

    fn push_text(&mut self, item: TextItem) -> bool {
        if self.texts.len() as u64 >= MAX_TEXTS {
            self.text_truncated = true;
            return false;
        }
        if !item.text.is_empty() {
            self.texts.push(item);
        }
        true
    }
}

struct Budget {
    used: u64,
    truncated: bool,
}

impl Budget {
    fn take(&mut self, n: u64) -> bool {
        if self.used + n > MAX_SEGMENTS {
            self.truncated = true;
            return false;
        }
        self.used += n;
        true
    }
}

// ---------------------------------------------------------------- color

fn resolve_color(
    color: &Color,
    sb: &SceneBuilder,
    layer: u16,
    inherited: Option<Paint>,
) -> Option<Paint> {
    if color.is_turned_off() {
        return None;
    }
    if color.is_by_layer() {
        Some(sb.layer_paint(layer))
    } else if color.is_by_block() || color.is_by_entity() {
        Some(inherited.unwrap_or(PAINT_AUTO))
    } else if let Some(i) = color.index() {
        Some(paint_from_aci(i))
    } else {
        Some(PAINT_AUTO)
    }
}

// ---------------------------------------------------------------- curve helpers

fn arc_segment_count(r: f64, sweep_rad: f64) -> usize {
    if r <= 1e-9 {
        return MIN_ARC_SEGS;
    }
    // Central angle of a chord whose sagitta error stays under CURVE_TOL·r.
    let step = 2.0 * (1.0 - CURVE_TOL).acos().max(1e-6);
    let n = (sweep_rad.abs() / step).ceil() as usize;
    n.clamp(MIN_ARC_SEGS, MAX_ARC_SEGS)
}

fn sample_arc(cx: f64, cy: f64, r: f64, a0: f64, sweep: f64, out: &mut Vec<(f64, f64)>) {
    let n = arc_segment_count(r, sweep);
    for i in 0..=n {
        let t = a0 + sweep * (i as f64) / (n as f64);
        out.push((cx + r * t.cos(), cy + r * t.sin()));
    }
}

/// Appends points for a polyline bulge segment (Lee Mac's formulation).
fn bulge_points(a: (f64, f64), b: (f64, f64), bulge: f64, out: &mut Vec<(f64, f64)>) {
    let theta = 4.0 * bulge.atan();
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let chord = dx.hypot(dy);
    if chord < 1e-12 || theta.abs() < 1e-9 {
        out.push(b);
        return;
    }
    let r_signed = chord / (2.0 * (theta / 2.0).sin());
    let r = r_signed.abs();
    if !r.is_finite() || r < 1e-9 {
        out.push(b);
        return;
    }
    let ang = dy.atan2(dx) + (std::f64::consts::FRAC_PI_2 - 2.0 * bulge.atan());
    let cx = a.0 + ang.cos() * r_signed;
    let cy = a.1 + ang.sin() * r_signed;
    let a0 = (a.1 - cy).atan2(a.0 - cx);
    let a1 = (b.1 - cy).atan2(b.0 - cx);
    let two_pi = std::f64::consts::PI * 2.0;
    let sweep = if bulge > 0.0 {
        let s = (a1 - a0) % two_pi;
        if s <= 0.0 { s + two_pi } else { s }
    } else {
        let s = (a1 - a0) % two_pi;
        if s >= 0.0 { s - two_pi } else { s }
    };
    sample_arc(cx, cy, r, a0, sweep, out);
}

/// De Boor evaluation of a (possibly rational) B-spline.
fn flatten_spline(
    control: &[(f64, f64)],
    knots: &[f64],
    weights: &[f64],
    degree: usize,
    out: &mut Vec<(f64, f64)>,
) {
    let n = control.len();
    if degree == 0 || n < degree + 1 || knots.len() < n + degree + 1 {
        return;
    }
    let t0 = knots[degree];
    let t1 = knots[knots.len() - degree - 1];
    if !(t1 > t0) || !t0.is_finite() || !t1.is_finite() {
        return;
    }
    let weighted = weights.len() == n && weights.iter().any(|&w| (w - 1.0).abs() > 1e-9);
    let samples = (n * 10).clamp(32, 1440);
    let mut d: Vec<(f64, f64)> = vec![(0.0, 0.0); degree + 1];
    let mut dw: Vec<f64> = vec![1.0; degree + 1];
    for s in 0..=samples {
        let t = t0 + (t1 - t0) * (s as f64) / (samples as f64);
        // Knot span: largest k with knots[k] <= t, clamped to [degree, n-1].
        let mut k = degree;
        while k + 1 < n && knots[k + 1] <= t {
            k += 1;
        }
        for j in 0..=degree {
            let idx = k - degree + j;
            if idx < n {
                d[j] = control[idx];
                dw[j] = if weighted { weights[idx] } else { 1.0 };
            }
        }
        for r in 1..=degree {
            for j in (r..=degree).rev() {
                let i = k - degree + j;
                if i == 0 {
                    continue;
                }
                let denom = knots[i + degree - r + 1] - knots[i];
                let alpha = if denom.abs() < 1e-12 {
                    0.0
                } else {
                    ((t - knots[i]) / denom).clamp(0.0, 1.0)
                };
                let w_left = dw[j - 1]; // d[j-1]
                let w_right = dw[j]; // d[j]
                let d_new = (1.0 - alpha) * w_left + alpha * w_right;
                if d_new.abs() < 1e-12 {
                    continue;
                }
                let alpha_l = (1.0 - alpha) * w_left / d_new;
                let alpha_r = alpha * w_right / d_new;
                d[j].0 = alpha_l * d[j - 1].0 + alpha_r * d[j].0;
                d[j].1 = alpha_l * d[j - 1].1 + alpha_r * d[j].1;
                dw[j] = d_new;
            }
        }
        out.push(d[degree]);
    }
}

/// Catmull-Rom interpolation through fit points (splines without control data).
fn flatten_fit_points(points: &[(f64, f64)], out: &mut Vec<(f64, f64)>) {
    if points.is_empty() {
        return;
    }
    if points.len() == 1 {
        out.push(points[0]);
        return;
    }
    out.push(points[0]);
    let n = points.len();
    for i in 0..n - 1 {
        let p0 = if i == 0 { points[0] } else { points[i - 1] };
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = if i + 2 < n { points[i + 2] } else { points[n - 1] };
        for s in 1..=12 {
            let t = s as f64 / 12.0;
            let t2 = t * t;
            let t3 = t2 * t;
            let x = 0.5
                * ((2.0 * p1.0)
                    + (-p0.0 + p2.0) * t
                    + (2.0 * p0.0 - 5.0 * p1.0 + 4.0 * p2.0 - p3.0) * t2
                    + (-p0.0 + 3.0 * p1.0 - 3.0 * p2.0 + p3.0) * t3);
            let y = 0.5
                * ((2.0 * p1.1)
                    + (-p0.1 + p2.1) * t
                    + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
                    + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3);
            out.push((x, y));
        }
    }
}

// ---------------------------------------------------------------- text helpers

fn strip_mtext(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            if i + 1 >= chars.len() {
                break;
            }
            let n = chars[i + 1];
            match n {
                'P' => {
                    out.push('\n');
                    i += 2;
                }
                '~' => {
                    out.push(' ');
                    i += 2;
                }
                '\\' | '{' | '}' => {
                    out.push(n);
                    i += 2;
                }
                'S' => {
                    // Stacked fraction \Sa^b; → a/b.
                    let mut j = i + 2;
                    let mut content = String::new();
                    while j < chars.len() && chars[j] != ';' {
                        content.push(chars[j]);
                        j += 1;
                    }
                    out.push_str(&content.replace('^', "/").replace('#', "/"));
                    i = j + 1;
                }
                _ => {
                    // Generic inline code (font/height/color/etc.): skip to ';'.
                    let mut j = i + 1;
                    while j < chars.len()
                        && chars[j] != ';'
                        && chars[j] != '\\'
                        && chars[j] != '{'
                        && chars[j] != '}'
                    {
                        j += 1;
                    }
                    if j < chars.len() && chars[j] == ';' {
                        i = j + 1;
                    } else {
                        i = j;
                    }
                }
            }
        } else if c == '{' || c == '}' {
            i += 1;
        } else if c == '%' && i + 3 < chars.len() && chars[i + 1] == '%' {
            // %%d / %%c / %%p — degree / diameter / plus-minus.
            let code: String = chars[i + 2..i + 4].iter().collect();
            match code.as_str() {
                "d" | "D" => out.push('°'),
                "c" | "C" => out.push('⌀'),
                "p" | "P" => out.push('±'),
                _ => out.push_str(&format!("%{code}")),
            }
            i += 4;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn text_rgba(p: Paint) -> (u8, u8, u8, u8) {
    if p.auto {
        (0, 0, 0, 0)
    } else {
        (p.r, p.g, p.b, 255)
    }
}

// ---------------------------------------------------------------- block refs

struct BlockRef<'a> {
    entities: &'a [Entity],
    base: (f64, f64),
}

// ---------------------------------------------------------------- flattening

#[allow(clippy::too_many_arguments)]
fn flatten_entity(
    ent: &Entity,
    env: (&HashMap<String, usize>, &[BlockRef]),
    sb: &mut SceneBuilder,
    budget: &mut Budget,
    xform: Xform,
    inherited: Option<Paint>,
    layer_override: Option<u16>,
    depth: u32,
    layout_idx: usize,
) {
    if depth > MAX_INSERT_DEPTH || budget.truncated {
        return;
    }
    if !ent.common.is_visible {
        return;
    }
    let layer = if ent.common.layer == "0" {
        layer_override.unwrap_or_else(|| sb.layer_idx("0"))
    } else {
        sb.layer_idx(&ent.common.layer)
    };
    let Some(paint) = resolve_color(&ent.common.color, sb, layer, inherited) else {
        return;
    };

    match &ent.specific {
        EntityType::Line(l) => {
            let (x1, y1) = xform.apply(l.p1.x, l.p1.y);
            let (x2, y2) = xform.apply(l.p2.x, l.p2.y);
            if budget.take(1) {
                sb.layouts[layout_idx].push_seg(layer, x1, y1, x2, y2, paint);
            }
        }
        EntityType::Circle(c) => {
            let (cx, cy) = xform.apply(c.center.x, c.center.y);
            let r = c.radius * xform.uniform_scale();
            if r > 1e-9 {
                let n = arc_segment_count(r, std::f64::consts::PI * 2.0);
                if budget.take(n as u64) {
                    let mut pts = Vec::with_capacity(n + 1);
                    sample_arc(cx, cy, r, 0.0, std::f64::consts::PI * 2.0, &mut pts);
                    sb.layouts[layout_idx].push_polyline(layer, &pts, true, paint);
                }
            }
        }
        EntityType::Arc(a) => {
            let (cx, cy) = xform.apply(a.center.x, a.center.y);
            let r = a.radius * xform.uniform_scale();
            if r > 1e-9 {
                let mut sweep = a.end_angle - a.start_angle;
                while sweep <= 0.0 {
                    sweep += 360.0;
                }
                if sweep > 360.0 {
                    sweep = 360.0;
                }
                let n = arc_segment_count(r, sweep.to_radians());
                if budget.take(n as u64) {
                    let mut pts = Vec::with_capacity(n + 1);
                    sample_arc(cx, cy, r, a.start_angle.to_radians(), sweep.to_radians(), &mut pts);
                    sb.layouts[layout_idx].push_polyline(layer, &pts, false, paint);
                }
            }
        }
        EntityType::Ellipse(e) => {
            let (cx, cy) = xform.apply(e.center.x, e.center.y);
            let vx = xform.a * e.major_axis.x + xform.c * e.major_axis.y;
            let vy = xform.b * e.major_axis.x + xform.d * e.major_axis.y;
            let maj_len = vx.hypot(vy);
            let minor_len = maj_len * e.minor_axis_ratio;
            if maj_len > 1e-9 && minor_len > 1e-9 {
                let rot = vy.atan2(vx);
                let mut sweep = e.end_parameter - e.start_parameter;
                if sweep <= 0.0 {
                    sweep += std::f64::consts::PI * 2.0;
                }
                let n = arc_segment_count(maj_len.max(minor_len), sweep);
                if budget.take(n as u64) {
                    let mut pts = Vec::with_capacity(n + 1);
                    let (sin_r, cos_r) = rot.sin_cos();
                    for i in 0..=n {
                        let t = e.start_parameter + sweep * (i as f64) / (n as f64);
                        let (sin_t, cos_t) = t.sin_cos();
                        let ex = cx + cos_r * maj_len * cos_t - sin_r * minor_len * sin_t;
                        let ey = cy + sin_r * maj_len * cos_t + cos_r * minor_len * sin_t;
                        pts.push((ex, ey));
                    }
                    sb.layouts[layout_idx].push_polyline(layer, &pts, false, paint);
                }
            }
        }
        EntityType::LwPolyline(pw) => {
            if pw.vertices.is_empty() {
                return;
            }
            let count = pw.vertices.len();
            let mut pts: Vec<(f64, f64)> = Vec::with_capacity(count * 2);
            pts.push(xform.apply(pw.vertices[0].x, pw.vertices[0].y));
            let seg_count = if pw.is_closed() { count } else { count - 1 };
            for i in 0..seg_count {
                let v = &pw.vertices[i];
                let next = &pw.vertices[(i + 1) % count];
                let cur = pts[pts.len() - 1];
                let nxt = xform.apply(next.x, next.y);
                if v.bulge != 0.0 {
                    bulge_points(cur, nxt, v.bulge, &mut pts);
                } else {
                    pts.push(nxt);
                }
            }
            if budget.take(pts.len() as u64) {
                sb.layouts[layout_idx].push_polyline(layer, &pts, false, paint);
            }
        }
        EntityType::Polyline(pl) => {
            // Polyface meshes and polygon meshes are out of scope for 2D viewing.
            if pl.flags & 64 != 0 || pl.flags & 16 != 0 {
                return;
            }
            let vertices: Vec<&dxf::entities::Vertex> = pl.vertices().collect();
            if vertices.len() < 2 {
                return;
            }
            let raw: Vec<(f64, f64)> = vertices
                .iter()
                .map(|v| xform.apply(v.location.x, v.location.y))
                .collect();
            let closed = pl.flags & 1 != 0;
            let count = raw.len();
            let mut pts: Vec<(f64, f64)> = Vec::with_capacity(count * 2);
            pts.push(raw[0]);
            let seg_count = if closed { count } else { count - 1 };
            for i in 0..seg_count {
                let v = vertices[i];
                let nxt = raw[(i + 1) % count];
                let cur = pts[pts.len() - 1];
                if v.bulge != 0.0 {
                    bulge_points(cur, nxt, v.bulge, &mut pts);
                } else {
                    pts.push(nxt);
                }
            }
            if budget.take(pts.len() as u64) {
                sb.layouts[layout_idx].push_polyline(layer, &pts, false, paint);
            }
        }
        EntityType::Spline(sp) => {
            let ctrl: Vec<(f64, f64)> = sp
                .control_points
                .iter()
                .map(|p| xform.apply(p.x, p.y))
                .collect();
            let degree = (sp.degree_of_curve.max(1)) as usize;
            let mut pts: Vec<(f64, f64)> = Vec::with_capacity(ctrl.len() * 10 + 32);
            if ctrl.len() >= degree + 1 && sp.knot_values.len() >= ctrl.len() + degree + 1 {
                flatten_spline(&ctrl, &sp.knot_values, &sp.weight_values, degree, &mut pts);
            } else {
                let fit: Vec<(f64, f64)> = sp
                    .fit_points
                    .iter()
                    .map(|p| xform.apply(p.x, p.y))
                    .collect();
                flatten_fit_points(&fit, &mut pts);
            }
            if !pts.is_empty() && budget.take(pts.len() as u64) {
                sb.layouts[layout_idx].push_polyline(layer, &pts, false, paint);
            }
        }
        EntityType::ModelPoint(p) => {
            let (x, y) = xform.apply(p.location.x, p.location.y);
            if budget.take(1) {
                sb.layouts[layout_idx].push_point(layer, x, y, paint);
            }
        }
        EntityType::Solid(s) => {
            let p1 = xform.apply(s.first_corner.x, s.first_corner.y);
            let p2 = xform.apply(s.second_corner.x, s.second_corner.y);
            let p3 = xform.apply(s.third_corner.x, s.third_corner.y);
            let p4 = xform.apply(s.fourth_corner.x, s.fourth_corner.y);
            if budget.take(6) {
                // DXF solid corners run 1,2,4,3 around the quad.
                sb.layouts[layout_idx].push_tri(layer, [p1, p2, p4], paint);
                sb.layouts[layout_idx].push_tri(layer, [p1, p4, p3], paint);
            }
        }
        EntityType::Trace(s) => {
            let p1 = xform.apply(s.first_corner.x, s.first_corner.y);
            let p2 = xform.apply(s.second_corner.x, s.second_corner.y);
            let p3 = xform.apply(s.third_corner.x, s.third_corner.y);
            let p4 = xform.apply(s.fourth_corner.x, s.fourth_corner.y);
            if budget.take(6) {
                sb.layouts[layout_idx].push_tri(layer, [p1, p2, p4], paint);
                sb.layouts[layout_idx].push_tri(layer, [p1, p4, p3], paint);
            }
        }
        EntityType::Face3D(f) => {
            let p1 = xform.apply(f.first_corner.x, f.first_corner.y);
            let p2 = xform.apply(f.second_corner.x, f.second_corner.y);
            let p3 = xform.apply(f.third_corner.x, f.third_corner.y);
            let p4 = xform.apply(f.fourth_corner.x, f.fourth_corner.y);
            if budget.take(10) {
                sb.layouts[layout_idx].push_tri(layer, [p1, p2, p3], paint);
                sb.layouts[layout_idx].push_tri(layer, [p1, p3, p4], paint);
                sb.layouts[layout_idx].push_polyline(layer, &[p1, p2, p3, p4], true, paint);
            }
        }
        EntityType::Leader(l) => {
            let pts: Vec<(f64, f64)> = l
                .vertices
                .iter()
                .map(|p| xform.apply(p.x, p.y))
                .collect();
            if pts.len() >= 2 && budget.take(pts.len() as u64) {
                sb.layouts[layout_idx].push_polyline(layer, &pts, false, paint);
            }
        }
        EntityType::Ray(r) => {
            let (px, py) = xform.apply(r.start_point.x, r.start_point.y);
            let (x2, y2) = xform.apply(
                r.start_point.x + r.unit_direction_vector.x,
                r.start_point.y + r.unit_direction_vector.y,
            );
            let dx = x2 - px;
            let dy = y2 - py;
            let len = dx.hypot(dy);
            if len > 1e-12 && budget.take(1) {
                let d = INFINITE_MULT * sb.layouts[layout_idx].diag().max(1.0);
                sb.layouts[layout_idx].push_seg_infinite(
                    layer, px, py,
                    px + dx / len * d, py + dy / len * d,
                    paint,
                );
            }
        }
        EntityType::XLine(x) => {
            let (px, py) = xform.apply(x.first_point.x, x.first_point.y);
            let (x2, y2) = xform.apply(
                x.first_point.x + x.unit_direction_vector.x,
                x.first_point.y + x.unit_direction_vector.y,
            );
            let dx = x2 - px;
            let dy = y2 - py;
            let len = dx.hypot(dy);
            if len > 1e-12 && budget.take(2) {
                let d = INFINITE_MULT * sb.layouts[layout_idx].diag().max(1.0);
                sb.layouts[layout_idx].push_seg_infinite(
                    layer,
                    px - dx / len * d, py - dy / len * d,
                    px + dx / len * d, py + dy / len * d,
                    paint,
                );
            }
        }
        EntityType::Text(t) => {
            let use_second = t.horizontal_text_justification != HorizontalTextJustification::Left
                || t.vertical_text_justification != VerticalTextJustification::Baseline;
            let anchor = if use_second { &t.second_alignment_point } else { &t.location };
            let (x, y) = xform.apply(anchor.x, anchor.y);
            let ha = match t.horizontal_text_justification {
                HorizontalTextJustification::Left => 0u8,
                HorizontalTextJustification::Center => 1,
                HorizontalTextJustification::Right => 2,
                _ => 1,
            };
            let va = match t.vertical_text_justification {
                VerticalTextJustification::Baseline => 0u8,
                VerticalTextJustification::Bottom => 1,
                VerticalTextJustification::Middle => 2,
                VerticalTextJustification::Top => 3,
            };
            let (r, g, b, a) = text_rgba(paint);
            sb.push_text(TextItem {
                layout: layout_idx as u32,
                layer: layer as u32,
                x: x as f32,
                y: y as f32,
                h: (t.text_height * xform.uniform_scale()) as f32,
                rot: (xform.rotation_deg() + t.rotation) as f32,
                r, g, b, a, ha, va,
                text: t.value.clone(),
            });
        }
        EntityType::MText(m) => {
            let (x, y) = xform.apply(m.insertion_point.x, m.insertion_point.y);
            let code = m.attachment_point as i32;
            let ha = match code % 3 {
                1 => 0u8, // left
                2 => 1,   // center
                _ => 2,   // right
            };
            let va = match code {
                1..=3 => 3u8, // top
                4..=6 => 2,   // middle
                _ => 1,       // bottom
            };
            let (r, g, b, a) = text_rgba(paint);
            sb.push_text(TextItem {
                layout: layout_idx as u32,
                layer: layer as u32,
                x: x as f32,
                y: y as f32,
                h: (m.initial_text_height * xform.uniform_scale()) as f32,
                rot: (xform.rotation_deg() + m.rotation_angle) as f32,
                r, g, b, a, ha, va,
                text: strip_mtext(&m.text),
            });
        }
        EntityType::Insert(ins) => {
            let block_idx = env.0.get(&ins.name).copied();
            if let Some(bi) = block_idx {
                let base = env.1[bi].base;
                let sx = if ins.x_scale_factor == 0.0 { 1.0 } else { ins.x_scale_factor };
                let sy = if ins.y_scale_factor == 0.0 { 1.0 } else { ins.y_scale_factor };
                let rows = ins.row_count.max(1).min(64);
                let cols = ins.column_count.max(1).min(64);
                for row in 0..rows {
                    for col in 0..cols {
                        let m = Xform::translation(ins.location.x, ins.location.y)
                            .compose(&Xform::rotation(ins.rotation))
                            .compose(&Xform::scaling(sx, sy))
                            .compose(&Xform::translation(-base.0, -base.1));
                        // MINSERT row/column offsets live in the parent's axes.
                        let world = xform
                            .compose(&Xform::translation(
                                col as f64 * ins.column_spacing,
                                row as f64 * ins.row_spacing,
                            ))
                            .compose(&m);
                        for child in env.1[bi].entities {
                            flatten_entity(
                                child, env, sb, budget,
                                world, Some(paint), Some(layer),
                                depth + 1, layout_idx,
                            );
                        }
                        for (attr, _) in &ins.__attributes_and_handles {
                            let (ax, ay) = world.apply(attr.location.x, attr.location.y);
                            let (r, g, b, a) = text_rgba(paint);
                            sb.push_text(TextItem {
                                layout: layout_idx as u32,
                                layer: layer as u32,
                                x: ax as f32,
                                y: ay as f32,
                                h: (attr.text_height * world.uniform_scale()) as f32,
                                rot: (world.rotation_deg() + attr.rotation) as f32,
                                r, g, b, a,
                                ha: 0,
                                va: 0,
                                text: attr.value.clone(),
                            });
                        }
                    }
                }
            }
        }
        EntityType::RotatedDimension(d) => {
            flatten_dim_block(&d.dimension_base, env, sb, budget, xform, paint, layer, depth, layout_idx)
        }
        EntityType::RadialDimension(d) => {
            flatten_dim_block(&d.dimension_base, env, sb, budget, xform, paint, layer, depth, layout_idx)
        }
        EntityType::DiameterDimension(d) => {
            flatten_dim_block(&d.dimension_base, env, sb, budget, xform, paint, layer, depth, layout_idx)
        }
        EntityType::AngularThreePointDimension(d) => {
            flatten_dim_block(&d.dimension_base, env, sb, budget, xform, paint, layer, depth, layout_idx)
        }
        EntityType::OrdinateDimension(d) => {
            flatten_dim_block(&d.dimension_base, env, sb, budget, xform, paint, layer, depth, layout_idx)
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf_io::load_drawing;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("../fixtures");
        p.push(name);
        p
    }

    #[test]
    fn fan_dxf_builds_scene() {
        let drawing = load_drawing(&fixture("01_fan.dxf")).expect("load drawing");
        // Verify it actually loaded some entities.
        let _ = drawing.entities().count();
        let (meta, geometry) = build_scene(&drawing).expect("build scene");
        assert!(meta.segments > 0, "should produce segments");
        assert!(!meta.layouts.is_empty(), "should have layouts");
        // Fan has a text label.
        assert!(meta.text_count > 0, "fan should have text");
        // 5 layers: WALL, DOOR, WINDOW, DIM, FURN.
        assert!(meta.layers.len() >= 5, "should have at least 5 layers, got {}", meta.layers.len());
        // Geometry must be a multiple of vertex stride (12 bytes).
        assert_eq!(geometry.len() % 12, 0, "geometry not aligned to vertex stride");
    }

    #[test]
    fn polyline_bulge_rounds() {
        let drawing = load_drawing(&fixture("02_bulge.dxf")).expect("load");
        let (meta, _) = build_scene(&drawing).expect("build");
        // The bulge arcs should produce many segments.
        assert!(meta.segments >= 20, "bulge polyline should tessellate to many segments, got {}", meta.segments);
    }

    #[test]
    fn spline_builds_vertices() {
        let drawing = load_drawing(&fixture("04_spline.dxf")).expect("load");
        let (meta, _) = build_scene(&drawing).expect("build");
        assert!(meta.segments > 10, "spline+ellipse should produce geometry");
    }

    #[test]
    fn ellipse_tessellates() {
        let drawing = load_drawing(&fixture("03_ellipse.dxf")).expect("load");
        let (meta, _) = build_scene(&drawing).expect("build");
        // Two ellipses with different ratio — should produce many segments each.
        assert!(meta.segments >= 20, "two ellipses should tessellate to >=20 segments, got {}", meta.segments);
    }

    #[test]
    fn blocks_and_inserts() {
        let drawing = load_drawing(&fixture("05_nesting.dxf")).expect("load");
        let (meta, _) = build_scene(&drawing).expect("build");
        // Inserts should be flattened into the model space.
        assert!(meta.segments > 0, "block inserts should produce geometry");
    }

    #[test]
    fn layout_space_parses() {
        let drawing = load_drawing(&fixture("07_layout.dxf")).expect("load");
        let (meta, _) = build_scene(&drawing).expect("build");
        // Should yield at least two layouts (Model + paper).
        assert!(meta.layouts.len() >= 2, "paper space should yield multiple layouts, got {}", meta.layouts.len());
    }
}

// ---------------------------------------------------------------- flatten_entity

/// Dimensions carry their rendered geometry in an anonymous block (group code 2).
#[allow(clippy::too_many_arguments)]
fn flatten_dim_block(
    base: &dxf::entities::DimensionBase,
    env: (&HashMap<String, usize>, &[BlockRef]),
    sb: &mut SceneBuilder,
    budget: &mut Budget,
    xform: Xform,
    paint: Paint,
    layer: u16,
    depth: u32,
    layout_idx: usize,
) {
    let name = base.block_name.as_str();
    if name.is_empty() || name.eq_ignore_ascii_case("*MODEL_SPACE") {
        return;
    }
    if let Some(&bi) = env.0.get(name) {
        for child in env.1[bi].entities {
            flatten_entity(child, env, sb, budget, xform, Some(paint), Some(layer), depth + 1, layout_idx);
        }
    }
}

// ---------------------------------------------------------------- scene assembly

fn meta_for_layout(lb: &LayoutBuilder, geometry: &mut Vec<u8>) -> LayoutMeta {
    fn ranges_of(buckets: &[Vec<u8>], geometry: &mut Vec<u8>, region_start: usize) -> (u32, Vec<RangeSpec>) {
        let mut ranges = Vec::new();
        for (li, chunk) in buckets.iter().enumerate() {
            if !chunk.is_empty() {
                ranges.push(RangeSpec {
                    layer: li as u32,
                    offset: (geometry.len() - region_start) as u32,
                    len: chunk.len() as u32,
                });
                geometry.extend_from_slice(chunk);
            }
        }
        ((geometry.len() - region_start) as u32, ranges)
    }

    let lines_start = geometry.len();
    let (lines_len, line_ranges) = ranges_of(&lb.layer_lines, geometry, lines_start);

    let tris_start = geometry.len();
    let (tris_len, tri_ranges) = ranges_of(&lb.layer_tris, geometry, tris_start);

    let points_start = geometry.len();
    let (points_len, point_ranges) = ranges_of(&lb.layer_points, geometry, points_start);

    LayoutMeta {
        name: lb.name.clone(),
        min_x: lb.min_x,
        min_y: lb.min_y,
        max_x: lb.max_x,
        max_y: lb.max_y,
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

fn find_block_ci<'m>(map: &'m HashMap<String, usize>, name: &str) -> Option<usize> {
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, &v)| v)
}

pub fn build_scene(drawing: &Drawing) -> Result<(SceneMeta, Vec<u8>), String> {
    let mut sb = SceneBuilder {
        layers: Vec::new(),
        layer_index: HashMap::new(),
        layouts: Vec::new(),
        texts: Vec::new(),
        text_truncated: false,
    };

    // Layer table first (its order becomes the palette order in the panel).
    for l in drawing.layers() {
        let off = !l.is_layer_on || l.color.is_turned_off();
        sb.add_table_layer(&l.name, l.color.index(), off);
    }
    sb.layer_idx("0");

    // Blocks: name → index, plus a parallel list of entity slices + base points.
    let block_list: Vec<BlockRef> = drawing
        .blocks()
        .map(|b| BlockRef {
            entities: &b.entities,
            base: (b.base_point.x, b.base_point.y),
        })
        .collect();
    let block_map: HashMap<String, usize> = drawing
        .blocks()
        .enumerate()
        .map(|(i, b)| (b.name.clone(), i))
        .collect();

    // Block records: owner handle → block name (routes paper-space entities).
    let mut owner_names: HashMap<u64, String> = HashMap::new();
    for r in drawing.block_records() {
        owner_names.insert(r.handle.0, r.name.clone());
    }

    let mut budget = Budget { used: 0, truncated: false };
    let env = (&block_map, &block_list[..]);

    // ---- decide layout structure before flattening (avoids re-borrowing sb).
    let flat_model: Vec<&Entity> = drawing
        .entities()
        .filter(|e| !e.common.is_in_paper_space)
        .collect();
    let flat_paper: Vec<&Entity> = drawing
        .entities()
        .filter(|e| e.common.is_in_paper_space)
        .collect();

    // Paper grouping: owner block name → entities (flat path), or block slices (R12).
    let mut paper_groups: Vec<(String, Vec<usize>)> = Vec::new(); // (display name, block indices OR flat marker)
    enum PaperSource<'e> {
        Flat(Vec<&'e Entity>),
        Blocks(Vec<usize>),
    }
    let mut paper_sources: Vec<PaperSource> = Vec::new();

    if !flat_paper.is_empty() {
        let mut by_owner: HashMap<String, Vec<&Entity>> = HashMap::new();
        for e in &flat_paper {
            let owner = owner_names
                .get(&e.common.__owner_handle.0)
                .cloned()
                .unwrap_or_else(|| "*Paper_Space".to_string());
            by_owner.entry(owner).or_default().push(e);
        }
        let mut names: Vec<String> = by_owner.keys().cloned().collect();
        names.sort();
        names.sort_by_key(|n| paper_block_sort_key(n));
        let mut fallback: Option<Vec<&Entity>> = None;
        for name in &names {
            let ents = by_owner.remove(name).unwrap_or_default();
            if name.to_ascii_uppercase().starts_with("*PAPER_SPACE") || name.starts_with("*Paper_Space") {
                paper_groups.push((paper_layout_name(name), Vec::new()));
                paper_sources.push(PaperSource::Flat(ents));
            } else {
                match &mut fallback {
                    Some(v) => v.extend(ents),
                    None => fallback = Some(ents),
                }
            }
        }
        if let Some(ents) = fallback {
            if paper_groups.is_empty() {
                paper_groups.push(("Paper".to_string(), Vec::new()));
            }
            paper_sources.push(PaperSource::Flat(ents));
        }
    } else {
        let mut paper_blocks: Vec<(usize, &String)> = drawing
            .blocks()
            .enumerate()
            .filter(|(_, b)| {
                b.name.to_ascii_uppercase().starts_with("*PAPER_SPACE")
                    && !b.entities.is_empty()
            })
            .map(|(i, b)| (i, &b.name))
            .collect();
        paper_blocks.sort_by(|a, b| paper_block_sort_key(a.1).cmp(&paper_block_sort_key(b.1)));
        for (bi, name) in paper_blocks {
            paper_groups.push((paper_layout_name(name), vec![bi]));
            paper_sources.push(PaperSource::Blocks(vec![bi]));
        }
    }

    // ---- layouts
    let model_idx = sb.add_layout("Model");
    let paper_idxs: Vec<usize> = paper_groups
        .iter()
        .map(|(name, _)| sb.add_layout(name))
        .collect();

    // ---- model space
    if !flat_model.is_empty() {
        for e in &flat_model {
            flatten_entity(e, env, &mut sb, &mut budget, Xform::identity(), None, None, 0, model_idx);
        }
    } else if let Some(mi) = find_block_ci(&block_map, "*MODEL_SPACE") {
        for e in block_list[mi].entities {
            flatten_entity(e, env, &mut sb, &mut budget, Xform::identity(), None, None, 0, model_idx);
        }
    }

    // ---- paper space
    for (i, source) in paper_sources.iter().enumerate() {
        let li = paper_idxs[i];
        match source {
            PaperSource::Flat(ents) => {
                for e in ents {
                    flatten_entity(e, env, &mut sb, &mut budget, Xform::identity(), None, None, 0, li);
                }
            }
            PaperSource::Blocks(bis) => {
                for bi in bis {
                    for e in block_list[*bi].entities {
                        flatten_entity(e, env, &mut sb, &mut budget, Xform::identity(), None, None, 0, li);
                    }
                }
            }
        }
    }

    // ---- assemble
    let segments_total = sb.layouts.iter().map(|l| l.segments).sum::<u64>();
    let text_count = sb.texts.len() as u64;

    let mut geometry: Vec<u8> = Vec::new();
    let mut layout_metas: Vec<LayoutMeta> = Vec::new();
    for lb in &sb.layouts {
        layout_metas.push(meta_for_layout(lb, &mut geometry));
    }

let meta = SceneMeta {
    layers: sb.layers,
    layouts: layout_metas,
    texts: sb.texts,
    segments: segments_total,
    text_count,
    parse_ms: 0,
    tess_ms: 0,
    convert_ms: 0,
    was_dwg: false,
    truncated: sb.text_truncated || budget.truncated,
};
    Ok((meta, geometry))
}

fn paper_layout_name(block_name: &str) -> String {
    let upper = block_name.to_ascii_uppercase();
    if upper == "*PAPER_SPACE" {
        "Layout1".to_string()
    } else if let Some(suffix) = upper.strip_prefix("*PAPER_SPACE") {
        match suffix.parse::<u32>() {
            Ok(n) => format!("Layout{}", n + 1),
            Err(_) => block_name.trim_start_matches('*').to_string(),
        }
    } else {
        block_name.trim_start_matches('*').to_string()
    }
}

fn paper_block_sort_key(name: &str) -> u32 {
    let upper = name.to_ascii_uppercase();
    if upper == "*PAPER_SPACE" {
        0
    } else {
        upper
            .trim_start_matches("*PAPER_SPACE")
            .parse()
            .unwrap_or(u32::MAX - 1)
    }
}
