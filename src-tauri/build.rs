fn main() {
    // Sets up the `desktop` / `mobile` cfgs and validates `tauri.conf.json`
    // (including bundle.fileAssociations).
    tauri_build::build();
}
