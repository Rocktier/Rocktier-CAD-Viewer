#[cfg(test)]
mod dwg_tests;

use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;
use tauri::ipc::Response;

use crate::dxf_ascii;
use crate::dwg_geojson;
use crate::model::encode_blob;

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
        let input_path = PathBuf::from(&path);
        let ext = input_path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        let is_dwg = ext.as_deref() == Some("dwg");

        if is_dwg {
            // DWG → CLI `dwgread -OGeoJSON` → parse JSON into geometry.
            // The GeoJSON exporter in LibreDWG sidesteps the AC1021 "Invalid
            // num_pages 0" bug that breaks `dwg2dxf` for pre-2010 files.
            let t0 = Instant::now();
            let (mut meta, geometry) = dwg_geojson::read_dwg_geojson(&input_path, 0, 0)?;
            meta.parse_ms = t0.elapsed().as_millis() as u64;

            let meta_json =
                serde_json::to_string(&meta).map_err(|e| format!("序列化失败: {e}"))?;
            Ok(Response::new(encode_blob(&meta_json, &geometry)))
        } else {
            // DXF / custom ASCII formats → our hand-written parser.
            let t0 = Instant::now();
            let text = std::fs::read_to_string(&input_path)
                .map_err(|e| format!("无法读取文件: {e}"))?;
            let (mut meta, geometry) =
                dxf_ascii::parse_and_build(&text, 0, false, 0)?;
            meta.parse_ms = t0.elapsed().as_millis() as u64;

            let meta_json =
                serde_json::to_string(&meta).map_err(|e| format!("序列化失败: {e}"))?;
            Ok(Response::new(encode_blob(&meta_json, &geometry)))
        }
    })
    .await
    .map_err(|e| format!("解析任务失败: {e}"))?
}
