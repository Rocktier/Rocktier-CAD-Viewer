// License service — trial & activation（家族 L6，Rust 侧见 src-tauri/src/license.rs）。
// 文案为行内双语（本产品惯例，见 family-check 对 cad-viewer 的 i18n NA 备注）；
// 界面在 components/LicenseDialog.svelte。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { app } from "./state.svelte";

/** 产品页：购买与试用说明的唯一入口，官方站点上的文案以此为准。 */
export const BUY_URL = "https://rocktier.com/cad-viewer";

/** Mirrors `LicenseInfo` in src-tauri/src/commands.rs. */
export interface LicenseInfo {
  status: "trial" | "expired" | "licensed";
  daysLeft: number;
  product: string | null;
  /** Whether writes are actually blocked right now (channel + key + switch). */
  enforcing: boolean;
  /** `direct` for the website build, `store` for the Microsoft Store build. */
  channel: "direct" | "store";
  /** False when this build has no verification key, i.e. nothing can be activated. */
  activationConfigured: boolean;
}

/** Current trial / activation state. */
export async function licenseStatus(): Promise<LicenseInfo> {
  return invoke<LicenseInfo>("license_status");
}

/**
 * Hands the server-signed receipt to Rust, which verifies it against the built-in
 * public key and stores it. The check has to happen there: the frontend only has
 * a string, and only the public key can tell whether it means anything.
 */
export async function storeReceipt(signed: string): Promise<LicenseInfo> {
  return invoke<LicenseInfo>("store_receipt", { signed });
}

/**
 * Exchanges an activation code for a signed receipt.
 *
 * The code itself is checked **on the server**, not here: verifying it locally
 * would require shipping the signing secret inside the app, which would let
 * anyone mint their own codes. The frontend only carries the reply to Rust.
 */
export async function activate(code: string): Promise<LicenseInfo> {
  let payload: { receipt?: string; error?: string };
  try {
    const res = await fetch("https://rocktier.com/api/activate", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: code.trim() }),
    });
    payload = (await res.json()) as { receipt?: string; error?: string };
    if (!res.ok || !payload.receipt) {
      throw new Error(payload.error || `activation failed (${res.status})`);
    }
  } catch (e) {
    // Offline is the common case here — say so instead of showing a fetch error.
    throw new Error(
      e instanceof Error && e.message && !e.message.includes("fetch")
        ? e.message
        : "offline",
    );
  }
  return storeReceipt(payload.receipt);
}

/** Fires when a write was refused because the trial ran out and nothing is activated. */
export function onLicenseExpired(handler: () => void): Promise<() => void> {
  return listen("license-expired", () => handler());
}

/**
 * 写命令被授权闸门拦下的统一判据：Rust 的 ensure_write_allowed 返回的错误码固定为
 * LICENSE_EXPIRED。调用点的 catch 用它分流「弹激活对话框」还是「普通失败提示」。
 */
export function isLicenseExpiredError(e: unknown): boolean {
  return e instanceof Error
    ? e.message.includes("LICENSE_EXPIRED")
    : String(e).includes("LICENSE_EXPIRED");
}

// ------------------------------------------------------------- app-state glue

/** 拉取一次授权状态写进 app.license；失败按 null 处理（浏览器 dev / 取不到 = 不弹）。 */
export function refreshLicense() {
  licenseStatus()
    .then((info) => (app.license = info))
    .catch(() => (app.license = null));
}

/** 打开许可对话框并顺手刷新状态（状态栏胶囊 / 帮助菜单 / 导出被拦三个入口都走这里）。 */
export function openLicense() {
  app.licenseOpen = true;
  refreshLicense();
}
