// Tauri commands live in the `commands/` module; processing logic in
// `processing/` is pure and unit-testable without spawning the UI.

mod commands;
mod error;
mod paths;
mod processing;

use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::image::StagingManager {
            dir: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            commands::hash::hash_files,
            commands::hash::hash_cancel,
            commands::hash::compare_files,
            commands::hash::export_checksums,
            commands::image::convert_files,
            commands::image::image_cancel,
            commands::image::image_limits,
            commands::image::save_converted,
            commands::image::save_all_converted,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
