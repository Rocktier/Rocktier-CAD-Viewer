//! DXF loading with text-encoding detection.
//!
//! DXF files before AutoCAD R2007 store text in a legacy codepage announced by
//! the $DWGCODEPAGE header variable (e.g. ANSI_936 for Simplified Chinese).
//! R2007+ files are UTF-8. The `dxf` crate accepts an `encoding_rs` encoding,
//! so we sniff the right one from the raw bytes before parsing.

use std::io::Cursor;
use std::path::Path;

use encoding_rs::Encoding;
use dxf::Drawing;

fn encoding_for_codepage(codepage: &str) -> &'static Encoding {
    // Only the lower-case, trimmed value matters.
    match codepage.to_ascii_uppercase().as_str() {
        "ANSI_936" | "ANSI_GB2312" => encoding_rs::GB18030,
        "ANSI_950" | "ANSI_CHINESETRAD" => encoding_rs::BIG5,
        "ANSI_932" | "ANSI_SHIFTJIS" => encoding_rs::SHIFT_JIS,
        "ANSI_949" | "ANSI_WANSUNG" => encoding_rs::EUC_KR,
        "ANSI_1250" => encoding_rs::WINDOWS_1250,
        "ANSI_1251" => encoding_rs::WINDOWS_1251,
        "ANSI_1253" => encoding_rs::WINDOWS_1253,
        "ANSI_1254" => encoding_rs::WINDOWS_1254,
        "ANSI_1255" => encoding_rs::WINDOWS_1255,
        "ANSI_1256" => encoding_rs::WINDOWS_1256,
        "ANSI_1257" => encoding_rs::WINDOWS_1257,
        "ANSI_874" | "ANSI_THAI" => encoding_rs::WINDOWS_874,
        "ANSI_1252" | "ANSI_LATIN1" => encoding_rs::WINDOWS_1252,
        "UTF8" => encoding_rs::UTF_8,
        _ => encoding_rs::WINDOWS_1252,
    }
}

/// Scans the raw bytes for the $DWGCODEPAGE header value.
/// Header layout is "…\n9\n$DWGCODEPAGE\n3\nANSI_936\n…", so after the
/// variable name we take the next non-numeric, non-$ line.
fn detect_codepage(bytes: &[u8]) -> Option<String> {
    let hay = bytes.to_ascii_uppercase();
    let needle = b"$DWGCODEPAGE";
    let mut from = 0usize;
    while let Some(pos) = find(&hay[from..], needle) {
        let abs = from + pos + needle.len();
        // Read up to 8 following lines.
        let mut lines = hay[abs..].split(|&b| b == b'\n').skip(1).take(8);
        while let Some(line) = lines.next() {
            let line = trim_cr(line);
            if line.is_empty() {
                continue;
            }
            if line.iter().all(|b| b.is_ascii_digit()) {
                // group code line — the value follows next
                continue;
            }
            if line.first() == Some(&b'$') {
                break; // another variable started; codepage missing
            }
            return Some(String::from_utf8_lossy(line).trim().to_string());
        }
        from = abs;
        if from >= hay.len() {
            break;
        }
    }
    None
}

fn trim_cr(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && (line[end - 1] == b'\r' || line[end - 1] == b' ') {
        end -= 1;
    }
    let mut start = 0usize;
    while start < end && (line[start] == b' ' || line[start] == 0) {
        start += 1;
    }
    &line[start..end]
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

pub fn load_drawing(path: &Path) -> Result<Drawing, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("无法读取文件: {e}"))?;
    if bytes.is_empty() {
        return Err("文件为空".into());
    }

    let encoding: &'static Encoding =
        if std::str::from_utf8(&bytes).is_ok() {
            encoding_rs::UTF_8
        } else {
            detect_codepage(&bytes)
                .map(|cp| encoding_for_codepage(&cp))
                .unwrap_or(encoding_rs::WINDOWS_1252)
        };

    let mut cursor = Cursor::new(bytes);
    Drawing::load_with_encoding(&mut cursor, encoding).map_err(|e| format!("DXF 解析失败: {e}"))
}
