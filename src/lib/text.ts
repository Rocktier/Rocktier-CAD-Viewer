import type { Scene } from "./scene";
import type { Camera } from "./camera";

export interface TextDrawOptions {
  autoColor: string;
  hidden: Set<number>;
  dpr: number;
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
  const vw = canvas.clientWidth;
  const vh = canvas.clientHeight;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, vw, vh);

  const layout = scene.meta.layouts[layoutIdx];
  if (!layout) return;
  // Viewport bounds in world space for culling.
  const tl = cam.worldFromScreen(0, 0);
  const br = cam.worldFromScreen(vw, vh);
  const margin = 2;

  for (const item of scene.meta.texts) {
    if (item.layout !== layoutIdx || opts.hidden.has(item.layer)) continue;
    const hPx = item.h * cam.scale;
    if (hPx < 3.4 || hPx > 4000) continue;
    if (item.x < tl.x - margin || item.x > br.x + margin) continue;
    if (item.y < br.y - margin || item.y > tl.y + margin) continue;

    const p = cam.screenFromWorld(item.x, item.y);
    const lines = item.text.split("\n");
    const lineH = hPx * 1.66;
    const fill = item.a === 0 ? opts.autoColor : `rgb(${item.r},${item.g},${item.b})`;
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

/** Overlay for the active measure segment + result. */
export function drawMeasure(
  ctx: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  cam: Camera,
  pending: { x: number; y: number } | null,
  result: { x1: number; y1: number; x2: number; y2: number; dist: number } | null,
  cursor: { x: number; y: number } | null,
  dpr: number,
  red: string,
  label: string | null,
) {
  const vw = canvas.clientWidth;
  const vh = canvas.clientHeight;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, vw, vh);
  ctx.lineWidth = 1.25;
  ctx.font = `11px "SF Mono", ui-monospace, monospace`;

  const seg = (x1: number, y1: number, x2: number, y2: number) => {
    const a = cam.screenFromWorld(x1, y1);
    const b = cam.screenFromWorld(x2, y2);
    ctx.strokeStyle = red;
    ctx.beginPath();
    ctx.moveTo(a.sx, a.sy);
    ctx.lineTo(b.sx, b.sy);
    ctx.stroke();
    for (const pt of [a, b]) {
      ctx.fillStyle = red;
      ctx.beginPath();
      ctx.arc(pt.sx, pt.sy, 3.2, 0, Math.PI * 2);
      ctx.fill();
    }
  };

  if (result) {
    seg(result.x1, result.y1, result.x2, result.y2);
    if (label) {
      const mid = cam.screenFromWorld((result.x1 + result.x2) / 2, (result.y1 + result.y2) / 2);
      ctx.fillStyle = red;
      ctx.textAlign = "center";
      ctx.textBaseline = "bottom";
      ctx.fillText(label, mid.sx, mid.sy - 8);
    }
  } else if (pending && cursor) {
    seg(pending.x, pending.y, cursor.x, cursor.y);
  }
}
