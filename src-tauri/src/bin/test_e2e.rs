//! End-to-end blob validation for the ASCII DXF parser pipeline.
//!
//! Run with: `cargo run --bin test_e2e --features e2e-test`

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
    eprintln!("=== Rocktier CAD Viewer E2E Blob Validation (ASCII parser) ===\n");

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

    // Test 1: ASCII parse → Tessellate → Blob → Validate
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

    // Test 2: Range containment (validates layer ranges stay within layout region)
    eprint!("\n  [T2] Range/sub-buffer containment ... ");
    match test_range_containment() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("FAIL\n         {}", e); fail += 1; }
    }

    // Test 3: Single-layout geometry contiguity (lines → tris → points)
    eprint!("  [T3] Region contiguity ... ");
    match test_contiguity() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("FAIL\n         {}", e); fail += 1; }
    }

    // Test 4: Minimal inline DXF (ensures the parser handles the simplest input)
    eprint!("  [T4] Minimal inline DXF ... ");
    match test_minimal_inline() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("FAIL\n         {}", e); fail += 1; }
    }

    // Test 5: DWG pipeline (only if the user's real DWG file is present)
    eprint!("  [T5] Real DWG fixture ... ");
    match test_real_dwg() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("SKIP ({})", e); }
    }

    eprintln!("\n=== Results: {} passed, {} failed ===", pass, fail);
    if fail > 0 {
        std::process::exit(1);
    }
}

fn load_fixture_text(name: &str) -> Result<String, String> {
    let p = fixture(name);
    if !p.is_file() {
        return Err(format!("fixture missing: {}", p.display()));
    }
    std::fs::read_to_string(&p).map_err(|e| format!("read {name}: {e}"))
}

fn validate_fixture(file: &str) -> Result<FixtureStats, String> {
    let text = load_fixture_text(file)?;
    let (meta, geometry) =
        rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(&text, 0, false, 0)
            .map_err(|e| format!("parse_and_build: {e}"))?;
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
        let text = load_fixture_text(name)?;
        let (meta, _) =
            rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(&text, 0, false, 0)
                .map_err(|e| format!("{name}: parse: {e}"))?;

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

fn test_contiguity() -> Result<(), String> {
    for name in &["01_fan.dxf", "02_bulge.dxf", "03_ellipse.dxf", "04_spline.dxf"] {
        let text = load_fixture_text(name)?;
        let (meta, _) =
            rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(&text, 0, false, 0)
                .map_err(|e| format!("{name}: parse: {e}"))?;

        for (li, lo) in meta.layouts.iter().enumerate() {
            if lo.tris_offset != lo.lines_offset + lo.lines_len {
                return Err(format!("{name} L{} tris region not contiguous after lines", li));
            }
            if lo.points_offset != lo.tris_offset + lo.tris_len {
                return Err(format!("{name} L{} points region not contiguous after tris", li));
            }
        }
    }
    Ok(())
}

fn test_minimal_inline() -> Result<(), String> {
    let dxf = "\
  0
SECTION
  2
ENTITIES
  0
LINE
  8
0
 10
0.0
 20
0.0
 11
10.0
 21
10.0
  0
ENDSEC
  0
EOF
";
    let (meta, geometry) =
        rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(dxf, 0, false, 0)
            .map_err(|e| format!("parse: {e}"))?;
    if meta.segments < 1 {
        return Err(format!("expected >=1 segment, got {}", meta.segments));
    }
    if geometry.len() < 24 {
        return Err(format!("geo too short: {}", geometry.len()));
    }
    Ok(())
}

fn test_real_dwg() -> Result<(), String> {
    let dwg_path = PathBuf::from("/Users/danglei/Downloads/久裕设计-融侨华府定稿平面.dwg");
    if !dwg_path.is_file() {
        return Err("real DWG file not in Downloads".into());
    }
    // GeoJSON CLI path — sidesteps LibreDWG 0.14 AC1021 DXF-writer bug.
    let (meta, geometry) =
        rocktier_cad_viewer_lib::dwg_geojson::read_dwg_geojson(&dwg_path, 0, 0)
            .map_err(|e| format!("GeoJSON parse real DWG: {e}"))?;
    eprintln!("\n         real DWG → {} segs, {} texts, {} layers, {} kb geometry",
        meta.segments, meta.text_count, meta.layers.len(), geometry.len() / 1024);
    Ok(())
}
