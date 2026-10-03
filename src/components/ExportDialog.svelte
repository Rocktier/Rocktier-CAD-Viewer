<script lang="ts">
  import { app, setStatus } from "../lib/state.svelte";
  import { t } from "../lib/i18n.svelte";
  import { saveExport, type ExportFormat, type ExportScope, type ExportTheme } from "../lib/export";
  import { isLicenseExpiredError, openLicense } from "../lib/license";

  let scope = $state<ExportScope>("drawing");
  let theme = $state<ExportTheme>("dark");
  let format = $state<ExportFormat>("png");
  let factor = $state(2);
  let busy = $state(false);

  const FACTORS = [1, 2, 4] as const;

  /** Escape closes the dialog — but never while a render is in flight, or the
   *  busy flag would strand on an unmounted component. */
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && app.exportOpen && !busy) {
      e.stopPropagation();
      app.exportOpen = false;
    }
  }

  function requestClose() {
    if (!busy) app.exportOpen = false;
  }

  async function run() {
    if (!app.scene || busy) return;
    busy = true;
    try {
      const { path, capped } = await saveExport(
        {
          scene: app.scene,
          layoutIdx: app.activeLayout,
          hidden: app.hidden,
          scope,
          theme,
          factor,
          view: app.camera,
          upright: app.uprightText,
          fileName: app.fileName,
        },
        format,
      );
      if (path) {
        app.exportOpen = false;
        const name = path.split(/[\\/]/).pop();
        setStatus(capped ? `${t("exportOk")} ${name} · ${t("exportCapped")}` : `${t("exportOk")} ${name}`);
      }
    } catch (e) {
      // 导出被授权闸门拦下 → 弹激活对话框（事件链已弹一次，这里兜错误串防事件丢失）；
      // 其余失败维持原样的裸错误提示。
      if (isLicenseExpiredError(e)) openLicense();
      else setStatus(`${t("exportFail")}: ${e}`);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if app.exportOpen}
  <div
    class="modal-mask"
    role="button"
    tabindex="0"
    aria-label={t("close")}
    onclick={requestClose}
    onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") requestClose(); }}
  >
    <div
      class="modal export-modal"
      role="dialog"
      aria-modal="true"
      aria-label={t("exportTitle")}
      tabindex="0"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <button class="ghost modal-close" onclick={requestClose} aria-label={t("close")}>✕</button>
      <h3 class="export-title">{t("exportTitle")}</h3>

      <div class="export-row">
        <span class="export-label">{t("exportScope")}</span>
        <div class="pill-group">
          <button class="pill-btn" class:active={scope === "drawing"} onclick={() => (scope = "drawing")}>
            {t("exportScopeDrawing")}
          </button>
          <button class="pill-btn" class:active={scope === "view"} onclick={() => (scope = "view")}>
            {t("exportScopeView")}
          </button>
        </div>
      </div>

      <div class="export-row">
        <span class="export-label">{t("exportTheme")}</span>
        <div class="pill-group">
          <button class="pill-btn" class:active={theme === "dark"} onclick={() => (theme = "dark")}>
            {t("exportThemeDark")}
          </button>
          <button class="pill-btn" class:active={theme === "print"} onclick={() => (theme = "print")}>
            {t("exportThemePrint")}
          </button>
        </div>
      </div>

      <div class="export-row">
        <span class="export-label">{t("exportFormat")}</span>
        <div class="pill-group">
          {#each ["png", "pdf"] as const as f}
            <button class="pill-btn" class:active={format === f} onclick={() => (format = f)}>
              {f.toUpperCase()}
            </button>
          {/each}
        </div>
      </div>

      <div class="export-row">
        <span class="export-label">{t("exportResolution")}</span>
        <div class="pill-group">
          {#each FACTORS as const as f}
            <button class="pill-btn" class:active={factor === f} onclick={() => (factor = f)}>{f}×</button>
          {/each}
        </div>
      </div>

      <p class="export-hint">{format === "pdf" ? t("exportPdfHint") : t("exportPngHint")}</p>

      <button class="btn export-run" disabled={busy} onclick={run}>
        {busy ? t("exporting") : t("exportRun")}
      </button>
    </div>
  </div>
{/if}

<style>
  .export-modal {
    max-width: 420px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .export-title {
    font-size: 1rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .export-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .export-label {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.16em;
    color: var(--text3);
  }
  .export-hint {
    font-size: 0.72rem;
    color: var(--text3);
    line-height: 1.5;
  }
  .export-run {
    width: 100%;
  }
</style>
