import { SvelteSet } from "svelte/reactivity";
import { invoke } from "@tauri-apps/api/core";
import type { Scene } from "./scene";
import { parseBlob } from "./scene";
import { t } from "./i18n.svelte";

export type Tool = "pan" | "measure";

/** localStorage throws when cookies/site data are blocked — never let that
 *  take the whole app down at module load. */
function stored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function initialLang(): "zh" | "en" {
  const saved = stored("rcv.lang");
  if (saved === "zh" || saved === "en") return saved;
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

export const app = $state({
  lang: initialLang(),
  loading: false,
  loadingMsg: "",
  error: "",
  fileName: "",
  filePath: "",
  scene: null as Scene | null,
  activeLayout: 0,
  hidden: new SvelteSet<number>(),
  tool: "pan" as Tool,
  measureResult: null as null | { x1: number; y1: number; x2: number; y2: number; dist: number },
  coords: { x: 0, y: 0 },
  zoomPct: 100,
  aboutOpen: false,
  exportOpen: false,
  panelOpen: true,
  /** Live camera snapshot — the exporter reproduces the current view from it. */
  camera: { cx: 0, cy: 0, scale: 0, vw: 1, vh: 1 },
  dragging: false,
  /** Bumped to request "fit the whole drawing" (includes stray geometry). */
  fitTick: 0,
  /** Bumped to request "fit the content" (trims far-flung outliers). */
  fitCoreTick: 0,
  status: "",
  recents: [] as RecentFile[],
});

export function setLang(lang: "zh" | "en") {
  app.lang = lang;
  try {
    localStorage.setItem("rcv.lang", lang);
  } catch {
    /* storage unavailable */
  }
  document.documentElement.lang = lang;
}

// ---------------------------------------------------------------- recents

export interface RecentFile {
  path: string;
  name: string;
  ts: number;
}

export function getRecent(): RecentFile[] {
  const raw = stored("rcv.recent");
  if (!raw) return [];
  try {
    return JSON.parse(raw) as RecentFile[];
  } catch {
    return [];
  }
}

export function addRecent(path: string) {
  const name = path.split(/[\\/]/).pop() ?? path;
  const list = [{ path, name, ts: Date.now() }, ...getRecent().filter((r) => r.path !== path)];
  const kept = list.slice(0, 6);
  try {
    localStorage.setItem("rcv.recent", JSON.stringify(kept));
  } catch {
    /* storage unavailable */
  }
  // Mirror into state: the empty-state list is rendered from here, so it can
  // no longer go stale after opening a file.
  app.recents = kept;
}

app.recents = getRecent();

// ---------------------------------------------------------------- open

/** Loads a drawing file through the Rust backend (auto-converts DWG via dwg2dxf). */
let statusTimer: ReturnType<typeof setTimeout> | undefined;
export function setStatus(msg: string) {
  app.status = msg;
  clearTimeout(statusTimer);
  statusTimer = setTimeout(() => (app.status = ""), 3000);
}

export async function openFile(path: string) {
  // One load at a time: a second drop while a big drawing is parsing would
  // race the first, and whichever finished last would win.
  if (app.loading) return;
  const lower = path.toLowerCase();
  if (!lower.endsWith(".dxf") && !lower.endsWith(".dwg")) {
    app.error = t("unsupportedFmt");
    return;
  }
  const wasDwg = lower.endsWith(".dwg");
  app.error = "";
  app.loading = true;
  app.loadingMsg = wasDwg ? t("converting") : t("parsing");
  app.fileName = path.split(/[\\/]/).pop() ?? path;
  app.filePath = path;
  try {
    const res = await invoke("open_drawing", { path });
    app.loadingMsg = t("building");
    const scene = parseBlob(res as ArrayBuffer);
    app.scene = scene;
    app.activeLayout = 0;
    app.hidden.clear();
    // Layers switched off inside the drawing start hidden ("Show all" reveals).
    scene.meta.layers.forEach((l, i) => {
      if (l.off) app.hidden.add(i);
    });
    app.measureResult = null;
    app.tool = "pan";
    addRecent(path);
  } catch (e) {
    app.scene = null;
    app.error = `${t("errTitle")}: ${e}`;
  } finally {
    app.loading = false;
  }
}
