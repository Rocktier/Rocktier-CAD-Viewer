import type { Scene } from "./scene";

/** Candidate kind — also drives the marker shape (CAD convention). */
export type SnapKind = "endpoint" | "midpoint";

export interface SnapHit {
  x: number;
  y: number;
  kind: SnapKind;
}

export interface SnapIndex {
  /** Candidate positions, x,y interleaved, world units. */
  points: Float32Array;
  /** Parallel kinds: 0 = endpoint, 1 = midpoint (lower wins ties). */
  kinds: Uint8Array;
  count: number;
}

const EMPTY: SnapIndex = { points: new Float32Array(0), kinds: new Uint8Array(0), count: 0 };

export const ENDPOINT = 0;
export const MIDPOINT = 1;

/** Screen-space pick radius, in CSS pixels (Autodesk-style OSNAP aperture). */
export const SNAP_PX = 14;

/**
 * Collect snap candidates from one layout's already-tessellated line segments:
 * every endpoint plus every midpoint, skipping hidden layers.
 *
 * ponytail: because the pipeline flattens everything to world-space segments,
 * one pass covers lines, arcs, splines, exploded blocks and dimensions — no
 * per-entity `subGetOsnapPoints` dispatch (the part mlightcad documents as
 * unreliable).  Add circle centres/quadrants only if a drawing needs them.
 */
export function buildSnapIndex(scene: Scene | null, layoutIdx: number, hidden: Set<number>): SnapIndex {
  const layout = scene?.meta.layouts[layoutIdx];
  if (!scene || !layout) return EMPTY;

  const view = new DataView(scene.buffer);
  const base = scene.geometryOffset + layout.lines_offset;

  let segments = 0;
  for (const r of layout.line_ranges) {
    // Floor: a range length that is not a whole multiple of the 24-byte
    // stride would make `new Float32Array(segments * 6)` throw a RangeError,
    // killing the effect that builds the index.
    if (!hidden.has(r.layer)) segments += Math.floor(r.len / 24);
  }
  if (segments === 0) return EMPTY;

  const points = new Float32Array(segments * 6); // 2 endpoints + 1 midpoint
  const kinds = new Uint8Array(segments * 3);
  let n = 0;

  for (const r of layout.line_ranges) {
    if (hidden.has(r.layer)) continue;
    const end = base + r.offset + r.len;
    for (let o = base + r.offset; o + 24 <= end; o += 24) {
      const x1 = view.getFloat32(o, true);
      const y1 = view.getFloat32(o + 4, true);
      const x2 = view.getFloat32(o + 12, true);
      const y2 = view.getFloat32(o + 16, true);
      points[n * 2] = x1;
      points[n * 2 + 1] = y1;
      kinds[n] = ENDPOINT;
      n++;
      points[n * 2] = x2;
      points[n * 2 + 1] = y2;
      kinds[n] = ENDPOINT;
      n++;
      points[n * 2] = (x1 + x2) / 2;
      points[n * 2 + 1] = (y1 + y2) / 2;
      kinds[n] = MIDPOINT;
      n++;
    }
  }

  return { points: points.subarray(0, n * 2), kinds: kinds.subarray(0, n), count: n };
}

/**
 * Nearest candidate to (x, y) within `radius` world units, or null.
 *
 * Endpoints win ties with midpoints, matching AutoCAD's priority.  The scan is
 * linear — called once per frame while measuring, ~150k candidates for a large
 * plan costs a couple of ms, which is cheaper than maintaining a spatial index
 * (rebuild it if a drawing ever makes this visible).
 */
export function nearestSnap(index: SnapIndex, x: number, y: number, radius: number): SnapHit | null {
  const { points, kinds, count } = index;
  let bestD2 = radius * radius;
  let bestKind = 2;
  let best = -1;

  for (let i = 0; i < count; i++) {
    const dx = points[i * 2] - x;
    if (dx > radius || dx < -radius) continue;
    const dy = points[i * 2 + 1] - y;
    if (dy > radius || dy < -radius) continue;
    const d2 = dx * dx + dy * dy;
    if (d2 > bestD2) continue;
    if (d2 === bestD2 && kinds[i] >= bestKind) continue;
    bestD2 = d2;
    bestKind = kinds[i];
    best = i;
  }

  if (best < 0) return null;
  return {
    x: points[best * 2],
    y: points[best * 2 + 1],
    kind: kinds[best] === MIDPOINT ? "midpoint" : "endpoint",
  };
}
