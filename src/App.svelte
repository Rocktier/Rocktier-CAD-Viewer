<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Stage from "./components/Stage.svelte";
  import LayersPanel from "./components/LayersPanel.svelte";
  import AboutDialog from "./components/AboutDialog.svelte";
  import { app, openFile, setTheme, setLang } from "./lib/state.svelte";
  import { t } from "./lib/i18n.svelte";
  import { formatCoord, formatCount, formatDuration } from "./lib/format";

  const VERSION = __APP_VERSION__;

  async function pickFile() {
    const sel = await open({
      multiple: false,
      filters: [{ name: "CAD", extensions: ["dxf", "dxb", "dwg"] }],
    });
    if (typeof sel === "string") await openFile(sel);
  }

  function toggleTool() {
    app.tool = app.tool === "measure" ? "pan" : "measure";
  }

  onMount(() => {
    let unlisten: (() => void) | null = null;
    getCurrentWindow()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") {
          app.dragging = true;
        } else if (p.type === "leave") {
          app.dragging = false;
        } else if (p.type === "drop") {
          app.dragging = false;
          const path = p.paths?.[0];
          if (path) void openFile(path);
        }
      })
      .then((u) => (unlisten = u));
    return () => unlisten?.();
  });
</script>

<div class="app" data-theme={app.theme}>
  <header class="app-head">
    <div class="brand">
      <svg class="brand-mark" viewBox="0 0 32 32" fill="none">
        <path d="M6 22 L13 7 L26 12 L22 25 Z" fill="currentColor" />
      </svg>
      <span class="brand-word">Rocktier CAD Viewer<i></i></span>
    </div>
    <span class="brand-sub">DXF · DWG · OFFLINE</span>

    {#if app.filePath}
      <span class="file-chip" title={app.filePath}>{app.fileName}</span>
    {/if}

    <div class="head-actions">
      <button class="btn" onclick={pickFile}>{t("open")}</button>
      {#if app.scene}
        <button
          class="ghost"
          class:active={app.tool === "measure"}
          title={t("measure")} aria-label={t("measure")}
          onclick={toggleTool}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21.3 8.7 15.3 14.7 9.3 8.7 15.3 2.7 21.3 8.7z" />
            <path d="m2.7 21.3 6-6" />
          </svg>
        </button>
        <button
          class="ghost"
          title={t("fitView")} aria-label={t("fitView")}
          onclick={() => app.fitTick++}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3" />
          </svg>
        </button>
        <button
          class="ghost"
          class:active={app.panelOpen}
          title={t("panel")} aria-label={t("panel")}
          onclick={() => (app.panelOpen = !app.panelOpen)}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="3" y="3" width="18" height="18" rx="2" />
            <line x1="15" y1="3" x2="15" y2="21" />
          </svg>
        </button>
      {/if}
      <button
        class="ghost"
        title={t("theme")} aria-label={t("theme")}
        onclick={() => setTheme(app.theme === "dark" ? "light" : "dark")}
      >
        {#if app.theme === "dark"}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="5" />
            <path d="M12 1v2m0 18v2M4.22 4.22l1.42 1.42m12.72 12.72 1.42 1.42M1 12h2m18 0h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42" />
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
          </svg>
        {/if}
      </button>
      <button class="ghost lang-btn" title={t("langLabel")} onclick={() => setLang(app.lang === "zh" ? "en" : "zh")}>
        {app.lang === "zh" ? "EN" : "中文"}
      </button>
      <button class="ghost" title={t("about")} aria-label={t("about")} onclick={() => (app.aboutOpen = true)}>i</button>
    </div>
  </header>

  <div class="app-body">
    <Stage onOpen={pickFile} />
    {#if app.scene && app.panelOpen}
      <LayersPanel />
    {/if}
  </div>

  <footer class="statusbar">
    {#if app.scene}
      <span class="status-item"><i class="status-red"></i>{app.scene.meta.layouts[app.activeLayout]?.name ?? "—"}</span>
      <span class="status-item">X {formatCoord(app.coords.x)}</span>
      <span class="status-item">Y {formatCoord(app.coords.y)}</span>
      <span class="grow"></span>
      {#if app.scene.meta.truncated}
        <span class="status-item status-warn">⚠ {t("truncated")}</span>
      {/if}
      <span class="status-item">{t("zoom")} {app.zoomPct}%</span>
      <span class="status-item">{t("segs")} {formatCount(app.scene.meta.segments)}</span>
      <span class="status-item">{t("texts")} {formatCount(app.scene.meta.text_count)}</span>
      <span class="status-item">{formatDuration(app.scene.meta.parse_ms + app.scene.meta.tess_ms + app.scene.meta.convert_ms)}{app.scene.meta.was_dwg ? " (dwg)" : ""}</span>
    {:else}
      <span class="status-item"><i class="status-red"></i>{t("offline")}</span>
      <span class="grow"></span>
      <span class="status-item">v{VERSION}</span>
    {/if}
  </footer>

  <AboutDialog />
</div>
