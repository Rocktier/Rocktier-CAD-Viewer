use std::path::Path;

use tauri::ipc::Response;
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

use crate::drawing;
use crate::model::encode_blob;

/// Event name for load progress: `{ pct, phase, detail }`.
const LOAD_PROGRESS: &str = "load-progress";

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
    const ALLOWED: [&str; 3] = ["https://rocktier.com/", "https://www.rocktier.com/", "mailto:"];
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
pub fn save_export(
    path: String,
    data_base64: String,
    kind: String,
    px_w: u32,
    px_h: u32,
    title: String,
) -> Result<String, String> {
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
}
