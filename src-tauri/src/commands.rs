use std::path::Path;

use tauri::ipc::Response;
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

use crate::drawing;
use crate::model::encode_blob;

/// Event name for load progress: `{ pct, phase, detail }`.
const LOAD_PROGRESS: &str = "load-progress";

/* ── 授权：试用与激活（见 license.rs 的模块说明，规程 FAMILY-LICENSE.md）── */

/// 试用与授权状态的落盘目录。由 `lib.rs` 的 setup 注入。
///
/// 用全局而不是给写命令各加一个参数：那会让命令签名多一个与业务无关的
/// 参数，而它也不是业务状态，读它不需要与文档状态同步。
static LICENSE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn init_license_dir(dir: std::path::PathBuf) {
    let _ = LICENSE_DIR.set(dir);
}

/// 供闸门发事件用。setup 注入；即使没注入也照样能拦截，只是界面不会自动弹窗。
static APP_HANDLE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

pub fn init_app_handle(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 当前授权状态。
///
/// 目录未注入（setup 失败）时按"试用中、满额天数"处理 —— 失败方向刻意选**放行**：
/// 一个取不到的目录不该变成一次锁死。
fn current_license() -> crate::license::Status {
    let Some(dir) = LICENSE_DIR.get() else {
        return crate::license::Status::Trialing { days_left: crate::license::TRIAL_DAYS };
    };
    let now = now_secs();
    /* 试用起点双写（AppData + 副存储）并按机器指纹判定，
       见 trial.rs 的模块说明。app_key 用 bundle identifier ——
       家族内唯一，避免两个产品的副存储互相覆盖。 */
    let started = crate::trial::ensure_started(
        dir,
        crate::APP_KEY,
        now,
        &crate::trial::machine_fingerprint(),
    );
    // 只认本单品与全家桶的回执：别人的回执即使验签通过，也不是本应用的授权。
    let receipt = crate::license::read_valid_receipt(dir, crate::license::PUBLIC_KEY_B64)
        .filter(crate::license::accepts);
    crate::license::status_from(Some(started), receipt.as_ref(), now)
}

/// 写操作的统一闸门。
///
/// 在**命令层**拦，而不是在每个界面路径上判断：界面路径会随功能增长而增加，漏掉一条
/// 就是一道缝；命令层是所有写操作的必经之路。CAD Viewer 的写命令只有 save_export
/// 一条（导出 PNG/PDF 落盘）；读操作（open_drawing / open_url）一律不拦。
///
/// 错误码固定为 `LICENSE_EXPIRED`，前端凭它弹购买/激活框。
fn ensure_write_allowed() -> Result<(), String> {
    if current_license().allows_write(crate::license::enforced()) {
        return Ok(());
    }
    // 让界面主动知道"被拦下了"，而不是在每个动作的 catch 里各判一次错误码 ——
    // 那种写法漏掉一处，用户看到的就只是一个没有解释的失败。
    if let Some(app) = APP_HANDLE.get() {
        let _ = tauri::Emitter::emit(app, "license-expired", ());
    }
    Err("LICENSE_EXPIRED".to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseInfo {
    /// `trial` / `expired` / `licensed`。
    pub status: String,
    /// 仅 `trial` 时有意义。
    pub days_left: i64,
    /// 仅 `licensed` 时有值（`CV` 单品 / `FL` 全家桶）。
    pub product: Option<String>,
    /// 当前是否真的会拦截写操作（渠道 + 公钥 + 总开关三者决定）。
    pub enforcing: bool,
    /// `direct`（官网直链）/ `store`（微软商店）。
    pub channel: String,
    /// 本构建是否已配置验签公钥。
    ///
    /// 没配置时**任何人都激活不了**（回执必然验不过）。界面据此如实说明，而不是
    /// 拿"激活码未被接受"去搪塞一位已经付过钱的用户。
    pub activation_configured: bool,
}

fn license_info() -> LicenseInfo {
    let status = current_license();
    LicenseInfo {
        status: status.as_str().to_string(),
        days_left: match &status {
            crate::license::Status::Trialing { days_left } => *days_left,
            _ => 0,
        },
        product: match &status {
            crate::license::Status::Licensed { product } => Some(product.clone()),
            _ => None,
        },
        enforcing: crate::license::enforced(),
        channel: crate::license::channel().to_string(),
        activation_configured: !crate::license::PUBLIC_KEY_B64.trim().is_empty(),
    }
}

/// 供界面展示：剩余试用天数 / 是否已激活 / 当前渠道。
#[tauri::command]
pub async fn license_status() -> Result<LicenseInfo, String> {
    Ok(license_info())
}

/// 保存服务端签出的回执并立即验签。
///
/// 联网换回执的那一步在**前端**做（`fetch` 到 rocktier.com/api/activate），
/// 为的是不引入 HTTP 客户端依赖；但**验签与落盘必须在这里** —— 前端拿到的只是一段
/// 待验的字符串，能证明它有效与否的只有公钥。
/// 本机指纹，供前端在**激活时**上报给服务端做设备计数。
///
/// 为什么单独开一个命令而不是让前端自己算：指纹要读注册表 / ioreg，
/// 只有 Rust 侧做得到；且**试用与激活必须用同一个指纹** ——
/// 用两套标识会出现「A 说没试过、B 说试过」这种自相矛盾。
///
/// 取不到时返回空串：服务端据此不计数也不拦激活（见
/// `rocktier.com/api/devices.js` 的模块说明）。
#[tauri::command]
pub fn machine_fingerprint() -> String {
    crate::trial::machine_fingerprint()
}

#[tauri::command]
pub async fn store_receipt(signed: String) -> Result<LicenseInfo, String> {
    let dir = LICENSE_DIR
        .get()
        .ok_or_else(|| "no app data directory".to_string())?;
    let trimmed = signed.trim();
    let receipt = crate::license::verify_receipt(trimmed, crate::license::PUBLIC_KEY_B64)?;

    // 其它单品的码虽然签名有效，但**不属于**本应用 —— 而且不要落盘：落下去以后
    // 会被当成有效回执读回来，等于自己给自己开后门。
    if !crate::license::accepts(&receipt) {
        return Err("LICENSE_WRONG_PRODUCT".to_string());
    }

    crate::license::save_receipt(dir, trimmed)?;
    Ok(license_info())
}

#[tauri::command]
pub async fn open_drawing(app: tauri::AppHandle, path: String) -> Result<Response, String> {
    let sink = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // A big DWG takes ~10 s; stream the phases so the UI can show a bar
        // instead of a spinner (see `drawing::Reporter`).
        let report = |pct: f32, phase: &'static str, detail: &str| {
            let _ = sink.emit(
                LOAD_PROGRESS,
                serde_json::json!({ "pct": pct, "phase": phase, "detail": detail }),
            );
        };
        let (meta, geometry) = drawing::load_with(Path::new(&path), &report)?;
        let meta_json = serde_json::to_string(&meta).map_err(|e| format!("序列化失败: {e}"))?;
        Ok(Response::new(encode_blob(&meta_json, &geometry)))
    })
    .await
    .map_err(|e| format!("解析任务失败: {e}"))?
}

