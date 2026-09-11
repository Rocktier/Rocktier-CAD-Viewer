//! End-to-end blob validation. Run with: `cargo run --bin test_e2e --features e2e-test`

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../fixtures");
    p.push(name);
    p
}

struct FixtureStats {
    segments: u64,
    texts: u64,
    layouts: usize,
    geom_bytes: usize,
}

fn main() {
    eprintln!("=== Rocktier CAD Viewer E2E Blob Validation ===\n");

    let fixtures = [
        ("01_fan.dxf", "Multi-layer architectural plan"),
        ("02_bulge.dxf", "LWPolylines with bulge arcs"),
        ("03_ellipse.dxf", "Ellipses with varying axis ratios"),
        ("04_spline.dxf", "B-splines with control points"),
        ("05_nesting.dxf", "Block INSERTs"),
        ("06_mtext.dxf", "MText with formatting"),
        ("07_layout.dxf", "Model + Paper space"),
    ];

    let mut pass = 0u32;
    let mut fail = 0u32;

    // Test 1: Parse → Tessellate → Blob → Validate
    for (file, desc) in fixtures {
        eprint!("  [T1] {} ({}) ... ", file, desc);
        match validate_fixture(file) {
            Ok(s) => {
                eprintln!("OK — {} segs, {} texts, {} layouts, {} bytes geom",
                    s.segments, s.texts, s.layouts, s.geom_bytes);
                pass += 1;
            }
            Err(e) => {
                eprintln!("FAIL\n         {}", e);
                fail += 1;
            }
        }
    }

    // Test 2: Range containment (validates the renderer offset fix)
    eprint!("\n  [T2] Range/sub-buffer containment ... ");
    match test_range_containment() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("FAIL\n         {}", e); fail += 1; }
    }

    // Test 3: Multi-layout geometry
    eprint!("  [T3] Multi-layout region contiguity ... ");
    match test_layout_contiguity() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("FAIL\n         {}", e); fail += 1; }
    }

    eprintln!("\n=== Results: {} passed, {} failed ===", pass, fail);
    if fail > 0 {
        std::process::exit(1);
    }
}

fn validate_fixture(file: &str) -> Result<FixtureStats, String> {
    let drawing = rocktier_cad_viewer_lib::dxf_io::load_drawing(&fixture(file))
        .map_err(|e| format!("load_drawing: {e}"))?;
    let (meta, geometry) = rocktier_cad_viewer_lib::tess::build_scene(&drawing)
        .map_err(|e| format!("build_scene: {e}"))?;
    let meta_json = serde_json::to_string(&meta).map_err(|e| format!("serialize: {e}"))?;
    let blob = rocktier_cad_viewer_lib::model::encode_blob(&meta_json, &geometry);

    if blob.len() < 8 {
        return Err("blob too short".into());
    }
    if &blob[..4] != b"RCV1" {
        return Err(format!("bad magic: {:?}", &blob[..4]));
    }
    let meta_len = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]) as usize;
    if 8 + meta_len > blob.len() {
        return Err("meta_len overruns blob".into());
    }
    // Validate the JSON is well-formed (parse to generic Value)
    if serde_json::from_str::<serde_json::Value>(
        std::str::from_utf8(&blob[8..8 + meta_len]).unwrap(),
    ).is_err()
    {
        return Err("meta JSON invalid".into());
    }

    let geo_len = blob.len() - 8 - meta_len;
    if geo_len % 12 != 0 {
        return Err(format!("geo_len {} not multiple of 12", geo_len));
    }

    for (li, layout) in meta.layouts.iter().enumerate() {
        if layout.lines_len % 12 != 0 {
            return Err(format!("L{} lines_len {} unaligned", li, layout.lines_len));
        }
        if layout.tris_len % 12 != 0 {
            return Err(format!("L{} tris_len {} unaligned", li, layout.tris_len));
        }
        if layout.points_len % 12 != 0 {
            return Err(format!("L{} points_len {} unaligned", li, layout.points_len));
        }
        let le = layout.lines_offset as u32 + layout.lines_len as u32;
        let te = layout.tris_offset as u32 + layout.tris_len as u32;
        let pe = layout.points_offset as u32 + layout.points_len as u32;
        if le as usize > geo_len || te as usize > geo_len || pe as usize > geo_len {
            return Err(format!("L{} region overruns geo (le={}, te={}, pe={}, geo={})",
                li, le, te, pe, geo_len));
        }
    }

    Ok(FixtureStats {
        segments: meta.segments,
        texts: meta.text_count,
        layouts: meta.layouts.len(),
        geom_bytes: geometry.len(),
    })
}

fn test_range_containment() -> Result<(), String> {
    for name in &["01_fan.dxf", "02_bulge.dxf", "03_ellipse.dxf", "05_nesting.dxf", "07_layout.dxf"] {
        let drawing = rocktier_cad_viewer_lib::dxf_io::load_drawing(&fixture(name))
            .map_err(|e| format!("{name}: load: {e}"))?;
        let (meta, _) = rocktier_cad_viewer_lib::tess::build_scene(&drawing)
            .map_err(|e| format!("{name}: build: {e}"))?;

        for (li, lo) in meta.layouts.iter().enumerate() {
            for r in &lo.line_ranges {
                if r.offset + r.len > lo.lines_len {
                    return Err(format!("{name} L{} line range overflow: {} > {}", li, r.offset + r.len, lo.lines_len));
                }
                if r.len % 12 != 0 {
                    return Err(format!("{name} L{} line range len {} unaligned", li, r.len));
                }
            }
            for r in &lo.tri_ranges {
                if r.offset + r.len > lo.tris_len {
                    return Err(format!("{name} L{} tri range overflow: {} > {}", li, r.offset + r.len, lo.tris_len));
                }
            }
            for r in &lo.point_ranges {
                if r.offset + r.len > lo.points_len {
                    return Err(format!("{name} L{} point range overflow: {} > {}", li, r.offset + r.len, lo.points_len));
                }
            }
        }
    }
    Ok(())
}

fn test_layout_contiguity() -> Result<(), String> {
    let drawing = rocktier_cad_viewer_lib::dxf_io::load_drawing(&fixture("07_layout.dxf"))
        .map_err(|e| format!("load: {e}"))?;
    let (meta, _) = rocktier_cad_viewer_lib::tess::build_scene(&drawing)
        .map_err(|e| format!("build: {e}"))?;

    // Must have at least 2 layouts (Model + paper)
    if meta.layouts.len() < 2 {
        return Err(format!("expected >= 2 layouts, got {}", meta.layouts.len()));
    }
    for (li, lo) in meta.layouts.iter().enumerate() {
        // Each layout's three sub-regions should be contiguous: lines → tris → points.
        if lo.tris_offset != lo.lines_offset + lo.lines_len {
            return Err(format!("L{} tris region not contiguous after lines", li));
        }
        if lo.points_offset != lo.tris_offset + lo.tris_len {
            return Err(format!("L{} points region not contiguous after tris", li));
        }
    }
    Ok(())
}
