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
  import LicenseDialog from "./components/LicenseDialog.svelte";
  import UpdateDialog from "./components/UpdateDialog.svelte";
  import { LOCALES, app, openFile, retryOpen, setStatus, setLang, setUprightText, type LangCode } from "./lib/state.svelte";
  import { openLicense, onLicenseExpired, refreshLicense, revalidateLicence } from "./lib/license";
  import { checkForUpdates, currentVersion, type UpdateResult, type UpdateState } from "./lib/updates";
  import { t } from "./lib/i18n.svelte";
  import { formatCoord, formatCount, formatDuration, unitLabel } from "./lib/format";

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

  /* ── 界面主题 ───────────────────────────────────────────────────
     三态 auto → light → dark → auto（家族 §6.5 唯一状态机）。
     注意与「图纸主题」区分：导出时的 dark / print 是画布配色（见 ExportDialog），
     这里的 data-theme 只管界面外壳。
     存储键沿用 "rocktier-cad-ui-theme"：改键会让老用户偏好丢失，只扩值域。 */
  // 2026-10-04 键改名 → "rocktier.theme"，与家族其余产品同名同形（跨产品记忆负担最小）。
  // 上一行注释担心「改键会让老用户偏好丢失」是对的 —— 所以读取处回落旧键，两条同时成立。
  const THEME_KEY = "rocktier.theme";
  const THEME_KEY_LEGACY = "rocktier-cad-ui-theme";
  const APP_VERSION = currentVersion();

  const THEME_CYCLE = ["auto", "light", "dark"] as const;
  type UiThemeMode = (typeof THEME_CYCLE)[number];
  let uiTheme = $state<UiThemeMode>("auto");

  /* 更新检查。状态只由用户点击推进 —— 没有任何 effect 会自动发起联网请求，
     这是「除非你要求，应用不会联网」这条承诺的落点。 */
  let updateState = $state<UpdateState>("idle");
  let updateResult = $state<UpdateResult | null>(null);
  let updateChecking = false;

  async function runUpdateCheck() {
    if (updateChecking) return;
    updateChecking = true;
    updateState = "checking";
    try {
      const result = await checkForUpdates();
      updateResult = result;
      updateState = result.hasUpdate ? "available" : "uptodate";
      /* 顺带吊销复检：同一次点击、同一承诺（只在点击时联网）。revoked → 清回执，
         授权状态回到未激活并弹激活对话框。 */
      const verdict = await revalidateLicence();
      if (verdict === "revoked") {
        app.licenseRevoked = true;
        refreshLicense();
        openLicense();
      }
    } catch {
      updateState = "failed";
    } finally {
      updateChecking = false;
    }
  }

  function systemTheme(): "dark" | "light" {
    return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }

  /** auto 落成实际生效值 —— data-theme 只接受 light/dark。 */
  function resolveUiTheme(mode: UiThemeMode): "dark" | "light" {
    return mode === "auto" ? systemTheme() : mode;
  }

  function readUiTheme(): UiThemeMode {
    try {
      const saved = localStorage.getItem(THEME_KEY) ?? localStorage.getItem(THEME_KEY_LEGACY);
      if (saved === "light" || saved === "dark" || saved === "auto") return saved;
    } catch {
      /* 隐私模式下读不了，仅本次会话生效 */
    }
    return "auto";
  }

  function applyUiTheme(mode: UiThemeMode) {
    uiTheme = mode;
    document.documentElement.setAttribute("data-theme", resolveUiTheme(mode));
  }

  function toggleUiTheme() {
    const next = THEME_CYCLE[(THEME_CYCLE.indexOf(uiTheme) + 1) % THEME_CYCLE.length];
    try {
      localStorage.setItem(THEME_KEY, next);
    } catch {
      /* 隐私模式下写不了 localStorage，仅本次会话生效 */
    }
    applyUiTheme(next);
  }

  function themeLabel(): string {
    if (uiTheme === "light") return t("themeModeLight");
    if (uiTheme === "dark") return t("themeModeDark");
    return t("themeModeAuto");
  }

  /** Open a drawing the OS handed us (file association / "Open with"). */
  function openFirst(paths: string[] | null | undefined) {
    if (!paths?.length) return;
    if (paths.length > 1) setStatus(t("oneFileOnly"));
    void openFile(paths[0]);
  }

  onMount(() => {
    // 恢复上次选择的界面主题（没选过 = 跟随系统）
    applyUiTheme(readUiTheme());
    // auto 态下系统外观变了要跟着变；light/dark 是明确选择，不动。
    const themeMq = window.matchMedia("(prefers-color-scheme: light)");
    const onThemeChange = () => {
      if (uiTheme === "auto") applyUiTheme("auto");
    };
    themeMq.addEventListener("change", onThemeChange);
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
          case "theme": toggleUiTheme(); break;
          case "license": openLicense(); break;
          case "check-updates": app.updateOpen = true; void runUpdateCheck(); break;
          case "website": void invoke("open_url", { url: "https://rocktier.com/" }).catch(() => {}); break;
          case "feedback": void invoke("open_url", { url: "mailto:hello@rocktier.com" }).catch(() => {}); break;
        }
      }),
    );

    // 授权（家族 L6）：启动读一次试用状态；导出被拦时由 Rust 发 license-expired 事件
    //（命令层统一发），这里弹激活对话框并刷新状态。前端另有兜底：ExportDialog 的
    // catch 里判错误串含 LICENSE_EXPIRED 也开对话框，防事件丢失时只剩裸失败。
    refreshLicense();
    keep(
      onLicenseExpired(() => {
        openLicense();
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
      <span class="brand-word">Rocktier CAD Viewer</span>
      <!-- 家族契约 §一.4：产品名后的一句话自述（05-cad 设计稿 .mark .suffix）。
           原驻此处的 dot-live 无伴随文字，违反「红点永远配文字」——已移入
           状态栏，只在进行中态出现。 -->
      <span class="brand-suffix">{t("tagline")}</span>
    </div>

    {#if app.filePath}
      <span class="file-chip" title={app.filePath}>{app.fileName}</span>
    {/if}

    <div class="head-actions">
      <!-- 空态时 EmptyState 已有同款「打开」主 CTA：页头不再并列一个一模一样的
         （2026-10-25 审计 D-78：两个一模一样的 Open drawing 主 CTA 并列） -->
    {#if app.scene}
      <button class="btn ghosty" disabled={app.loading} onclick={pickFile}>{t("open")}</button>
      <!-- 主操作只有一个（家族 §三.2）：导出。05-cad 设计稿把它做成实心主按钮，
           其余查看工具降级到第二行工具条。 -->
      <button class="btn" onclick={() => (app.exportOpen = true)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M12 3v11m0 0-4-4m4 4 4-4" />
          <path d="M5 19h14" />
        </svg>
        {t("exportTitle")}
      </button>
    {/if}
      <!-- 家族唯一主题按钮：.icon-btn（28×28 + 40×40 命中区），三态 auto→light→dark。
           此前 CAD 只有显示菜单里的「主题」入口，界面上根本没有按钮。 -->
      <button
        class="icon-btn"
        data-mode={uiTheme}
        title={`${t("themeBtn")} · ${themeLabel()}`}
        aria-label={`${t("themeBtn")}: ${themeLabel()}`}
        onclick={toggleUiTheme}
      >
        {#if uiTheme === "auto"}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="2.5" y="4" width="19" height="13" rx="2" />
            <path d="M8 20.5h8M12 17v3.5" />
          </svg>
        {:else if uiTheme === "dark"}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="4" />
            <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8z" />
          </svg>
        {/if}
      </button>
      <!-- 中英两门（2026-10-13 家族收敛裁定，见 family.json globalRules.$locales_2026_10_13）。
           用原生 <select>：两个选项不需要搜索，原生控件在 Windows/macOS 行为一致，
           键盘与读屏器支持免费获得。选项显示 endonym（语言自称）。
           保留原来的 invoke("build_menu")：切换语言必须同步重建原生菜单，
           否则工具栏是英文、菜单还是中文。 -->
      <select
        class="ghost lang-btn"
        value={app.lang}
        onchange={(e) => {
          const next = e.currentTarget.value as LangCode;
          setLang(next);
          invoke("build_menu", { lang: next }).catch(() => {});
        }}
        title={t("langLabel")}
        aria-label={t("langLabel")}
      >
        {#each LOCALES as l (l.code)}
          <option value={l.code}>{l.endonym}</option>
        {/each}
      </select>
      <button class="ghost" title={t("about")} aria-label={t("about")} onclick={() => (app.aboutOpen = true)}>i</button>
    </div>
  </header>

  <!-- 第二行工具条（05-cad 设计稿 .toolbar）：只放真实存在的查看工具 ——
       适应窗口 / 适应内容 / 测距 / 文字转正 / 图层面板。右侧单位来自
       $INSUNITS（unitless 时不显示，不编造）。 -->
  {#if app.scene}
    <div class="toolbar" role="toolbar" aria-label={t("toolbarLabel")}>
      <button
        class="tool"
        title={t("fitView")} aria-label={t("fitView")}
        onclick={() => app.fitTick++}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M4 9V4h5M20 15v5h-5" />
          <path d="M4 4l6 6M20 20l-6-6" />
        </svg>
      </button>
      <button
        class="tool"
        title={t("fitContent")} aria-label={t("fitContent")}
        onclick={() => app.fitCoreTick++}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3" />
          <circle cx="12" cy="12" r="2.5" />
        </svg>
      </button>
      <div class="sep" aria-hidden="true"></div>
      <button
        class="tool"
        aria-pressed={app.tool === "measure"}
        title={t("measure")} aria-label={t("measure")}
        onclick={toggleTool}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M21.3 8.7 15.3 14.7 9.3 8.7 15.3 2.7 21.3 8.7z" />
          <path d="m2.7 21.3 6-6" />
        </svg>
      </button>
      <button
        class="tool"
        aria-pressed={app.uprightText}
        title={t("uprightText")} aria-label={t("uprightText")}
        onclick={() => setUprightText(!app.uprightText)}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="m5 7 7 10m0-10-7 10M6.2 14h11.6" />
          <path d="M17 3v4m0 0 1.8-1.8M17 7l-1.8-1.8" />
        </svg>
      </button>
      <div class="sep" aria-hidden="true"></div>
      <button
        class="tool"
        aria-pressed={app.panelOpen}
        title={t("panel")} aria-label={t("panel")}
        onclick={() => (app.panelOpen = !app.panelOpen)}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <line x1="15" y1="3" x2="15" y2="21" />
        </svg>
      </button>
      <div class="spacer"></div>
      {#if unitLabel(app.scene.meta.insunits)}
        <div class="units"><span>{t("units")}</span><b>{unitLabel(app.scene.meta.insunits)}</b></div>
      {/if}
    </div>
  {/if}

  <!-- main 地标 + 已加载时的文档标题 h1（视觉隐藏）；空态的可见 h1 在 EmptyState 里。
       两种状态互斥，任何时刻恰好一个 h1（2026-10-25 审计 D-78：整屏 0 个标题、无 main） -->
  <main class="app-body">
    {#if app.scene}
      <h1 class="visually-hidden">{app.fileName}</h1>
    {/if}
    <!-- 图层面板在画布左侧（05-cad 设计稿 .body 三栏的第一栏） -->
    {#if app.scene && app.panelOpen}
      <LayersPanel />
    {/if}
    <Stage onOpen={pickFile} />
    {#if app.error && !app.loading}
      <div class="app-error" role="alert">
        <p class="app-error-msg">{app.error}</p>
        <div class="app-error-actions">
          <button class="btn" onclick={retryOpen}>{t("retry")}</button>
          <button class="btn ghost" onclick={pickFile}>{t("openAnother")}</button>
        </div>
      </div>
    {/if}
  </main>

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
      {/if}
      <!-- 说人话（家族 §三.3）：进行中 = 呼吸点 + 文字（点永远不单独编码状态）；
           结束 = 结果句（图层、耗时都来自真实 meta，不编造）。 -->
      {#if app.loading}
        <span class="status-item">
          <i class="dot-live" role="img" aria-label={t("opening")}></i>{t("opening")}{#if app.progress.pct >= 0} {Math.round(app.progress.pct)}%{/if}
        </span>
      {:else}
        <!-- 单布局时布局名并入结果句（多布局走上方 tabs），信息不丢 -->
        <span class="status-item">
          {app.scene.meta.layouts[0]?.name ? `${app.scene.meta.layouts[0].name} · ` : ""}{t("loaded")} · {formatCount(app.scene.meta.layers.length)} {t("layers")} · {formatDuration(app.scene.meta.parse_ms + app.scene.meta.convert_ms)}{app.scene.meta.was_dwg ? " (dwg)" : ""}
        </span>
      {/if}
      <span class="grow"></span>
      <span class="status-item mono">X {formatCoord(app.coords?.x ?? NaN, unitLabel(app.scene?.meta.insunits))}</span>
      <span class="status-item mono">Y {formatCoord(app.coords?.y ?? NaN, unitLabel(app.scene?.meta.insunits))}</span>
      {#if app.scene.meta.skipped > 0}
        <span class="status-item mono status-muted" title={t("skippedHint")}>
          {t("skipped")} {formatCount(app.scene.meta.skipped)}
        </span>
      {/if}
      <span class="status-item mono">{t("zoom")} {app.zoomPct}%</span>
      <span class="status-item mono">{t("segs")} {formatCount(app.scene.meta.segments)}</span>
      <span class="status-item mono">{t("texts")} {formatCount(app.scene.meta.text_count)}</span>
    {:else}
      {#if app.loading}
        <span class="status-item">
          <i class="dot-live" role="img" aria-label={t("opening")}></i>{t("opening")}{#if app.progress.pct >= 0} {Math.round(app.progress.pct)}%{/if}
        </span>
      {:else}
        <!-- Offline is a feature, not a fault — neutral dot, red stays for in-progress. -->
        <span class="status-item"><i class="status-dot" aria-hidden="true"></i>{t("offline")}</span>
      {/if}
      <span class="grow"></span>
      <span class="status-item mono">v{VERSION}</span>
    {/if}
    <!-- License chip (family L6): shown only for the direct channel before it is
         activated — the Store build is charged by the Store, and an extra
         trial/purchase hint there reads as out-of-store purchasing to review.
         Activated builds render nothing; the Help menu keeps a permanent entry. -->
    {#if app.license && app.license.channel === "direct" && app.license.status !== "licensed"}
      <!-- 徽标颜色统一由 .badge-license（母版 tokens.css）负责，试用/过期同色，
           不再用 expired 修饰类切品牌红（旧 3.98:1 不达 AA）。 -->
      <button
        class="badge-license"
        title={app.license.status === "expired"
          ? (app.lang === "zh" ? "试用已结束 —— 点击激活" : "Trial ended — click to activate")
          : (app.lang === "zh" ? `免费试用中，还剩 ${app.license.daysLeft} 天` : `Free trial, ${app.license.daysLeft} day(s) left`)}
        onclick={openLicense}
      >
        {app.license.status === "expired"
          ? (app.lang === "zh" ? "未激活" : "Not activated")
          : (app.lang === "zh" ? `试用剩 ${app.license.daysLeft} 天` : `Trial · ${app.license.daysLeft}d`)}
      </button>
    {/if}

  </footer>

  <AboutDialog />
  <ExportDialog />
  <LicenseDialog />
  <!-- 检查更新：入口在帮助菜单「检查更新…」，结果弹 UpdateDialog。
       footer 常驻入口已移除（用户要求入口全部收进菜单）。 -->
  <UpdateDialog state={updateState} result={updateResult} currentVersion={APP_VERSION} />
</div>