/// Open an external link in the system browser / mail client.
///
/// Goes through the opener plugin's Rust API, so the frontend needs no IPC
/// permission for it; the allow-list keeps this from becoming an arbitrary
/// URL opener.
#[tauri::command]
pub fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    const ALLOWED: [&str; 4] = [
        "https://rocktier.com/",
        "https://www.rocktier.com/",
        // GPL-3.0 requires the corresponding source to be offered, and the About
        // dialog links to it — so the family repo root has to be reachable.
        "https://github.com/Rocktier/",
        "mailto:",
    ];
    if !ALLOWED.iter().any(|p| url.starts_with(p)) {
        return Err(format!("blocked url: {url}"));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Write an exported bitmap to disk.
///
/// The frontend renders the drawing offscreen (WebGL + the text overlay) and
/// sends the encoded image as base64 — PNG for a straight export, JPEG for the
/// PDF path, where the bytes are embedded as a `/DCTDecode` image without
/// re-encoding.  Returns the path actually written.
#[tauri::command]
pub async fn save_export(
    path: String,
    data_base64: String,
    kind: String,
    px_w: u32,
    px_h: u32,
    title: String,
) -> Result<String, String> {
    // 导出会产出新文件（PNG/PDF 落盘）：受授权闸门保护（FAMILY-LICENSE.md §2）。
    // 拦截在 spawn_blocking 之前：过期时连渲染/编码都不必做。
    ensure_write_allowed()?;
    // Run the decode + write off the command thread so the UI never freezes
    // while a large export is being encoded to disk.
    tauri::async_runtime::spawn_blocking(move || {
        use base64::Engine as _;
        // A 40 MB ceiling: an export this large is a mistake, not a drawing.
        const MAX_BYTES: usize = 40 * 1024 * 1024;
        if data_base64.len() > MAX_BYTES * 2 {
            return Err("导出数据过大".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data_base64.as_bytes())
            .map_err(|e| format!("数据解码失败: {e}"))?;
        if bytes.is_empty() {
            return Err("导出数据为空".into());
        }
        let out = match kind.as_str() {
            "png" => bytes,
            "pdf" => crate::pdf::jpeg_page(&bytes, px_w, px_h, &title)?,
            other => return Err(format!("不支持的导出格式: {other}")),
        };
        let p = Path::new(&path);
        if let Some(dir) = p.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| format!("无法创建目录: {e}"))?;
            }
        }
        std::fs::write(p, &out).map_err(|e| format!("写入失败: {e}"))?;
        Ok(path)
    })
    .await
    .map_err(|e| format!("导出任务失败: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验收（FAMILY-LICENSE.md §6）：把试用起始时间改到过期后，
    /// 写命令的闸门 `ensure_write_allowed` 必须返回含 `LICENSE_EXPIRED` 的错误。
    /// 构造法照 license.rs 既有测试：直接往状态目录里写起始时间戳。
    #[test]
    fn an_expired_trial_makes_the_write_gate_return_license_expired() {
        // 闸门真实生效的前提：ENFORCE + 直链渠道 + 公钥已配（测试构建三条都成立，
        // 与 license.rs 的 a_configured_key_in_the_direct_channel_engages_the_gate 同源）。
        assert!(
            crate::license::enforced(),
            "测试前提：ENFORCE=true、直链渠道、公钥已配时 enforced() 应为 true"
        );

        let dir = std::env::temp_dir().join(format!("rt-cv-license-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // LICENSE_DIR 是进程级单例：本测试是唯一设置它的测试。若将来有人加第二条，
        // 后到的 set 会失败 —— 那时合并两条测试，不要让闸门测试静默跑偏。
        if LICENSE_DIR.set(dir.clone()).is_err() {
            panic!("LICENSE_DIR 已被其他测试设置，闸门测试无法控制状态目录");
        }

        // 试用期第一天：写操作放行。
        let now = now_secs();
        // 文件名即 license.rs 的 STATE_FILE（模块私有常量，这里按值写）。
        std::fs::write(dir.join("state.bin"), now.to_string()).unwrap();
        assert_eq!(ensure_write_allowed(), Ok(()), "试用期内写操作必须放行");

        // 把试用起始时间改到 30 天前：状态 = Expired，必须被拦，错误码固定。
        std::fs::write(dir.join("state.bin"), (now - 30 * 86_400).to_string()).unwrap();
        let err = ensure_write_allowed().unwrap_err();
        assert!(
            err.contains("LICENSE_EXPIRED"),
            "过期后写操作应返回 LICENSE_EXPIRED，实际为 {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
