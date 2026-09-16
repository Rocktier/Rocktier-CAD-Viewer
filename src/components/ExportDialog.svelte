<script lang="ts">
  import { app, setStatus } from "../lib/state.svelte";
  import { t } from "../lib/i18n.svelte";
  import { saveExport, type ExportFormat, type ExportScope, type ExportTheme } from "../lib/export";

  let scope = $state<ExportScope>("drawing");
  let theme = $state<ExportTheme>("dark");
  let format = $state<ExportFormat>("png");
  let factor = $state(2);
  let busy = $state(false);

  const FACTORS = [1, 2, 4] as const;

  async function run() {
    if (!app.scene || busy) return;
    busy = true;
    try {
      const path = await saveExport(
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
        setStatus(`${t("exportOk")} ${path.split(/[\\/]/).pop()}`);
      }
    } catch (e) {
      setStatus(`${t("exportFail")}: ${e}`);
    } finally {
      busy = false;
    }
  }
</script>

{#if app.exportOpen}
  <div
    class="modal-mask"
    role="button"
    tabindex="0"
    aria-label={t("close")}
    onclick={() => (app.exportOpen = false)}
    onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") app.exportOpen = false; }}
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
      <button class="ghost modal-close" onclick={() => (app.exportOpen = false)} aria-label={t("close")}>✕</button>
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
