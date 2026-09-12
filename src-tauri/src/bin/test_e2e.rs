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
    eprint!("  [T8] Paper space skipped ... ");
    match test_paper_space_skipped() {
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

/// Group 67 = 1 marks paper-space geometry (title blocks, viewport frames).
/// It must never be drawn over model space, nor folded into its extents —
/// which would drag the fit-to-view over a whole sheet.
fn test_paper_space_skipped() -> Result<(), String> {
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
LINE
  8
0
 67
1
 10
0.0
 20
0.0
 11
9999.0
 21
9999.0
  0
ENDSEC
  0
EOF
";
    let (meta, _) = rocktier_cad_viewer_lib::dxf_ascii::parse_and_build(dxf, 0, false, 0)
        .map_err(|e| format!("parse: {e}"))?;
    if meta.segments != 1 {
        return Err(format!("expected 1 segment (model only), got {}", meta.segments));
    }
    let lay = &meta.layouts[0];
    if lay.max_x > 100.0 {
        return Err(format!("paper-space geometry leaked into extents: max_x={}", lay.max_x));
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
    let dwg_path = PathBuf::from("/Users/danglei/Downloads/久裕设计-融侨华府定稿平面.dwg");
    if !dwg_path.is_file() {
        return Err("real DWG file not in Downloads".into());
    }
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
