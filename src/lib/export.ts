import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

import { Camera } from "./camera";
import { Renderer } from "./renderer";
import { drawTexts } from "./text";
import type { Scene } from "./scene";

export type ExportScope = "drawing" | "view";
export type ExportTheme = "dark" | "print";
export type ExportFormat = "png" | "pdf";

/** Long side of a 1× export; the factor scales it, `MAX_PX` caps it. */
const BASE_LONG_PX = 1600;
/** Beyond this the base64 hop over IPC stops being worth it. */
const MAX_PX = 6000;

/** Live camera snapshot, so the exporter can reproduce the current view. */
export interface ViewState {
  cx: number;
  cy: number;
  scale: number;
  vw: number;
  vh: number;
}

export interface ExportOptions {
  scene: Scene;
  layoutIdx: number;
  hidden: Set<number>;
  scope: ExportScope;
  theme: ExportTheme;
  factor: number;
  view: ViewState;
  /** Source drawing name, used for the suggested file name. */
  fileName: string;
}

interface Palette {
  bg: [number, number, number];
  auto: [number, number, number];
  autoCss: string;
  mono: boolean;
}

export function paletteOf(theme: ExportTheme): Palette {
  // "print" is AutoCAD's monochrome plot style: white paper, every entity black.
  return theme === "print"
    ? { bg: [255, 255, 255], auto: [0, 0, 0], autoCss: "rgba(0,0,0,0.92)", mono: true }
    : { bg: [10, 10, 10], auto: [255, 255, 255], autoCss: "rgba(255,255,255,0.95)", mono: false };
}

/** The world-space box an export covers. */
function exportBox(o: ExportOptions) {
  const lay = o.scene.meta.layouts[o.layoutIdx];
  if (o.scope === "drawing" || !o.view.scale) {
    return { min_x: lay.min_x, min_y: lay.min_y, max_x: lay.max_x, max_y: lay.max_y };
  }
  const halfW = o.view.vw / 2 / o.view.scale;
  const halfH = o.view.vh / 2 / o.view.scale;
  return {
    min_x: o.view.cx - halfW,
    min_y: o.view.cy - halfH,
    max_x: o.view.cx + halfW,
    max_y: o.view.cy + halfH,
  };
}

export interface RenderedExport {
  canvas: HTMLCanvasElement;
  width: number;
  height: number;
}

/**
 * Render a drawing offscreen at export resolution.
 *
 * Runs the same WebGL renderer and the same 2D text overlay as the viewport, on
 * detached canvases — so what lands in the file is what the app draws, not a
 * screenshot of a particular window size.
 */
export function renderScene(o: ExportOptions): RenderedExport {
  const box = exportBox(o);
  const spanX = Math.max(box.max_x - box.min_x, 1e-9);
  const spanY = Math.max(box.max_y - box.min_y, 1e-9);
  const baseLong = o.scope === "drawing" ? BASE_LONG_PX : Math.max(o.view.vw, o.view.vh, 1);
  const long = Math.max(64, Math.min(Math.round(baseLong * o.factor), MAX_PX));
  const w = spanX >= spanY ? long : Math.max(1, Math.round((long * spanX) / spanY));
  const h = spanX >= spanY ? Math.max(1, Math.round((long * spanY) / spanX)) : long;

  const pal = paletteOf(o.theme);
  const gl = document.createElement("canvas");
  gl.width = w;
  gl.height = h;

  const renderer = new Renderer();
  if (!renderer.init(gl)) {
    throw new Error("WebGL 初始化失败 / WebGL init failed");
  }
  try {
    renderer.uploadScene(o.scene);
    const cam = new Camera();
    cam.setViewport(w, h);
    // pad = 1: the file gets its own margins (PDF page / image edge), so an
    // extra 8 % inset here would just shrink the drawing.
    cam.fit(box.min_x, box.min_y, box.max_x, box.max_y, 1);
    renderer.draw(gl, cam, o.layoutIdx, {
      autoColor: pal.auto,
      background: pal.bg,
      hidden: o.hidden,
      dpr: 1,
      mono: pal.mono,
      size: { w, h },
    });

    const textCanvas = document.createElement("canvas");
    const tctx = textCanvas.getContext("2d");
    if (tctx) {
      drawTexts(tctx, textCanvas, o.scene, o.layoutIdx, cam, {
        autoColor: pal.autoCss,
        hidden: o.hidden,
        dpr: 1,
        viewport: { w, h },
        mono: pal.mono,
      });
    }

    const out = document.createElement("canvas");
    out.width = w;
    out.height = h;
    const ctx = out.getContext("2d");
    if (!ctx) throw new Error("无法创建导出画布 / canvas unavailable");
    ctx.fillStyle = `rgb(${pal.bg[0]},${pal.bg[1]},${pal.bg[2]})`;
    ctx.fillRect(0, 0, w, h);
    // The WebGL drawing buffer stays readable until the browser composites, so
    // copying it in this same task avoids needing preserveDrawingBuffer.
    ctx.drawImage(gl, 0, 0);
    ctx.drawImage(textCanvas, 0, 0);
    return { canvas: out, width: w, height: h };
  } finally {
    renderer.dispose();
  }
}

function toBlob(canvas: HTMLCanvasElement, format: ExportFormat): Promise<Blob> {
  const type = format === "png" ? "image/png" : "image/jpeg";
  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (b) => (b ? resolve(b) : reject(new Error("图像编码失败 / encoding failed"))),
      type,
      0.92,
    );
  });
}

/** base64 for the IPC hop — chunked so a big export cannot blow the call stack. */
function toBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let bin = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}

function suggestedName(fileName: string, format: ExportFormat): string {
  const stem = (fileName || "drawing").replace(/\.[^.]+$/, "");
  return `${stem}.${format}`;
}

/**
 * Render, ask for a destination and write the file.
 *
 * Returns the written path, or null when the user cancels the save dialog.
 */
export async function saveExport(o: ExportOptions, format: ExportFormat): Promise<string | null> {
  const { canvas, width, height } = renderScene(o);
  const blob = await toBlob(canvas, format);
  const path = await save({
    defaultPath: suggestedName(o.fileName, format),
    filters:
      format === "png"
        ? [{ name: "PNG", extensions: ["png"] }]
        : [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (!path) return null;
  const bytes = new Uint8Array(await blob.arrayBuffer());
  return (await invoke("save_export", {
    path,
    dataBase64: toBase64(bytes),
    kind: format,
    pxW: width,
    pxH: height,
    title: o.fileName,
  })) as string;
}
