<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import BrandMark from "./BrandMark.svelte";
  import { app } from "../lib/state.svelte";
  import { t } from "../lib/i18n.svelte";

  const WEBSITE = "https://rocktier.com/";
  const FEEDBACK = "mailto:hello@rocktier.com";
  /* GPL-3.0 requires the corresponding source to be offered, and the trademark
     notice belongs where a user can actually read it. */
  const SOURCE = "https://github.com/Rocktier/Rocktier-CAD-Viewer";
  const NOTICES = `${SOURCE}/blob/main/THIRD-PARTY-NOTICES.md`;

  /** Set when the OS (or the allow-list) refuses, so the failure is visible. */
  let failedUrl = $state("");

  /** Hand the link to the OS; the webview itself must not navigate. */
  async function openExternal(e: MouseEvent, url: string) {
    e.preventDefault();
    try {
      await invoke("open_url", { url });
      failedUrl = "";
    } catch {
      // The allow-list can legitimately refuse (a link we forgot to register);
      // surface it instead of swallowing it silently.
      failedUrl = url;
    }
  }
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
        <BrandMark />
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
        <span><a href={WEBSITE} onclick={(e) => openExternal(e, WEBSITE)}>{t("website")}</a></span>
        <span><a href={FEEDBACK} onclick={(e) => openExternal(e, FEEDBACK)}>{t("feedback")}</a></span>
      </div>
      <p class="gpl-note">{t("gpl")}</p>
      <p class="gpl-note">
        {t("trademarkNote")}
        <a href={SOURCE} onclick={(e) => openExternal(e, SOURCE)}>{t("sourceCode")}</a>
        <a href={NOTICES} onclick={(e) => openExternal(e, NOTICES)}>{t("thirdParty")}</a>
      </p>
      {#if failedUrl}
        <p class="about-open-failed" role="alert">
          {t("openFailed")}
          <code>{failedUrl}</code>
        </p>
      {/if}
    </div>
  </div>
{/if}

<style>
  .about-open-failed {
    margin-top: 12px;
    font-size: 0.68rem;
    color: var(--text2);
    line-height: 1.5;
  }
  .about-open-failed code {
    display: block;
    margin-top: 4px;
    font-family: var(--mono);
    font-size: 0.66rem;
    color: var(--text3);
    word-break: break-all;
    user-select: all;
  }
</style>
