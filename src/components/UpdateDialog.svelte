<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "../lib/state.svelte";
  import { t } from "../lib/i18n.svelte";
  import type { UpdateResult, UpdateState } from "../lib/updates";

  /**
   * 检查更新对话框。入口只在帮助菜单「检查更新…」—— 点击才联网，
   * 启动时、工作中、后台都不检查，见 lib/updates.ts 头注。
   * 结果一律在这里展示：footer 里的常驻入口已移除（用户要求入口全部
   * 收进菜单；Compressor v0.1.10 的右下角悬浮入口实测还会盖住底部内容），
   * 统一改为菜单 + 弹框。
   */
  let {
    state,
    result,
    currentVersion,
  }: {
    state: UpdateState;
    result: UpdateResult | null;
    currentVersion: string;
  } = $props();

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") app.updateOpen = false;
  }

  /** 打开发布页：交给 Rust 白名单入口，webview 自己不导航。 */
  async function open(url: string) {
    try {
      await invoke("open_url", { url });
    } catch {
      /* 白名单拒绝时静默 —— 用户可从菜单「官网」自己过去 */
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if app.updateOpen}
  <div
    class="modal-mask"
    role="button"
    tabindex="0"
    aria-label={t("close")}
    onclick={() => (app.updateOpen = false)}
    onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") app.updateOpen = false; }}
  >
    <div
      class="modal update-modal"
      role="dialog"
      aria-modal="true"
      aria-label={t("updateCheck")}
      tabindex="0"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <button class="ghost modal-close" onclick={() => (app.updateOpen = false)} aria-label={t("close")}>✕</button>
      <h3 class="update-title">{t("updateCheck")}</h3>
      <div class="update-body">
        {#if state === "checking"}
          <p>
            {t("updateChecking")}
            <br />
            <small>{t("updatePrivacy")}</small>
          </p>
        {:else if state === "uptodate"}
          <p>
            {t("updateUpToDate")}
            <br />
            <small>v{currentVersion}</small>
          </p>
        {:else if state === "failed"}
          <p>
            {t("updateFailed")}
            <br />
            <small>{t("updateOpenReleasesHint")}</small>
          </p>
        {:else if state === "available" && result}
          <p>
            {t("updateAvail")} v{result.latest}
            <br />
            <small>v{currentVersion} → v{result.latest}</small>
          </p>
        {/if}
      </div>
      <div class="update-footer">
        <button class="ghost" onclick={() => (app.updateOpen = false)}>{t("updateClose")}</button>
        <span class="update-spacer"></span>
        {#if state === "failed"}
          <button class="ghost" onclick={() => open("https://github.com/Rocktier/Rocktier-CAD-Viewer/releases/latest")}>
            {t("updateOpenReleases")}
          </button>
        {/if}
        {#if state === "available" && result}
          <button class="primary" onclick={() => open(result.url)}>
            {t("updateGet")} v{result.latest}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}
