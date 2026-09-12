use std::path::Path;

use tauri::ipc::Response;
use tauri_plugin_opener::OpenerExt;

use crate::drawing;
use crate::model::encode_blob;

#[tauri::command]
pub async fn open_drawing(path: String) -> Result<Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (meta, geometry) = drawing::load(Path::new(&path))?;
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
