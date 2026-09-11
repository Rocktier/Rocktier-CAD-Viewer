use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;
use tauri::ipc::Response;

use crate::dxf_io;
use crate::model::encode_blob;
use crate::tess;

/// Convert a DWG file to DXF via the system `dwg2dxf` CLI (LibreDWG).
/// Returns the path of the temporary DXF file on success.
fn convert_dwg_to_temp(dwg_path: &Path) -> Result<PathBuf, String> {
    if !dwg_path.is_file() {
        return Err(format!("文件不存在: {}", dwg_path.display()));
    }

    // Build a temp path: same stem, .dxf suffix, in the system temp dir.
    let stem = dwg_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("drawing");
    let mut tmp = std::env::temp_dir();
    let name = format!("rcv_dwg_{stem}_{}.dwg", std::process::id());
    tmp.push(&name);
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }
    tmp.set_extension("dxf");
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }

    // Run: dwg2dxf -y -o output.dxf input.dwg
    // We need the output path to match the -o arg exactly.
    let output_path = tmp.clone();
    let out_arg = output_path.to_string_lossy().to_string();
    let in_arg = dwg_path.to_string_lossy().to_string();

    let status = Command::new("dwg2dxf")
        .args(["-y", "-o", &out_arg, &in_arg])
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
