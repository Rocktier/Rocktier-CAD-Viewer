//! E2E-ish regression tests for DWG support.
//!
//! Strategy: `dxf` 0.6.1 (our DXF parser) cannot load the r14 DXF that
//! `dwg2dxf --as r14` emits for real AutoCAD drawings — it trips on a
//! post-ENDSEC `63/   256` (BYLAYER) cell around line 11800.  That is a crate
//! bug we cannot fix without replacing the parser, so the **full-pipeline**
//! tests live behind `#[ignore]` and serve as acceptance gates for the future
//! dx parser swap.  The conversion-only test runs on every CI: it only checks
//! that `dwg2dxf --as r14 → sanitize → tempfile` doesn't crash.

use std::path::PathBuf;

fn real_dwg() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push("Downloads");
    p.push("久裕设计-融侨华府定稿平面.dwg");
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

#[test]
fn real_dwg_conversion_pipeline_does_not_crash() {
    use super::convert_dwg_to_temp;
    if let Some(p) = real_dwg() {
        eprint!("  converting {} ... ", p.display());
        let dxf = convert_dwg_to_temp(&p).expect("conversion step crashed");
        assert!(dxf.is_file(), "dxf file not produced");
        let size = std::fs::metadata(&dxf).unwrap().len();
        assert!(size > 1000, "suspiciously small DXF: {size}");
        eprintln!("OK ({size} bytes)");
        let _ = std::fs::remove_file(&dxf);
    } else {
        eprintln!("  (skipped: real DWG fixture not found)");
    }
}

/// Acceptance gate for future DXF parser swap.  Ignored until then — see file
/// header.  Run manually with `cargo test -- --ignored`.
#[test]
#[ignore = "dxf 0.6 crate cannot parse r14 real-world DXF (post-ENDSEC 63/256)"]
fn real_dxf_loads_after_conversion() {
    use super::convert_dwg_to_temp;
    use crate::dxf_io::load_drawing;
    use crate::tess::build_scene;

    let p = real_dwg().expect("fixture missing");
    let dxf = convert_dwg_to_temp(&p).expect("convert failed");
    let drawing = load_drawing(&dxf).expect("load_drawing failed on real-world DXF");
    let ent_count = drawing.entities().count();
    eprintln!("{ent_count} entities");
    assert!(ent_count > 10, "expected many entities, got {ent_count}");

    let (_meta, geometry) =
        build_scene(&drawing).expect("build_scene failed on real-world DXF");
    assert!(geometry.len() % 12 == 0);
    let _ = std::fs::remove_file(&dxf);
}
