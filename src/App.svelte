<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Stage from "./components/Stage.svelte";
  import LayersPanel from "./components/LayersPanel.svelte";
  import AboutDialog from "./components/AboutDialog.svelte";
  import { app, openFile, setStatus, setLang } from "./lib/state.svelte";
  import { t } from "./lib/i18n.svelte";
  import { formatCoord, formatCount, formatDuration } from "./lib/format";

  const VERSION = __APP_VERSION__;

  async function pickFile() {
    const sel = await open({
      multiple: false,
      filters: [{ name: "CAD", extensions: ["dxf", "dwg"] }],
    });
    if (typeof sel === "string") await openFile(sel);
  }

  function toggleTool() {
    app.tool = app.tool === "measure" ? "pan" : "measure";
  }

  /** Open a drawing the OS handed us (file association / "Open with"). */
  function openFirst(paths: string[] | null | undefined) {
    if (!paths?.length) return;
    if (paths.length > 1) setStatus(t("oneFileOnly"));
    void openFile(paths[0]);
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    let dragTimer: ReturnType<typeof setTimeout> | undefined;
    // If the component dies before `unlisten` resolves, drop the listener
    // immediately instead of leaving it attached to a dead view.
    const keep = (p: Promise<() => void>) =>
      p.then((u) => (cancelled ? u() : unlisteners.push(u))).catch(() => {});

    keep(
      getCurrentWindow().onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") {
          app.dragging = true;
          // A drag cancelled with Esc leaves no `leave`/`drop` behind; without
          // this the drop overlay would cover the canvas forever.
          clearTimeout(dragTimer);
          dragTimer = setTimeout(() => (app.dragging = false), 1200);
        } else if (p.type === "leave" || p.type === "drop") {
          clearTimeout(dragTimer);
          app.dragging = false;
          if (p.type === "drop") openFirst(p.paths);
        }
      }),
    );

    // Files opened before the UI existed (cold start) …
    invoke<string[]>("opened_files")
      .then(openFirst)
      .catch(() => {});
    // … and while it was already running.
    keep(listen<string[]>("opened", (e) => openFirst(e.payload)));

    return () => {
      cancelled = true;
      clearTimeout(dragTimer);
      unlisteners.forEach((u) => u());
    };
  });
</script>

<div class="app">
  <header class="app-head">
    <div class="brand">
      <img class="brand-mark" src="/icon.svg" alt="" aria-hidden="true" />
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
      <button class="ghost lang-btn" title={t("langLabel")} aria-label={t("langLabel")} onclick={() => setLang(app.lang === "zh" ? "en" : "zh")}>
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

  {#if app.status}
    <div class="app-snackbar" role="status">{app.status}</div>
  {/if}

  <footer class="statusbar">
    {#if app.scene}
      <span class="status-item"><i class="status-red"></i>{app.scene.meta.layouts[app.activeLayout]?.name ?? "—"}</span>
      <span class="status-item">X {formatCoord(app.coords.x)}</span>
      <span class="status-item">Y {formatCoord(app.coords.y)}</span>
      <span class="grow"></span>
      {#if app.scene.meta.truncated}
        <span class="status-item status-warn">⚠ {t("truncated")}</span>
      {/if}
      {#if app.scene.meta.skipped > 0}
        <span class="status-item status-warn" title={t("skippedHint")}>
          ⚠ {t("skipped")} {formatCount(app.scene.meta.skipped)}
        </span>
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
