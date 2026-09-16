<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import BrandMark from "./components/BrandMark.svelte";
  import Stage from "./components/Stage.svelte";
  import LayersPanel from "./components/LayersPanel.svelte";
  import AboutDialog from "./components/AboutDialog.svelte";
  import ExportDialog from "./components/ExportDialog.svelte";
  import { app, openFile, setStatus, setLang, setUprightText } from "./lib/state.svelte";
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

    // 家族标准菜单：按 UI 语言构建，自定义项经 menu-action 事件回到这里。
    invoke("build_menu", { lang: app.lang }).catch(() => {});
    keep(
      listen<string>("menu-action", (e) => {
        switch (e.payload) {
          case "open": void pickFile(); break;
          case "fit": app.fitTick++; break;
          case "fit-core": app.fitCoreTick++; break;
          case "panel": app.panelOpen = !app.panelOpen; break;
          case "website": void invoke("open_url", { url: "https://rocktier.com/" }).catch(() => {}); break;
          case "feedback": void invoke("open_url", { url: "mailto:hello@rocktier.com" }).catch(() => {}); break;
        }
      }),
    );

    // 卸载标记：配合下面的 keep() 清理
    let disposed = false;

    // Files opened before the UI existed (cold start) …
    invoke<string[]>("opened_files")
      .then((paths) => {
        // 组件已卸载则丢弃结果：否则卸载后仍会触发打开
        if (!disposed) openFirst(paths);
      })
      .catch(() => {});
    // … and while it was already running.
    keep(listen<string[]>("opened", (e) => openFirst(e.payload)));

    return () => {
      disposed = true;
      cancelled = true;
      clearTimeout(dragTimer);
      unlisteners.forEach((u) => u());
    };
  });
</script>

<div class="app">
  <header class="app-head">
    <div class="brand">
      <BrandMark />
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
          title={t("exportTitle")} aria-label={t("exportTitle")}
          onclick={() => (app.exportOpen = true)}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 3v12" />
            <path d="m7 10 5 5 5-5" />
            <path d="M4 19h16" />
          </svg>
        </button>
        <button
          class="ghost"
          title={t("fitContent")} aria-label={t("fitContent")}
          onclick={() => app.fitCoreTick++}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3" />
            <circle cx="12" cy="12" r="2.5" />
          </svg>
        </button>
        <button
          class="ghost"
          class:active={app.uprightText}
          title={t("uprightText")} aria-label={t("uprightText")}
          onclick={() => setUprightText(!app.uprightText)}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="m5 7 7 10m0-10-7 10M6.2 14h11.6" />
            <path d="M17 3v4m0 0 1.8-1.8M17 7l-1.8-1.8" />
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
      <button class="ghost lang-btn" title={t("langLabel")} aria-label={t("langLabel")} onclick={() => { setLang(app.lang === "zh" ? "en" : "zh"); invoke("build_menu", { lang: app.lang }).catch(() => {}); }}>
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
      {#if app.scene.meta.layouts.length > 1}
        <span class="layout-tabs">
          {#each app.scene.meta.layouts as lay, i}
            <button
              class="layout-tab"
              class:active={app.activeLayout === i}
              title={lay.name}
              onclick={() => (app.activeLayout = i)}
            >{lay.name}</button>
          {/each}
        </span>
      {:else}
        <span class="status-item"><i class="status-red"></i>{app.scene.meta.layouts[0]?.name ?? "—"}</span>
      {/if}
      <span class="status-item">X {formatCoord(app.coords.x)}</span>
      <span class="status-item">Y {formatCoord(app.coords.y)}</span>
      <span class="grow"></span>
      {#if app.scene.meta.skipped > 0}
        <span class="status-item status-muted" title={t("skippedHint")}>
          {t("skipped")} {formatCount(app.scene.meta.skipped)}
        </span>
      {/if}
      <span class="status-item">{t("zoom")} {app.zoomPct}%</span>
      <span class="status-item">{t("segs")} {formatCount(app.scene.meta.segments)}</span>
      <span class="status-item">{t("texts")} {formatCount(app.scene.meta.text_count)}</span>
      <span class="status-item">{formatDuration(app.scene.meta.parse_ms + app.scene.meta.convert_ms)}{app.scene.meta.was_dwg ? " (dwg)" : ""}</span>
    {:else}
      {#if app.loading}
        <span class="status-item"><i class="status-red"></i>{t("opening")}{#if app.progress.pct >= 0} {Math.round(app.progress.pct)}%{/if}</span>
      {:else}
        <span class="status-item"><i class="status-red"></i>{t("offline")}</span>
      {/if}
      <span class="grow"></span>
      <span class="status-item">v{VERSION}</span>
    {/if}
  </footer>

  <AboutDialog />
  <ExportDialog />
</div>
