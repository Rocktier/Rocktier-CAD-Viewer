export interface LayerInfo {
  name: string;
  r: number;
  g: number;
  b: number;
  auto: boolean;
  off: boolean;
}

export interface RangeSpec {
  layer: number;
  offset: number;
  len: number;
}

export interface LayoutMeta {
  name: string;
  min_x: number;
  min_y: number;
  max_x: number;
  max_y: number;
  /** Outlier-resistant box (0.5 %..99.5 % of vertices) — the "fit content" target. */
  core_min_x: number;
  core_min_y: number;
  core_max_x: number;
  core_max_y: number;
  lines_offset: number;
  lines_len: number;
  points_offset: number;
  points_len: number;
  /** Filled triangles: HATCH fills and SOLID/TRACE/3DFACE (dimension arrows). */
  tris_offset: number;
  tris_len: number;
  /** WIPEOUT masks — painted last, in the background colour. */
  masks_offset: number;
  masks_len: number;
  line_ranges: RangeSpec[];
  point_ranges: RangeSpec[];
  tri_ranges: RangeSpec[];
  mask_ranges: RangeSpec[];
}

export interface TextItem {
  layout: number;
  layer: number;
  x: number;
  y: number;
  h: number;
  rot: number;
  r: number;
  g: number;
  b: number;
  a: number;
  ha: number;
  va: number;
  text: string;
}

export interface SceneMeta {
  layers: LayerInfo[];
  layouts: LayoutMeta[];
  texts: TextItem[];
  segments: number;
  text_count: number;
  parse_ms: number;
  /** Time spent in dwg2dxf conversion (0 for native DXF). */
  convert_ms: number;
  /** True when the input was a DWG that was converted on-the-fly. */
  was_dwg: boolean;
  /** Entities recognised but not renderable (HATCH, unresolved INSERT, …). */
  skipped: number;
}

/**
 * A parsed scene.  Deliberately a class: Svelte's `$state` deep-proxies plain
 * objects and arrays, and a drawing carries tens of thousands of text items and
 * range specs that the renderer walks every frame — proxying them costs more
 * than the drawing itself.  Class instances are left untouched by the proxy.
 */
export class Scene {
  constructor(
    readonly meta: SceneMeta,
    readonly buffer: ArrayBuffer,
    /** Byte offset of the geometry region inside `buffer`. */
    readonly geometryOffset: number,
  ) {}
}

const decoder = new TextDecoder();

/** Parses the binary scene blob: [magic "RCV1"][u32 meta_len][meta JSON][geometry]. */
export function parseBlob(buf: ArrayBuffer): Scene {
  if (buf.byteLength < 8) throw new Error("响应数据不完整 / incomplete response");
  const head = new Uint8Array(buf, 0, 4);
  if (decoder.decode(head) !== "RCV1") throw new Error("未知的数据格式 / unknown data format");
  const metaLen = new DataView(buf).getUint32(4, true);
  if (8 + metaLen > buf.byteLength) throw new Error("数据损坏 / corrupted data");
  const meta = JSON.parse(decoder.decode(new Uint8Array(buf, 8, metaLen))) as SceneMeta;
  return new Scene(meta, buf, 8 + metaLen);
}
