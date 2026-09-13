//! Drawing file → scene: the single entry point used by the UI command and
//! the E2E test.
//!
//! DXF goes straight to `dxf_ascii`.  DWG is converted first with LibreDWG's
//! `dwg2dxf` CLI and then parsed by that same DXF parser.  That keeps block
//! definitions/INSERTs, the LAYER table with colours, and TEXT/MTEXT with real
//! heights — all of which LibreDWG's GeoJSON writer used to flatten (block
//! references became bare points) or drop (text height/rotation).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::dxf_ascii;
use crate::model::SceneMeta;

/// Upper bound on input file size (MiB).  Larger files are rejected before
/// being slurped into memory so a casual drag of a 1 GB dump does not OOM the
/// viewer.
pub const MAX_DRAWING_MIB: u64 = 200;

/// Loading progress: `(percent, phase, detail)`.
///
/// `percent` is `0..=100`, or `-1` when the step cannot be measured (the DWG →
/// DXF conversion — LibreDWG reports nothing and the DXF size is unknown up
/// front, so the UI shows an indeterminate bar with the bytes written so far).
/// Phases: `"convert"`, `"parse"`, `"build"`.
pub type Reporter<'a> = &'a dyn Fn(f32, &'static str, &str);

/// Load a `.dxf` / `.dwg` drawing into `(SceneMeta, geometry_blob)`.
pub fn load(path: &Path) -> Result<(SceneMeta, Vec<u8>), String> {
    load_with(path, &|_, _, _| {})
}

/// Same as `load`, but reports progress through `report` so the UI can show a
/// real bar instead of a spinner.
pub fn load_with(path: &Path, report: Reporter) -> Result<(SceneMeta, Vec<u8>), String> {
    let md = std::fs::metadata(path).map_err(|e| format!("无法读取文件: {e}"))?;
    if md.len() > MAX_DRAWING_MIB * 1024 * 1024 {
        return Err(format!(
            "文件过大（{} MiB > 上限 {} MiB）",
            md.len() / (1024 * 1024),
            MAX_DRAWING_MIB
        ));
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("dxf") => {
            let t0 = Instant::now();
            report(2.0, "build", "");
            let text = read_text(path)?;
            let (mut meta, geometry) = dxf_ascii::parse_and_build_with_progress(
                &text,
                0,
                false,
                0,
                &|p| report(5.0 + p * 90.0, "parse", ""),
            )?;
            meta.parse_ms = t0.elapsed().as_millis() as u64;
            Ok((meta, geometry))
        }
        Some("dwg") => parse_dwg(path, report),
        other => Err(format!(
            "不支持的文件格式: {}",
            other
                .map(|e| format!(".{e}"))
                .unwrap_or_else(|| "(无扩展名)".to_string())
        )),
    }
}

/// Read a drawing as text.
///
/// Three cases, in order of likelihood:
///
/// 1. Clean UTF-8 — take it.
/// 2. UTF-8 with a *few* broken byte sequences.  LibreDWG splits multi-byte
///    characters when it wraps a string at a line width, and its Windows build
///    (compiled without iconv) writes UTF-8 while still declaring
///    `$DWGCODEPAGE ANSI_936`.  Decoding the whole file as GBK here turns 23 MB
///    of correct Chinese into mojibake — so the decoder measures the damage
///    instead of trusting the declared code page.
/// 3. Genuinely GBK (what a Chinese AutoCAD export looks like): nearly every
///    non-ASCII byte is invalid UTF-8, so the same measurement sends it here.
fn decode_drawing_text(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    let lossy = String::from_utf8_lossy(bytes);
    let damaged = lossy.chars().filter(|c| *c == '\u{FFFD}').count();
    let non_ascii = lossy.chars().filter(|c| !c.is_ascii()).count();
    // Under 1 % damaged ⇒ the file really is UTF-8 (a real GBK file lands
    // around 60 %, so the margin is enormous either way).
    if non_ascii > 0 && damaged * 100 < non_ascii {
        return lossy.into_owned();
    }
    let (text, _, _) = encoding_rs::GBK.decode(bytes);
    text.into_owned()
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("无法读取文件: {e}"))?;
    Ok(decode_drawing_text(&bytes))
}

/// Resolve a LibreDWG CLI: a copy next to the executable wins (so a bundled
/// sidecar is picked up), otherwise PATH.  Windows installers place the
/// declared bundle resources under `<install>/resources/libredwg/`, and a
/// dev-mode checkout can point at `src-tauri/resources/libredwg/` via CWD.
fn cli_bin(name: &str) -> PathBuf {
    let mut names = vec![name.to_string()];
    if cfg!(windows) {
        names.push(format!("{name}.exe"));
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(dir.clone());
        // Tauri installs declared resources relative to the executable dir.
        dirs.push(dir.join("resources").join("libredwg"));
        dirs.push(dir.join("libredwg"));
    }
    // Dev-mode fallback relative to the project root / src-tauri.
    dirs.push(PathBuf::from("src-tauri/resources/libredwg"));
    dirs.push(PathBuf::from("resources/libredwg"));
    for dir in &dirs {
        for n in &names {
            let p = dir.join(n);
            if p.is_file() {
                return p;
            }
        }
    }
    PathBuf::from(name)
}

