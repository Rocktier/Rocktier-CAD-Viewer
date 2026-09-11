use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;
use tauri::ipc::Response;

use crate::dxf_io;
use crate::model::encode_blob;
use crate::tess;

#[derive(Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub brand: &'static str,
    pub offline: bool,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "Rocktier CAD Viewer",
        version: env!("CARGO_PKG_VERSION"),
        brand: "Create with grit.",
        offline: true,
    }
}

#[tauri::command]
pub async fn open_drawing(path: String) -> Result<Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let t0 = Instant::now();
        let drawing = dxf_io::load_drawing(&PathBuf::from(&path))?;
        let parse_ms = t0.elapsed().as_millis() as u64;

        let t1 = Instant::now();
        let (mut meta, geometry) = tess::build_scene(&drawing)?;
        meta.parse_ms = parse_ms;
        meta.tess_ms = t1.elapsed().as_millis() as u64;

        let meta_json =
            serde_json::to_string(&meta).map_err(|e| format!("序列化失败: {e}"))?;
        Ok(Response::new(encode_blob(&meta_json, &geometry)))
    })
    .await
    .map_err(|e| format!("解析任务失败: {e}"))?
}
