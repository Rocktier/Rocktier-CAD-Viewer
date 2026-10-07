import { SvelteSet } from "svelte/reactivity";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Scene } from "./scene";
import { parseBlob } from "./scene";
import { t } from "./i18n.svelte";
import type { LicenseInfo } from "./license";

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

/* 语言集合的单一真源。家族标准 8 门（en/zh/ja/ko/de/es/pt/ar）——
   与 PDF 的 src/i18n/locales.ts 同构，两边保持一致。
   注意：这里不用 `as const` —— .svelte.ts 里 svelte-check 不接受它
   （"const assertion can only be applied to…"），改成显式类型。 */
type LangCode = "en" | "zh" | "ja" | "ko" | "de" | "es" | "pt" | "ar";
export type { LangCode };
export const LOCALES: ReadonlyArray<{ code: LangCode; endonym: string }> = [
  { code: "en", endonym: "English" },
  { code: "zh", endonym: "中文" },
  { code: "ja", endonym: "日本語" },
  { code: "ko", endonym: "한국어" },
  { code: "de", endonym: "Deutsch" },
  { code: "es", endonym: "Español" },
  { code: "pt", endonym: "Português" },
  { code: "ar", endonym: "العربية" },
];

const CODES: readonly string[] = LOCALES.map((l) => l.code);

function isLang(v: unknown): v is LangCode {
  return typeof v === "string" && CODES.includes(v);
}

/* English is the family default (family.json `defaultLanguage: "en"`), and the
   family deliberately does NOT follow the system locale — the user's manual
   choice is the only thing remembered. Previously this returned "zh" for any
   zh-* system locale, which is backwards for a product aimed overseas. */
function initialLang(): LangCode {
  const saved = stored("rcv.lang");
  if (isLang(saved)) return saved;
  return "en";
}

export const app = $state({
  lang: initialLang(),
  /** Turn upside-down text upright for readability ("按图纸原样" turns it off). */
  uprightText: stored("rcv.upright") !== "0",
  loading: false,
  loadingMsg: "",
  /** Live load progress from the Rust side: `pct` < 0 means indeterminate. */
  progress: { pct: -1, phase: "", detail: "" },
  error: "",
  /** Last path handed to `openFile` — lets the error card offer a retry. */
  lastPath: "",
  fileName: "",
  filePath: "",
  scene: null as Scene | null,
  activeLayout: 0,
  hidden: new SvelteSet<number>(),
  tool: "pan" as Tool,
  measureResult: null as null | { x1: number; y1: number; x2: number; y2: number; dist: number },
  /** Cursor position in world units; `null` while the pointer is off-canvas. */
  coords: null as null | { x: number; y: number },
  zoomPct: 100,
  aboutOpen: false,
  exportOpen: false,
  /** 家族 L6 授权：当前状态（null = 尚未取到，或浏览器 dev）+ 对话框开关。 */
  license: null as LicenseInfo | null,
  licenseOpen: false,
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

export function setLang(lang: LangCode) {
  app.lang = lang;
  try {
    localStorage.setItem("rocktier.lang", lang);
  } catch {
    /* storage unavailable */
  }
  document.documentElement.lang = lang;
}

export function setUprightText(on: boolean) {
  app.uprightText = on;
  try {
    localStorage.setItem("rocktier.upright", on ? "1" : "0");
  } catch {
    /* storage unavailable */
  }
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
    localStorage.setItem("rocktier.recent", JSON.stringify(kept));
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
  // race the first, and whichever finished last would win.  Say so instead of
  // swallowing the request — a silent no-op reads as a broken button.
  if (app.loading) {
    setStatus(t("busyOpening"));
    return;
  }
  const lower = path.toLowerCase();
  if (!lower.endsWith(".dxf") && !lower.endsWith(".dwg")) {
    app.error = t("unsupportedFmt");
    return;
  }
  const wasDwg = lower.endsWith(".dwg");
  app.error = "";
  app.loading = true;
  app.loadingMsg = wasDwg ? t("converting") : t("parsing");
  app.progress = { pct: 0, phase: wasDwg ? "convert" : "parse", detail: "" };
  app.fileName = path.split(/[\\/]/).pop() ?? path;
  app.filePath = path;
  app.lastPath = path;
  // Subscribe before invoking: the backend emits phases from a worker thread.
  const unlisten = await listen<{ pct: number; phase: string; detail: string }>(
    "load-progress",
    (e) => {
      app.progress = e.payload;
      app.loadingMsg =
        e.payload.phase === "convert"
          ? t("converting")
          : e.payload.phase === "build"
            ? t("building")
            : t("parsing");
    },
  );
  try {
    const res = await invoke("open_drawing", { path });
    app.loadingMsg = t("building");
    app.progress = { pct: 97, phase: "build", detail: "" };
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
    unlisten();
    app.loading = false;
  }
}

/** Re-open the drawing that just failed (error card "retry" action). */
export function retryOpen() {
  if (app.lastPath) void openFile(app.lastPath);
}
