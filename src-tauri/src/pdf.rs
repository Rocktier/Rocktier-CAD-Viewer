//! Minimal single-page PDF writer.
//!
//! The renderer hands us a JPEG (canvas `toBlob("image/jpeg")`) and we wrap it
//! into a one-page PDF using `/DCTDecode` — JPEG bytes go into the file
//! verbatim, so there is no re-encoding, no image crate and no zlib dependency.
//!
//! Scope note: this writes a *picture* of the drawing, not vector geometry.
//! Vector output needs embedded CJK fonts (text is drawn by the webview, not by
//! us), which is a bigger project than the feature it serves.

/// A4 / A3 in PostScript points (1 pt = 1/72 in).
const A4: (f64, f64) = (595.28, 841.89);
const A3: (f64, f64) = (841.89, 1190.55);
/// Margin kept around the drawing, in points.
const MARGIN: f64 = 14.0;

/// Wrap a JPEG into a single-page PDF sized to the image's aspect ratio.
///
/// Page size is picked from A4/A3 (portrait or landscape) so the drawing fills
/// the sheet; the image is then scaled to fit inside the margins and centred.
pub fn jpeg_page(jpeg: &[u8], px_w: u32, px_h: u32, title: &str) -> Result<Vec<u8>, String> {
    if jpeg.is_empty() {
        return Err("JPEG 数据为空".into());
    }
    if px_w == 0 || px_h == 0 {
        return Err("图像尺寸无效".into());
    }
    let aspect = px_w as f64 / px_h as f64;
    let (pw, ph) = pick_page(aspect);
    let avail_w = pw - 2.0 * MARGIN;
    let avail_h = ph - 2.0 * MARGIN;
    // Uniform scale so the whole drawing is on the page.
    let scale = (avail_w / px_w as f64).min(avail_h / px_h as f64);
    let draw_w = px_w as f64 * scale;
    let draw_h = px_h as f64 * scale;
    let x = (pw - draw_w) / 2.0;
    let y = (ph - draw_h) / 2.0;

    let content = format!(
        "q\n{w:.3} 0 0 {h:.3} {x:.3} {y:.3} cm\n/Im0 Do\nQ\n",
        w = draw_w,
        h = draw_h,
        x = x,
        y = y
    );

    // Objects: 1 catalog, 2 pages, 3 page, 4 contents, 5 image, 6 info.
    let mut out: Vec<u8> = Vec::with_capacity(jpeg.len() + 1024);
    let mut offsets: Vec<usize> = Vec::with_capacity(6);
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let push_obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        let n = offsets.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    push_obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Catalog /Pages 2 0 R >>",
    );
    push_obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    );
    push_obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] \
             /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
        )
        .as_bytes(),
    );
    let mut contents_obj =
        format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    contents_obj.extend_from_slice(content.as_bytes());
    contents_obj.extend_from_slice(b"endstream");
    push_obj(&mut out, &mut offsets, &contents_obj);

    let mut img_obj = format!(
        "<< /Type /XObject /Subtype /Image /Width {px_w} /Height {px_h} \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode \
         /Length {} >>\nstream\n",
        jpeg.len()
    )
    .into_bytes();
    img_obj.extend_from_slice(jpeg);
    img_obj.extend_from_slice(b"\nendstream");
    push_obj(&mut out, &mut offsets, &img_obj);

    let info = format!(
        "<< /Title ({}) /Producer (Rocktier CAD Viewer) >>",
        escape_text(title)
    );
    push_obj(&mut out, &mut offsets, info.as_bytes());

    // Cross-reference table — offsets must point exactly at each "N 0 obj".
    let xref_at = out.len();
    let count = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {count}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {count} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
        )
        .as_bytes(),
    );
    Ok(out)
}

