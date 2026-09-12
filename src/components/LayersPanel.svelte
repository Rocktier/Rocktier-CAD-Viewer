<script lang="ts">
  import { app } from "../lib/state.svelte";
  import { t } from "../lib/i18n.svelte";

  let search = $state("");

  const rows = $derived(
    app.scene
      ? app.scene.meta.layers
          .map((l, i) => ({ ...l, i }))
          .filter((l) => l.name.toLowerCase().includes(search.trim().toLowerCase()))
      : [],
  );

  function swatchStyle(l: { r: number; g: number; b: number; auto: boolean }) {
    return l.auto
      ? `background:${app.theme === "dark" ? "#fff" : "#000"}`
      : `background:rgb(${l.r},${l.g},${l.b})`;
  }

  function toggle(i: number) {
    if (app.hidden.has(i)) app.hidden.delete(i);
    else app.hidden.add(i);
  }

  function showAll() {
    app.hidden.clear();
  }

  function hideAll() {
    for (let i = 0; i < (app.scene?.meta.layers.length ?? 0); i++) {
      app.hidden.add(i);
    }
  }
</script>

<div class="panel">
  <div class="panel-head">
    <div class="panel-title">
      <span>{t("layers")}</span>
      <span class="count">{app.scene ? app.scene.meta.layers.length : 0}</span>
    </div>
    <input class="panel-search" placeholder={t("searchLayer")} aria-label={t("searchLayer")} bind:value={search} />
  </div>

  <div class="panel-list">
    {#if rows.length === 0}
      <div class="panel-empty">—</div>
    {/if}
    {#each rows as l (l.i)}
      <!-- `off` (switched off inside the drawing) only seeds `app.hidden`;
           visibility itself is owned by `app.hidden`, so a layer stays
           toggleable and the rows match what the canvas draws. -->
      <div class="layer-row" class:off={app.hidden.has(l.i)}>
        <span class="layer-swatch" style={swatchStyle(l)}></span>
        <span class="layer-name" title={l.name}>{l.name}</span>
        <button class="eye-btn" onclick={() => toggle(l.i)} aria-pressed={app.hidden.has(l.i)} aria-label={l.name}>
          {#if app.hidden.has(l.i)}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
              <line x1="1" y1="1" x2="23" y2="23" />
            </svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
              <circle cx="12" cy="12" r="3" />
            </svg>
          {/if}
        </button>
      </div>
    {/each}
  </div>

  <div class="panel-foot">
    <button class="panel-link" onclick={showAll}>{t("showAll")}</button>
    <button class="panel-link" onclick={hideAll}>{t("hideAll")}</button>
  </div>
</div>
