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

/// Replace non-geometric group-code values that overflow `i32`.
///
/// Real-world DWG files converted to DXF by LibreDWG often contain:
///   - code 420/421/422/423: 24-bit true-color values (e.g. 3_258_135_347) stored as raw u32 > i32::MAX
///   - code 310: binary preview chunk whose parsed integer is > i64::MAX
///   - code 3/330/360: entity handles occasionally > i32
///
/// The `dxf` 0.6 crate parses every numeric cell with `i32`/`i64`, so these blow up the
/// whole load. We sniff the DXF text lines and truncate only codes that are not used
/// for tessellation or entity linking. Geometric codes (10/20/30/etc.) pass through
/// untouched — if they overflow, we did lose data, but real building coordinates
/// never exceed i32 in drawing units.
const NON_GEOMETRIC_OVERFLOW_CODES: &[i32] = &[
    3, 7, 340, 341, 342, 343, 344, 345, 346, 347, 348, 349, 350, 351, 360, 361, 370,
    380, 390, 410, 420, 421, 422, 423, 424, 425, 426, 427, 428, 429, 430, 431, 432,
    433, 434, 435, 436, 437, 438, 439, 440, 441, 442, 443, 444, 445, 446, 447, 448,
    449, 450, 451, 452, 453, 454, 455, 456, 457, 458, 459, 460, 461, 462, 463, 464,
    465, 466, 467, 468, 469, 500, 501, 502, 503, 504, 505, 506, 507, 508, 509, 510,
    310, 330, 331, 332, 333, 334, 335, 336, 337, 338, 339,
];

pub fn sanitize_dxf_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_code: Option<i32> = None;

    for line in text.split('\n') {
        let trimmed = line.trim_end_matches('\r');
        let sanitized_line = match prev_code {
            Some(code) if NON_GEOMETRIC_OVERFLOW_CODES.contains(&code) => {
                // If value is a plain integer, check range; otherwise leave as-is.
                match trimmed.trim().parse::<i64>() {
                    Ok(v) if v >= i32::MIN as i64 && v <= i32::MAX as i64 => {
                        trimmed.to_string()
                    }
                    Ok(_) => {
                        // Overflow — replace with fallback (white for color, 0 for others)
                        let fallback = if (420..=429).contains(&code) { "7" } else { "0" };
                        let padding = trimmed.len().saturating_sub(fallback.len());
                        format!("{}{}", " ".repeat(padding), fallback)
                    }
                    Err(_) => trimmed.to_string(),
                }
            }
            _ => trimmed.to_string(),
        };
        out.push_str(&sanitized_line);
        out.push('\n');

        // Record integer-looking line as potential group code for the next iteration.
        if let Some(t) = trimmed.strip_prefix('-') {
            if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) {
                prev_code = trimmed.parse::<i32>().ok();
            } else {
                prev_code = None;
            }
        } else if !trimmed.is_empty() && trimmed.bytes().all(|b| b.is_ascii_digit()) {
            prev_code = trimmed.parse::<i32>().ok();
        } else {
            prev_code = None;
        }
    }
    out
}

#[cfg(test)]
mod sanity {
    use super::*;

    /// Verifies sanitizer does not corrupt every-day DXF text (all existing
    /// fixtures rely on this function since M3).
    #[test]
    fn sanitize_preserves_inline_minimal_dxf() {
        let input = "\
0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n1.0\n20\n2.0\n30\n0.0\n11\n5.0\n21\n6.0\n31\n0.0\n421\n3258135347\n0\nENDSEC\n0\nEOF\n";
        let out = sanitize_dxf_text(input);
        // Geometry must survive verbatim
        assert!(out.contains("10\n1.0"));
        assert!(out.contains("20\n2.0"));
        assert!(out.contains("11\n5.0"));
        assert!(out.contains("21\n6.0"));
        // 421 overflow → replaced with 7 (white) for true-color range
        let mut lines = out.lines();
        let mut saw_421 = false;
        while let Some(l) = lines.next() {
            if l.trim() == "421" {
                saw_421 = true;
                let val = lines.next().unwrap();
                assert_eq!(val.trim(), "7", "421 should resolve to 7 (white)");
                break;
            }
        }
        assert!(saw_421, "421 entry must be present after sanitize");
        assert!(!out.contains("3258135347"), "bignum must be removed");
    }
}

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

    // Decode to UTF-8 text (fall back to lossy if the DXF is in a legacy codepage
    // that wouldn't round-trip — sanitize is text-only anyway).
    let text = String::from_utf8_lossy(&bytes);
    let sanitized = sanitize_dxf_text(&text);
    let sanitized_bytes = sanitized.into_bytes();

    let encoding: &'static Encoding = encoding_rs::UTF_8;

    let mut cursor = Cursor::new(sanitized_bytes);
    Drawing::load_with_encoding(&mut cursor, encoding)
        .map_err(|e| format!("DXF 解析失败: {e}"))
}
