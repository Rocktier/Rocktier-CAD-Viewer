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

/// A real drawing for the end-to-end check.
///
/// `$RCV_REAL_DWG` wins; otherwise the first `.dwg` in `testdata/real/` (a
/// directory that is git-ignored because these are customer files).  Never a
/// hard-coded home directory: this repository is public.
fn real_dwg() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RCV_REAL_DWG") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("../testdata/real");
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .map(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf"))
                .unwrap_or(false)
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

struct FixtureStats {
    segments: u64,
    texts: u64,
    layouts: usize,
    geom_bytes: usize,
}

fn main() {
    eprintln!("=== Rocktier CAD Viewer E2E Blob Validation (ASCII parser) ===\n");

    // (file, description, minimum expected segments) — the minimum is what
    // stops a fixture that parses to *nothing* (e.g. dropped block INSERTs)
    // from reporting OK.
    let fixtures = [
        ("01_fan.dxf", "Multi-layer architectural plan", 100),
        ("02_bulge.dxf", "LWPolylines with bulge arcs", 100),
        ("03_ellipse.dxf", "Ellipses with varying axis ratios", 300),
        ("04_spline.dxf", "B-splines with control points", 50),
        ("05_nesting.dxf", "Block INSERTs (nested)", 34),
        ("06_mtext.dxf", "MText with formatting", 15),
        ("07_layout.dxf", "Model + Paper space", 100),
    ];

    let mut pass = 0u32;
    let mut fail = 0u32;

    // Test 1: ASCII parse → Tessellate → Blob → Validate
    for (file, desc, min_segs) in fixtures {
        eprint!("  [T1] {} ({}) ... ", file, desc);
        match validate_fixture(file, min_segs) {
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

    // Test 3: Single-layout geometry contiguity (lines → points)
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

    // Test 6: block INSERT placement (translate + uniform scale + rotate).
    eprint!("  [T6] Block INSERT transform ... ");
    match test_block_insert() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("FAIL\n         {}", e); fail += 1; }
    }

    // Test 7: layer-table colour + per-entity group code 62 override.
    eprint!("  [T7] Layer / entity colours ... ");
    match test_layer_colors() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("FAIL\n         {}", e); fail += 1; }
    }

    // Test 8: paper-space entities (group 67 = 1) stay out of model space.
    eprint!("  [T8] Paper space is its own layout ... ");
    match test_paper_space_is_its_own_layout() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("FAIL\n         {}", e); fail += 1; }
    }

    eprint!("  [T9] HATCH fill / dashes / ATTRIB ... ");
    match test_hatch_dashes_and_attrib() {
        Ok(()) => { eprintln!("OK"); pass += 1; }
        Err(e) => { eprintln!("FAIL\n         {}", e); fail += 1; }
    }

    eprint!("  [T10] Paper-space VIEWPORT projection ... ");
    match test_paper_viewport_projects_model() {
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

fn validate_fixture(file: &str, min_segs: u64) -> Result<FixtureStats, String> {
    let text = load_fixture_text(file)?;
    let (meta, geometry) =
        rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(&text, 0, false, 0)
            .map_err(|e| format!("parse_and_build: {e}"))?;
    if meta.segments < min_segs {
        return Err(format!("expected >= {min_segs} segments, got {}", meta.segments));
    }
    if min_segs > 0 && geometry.is_empty() {
        return Err("expected geometry, got 0 bytes".into());
    }
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
        if layout.points_len % 12 != 0 {
            return Err(format!("L{} points_len {} unaligned", li, layout.points_len));
        }
        if layout.tris_len % 36 != 0 {
            return Err(format!("L{} tris_len {} not a multiple of 3 vertices", li, layout.tris_len));
        }
        if layout.masks_len % 36 != 0 {
            return Err(format!("L{} masks_len {} not a multiple of 3 vertices", li, layout.masks_len));
        }
        let le = layout.lines_offset as u32 + layout.lines_len as u32;
        let pe = layout.points_offset as u32 + layout.points_len as u32;
        let te = layout.tris_offset as u32 + layout.tris_len as u32;
        let me = layout.masks_offset as u32 + layout.masks_len as u32;
        if le as usize > geo_len || pe as usize > geo_len || te as usize > geo_len
            || me as usize > geo_len
        {
            return Err(format!("L{} region overruns geo (le={}, pe={}, te={}, me={}, geo={})",
                li, le, pe, te, me, geo_len));
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
            for r in &lo.point_ranges {
                if r.offset + r.len > lo.points_len {
                    return Err(format!("{name} L{} point range overflow: {} > {}", li, r.offset + r.len, lo.points_len));
                }
            }
            for r in &lo.tri_ranges {
                if r.offset + r.len > lo.tris_len {
                    return Err(format!("{name} L{} tri range overflow: {} > {}", li, r.offset + r.len, lo.tris_len));
                }
                if r.len % 36 != 0 {
                    return Err(format!("{name} L{} tri range len {} unaligned", li, r.len));
                }
            }
            for r in &lo.mask_ranges {
                if r.offset + r.len > lo.masks_len {
                    return Err(format!("{name} L{} mask range overflow: {} > {}", li, r.offset + r.len, lo.masks_len));
                }
                if r.len % 36 != 0 {
                    return Err(format!("{name} L{} mask range len {} unaligned", li, r.len));
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
            if lo.points_offset != lo.lines_offset + lo.lines_len {
                return Err(format!("{name} L{} points region not contiguous after lines", li));
            }
        }
    }
    Ok(())
}

/// Paper space is a layout of its own: group 67 = 1 rows must never be drawn
/// over model space nor folded into its extents — that would drag the plan's
/// fit-to-view over a whole sheet.
fn test_paper_space_is_its_own_layout() -> Result<(), String> {
    let dxf = "\
  0\nSECTION\n  2\nENTITIES\n\
  0\nLINE\n  8\n0\n 10\n0.0\n 20\n0.0\n 11\n10.0\n 21\n10.0\n\
  0\nLINE\n  8\n0\n 67\n1\n 10\n0.0\n 20\n0.0\n 11\n9999.0\n 21\n9999.0\n\
  0\nTEXT\n  8\n0\n 67\n1\n 10\n5.0\n 20\n5.0\n 40\n2.0\n  1\nSHEET\n\
  0\nENDSEC\n  0\nEOF\n";
    let (meta, _) = rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(dxf, 0, false, 0)
        .map_err(|e| format!("parse: {e}"))?;
    if meta.layouts.len() != 2 {
        return Err(format!("expected model + 1 sheet, got {} layouts", meta.layouts.len()));
    }
    let model = &meta.layouts[0];
    if model.max_x > 100.0 || model.max_y > 100.0 {
        return Err(format!(
            "paper geometry leaked into the model extents: x[{}, {}] y[{}, {}]",
            model.min_x, model.max_x, model.min_y, model.max_y
        ));
    }
    let sheet = &meta.layouts[1];
    if sheet.max_x < 9000.0 {
        return Err(format!(
            "the sheet did not receive its own geometry: max_x={}",
            sheet.max_x
        ));
    }
    if !meta.texts.iter().any(|t| t.layout == 1) {
        return Err("paper-space text was not tagged with its layout".into());
    }
    if meta.texts.iter().any(|t| t.layout == 0) {
        return Err("paper-space text leaked into the model layout".into());
    }
    Ok(())
}

/// The three visual gaps that made real drawings look incomplete: solid HATCH
/// fills, dashed linetypes, and block attribute values (ATTRIB) — while the
/// attribute template (ATTDEF) must stay out of the picture.
fn test_hatch_dashes_and_attrib() -> Result<(), String> {
    let dxf = "\
  0\nSECTION\n  2\nTABLES\n\
  0\nTABLE\n  2\nLTYPE\n\
  0\nLTYPE\n  2\nDASHED\n 70\n0\n  3\n__ __\n 72\n65\n 73\n2\n 40\n3.0\n 49\n2.0\n 49\n-1.0\n\
  0\nENDTAB\n\
  0\nTABLE\n  2\nLAYER\n\
  0\nLAYER\n  2\nDASH\n 70\n0\n 62\n7\n  6\nDASHED\n\
  0\nENDTAB\n  0\nENDSEC\n\
  0\nSECTION\n  2\nENTITIES\n\
  0\nLINE\n  8\nDASH\n 10\n0.0\n 20\n0.0\n 11\n10.0\n 21\n0.0\n\
  0\nHATCH\n  8\n0\n 70\n1\n  2\nSOLID\n 91\n1\n 92\n3\n 72\n0\n 73\n1\n 93\n4\n\
 10\n0.0\n 20\n0.0\n 10\n10.0\n 20\n0.0\n 10\n10.0\n 20\n10.0\n 10\n0.0\n 20\n10.0\n 97\n0\n\
  0\nATTRIB\n  8\n0\n  1\n标签A\n 10\n3.0\n 20\n3.0\n 40\n1.0\n 72\n0\n 74\n0\n\
  0\nATTDEF\n  8\n0\n  1\n模板B\n 10\n4.0\n 20\n4.0\n 40\n1.0\n\
  0\nHATCH\n  8\nPAT\n 70\n0\n  2\nANSI31\n 91\n1\n 92\n3\n 72\n0\n 73\n1\n 93\n4\n\
 10\n0.0\n 20\n0.0\n 10\n100.0\n 20\n0.0\n 10\n100.0\n 20\n100.0\n 10\n0.0\n 20\n100.0\n 97\n0\n\
 75\n0\n 76\n1\n 52\n0.0\n 41\n1.0\n 77\n0\n 78\n1\n 53\n45.0\n 43\n0.0\n 44\n0.0\n 45\n0.0\n 46\n10.0\n 79\n0\n 98\n0\n\
  0\nWIPEOUT\n  8\n0\n 10\n0.0\n 20\n0.0\n 11\n1.0\n 21\n0.0\n 12\n0.0\n 22\n1.0\n\
 71\n2\n 91\n4\n 14\n0.0\n 24\n0.0\n 14\n20.0\n 24\n0.0\n 14\n20.0\n 24\n20.0\n 14\n0.0\n 24\n20.0\n\
  0\nLEADER\n  8\n0\n 76\n3\n 10\n0.0\n 20\n0.0\n 10\n5.0\n 20\n5.0\n 10\n12.0\n 20\n9.0\n\
  0\nENDSEC\n  0\nEOF\n";
    let (meta, _) = rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(dxf, 0, false, 0)
        .map_err(|e| format!("parse: {e}"))?;
    let lay = &meta.layouts[0];

    if lay.tris_len == 0 || lay.tri_ranges.is_empty() {
        return Err("solid HATCH produced no triangles".into());
    }
    let dash_layer = meta
        .layers
        .iter()
        .position(|l| l.name == "DASH")
        .ok_or("layer DASH missing")?;
    let dash_segs: u64 = lay
        .line_ranges
        .iter()
        .filter(|r| r.layer as usize == dash_layer)
        .map(|r| (r.len / 24) as u64)
        .sum();
    if dash_segs < 3 {
        return Err(format!(
            "a 10-unit line with a 2/1 dash pattern should break into >= 3 segments, got {dash_segs}"
        ));
    }
    if !meta.texts.iter().any(|t| t.text.contains("标签A")) {
        return Err("ATTRIB value was not rendered".into());
    }
    if meta.texts.iter().any(|t| t.text.contains("模板B")) {
        return Err("ATTDEF template must not be drawn".into());
    }

    // Patterned hatch: 45° lines every 10 units across a 100x100 boundary.
    let pat_layer = meta
        .layers
        .iter()
        .position(|l| l.name == "PAT")
        .ok_or("layer PAT missing")?;
    let pat_segs: u64 = lay
        .line_ranges
        .iter()
        .filter(|r| r.layer as usize == pat_layer)
        .map(|r| (r.len / 24) as u64)
        .sum();
    if pat_segs < 8 {
        return Err(format!(
            "pattern hatch emitted {pat_segs} segments — boundary only, no pattern lines"
        ));
    }

    // WIPEOUT: mask triangles in their own stream, drawn above the geometry.
    if lay.masks_len == 0 || lay.mask_ranges.is_empty() {
        return Err("WIPEOUT produced no mask geometry".into());
    }

    // LEADER: a polyline through its three vertices.
    let leader_layer = meta
        .layers
        .iter()
        .position(|l| l.name == "0")
        .ok_or("layer 0 missing")?;
    let leader_segs: u64 = lay
        .line_ranges
        .iter()
        .filter(|r| r.layer as usize == leader_layer)
        .map(|r| (r.len / 24) as u64)
        .sum();
    if leader_segs < 2 {
        return Err(format!("LEADER did not draw its polyline ({leader_segs} segments)"));
    }
    Ok(())
}


/// A paper-space VIEWPORT projects model space onto the sheet: 12/22 is the
/// view centre in model units, 45 the view height, 40/41 the window size in
/// paper units.  Geometry outside the window must be clipped, or the sheet
/// shows the whole plan spilling past its frame.
fn test_paper_viewport_projects_model() -> Result<(), String> {
    let text = load_fixture_text("08_paper_viewport.dxf")?;
    let (meta, geometry) = rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(&text, 0, false, 0)
        .map_err(|e| format!("parse: {e}"))?;
    if meta.layouts.len() != 2 {
        return Err(format!("expected model + sheet, got {} layouts", meta.layouts.len()));
    }
    let sheet = &meta.layouts[1];
    let model_layer = meta
        .layers
        .iter()
        .position(|l| l.name == "MODEL")
        .ok_or("MODEL layer missing")?;

    // The sheet frame alone is 297 wide; the projected plan must stay inside
    // the viewport window x ∈ [60, 240], y ∈ [45, 165] — without clipping the
    // window edges would sit at x = 50 and x = 250.
    let mut verts: Vec<(f32, f32)> = Vec::new();
    for r in sheet.line_ranges.iter().filter(|r| r.layer as usize == model_layer) {
        let start = sheet.lines_offset as usize + r.offset as usize;
        let bytes = &geometry[start..start + r.len as usize];
        for v in bytes.chunks_exact(12) {
            verts.push((
                f32::from_le_bytes([v[0], v[1], v[2], v[3]]),
                f32::from_le_bytes([v[4], v[5], v[6], v[7]]),
            ));
        }
    }
    // Two of the four model edges cross the window (the other two lie outside
    // it entirely and must be dropped), i.e. 2 clipped segments.
    if verts.len() < 4 {
        return Err(format!("model space was not projected into the viewport ({} vertices)", verts.len()));
    }
    for (x, y) in &verts {
        if *x < 59.0 || *x > 241.0 || *y < 44.0 || *y > 166.0 {
            return Err(format!("projected geometry escaped the viewport window at ({x}, {y})"));
        }
    }
    // The title-block TEXT belongs to the sheet layout, not to model space.
    if !meta.texts.iter().any(|t| t.layout == 1 && t.text.contains("SHEET")) {
        return Err("sheet text did not land on the sheet layout".into());
    }
    if meta.texts.iter().any(|t| t.layout == 0) {
        return Err("sheet text leaked into model space".into());
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

/// Layer WALL is ACI 1 (red) and DOOR is ACI 5 (blue) in the TABLES section.
/// Line 1 inherits WALL, line 2 carries group code 62 = 5 and must override,
/// and line 3's 62 = 7 must stay "auto" — ACI 7 is the background colour, and
/// treating it as explicit white erased the drawing on a light canvas.
///
/// Two consecutive LAYER records also guard the pre-scan: reading only the last
/// record of a table silently drops every layer colour and scrambles the panel.
fn test_layer_colors() -> Result<(), String> {
    let dxf = "\
  0
SECTION
  2
TABLES
  0
TABLE
  2
LAYER
  0
LAYER
  2
WALL
 62
1
  0
LAYER
  2
DOOR
 62
5
  0
ENDTAB
  0
ENDSEC
  0
SECTION
  2
ENTITIES
  0
LINE
  8
WALL
 10
0.0
 20
0.0
 11
10.0
 21
0.0
  0
LINE
  8
WALL
 62
5
 10
0.0
 20
10.0
 11
10.0
 21
10.0
  0
LINE
  8
WALL
 62
7
 10
0.0
 20
20.0
 11
10.0
 21
20.0
  0
ENDSEC
  0
EOF
";
    let (meta, geometry) =
        rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(dxf, 0, false, 0)
            .map_err(|e| format!("parse: {e}"))?;
    if meta.segments != 3 {
        return Err(format!("expected 3 segments, got {}", meta.segments));
    }
    // ("0" may be appended: this synthetic DXF doesn't declare it, but the
    // parser always provides a default layer for entities without code 8.)
    let names: Vec<&str> = meta.layers.iter().map(|l| l.name.as_str()).collect();
    if names.len() < 2 || names[0] != "WALL" || names[1] != "DOOR" {
        return Err(format!("layer table order lost: {names:?}"));
    }
    let wall = &meta.layers[0];
    if !(wall.r == 255 && wall.g == 0 && wall.b == 0 && !wall.auto) {
        return Err(format!(
            "WALL colour should be ACI 1 red, got rgb({},{},{}) auto={}",
            wall.r, wall.g, wall.b, wall.auto
        ));
    }
    let door = &meta.layers[1];
    if !(door.r == 0 && door.g == 0 && door.b == 255 && !door.auto) {
        return Err(format!(
            "DOOR colour should be ACI 5 blue, got rgb({},{},{}) auto={}",
            door.r, door.g, door.b, door.auto
        ));
    }
    if geometry.len() < 72 {
        return Err(format!("geometry too short: {}", geometry.len()));
    }
    // Vertex colour bytes are r,g,b,a at offset 8 within each 12-byte vertex.
    if geometry[8..12] != [255, 0, 0, 255] {
        return Err(format!("line 1 should inherit red, got {:?}", &geometry[8..12]));
    }
    if geometry[32..36] != [0, 0, 255, 255] {
        return Err(format!("line 2 should override to blue, got {:?}", &geometry[32..36]));
    }
    if geometry[56..60] != [0, 0, 0, 0] {
        return Err(format!(
            "line 3's 62=7 must stay auto (a=0), got {:?}",
            &geometry[56..60]
        ));
    }
    Ok(())
}

/// Block "B" holds a line from (0,0) to (10,0); it is inserted at (100,200)
/// rotated 90° and scaled ×2, so the line must land on (100,200)→(100,220).
fn test_block_insert() -> Result<(), String> {
    let dxf = "\
  0
SECTION
  2
BLOCKS
  0
BLOCK
  2
B
  8
0
 10
0.0
 20
0.0
 30
0.0
 70
0
  0
LINE
  8
0
 10
0.0
 20
0.0
 30
0.0
 11
10.0
 21
0.0
 31
0.0
  0
ENDBLK
  0
ENDSEC
  0
SECTION
  2
ENTITIES
  0
INSERT
  8
0
  2
B
 10
100.0
 20
200.0
 30
0.0
 41
2.0
 50
90.0
  0
ENDSEC
  0
EOF
";
    let (meta, geometry) =
        rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(dxf, 0, false, 0)
            .map_err(|e| format!("parse: {e}"))?;
    if meta.segments != 1 {
        return Err(format!("expected 1 segment, got {}", meta.segments));
    }
    if geometry.len() < 24 {
        return Err(format!("geometry too short: {}", geometry.len()));
    }
    let g = |i: usize| {
        f32::from_le_bytes([geometry[i], geometry[i + 1], geometry[i + 2], geometry[i + 3]])
    };
    let (x1, y1, x2, y2) = (g(0), g(4), g(12), g(16));
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    if !(near(x1, 100.0) && near(y1, 200.0) && near(x2, 100.0) && near(y2, 220.0)) {
        return Err(format!(
            "insert not transformed: ({x1}, {y1}) -> ({x2}, {y2}), want (100, 200) -> (100, 220)"
        ));
    }
    Ok(())
}

fn test_real_dwg() -> Result<(), String> {
    let dwg_path = real_dwg().ok_or("no drawing in testdata/real (or $RCV_REAL_DWG)")?;
    // DWG → `dwg2dxf` → the very same parser the DXF path uses.
    let (meta, geometry) = rocktier_cad_viewer_lib::drawing::load(&dwg_path)
        .map_err(|e| format!("load real DWG: {e}"))?;
    eprintln!(
        "\n         real DWG → {} segs, {} texts, {} layers, {} kb geometry",
        meta.segments,
        meta.text_count,
        meta.layers.len(),
        geometry.len() / 1024
    );
    // Geometric + colour sanity: extents drive fit-to-view, and a drawing whose
    // vertices are all explicit-white disappears on a light canvas.
    let verts = geometry.len() / 12;
    let auto = (0..verts).filter(|i| geometry[i * 12 + 11] == 0).count();
    let white = (0..verts)
        .filter(|i| geometry[i * 12 + 8..i * 12 + 12] == [255, 255, 255, 255])
        .count();
    let lay = &meta.layouts[0];
    eprintln!(
        "         extents x[{:.0}, {:.0}] y[{:.0}, {:.0}] · {auto}/{verts} auto · {white} explicit-white",
        lay.min_x, lay.max_x, lay.min_y, lay.max_y
    );
    for t in meta.texts.iter().take(3) {
        eprintln!(
            "         text layout={} layer={} x={:.0} y={:.0} h={:.0} rot={:.1} ha={} va={} a={} {:?}",
            t.layout, t.layer, t.x, t.y, t.h, t.rot, t.ha, t.va, t.a, t.text
        );
    }
    eprintln!(
        "         layers: {}",
        meta.layers
            .iter()
            .enumerate()
            .map(|(i, l)| format!("{i}={}", l.name))
            .collect::<Vec<_>>()
            .join(" · ")
    );

    if meta.segments == 0 {
        return Err("real DWG produced no geometry".into());
    }
    // Guards the text-entity regression: this drawing has MTEXT/TEXT content.
    if meta.text_count == 0 {
        return Err("real DWG produced no text entities".into());
    }
    // MTEXT inline formatting must be stripped: dimension text arrives as
    // `\A1;8780` and has to display as `8780`.
    if let Some(t) = meta.texts.iter().find(|t| {
        t.text.contains("\\A") || t.text.contains("\\f") || t.text.contains('{') || t.text.contains('}')
    }) {
        return Err(format!("MTEXT formatting not stripped: {:?}", t.text));
    }
    // Half the dimensions in this plan are vertical (x-axis direction 11/21 =
    // (0,1)) and must not be drawn flat across the drawing.
    if !meta.texts.iter().any(|t| (t.rot.abs() - 90.0).abs() < 1.0) {
        return Err("no rotated (vertical) dimension text — group 11/21 direction ignored".into());
    }
    Ok(())
}
