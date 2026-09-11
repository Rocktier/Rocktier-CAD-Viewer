<script lang="ts">
  import { app } from "../lib/state.svelte";
  import { t } from "../lib/i18n.svelte";
</script>

{#if app.aboutOpen}
  <div
    class="modal-mask"
    role="button"
    tabindex="0"
    aria-label={t("close")}
    onclick={() => (app.aboutOpen = false)}
    onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") app.aboutOpen = false; }}
  >
    <div class="modal" role="dialog" aria-modal="true" aria-label={t("about")} tabindex="0" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <button class="ghost modal-close" onclick={() => (app.aboutOpen = false)} aria-label={t("close")}>✕</button>
      <div class="about-brand">
        <svg class="brand-mark" viewBox="0 0 32 32" fill="none">
          <path d="M6 22 L13 7 L26 12 L22 25 Z" fill="currentColor" opacity="0.9" />
        </svg>
        <div>
          <div class="about-name">Rocktier CAD Viewer</div>
          <div class="about-ver">v{__APP_VERSION__} · Tauri + WebGL</div>
        </div>
      </div>
      <p class="about-lead">{t("aboutLead")}</p>
      <div class="about-cols">
        <div class="about-col"><span class="tick">01</span><h4>{t("fast")}</h4><p>{t("fastDesc")}</p></div>
        <div class="about-col"><span class="tick">02</span><h4>{t("private")}</h4><p>{t("privateDesc")}</p></div>
        <div class="about-col"><span class="tick">03</span><h4>{t("solid")}</h4><p>{t("solidDesc")}</p></div>
      </div>
      <div class="about-foot">
        <span>Create with grit.</span>
        <span><a href="mailto:hello@rocktier.studio">{t("feedback")}</a></span>
      </div>
      <p class="gpl-note">{t("gpl")}</p>
    </div>
  </div>
{/if}
