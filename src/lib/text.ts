import type { Scene } from "./scene";
import type { Camera } from "./camera";
import type { SnapHit } from "./snap";

export interface TextDrawOptions {
  autoColor: string;
  hidden: Set<number>;
  dpr: number;
  /** Render into a surface of this CSS size instead of the canvas element's. */
  viewport?: { w: number; h: number };
  /** Draw every label in `autoColor` — the print/export mode. */
  mono?: boolean;
}

/**
 * Match a canvas backing store to its CSS box.
 *
 * A canvas that never gets `width`/`height` set keeps the 300×150 default, and
 * the browser stretches that tiny buffer over the whole element — magnifying
 * everything drawn on it several times, non-uniformly, and clipping past it.
 */
export function syncCanvasSize(canvas: HTMLCanvasElement, dpr: number): void {
  const bw = Math.max(1, Math.round(canvas.clientWidth * dpr));
  const bh = Math.max(1, Math.round(canvas.clientHeight * dpr));
  if (canvas.width !== bw || canvas.height !== bh) {
    canvas.width = bw;
    canvas.height = bh;
  }
}

/**
 * Renders text entities on a 2D canvas overlay above the WebGL canvas.
 * Uses the system font stack so CJK content renders correctly.
 */
export function drawTexts(
  ctx: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  scene: Scene,
  layoutIdx: number,
  cam: Camera,
  opts: TextDrawOptions,
) {
  const dpr = opts.dpr;
  const vw = opts.viewport?.w ?? canvas.clientWidth;
  const vh = opts.viewport?.h ?? canvas.clientHeight;
  if (opts.viewport) {
    // Offscreen export surface: it has no layout box, so the size is explicit.
    canvas.width = Math.max(1, Math.round(vw * dpr));
    canvas.height = Math.max(1, Math.round(vh * dpr));
  } else {
    syncCanvasSize(canvas, dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, vw, vh);

  const layout = scene.meta.layouts[layoutIdx];
  if (!layout) return;
  // Viewport bounds in world space for culling.
  const tl = cam.worldFromScreen(0, 0);
  const br = cam.worldFromScreen(vw, vh);
  const margin = 2;

  // Bug 4: 阈值以 CSS 像素（无关 DPR）衡量，避免 Retina 下本应显示的
  // 文字因为 hPx * dpr 被过滤成透明。
  const MIN_CSS_PX = 2.0;
  const MAX_CSS_PX = 4000.0;

  for (const item of scene.meta.texts) {
    if (item.layout !== layoutIdx || opts.hidden.has(item.layer)) continue;
    const hPx = item.h * cam.scale;
    if (hPx < MIN_CSS_PX || hPx > MAX_CSS_PX) continue;
    if (item.x < tl.x - margin || item.x > br.x + margin) continue;
    if (item.y < br.y - margin || item.y > tl.y + margin) continue;

    const p = cam.screenFromWorld(item.x, item.y);
    const lines = item.text.split("\n");
    const lineH = hPx * 1.66;
    const fill =
      opts.mono || item.a === 0 ? opts.autoColor : `rgb(${item.r},${item.g},${item.b})`;
    ctx.fillStyle = fill;
    ctx.font = `${hPx.toFixed(2)}px -apple-system, "SF Pro Display", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif`;
    ctx.textAlign = item.ha === 1 ? "center" : item.ha === 2 ? "right" : "left";

    ctx.save();
    ctx.translate(p.sx, p.sy);
    if (item.rot) ctx.rotate((-item.rot * Math.PI) / 180);

    for (let i = 0; i < lines.length; i++) {
      const n = lines.length;
      let baseline: CanvasTextBaseline;
      let dy: number;
      if (item.va === 1) {
        // bottom: stack grows upward
        baseline = "bottom";
        dy = -(n - 1 - i) * lineH;
      } else if (item.va === 2) {
        // middle: block centered on anchor
        baseline = "middle";
        dy = (i - (n - 1) / 2) * lineH;
      } else if (item.va === 3) {
        // top: stack grows downward
        baseline = "top";
        dy = i * lineH;
      } else {
        // baseline: first line on the anchor baseline
        baseline = "alphabetic";
        dy = i * lineH;
      }
      ctx.textBaseline = baseline;
      ctx.fillText(lines[i], 0, dy);
    }
    ctx.restore();
  }
}

/** Overlay for the active measure segment, its live readout and the snap marker. */
export interface MeasureOverlay {
  /** First picked point, or null. */
  pending: { x: number; y: number } | null;
  /** Completed measurement. */
  result: { x1: number; y1: number; x2: number; y2: number; dist: number } | null;
  /** Current pointer (already snapped when `snap` is set). */
  cursor: { x: number; y: number } | null;
  snap: SnapHit | null;
  dpr: number;
  red: string;
  /** Distance text for whichever segment is active. */
  label: string | null;
  /** Caption for the snap marker (e.g. "端点"). */
  snapLabel: string | null;
}

export function drawMeasure(ctx: CanvasRenderingContext2D, cam: Camera, o: MeasureOverlay) {
  ctx.setTransform(o.dpr, 0, 0, o.dpr, 0, 0);
  // No clearRect here: this overlay runs *after* drawTexts on the same canvas
  // and must not wipe the just-drawn text.
  ctx.lineWidth = 1.25;
  ctx.font = `11px "SF Mono", ui-monospace, monospace`;

  const seg = (x1: number, y1: number, x2: number, y2: number) => {
    const a = cam.screenFromWorld(x1, y1);
    const b = cam.screenFromWorld(x2, y2);
    ctx.strokeStyle = o.red;
    ctx.beginPath();
    ctx.moveTo(a.sx, a.sy);
    ctx.lineTo(b.sx, b.sy);
    ctx.stroke();
    for (const pt of [a, b]) {
      ctx.fillStyle = o.red;
      ctx.beginPath();
      ctx.arc(pt.sx, pt.sy, 3.2, 0, Math.PI * 2);
      ctx.fill();
    }
  };

  const label = (text: string, x: number, y: number) => {
    ctx.textAlign = "center";
    ctx.textBaseline = "bottom";
    ctx.lineWidth = 3.5;
    ctx.strokeStyle = "rgba(255,255,255,0.9)";
    ctx.strokeText(text, x, y);
    ctx.fillStyle = o.red;
    ctx.fillText(text, x, y);
  };

  const active = o.result
    ? ({ x1: o.result.x1, y1: o.result.y1, x2: o.result.x2, y2: o.result.y2 } as const)
    : o.pending && o.cursor
      ? ({ x1: o.pending.x, y1: o.pending.y, x2: o.cursor.x, y2: o.cursor.y } as const)
      : null;

  if (active) {
    seg(active.x1, active.y1, active.x2, active.y2);
    if (o.label) {
      const mid = cam.screenFromWorld((active.x1 + active.x2) / 2, (active.y1 + active.y2) / 2);
      label(o.label, mid.sx, mid.sy - 8);
    }
  }

  if (o.snap) drawSnapMarker(ctx, cam, o.snap, o.red, o.snapLabel);
}

/**
 * AutoCAD-style aperture marker: a square for endpoints, a triangle for
 * midpoints, haloed so it stays readable over dense linework.
 */
function drawSnapMarker(
  ctx: CanvasRenderingContext2D,
  cam: Camera,
  snap: SnapHit,
  red: string,
  caption: string | null,
) {
  const p = cam.screenFromWorld(snap.x, snap.y);
  const r = 5.5;
  const path = () => {
    ctx.beginPath();
    if (snap.kind === "midpoint") {
      ctx.moveTo(p.sx, p.sy - r);
      ctx.lineTo(p.sx + r, p.sy + r * 0.85);
      ctx.lineTo(p.sx - r, p.sy + r * 0.85);
      ctx.closePath();
    } else {
      ctx.rect(p.sx - r, p.sy - r, r * 2, r * 2);
    }
  };

  path();
  ctx.lineWidth = 3.5;
  ctx.strokeStyle = "rgba(255,255,255,0.9)";
  ctx.stroke();
  path();
  ctx.lineWidth = 1.6;
  ctx.strokeStyle = red;
  ctx.stroke();

  if (caption) {
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    ctx.lineWidth = 3.5;
    ctx.strokeStyle = "rgba(255,255,255,0.9)";
    ctx.strokeText(caption, p.sx + r + 5, p.sy - r - 4);
    ctx.fillStyle = red;
    ctx.fillText(caption, p.sx + r + 5, p.sy - r - 4);
  }
}
