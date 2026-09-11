//! Build script for the LibreDWG C helper.
//!
//! Compiles `src/bin/dwg_layer_names.c` and links it against the LibreDWG
//! static library.  The resulting helper binary is stashed in OUT_DIR and
//! exported as the `DWG_LAYER_HELPER` env-var so the Rust code can spawn it.

use std::path::PathBuf;

fn main() {
    // Tell Cargo to rerun if the helper source changes.
    println!("cargo:rerun-if-changed=src/bin/dwg_layer_names.c");

    // Locate LibreDWG.  Brew on macOS/arm64 is the primary target; fall
    // back to a prefix the user can override via LIBREDWG_DIR.
    let prefix = std::env::var("LIBREDWG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Default brew prefix on Apple Silicon.
            PathBuf::from("/opt/homebrew/opt/libredwg")
        });

    let include_dir = prefix.join("include");
    let lib_dir = prefix.join("lib");

    if !include_dir.join("dwg.h").is_file() {
        panic!(
            "LibreDWG headers not found at {}/dwg.h — install with `brew install libredwg` \
             or set LIBREDWG_DIR=/path/to/libredwg",
            include_dir.display()
        );
    }
    if !lib_dir.join("libredwg.a").is_file() {
        panic!(
            "LibreDWG static lib not found at {}/libredwg.a — install with `brew install libredwg` \
             or set LIBREDWG_DIR=/path/to/libredwg",
            lib_dir.display()
        );
    }

    let helper_src = PathBuf::from("src/bin/dwg_layer_names.c");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let helper_bin = out_dir.join("dwg_layer_names");

    // Compile the .c to an object, then invoke the C compiler again to link
    // a standalone binary against libredwg.a.  Using the same compiler for
    // both steps keeps libc/libstdc++ selection consistent.
    let obj = out_dir.join("dwg_layer_names.o");

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());

    // Step 1: compile to .o (no linking yet).
    let status = std::process::Command::new(&cc)
        .arg("-O2")
        .arg("-I")
        .arg(&include_dir)
        .arg("-c")
        .arg(&helper_src)
        .arg("-o")
        .arg(&obj)
        .status()
        .expect("failed to invoke CC for helper compile");
    if !status.success() {
        panic!("helper compile failed (exit {:?})", status.code());
    }

    // Step 2: link the .o + libredwg.a into a standalone executable.
    let status = std::process::Command::new(&cc)
        .arg(&obj)
        .arg(lib_dir.join("libredwg.a"))
        .arg("-o")
        .arg(&helper_bin)
        .arg("-lm") // libredwg calls into libm
        .args(if cfg!(target_os = "macos") {
            vec!["-framework", "Security", "-framework", "CoreFoundation"]
        } else {
            vec![]
        })
        .status()
        .expect("failed to invoke CC for helper link");
    if !status.success() {
        panic!("helper link failed (exit {:?})", status.code());
    }

    // Export the absolute path for the Rust code.
    println!(
        "cargo:rustc-env=DWG_LAYER_HELPER={}",
        helper_bin.display()
    );
}
