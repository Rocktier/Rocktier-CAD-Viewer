import { SvelteSet } from "svelte/reactivity";
import { invoke } from "@tauri-apps/api/core";
import type { Scene } from "./scene";
import { parseBlob } from "./scene";
import { t } from "./i18n.svelte";

export type Tool = "pan" | "measure";

function initialLang(): "zh" | "en" {
  const saved = localStorage.getItem("rcv.lang");
  if (saved === "zh" || saved === "en") return saved;
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

function initialTheme(): "dark" | "light" {
  const saved = localStorage.getItem("rcv.theme");
  if (saved === "dark" || saved === "light") return saved;
  return "dark";
}

export const app = $state({
  theme: initialTheme(),
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
  panelOpen: true,
  dragging: false,
  fitTick: 0,
});

export function setTheme(theme: "dark" | "light") {
  app.theme = theme;
  localStorage.setItem("rcv.theme", theme);
  document.documentElement.dataset.theme = theme;
}

export function setLang(lang: "zh" | "en") {
  app.lang = lang;
  localStorage.setItem("rcv.lang", lang);
}

// ---------------------------------------------------------------- recents

export interface RecentFile {
  path: string;
  name: string;
  ts: number;
}

export function getRecent(): RecentFile[] {
  try {
    const raw = localStorage.getItem("rcv.recent");
    return raw ? (JSON.parse(raw) as RecentFile[]) : [];
  } catch {
    return [];
  }
}

export function addRecent(path: string) {
  const name = path.split(/[\\/]/).pop() ?? path;
  const list = [{ path, name, ts: Date.now() }, ...getRecent().filter((r) => r.path !== path)];
  localStorage.setItem("rcv.recent", JSON.stringify(list.slice(0, 6)));
}

// ---------------------------------------------------------------- open

/** Loads a drawing file through the Rust backend. */
export async function openFile(path: string) {
  const lower = path.toLowerCase();
  if (lower.endsWith(".dwg")) {
    app.error = t("dwgSoon");
    return;
  }
  if (!lower.endsWith(".dxf") && !lower.endsWith(".dxb")) {
    app.error = "DXF only · 仅支持 DXF";
    return;
  }
  app.error = "";
  app.loading = true;
  app.loadingMsg = t("parsing");
  app.fileName = path.split(/[\\/]/).pop() ?? path;
  app.filePath = path;
  try {
    const res = await invoke("open_drawing", { path });
    app.loadingMsg = t("building");
    const scene = parseBlob(res as ArrayBuffer);
    app.scene = scene;
    app.activeLayout = 0;
    app.hidden.clear();
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
