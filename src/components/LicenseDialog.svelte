<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "../lib/state.svelte";
  import { t } from "../lib/i18n.svelte";
  import { activate, BUY_URL, refreshLicense, type LicenseInfo } from "../lib/license";

  /** 行内双语（本产品惯例：zh / en 同行成对，见 EmptyState 与 family-check 的 NA 备注）。 */
  const L = (zh: string, en: string) => (app.lang === "zh" ? zh : en);

  let code = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  /** Escape closes the dialog — activation keeps running in the background and
   *  still lands via refreshLicense when it finishes. */
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && app.licenseOpen) {
      e.stopPropagation();
      app.licenseOpen = false;
    }
  }

  function requestClose() {
    app.licenseOpen = false;
  }

  /** 打开产品页：桌面端走 Rust 白名单入口，浏览器 dev 直接开新窗口。 */
  function buy() {
    if ("__TAURI_INTERNALS__" in window) {
      invoke("open_url", { url: BUY_URL }).catch(() => {});
    } else {
      window.open(BUY_URL, "_blank", "noopener");
    }
  }

  async function submit() {
    if (!code.trim() || busy) return;
    busy = true;
    error = null;
    try {
      await activate(code);
      code = "";
      // 激活成功立刻刷新 license_status：无需重启即可继续导出
      refreshLicense();
    } catch (e) {
      /* 三种失败要分开说，用户的下一步动作不同：没连上网（重试即可）、码属于别的
         应用（要买对单品或全家桶）、码不对（检查有没有抄错）。 */
      const detail = e instanceof Error ? e.message : String(e);
      if (detail === "offline") error = L("连不上 rocktier.com。激活需要一次联网，之后便不再联网。", "Could not reach rocktier.com. Activating needs one connection; after that the app stays offline.");
      else if (detail.includes("WRONG_PRODUCT")) error = L("这个激活码属于另一个 Rocktier 应用。每个应用各有自己的码，或者用全家桶（可解锁全部）。", "That code belongs to a different Rocktier app. Each app has its own code — or the family bundle, which unlocks all of them.");
      else if (detail.includes("REFUNDED")) error = L("这个激活码对应的购买已退款，因此不能再解锁。如属误判，请把订单号发到 hello@rocktier.com。", "That code was refunded, so it no longer unlocks anything. If this is a mistake, write to hello@rocktier.com with your order number.");
      else error = L("该激活码未被接受。请检查是否输错（不区分大小写）。", "That code was not accepted. Check it for a typo — the code is not case-sensitive.");
    } finally {
      busy = false;
    }
  }

  /** 状态行：三态三套文案（trial / expired / licensed）。 */
  function statusLine(info: LicenseInfo | null): string {
    if (!info) return L("正在检查…", "Checking…");
    if (info.status === "licensed") {
      return info.product === "FL"
        ? L("已激活 —— 全家桶，所有 Rocktier 应用均已解锁。", "Licensed — family bundle. Every Rocktier app is unlocked.")
        : L("已激活。谢谢。", "Licensed. Thank you.");
    }
    if (info.status === "expired") return L("试用已结束。查看与测距仍可用；导出需要许可。", "Your trial has ended. Viewing and measuring still work; exporting needs a license.");
    return L(`免费试用中 —— 还剩 ${info.daysLeft} 天。`, `Free trial — ${info.daysLeft} day(s) left.`);
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if app.licenseOpen}
  <div
    class="modal-mask"
    role="button"
    tabindex="0"
    aria-label={t("close")}
    onclick={requestClose}
    onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") requestClose(); }}
  >
    <div
      class="modal license-modal"
      role="dialog"
      aria-modal="true"
      aria-label={L("许可与激活", "License")}
      tabindex="0"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <button class="ghost modal-close" onclick={requestClose} aria-label={t("close")}>✕</button>
      <h3 class="license-title">{L("许可与激活", "License")}</h3>

      <p class="license-status">{statusLine(app.license)}</p>

      {#if app.license?.status === "licensed"}
        <p class="license-note">
          {L("此副本已激活。此后不再有任何校验，也不联网。", "This copy is activated. No further checks, and no network access.")}
        </p>
      {:else if app.license && !app.license.activationConfigured}
        <!-- 这个构建没有验签公钥：任何回执都验不过。与其让买家以为码错了，不如说清。 -->
        <p class="license-note">
          {L("此构建尚未配置验签公钥，暂时无法激活。请写信到 hello@rocktier.com。", "This build cannot activate a code yet — it carries no verification key. Please write to hello@rocktier.com.")}
        </p>
      {:else if app.license?.channel === "store"}
        <!-- 商店版：说明授权由商店负责，并指向商店页面，不提供任何站外购买入口。 -->
        <p class="license-note">
          {L("此副本购自微软商店，许可由商店负责。", "This copy came from the Microsoft Store, so the Store handles the license for it.")}
        </p>
      {:else}
        <div class="license-field">
          <label for="license-code">{L("激活码", "Activation code")}</label>
          <input
            id="license-code"
            type="text"
            bind:value={code}
            spellcheck="false"
            autocomplete="off"
            placeholder="RKT-…"
            onkeydown={(e) => { if (e.key === "Enter") void submit(); }}
            disabled={busy}
          />
        </div>
        {#if error}
          <p class="license-error" role="alert">{error}</p>
        {/if}
        <p class="license-note">
          {L("付款后页面上会显示激活码，购买确认邮件里也有一份。", "Your code was shown on the page right after payment, and is in the purchase email too.")}
        </p>
        <p class="license-note">
          {L("激活会把激活码发送到 rocktier.com 一次，并把签名回执保存在本机。除此之外不传输任何内容。", "Activating sends the code to rocktier.com once and stores the signed reply locally. Nothing else is sent.")}
        </p>

        <div class="license-actions">
          <button class="btn ghosty" onclick={buy}>{L("购买 — $14.99", "Buy — $14.99")}</button>
          <button class="btn" disabled={busy || !code.trim()} onclick={() => void submit()}>
            {busy ? L("正在激活…", "Activating…") : L("激活", "Activate")}
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .license-modal {
    max-width: 440px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .license-title {
    font-size: 1rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .license-status {
    color: var(--text);
    font-size: 0.88rem;
    line-height: 1.6;
  }
  .license-note {
    font-size: 0.72rem;
    color: var(--text3);
    line-height: 1.55;
  }
  .license-error {
    font-size: 0.75rem;
    color: var(--red);
    line-height: 1.55;
  }
  .license-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .license-field label {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.16em;
    color: var(--text3);
  }
  .license-field input {
    padding: 9px 12px;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font: inherit;
    font-size: 0.88rem;
    color: var(--text);
    outline: none;
    transition: border-color 0.2s;
  }
  .license-field input:focus {
    border-color: var(--border-hover);
  }
  .license-field input:disabled {
    opacity: 0.45;
  }
  .license-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 4px;
  }
</style>
