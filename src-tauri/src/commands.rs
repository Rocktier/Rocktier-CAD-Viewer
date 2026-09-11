#[cfg(test)]
mod dwg_tests;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;
use tauri::ipc::Response;

use crate::dxf_io;
use crate::model::encode_blob;
use crate::tess;

/// Process-wide monotonic counter for unique temp-file names.
static DWG_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Convert a DWG file to DXF via the system `dwg2dxf` CLI (LibreDWG).
///
/// Output is written as R14 DXF — this suppresses LibreDWG's binary preview
/// chunks (which carry an unknown BMP header version that the `dxf` 0.6 crate
/// cannot decode) and emits only classic DXF group codes that our sanitizer
/// handles well.
///
/// Temp-file names embed a monotonic sequence number so concurrent calls never
/// collide even when `process::id` is reused across parallel test threads.
fn convert_dwg_to_temp(dwg_path: &Path) -> Result<PathBuf, String> {
    if !dwg_path.is_file() {
        return Err(format!("文件不存在: {}", dwg_path.display()));
    }

    let stem = dwg_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("drawing");
    let seq = DWG_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut output_path = std::env::temp_dir();
    output_path.push(format!("rcv_dwg_{stem}_{seq}.dxf"));
    if output_path.exists() {
        let _ = std::fs::remove_file(&output_path);
    }

    let out_arg = output_path.to_string_lossy().to_string();
    let in_arg = dwg_path.to_string_lossy().to_string();

    // --as r14: skip binary preview chunks; classic DXF only.
    let status = Command::new("dwg2dxf")
        .args(["--as", "r14", "-y", "-o", &out_arg, &in_arg])
        .status()
        .map_err(|e| format!("启动 dwg2dxf 失败: {e}（请先 brew install libredwg）"))?;

    if !status.success() {
        return Err(format!(
            "dwg2dxf 转换退出码 {:?}，输入: {}",
            status.code(),
            in_arg
        ));
    }

    if !output_path.is_file() {
        return Err("dwg2dxf 未生成目标文件".into());
    }

    Ok(output_path)
}

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
        let is_dwg = input_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("dwg"))
            .unwrap_or(false);

        let _t0_convert = Instant::now();
        let dxf_path = if is_dwg {
            convert_dwg_to_temp(&input_path)?
        } else {
            input_path
        };
        let convert_ms = _t0_convert.elapsed().as_millis() as u64;

        let t0 = Instant::now();
        let drawing = dxf_io::load_drawing(&dxf_path)?;
        let parse_ms = t0.elapsed().as_millis() as u64;

        let t1 = Instant::now();
        let (mut meta, geometry) = tess::build_scene(&drawing)?;
        meta.parse_ms = parse_ms;
        meta.tess_ms = t1.elapsed().as_millis() as u64;
        meta.convert_ms = convert_ms;
        meta.was_dwg = is_dwg;

        let meta_json =
            serde_json::to_string(&meta).map_err(|e| format!("序列化失败: {e}"))?;
        Ok(Response::new(encode_blob(&meta_json, &geometry)))
    })
    .await
    .map_err(|e| format!("解析任务失败: {e}"))?
}
