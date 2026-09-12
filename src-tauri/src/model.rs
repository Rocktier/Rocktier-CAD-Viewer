//! Scene wire model — the JSON "meta" half of the scene blob sent to the frontend.
//! Geometry itself travels as raw bytes (f32 x, f32 y, u8 r, g, b, a per vertex;
//! a == 0 marks "auto color" which the renderer resolves from the active theme).

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct LayerInfo {
    pub name: String,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// True when the color is ACI 7 — theme-dependent (white on dark, black on light).
    pub auto: bool,
    pub off: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct TextItem {
    pub layout: u32,
    pub layer: u32,
    pub x: f32,
    pub y: f32,
    /// Text height in world units.
    pub h: f32,
    /// Rotation in degrees, CCW.
    pub rot: f32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// a == 0 → auto color (theme-dependent).
    pub a: u8,
    /// 0 left, 1 center, 2 right.
    pub ha: u8,
    /// 0 baseline, 1 bottom, 2 middle, 3 top.
    pub va: u8,
    pub text: String,
}

/// Byte range of one layer's data inside a layout's region.
#[derive(Serialize, Clone, Debug)]
pub struct RangeSpec {
    pub layer: u32,
    pub offset: u32,
    pub len: u32,
}

#[derive(Serialize, Debug)]
pub struct LayoutMeta {
    pub name: String,
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub lines_offset: u32,
    pub lines_len: u32,
    pub points_offset: u32,
    pub points_len: u32,
    pub line_ranges: Vec<RangeSpec>,
    pub point_ranges: Vec<RangeSpec>,
}

#[derive(Serialize, Debug)]
pub struct SceneMeta {
    pub layers: Vec<LayerInfo>,
    pub layouts: Vec<LayoutMeta>,
    pub texts: Vec<TextItem>,
    pub segments: u64,
    pub text_count: u64,
    pub parse_ms: u64,
    /// Time spent in dwg2dxf conversion (0 for native DXF).
    pub convert_ms: u64,
    /// True when the input was a DWG that was converted on-the-fly.
    pub was_dwg: bool,
    /// Number of entities that were not understood / cannot be tessellated.
    pub skipped: u64,
}

/// Scene blob layout: [magic "RCV1"][u32 meta_len][meta JSON][geometry region].
/// All offsets in `LayoutMeta` are relative to the geometry region start.
pub const BLOB_MAGIC: &[u8; 4] = b"RCV1";

pub fn encode_blob(meta_json: &str, geometry: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + meta_json.len() + geometry.len());
    out.extend_from_slice(BLOB_MAGIC);
    out.extend_from_slice(&(meta_json.len() as u32).to_le_bytes());
    out.extend_from_slice(meta_json.as_bytes());
    out.extend_from_slice(geometry);
    out
}
