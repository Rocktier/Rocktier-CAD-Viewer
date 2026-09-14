<script lang="ts">
  import Viewer from "./Viewer.svelte";
  import EmptyState from "./EmptyState.svelte";
  import { app } from "../lib/state.svelte";
  import { t } from "../lib/i18n.svelte";
  import { formatDist } from "../lib/format";

  type Props = { onOpen: () => void };
  let { onOpen }: Props = $props();
</script>

<div class="stage">
  <div class="canvas-wrap" class:measuring={app.tool === "measure"}>
    <Viewer />

    {#if !app.scene && !app.loading}
      <EmptyState onOpen={onOpen} />
    {/if}

    {#if app.dragging}
      <div class="drop-hint"><span style="font-family:var(--mono);font-size:0.85rem;color:var(--text2)">{t("dropHere")}</span></div>
    {/if}

    {#if app.loading && !app.scene}
      <div class="load-hint">
        <div class="overlay-card">
          {#if app.progress.pct >= 0}
            <div
              class="progress"
              role="progressbar"
              aria-valuenow={Math.round(app.progress.pct)}
              aria-valuemin="0"
              aria-valuemax="100"
            >
              <div class="progress-fill" style="width:{app.progress.pct}%"></div>
            </div>
          {:else}
            <div class="spinner"></div>
          {/if}
          <div class="overlay-title">{app.fileName || "…"}</div>
          <div class="overlay-msg">
            {app.loadingMsg}{#if app.progress.detail}<span class="overlay-detail"> · {app.progress.detail}</span>{/if}
          </div>
          {#if app.progress.pct >= 0}
            <div class="overlay-pct">{Math.round(app.progress.pct)}%</div>
          {/if}
        </div>
      </div>
    {/if}

    {#if app.scene && app.tool === "measure"}
      <div class="measure-banner">
        <span>{t("measuring")}</span>
        {#if app.measureResult}
          <b>{t("measured")} {formatDist(app.measureResult.dist)}</b>
        {/if}
      </div>
    {/if}

    {#if app.error}
      <div class="toast" role="alert">
        <i></i>
        <span>{app.error}</span>
        <button class="ghost" style="width:26px;height:26px" onclick={() => (app.error = "")} aria-label={t("close")}>✕</button>
      </div>
    {/if}
  </div>
</div>
