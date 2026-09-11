<script lang="ts">
  import { app, getRecent, openFile } from "../lib/state.svelte";
  import { t } from "../lib/i18n.svelte";

  type Props = { onOpen: () => void };
  let { onOpen }: Props = $props();

  const recents = $derived(getRecent());

  function openRecent(path: string) {
    void openFile(path);
  }
</script>

<div class="empty">
  <div class="dotframe"></div>
  <div class="empty-in">
    <div class="eyebrow">ROCKTIER · CAD VIEWER</div>
    <div class="display">{app.lang === "zh" ? "看图，快就够了" : "See drawings. Fast."}<i></i></div>
    <p class="lead">{t("emptyLead")}</p>
    <div class="empty-cta">
      <button class="btn" onclick={onOpen}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
        </svg>
        {t("open")}
      </button>
    </div>
    {#if recents.length > 0}
      <div class="recent">
        <div class="recent-label">{t("recent")}</div>
        {#each recents as r (r.path)}
          <button class="recent-chip" title={r.path} onclick={() => openRecent(r.path)}>{r.name}</button>
        {/each}
      </div>
    {/if}
  </div>
</div>
