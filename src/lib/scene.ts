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
  lines_offset: number;
  lines_len: number;
  tris_offset: number;
  tris_len: number;
  points_offset: number;
  points_len: number;
  line_ranges: RangeSpec[];
  tri_ranges: RangeSpec[];
  point_ranges: RangeSpec[];
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
  tess_ms: number;
  truncated: boolean;
}

export interface Scene {
  meta: SceneMeta;
  buffer: ArrayBuffer;
  /** Byte offset of the geometry region inside `buffer`. */
  geometryOffset: number;
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
  return { meta, buffer: buf, geometryOffset: 8 + metaLen };
}

/** Model space display name. */
export function layoutLabel(name: string): string {
  return name === "Model" ? "Model" : name;
}