fn parse_dwg(path: &Path, report: Reporter) -> Result<(SceneMeta, Vec<u8>), String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    // dwg2dxf cannot write to stdout (`-o` requires exactly one input file), so
    // it goes through a uniquely named temp file that we delete right after.
    let dxf_path = std::env::temp_dir().join(format!(
        "rocktier-{}-{}.dxf",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));

    let t0 = Instant::now();
    // 不能无限等待：dwg2dxf 在损坏 DWG / 网络盘上可能一直卡住，前端会永远停在
    // 加载态且无法取消。这里给 120s 上限，超时就杀掉并报错。
    let output = (|| -> Result<std::process::Output, String> {
        use std::sync::mpsc::channel;
        let mut cmd = Command::new(cli_bin("dwg2dxf"));
        cmd.arg("-y").arg("-o").arg(&dxf_path).arg(path);
        // dwg2dxf is a console program: without CREATE_NO_WINDOW every DWG load
        // flashes a black terminal over the app for the whole conversion.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let child = cmd
            .spawn()
            .map_err(|e| {
                if cfg!(windows) {
                    format!("启动 dwg2dxf 失败: {e}（安装包自带 LibreDWG，请重新安装本应用）")
                } else {
                    format!("启动 dwg2dxf 失败: {e}（需要 LibreDWG：brew install libredwg）")
                }
            })?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        // Poll instead of one long recv_timeout: dwg2dxf writes the DXF
        // progressively, so the growing file is a real signal to show the user
        // (bytes written) even though the total is unknown.
        let mut waited_ms = 0u64;
        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(120)) {
                Ok(Ok(out)) => break Ok(out),
                Ok(Err(e)) => break Err(format!("读取 dwg2dxf 输出失败: {e}")),
                Err(_) => {
                    waited_ms += 120;
                    if waited_ms >= 120_000 {
                        let _ = std::fs::remove_file(&dxf_path);
                        break Err("dwg2dxf 转换超时（120 秒），文件可能已损坏".to_string());
                    }
                    let written = std::fs::metadata(&dxf_path).map(|m| m.len()).unwrap_or(0);
                    report(-1.0, "convert", &format!("{:.1} MB", written as f64 / 1_048_576.0));
                }
            }
        }
    })()?;
    let convert_ms = t0.elapsed().as_millis() as u64;

    if !output.status.success() {
        let _ = std::fs::remove_file(&dxf_path);
        let err_text = String::from_utf8_lossy(&output.stderr);
        let first_err = err_text.lines().next().unwrap_or("(no message)");
        return Err(format!(
            "dwg2dxf 退出码 {:?}: {}",
            output.status.code(),
            first_err
        ));
    }

    // 只查输入体积不够：dwg2dxf 产出的 DXF 常常是 DWG 的数倍，会把 1GB+ 直接读进内存
    if let Ok(m) = std::fs::metadata(&dxf_path) {
        if m.len() > MAX_DRAWING_MIB * 1024 * 1024 {
            let _ = std::fs::remove_file(&dxf_path);
            return Err(format!(
                "转换结果过大（{} MB），已取消加载",
                m.len() / 1024 / 1024
            ));
        }
    }

    report(35.0, "parse", "");
    let t1 = Instant::now();
    let text = read_text(&dxf_path);
    let _ = std::fs::remove_file(&dxf_path); // best effort — the text is in memory now
    let text = text?;

    let (mut meta, geometry) = dxf_ascii::parse_and_build_with_progress(&text, 0, true, convert_ms, &|p| {
        report(35.0 + p * 60.0, "parse", "")
    })?;
    report(96.0, "build", "");
    meta.parse_ms = t1.elapsed().as_millis() as u64;
    Ok((meta, geometry))
}

#[cfg(test)]
mod tests {
    use super::decode_drawing_text;

    #[test]
    fn clean_utf8_passes_through() {
        let s = "0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\n宋体\n";
        assert_eq!(decode_drawing_text(s.as_bytes()), s);
    }

    #[test]
    fn gbk_text_is_decoded_as_gbk() {
        let (gbk, _, _) = encoding_rs::GBK.encode("窗下 1300");
        let mut bytes = b"0\nTEXT\n1\n".to_vec();
        bytes.extend_from_slice(&gbk);
        bytes.extend_from_slice(b"\n");
        let text = decode_drawing_text(&bytes);
        assert!(text.contains("窗下"), "GBK drawing must decode, got {text:?}");
    }

    /// LibreDWG splits a multi-byte character when it wraps a string; the
    /// Windows build has no iconv, so it writes UTF-8 under an ANSI_936
    /// header.  A handful of broken sequences must not send the whole file
    /// down the GBK path (that is what turned every Chinese label to mojibake).
    #[test]
    fn a_few_broken_sequences_stay_utf8() {
        let mut bytes: Vec<u8> = Vec::new();
        for _ in 0..200 {
            bytes.extend_from_slice("建筑、结构".as_bytes());
        }
        let split = "窗下".as_bytes();
        bytes.extend_from_slice(&split[..1]);
        bytes.push(b'\n');
        bytes.extend_from_slice(&split[1..]);
        for _ in 0..200 {
            bytes.extend_from_slice("建筑、结构".as_bytes());
        }
        let text = decode_drawing_text(&bytes);
        assert!(
            text.contains("建筑、结构"),
            "the intact UTF-8 must survive one split character"
        );
    }
}
