//! Entity-coverage diagnostic — the tool that answers "what is missing?".
//!
//! It loads a drawing through the *real* pipeline (`drawing::load`, i.e. the
//! same dwg2dxf + `dxf_ascii` parse the viewer uses), prints how much geometry
//! landed on each layer, and optionally dumps the scene blob so
//! `tools/render_scene.py` can rasterise exactly what the viewer would draw.
//!
//! Two questions this settles:
//!   1. Did the *converter* (dwg2dxf) lose entities, or did *our parser* skip
//!      them?  Pair it with `simplify_coverage.py <dxf>` (counts entity types in
//!      the DXF itself) — compare the two tables.
//!   2. Does the rendered picture match a reference viewer?  Dump + rasterise
//!      and put the images side by side.
//!
//! Usage:
//!   cargo run --features tools --bin coverage -- <file.dwg|file.dxf> [--dump DIR]

use std::path::{Path, PathBuf};

use rocktier_cad_viewer_lib::drawing;
use rocktier_cad_viewer_lib::model::encode_blob;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("用法 / usage: coverage <file.dwg|file.dxf> [--dump DIR]");
        std::process::exit(2);
    }
    let path = PathBuf::from(&args[0]);
    let dump_dir = args
        .iter()
        .position(|a| a == "--dump")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    let t0 = std::time::Instant::now();
    let (meta, geometry) = match drawing::load(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("加载失败 / load failed: {e}");
            std::process::exit(1);
        }
    };
    let wall_ms = t0.elapsed().as_millis();

    println!("文件 / file        : {}", path.display());
    println!(
        "耗时 / timings     : dwg2dxf {} ms · 解析 {} ms · 总计 {} ms",
        meta.convert_ms, meta.parse_ms, wall_ms
    );
    println!(
        "实体 / segments    : {} 段 · {} 文字 · {} 图层 · {} 布局",
        meta.segments,
        meta.text_count,
        meta.layers.len(),
        meta.layouts.len()
    );
    println!(
        "跳过 / skipped     : {} 个实体未渲染",
        meta.skipped
    );

    for (li, lay) in meta.layouts.iter().enumerate() {
        println!(
            "\n布局 [{}] {} — 包围盒 x[{:.0}, {:.0}] y[{:.0}, {:.0}]",
            li, lay.name, lay.min_x, lay.max_x, lay.min_y, lay.max_y
        );
        let mut rows: Vec<(String, u64, u64)> = Vec::new();
        for r in &lay.line_ranges {
            let name = meta
                .layers
                .get(r.layer as usize)
                .map(|l| l.name.clone())
                .unwrap_or_else(|| format!("#{}", r.layer));
            let segs = (r.len / 24) as u64;
            let pts = lay
                .point_ranges
                .iter()
                .find(|p| p.layer == r.layer)
                .map(|p| (p.len / 12) as u64)
                .unwrap_or(0);
            rows.push((name, segs, pts));
        }
        for r in &lay.point_ranges {
            if !rows.iter().any(|(_, _, p)| *p > 0) {
                continue;
            }
            if !lay.line_ranges.iter().any(|l| l.layer == r.layer) {
                let name = meta
                    .layers
                    .get(r.layer as usize)
                    .map(|l| l.name.clone())
                    .unwrap_or_else(|| format!("#{}", r.layer));
                rows.push((name, 0, (r.len / 12) as u64));
            }
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.1 + r.2));
        for (name, segs, pts) in rows {
            println!("   {:<28} {:>8} 段 {:>7} 点", name, segs, pts);
        }
        let texts: Vec<_> = meta.texts.iter().filter(|t| t.layout as usize == li).collect();
        if !texts.is_empty() {
            println!("   --- 文字 {} 条（前 8）", texts.len());
            for t in texts.iter().take(8) {
                println!(
                    "      layer={} ({:.0},{:.0}) h={:.0} rot={:.0} ha={} va={} {:?}",
                    meta.layers
                        .get(t.layer as usize)
                        .map(|l| l.name.as_str())
                        .unwrap_or("?"),
                    t.x, t.y, t.h, t.rot, t.ha, t.va, t.text
                );
            }
        }
        let mut per_layer = vec![0usize; meta.layers.len()];
        for t in &texts {
            per_layer[t.layer as usize] += 1;
        }
        println!("   --- 文字分布");
        for (i, n) in per_layer.iter().enumerate() {
            if *n > 0 {
                println!("      {:<28} {:>6}", meta.layers[i].name, n);
            }
        }
    }

    if let Some(dir) = dump_dir {
        if let Err(e) = dump(&dir, &meta, &geometry) {
            eprintln!("dump 失败 / dump failed: {e}");
            std::process::exit(1);
        }
        println!("\n已导出 / dumped   : {}", dir.display());
    }
}

fn dump(
    dir: &Path,
    meta: &rocktier_cad_viewer_lib::model::SceneMeta,
    geometry: &[u8],
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(meta).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("meta.json"), &json).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("geometry.bin"), geometry).map_err(|e| e.to_string())?;
    // Same bytes the webview receives — lets other tools replay the real blob.
    std::fs::write(dir.join("scene.rcv"), encode_blob(&json, geometry)).map_err(|e| e.to_string())?;
    Ok(())
}
