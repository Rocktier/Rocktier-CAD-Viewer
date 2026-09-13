pub mod aci;
pub mod commands;
pub mod drawing;
pub mod dxf_ascii;
pub mod menu;
pub mod model;
pub mod pdf;

use std::sync::Mutex;

use tauri::{Emitter, Manager};

/// Paths the OS asked us to open, buffered until the webview is ready — on a
/// cold start the open event arrives before the frontend exists.
#[derive(Default)]
struct OpenedFiles(Mutex<Vec<String>>);

fn looks_like_drawing(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".dxf") || lower.ends_with(".dwg")
}

fn push_opened_files(app: &tauri::AppHandle, paths: Vec<String>) {
    let paths: Vec<String> = paths
        .into_iter()
        .filter(|p| looks_like_drawing(p))
        .collect();
    if paths.is_empty() {
        return;
    }
    if let Some(state) = app.try_state::<OpenedFiles>() {
        if let Ok(mut queued) = state.0.lock() {
            queued.extend(paths.iter().cloned());
        }
    }
    let _ = app.emit("opened", paths);
}

/// Drains the buffered paths; the frontend calls this once on startup.
/// 前端挂载后（及语言切换时）按 UI 语言重建菜单。
#[tauri::command]
fn build_menu(app: tauri::AppHandle, lang: String) -> Result<(), String> {
    menu::build(&app, &lang).map_err(|e| e.to_string())
}

#[tauri::command]
fn opened_files(state: tauri::State<OpenedFiles>) -> Vec<String> {
    state
        .0
        .lock()
        .map(|mut queued| std::mem::take(&mut *queued))
        .unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .on_menu_event(|app, event| {
            let _ = app.emit("menu-action", event.id().0.clone());
        });

    // Registered first: on Windows/Linux the shell opens a file by launching a
    // second process, and this forwards its argv to the running instance, so
    // the frontend only ever sees the `opened` event.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        push_opened_files(app, argv.into_iter().skip(1).collect());
    }));

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(OpenedFiles::default())
        .invoke_handler(tauri::generate_handler![
            commands::open_drawing,
            commands::open_url,
            commands::save_export,
            build_menu,
            opened_files
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS/iOS hand opened files over as an event rather than argv.
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            if let tauri::RunEvent::Opened { urls } = event {
                let paths = urls
                    .iter()
                    .filter_map(|u| u.to_file_path().ok())
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                push_opened_files(app, paths);
            }

            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            {
                let _ = (app, event);
            }
        });
}