/// Closest sheet for an aspect ratio.
///
/// Ties go to the larger sheet: A3 and A4 share an aspect ratio, and
/// construction drawings are normally plotted on A3.
fn pick_page(aspect: f64) -> (f64, f64) {
    let sheets = [
        (A3.1, A3.0), // A3 landscape
        (A4.1, A4.0), // A4 landscape
        A3,           // A3 portrait
        A4,           // A4 portrait
    ];
    let mut best = sheets[0];
    let mut best_err = f64::INFINITY;
    for (w, h) in sheets {
        let err = ((w / h) / aspect - 1.0).abs();
        // A-series sheets share an aspect ratio, so their errors differ only in
        // the last bits of the decimal literals — require a real improvement
        // (0.5 %) before stepping down to the smaller sheet.
        if err < best_err * 0.995 {
            best_err = err;
            best = (w, h);
        }
    }
    best
}

/// PDF literal strings escape `\`, `(` and `)`; keep it ASCII-safe.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            c if c.is_ascii() => out.push(c),
            // Non-ASCII in a PDF literal string needs a font encoding we do not
            // ship; a file name with CJK is still fine as UTF-8 bytes elsewhere.
            _ => out.push('?'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1×1 JPEG is enough: the writer never decodes it.
    const TINY_JPEG: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0xFF, 0xD9,
    ];

    fn offsets_of(pdf: &[u8]) -> Vec<usize> {
        let text = String::from_utf8_lossy(pdf);
        let xref_at: usize = text
            .rsplit("startxref")
            .next()
            .unwrap()
            .trim()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let table = &text[xref_at..];
        table
            .lines()
            .filter(|l| l.ends_with(" n "))
            .map(|l| l[..10].parse::<usize>().unwrap())
            .collect()
    }

    fn media_box(pdf: &[u8]) -> String {
        let text = String::from_utf8_lossy(pdf);
        let rest = text.split("/MediaBox ").nth(1).unwrap_or("");
        match rest.split_once(']') {
            Some((head, _)) => format!("{head}]"),
            None => rest.to_string(),
        }
    }

    #[test]
    fn wide_drawing_gets_a3_landscape() {
        let pdf = jpeg_page(TINY_JPEG, 4000, 1000, "plan").unwrap();
        assert_eq!(media_box(&pdf), "[0 0 1190.55 841.89]");
    }

    #[test]
    fn tall_drawing_gets_a3_portrait() {
        let pdf = jpeg_page(TINY_JPEG, 1000, 4000, "plan").unwrap();
        assert_eq!(media_box(&pdf), "[0 0 841.89 1190.55]");
    }

    /// The xref table is the one part a viewer cannot forgive: every offset must
    /// land on the matching "N 0 obj" header.
    #[test]
    fn xref_offsets_point_at_object_headers() {
        let pdf = jpeg_page(TINY_JPEG, 2000, 1500, "plan.dwg").unwrap();
        let offsets = offsets_of(&pdf);
        assert_eq!(offsets.len(), 6, "expected six objects");
        for (i, off) in offsets.iter().enumerate() {
            let expect = format!("{} 0 obj", i + 1);
            let got = &pdf[*off..(*off + expect.len()).min(pdf.len())];
            assert_eq!(
                String::from_utf8_lossy(got),
                expect,
                "object {} offset {} is wrong",
                i + 1,
                off
            );
        }
        assert!(pdf.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn jpeg_bytes_are_embedded_verbatim() {
        let pdf = jpeg_page(TINY_JPEG, 100, 100, "x").unwrap();
        assert!(
            pdf.windows(TINY_JPEG.len()).any(|w| w == TINY_JPEG),
            "JPEG payload must go into the file untouched"
        );
        assert!(String::from_utf8_lossy(&pdf).contains("/Filter /DCTDecode"));
    }

    #[test]
    fn rejects_empty_input() {
        assert!(jpeg_page(&[], 10, 10, "x").is_err());
        assert!(jpeg_page(TINY_JPEG, 0, 10, "x").is_err());
    }
}

