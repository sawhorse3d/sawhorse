//! Sawhorse desktop shell.
//!
//! Deliberately thin: the front end (the same Vite build as the browser demo) runs the
//! Rust kernel as WASM inside the webview (ADR-0001), so geometry never crosses IPC.
//! Native-only concerns live here: file dialogs and file access (lane 6: the dialog and fs
//! plugins), drag-and-drop of files onto the window; the OS secure store for the device
//! key and the .shz file association land in later lanes.
//!
//! File access is scoped: the fs plugin may only touch paths the user picked in a dialog
//! (the dialog plugin adds them to the scope) or dropped on the window (added below).

use tauri::{Emitter, Manager};
use tauri_plugin_fs::FsExt;

/// Reports the shell version to the front end (smoke test for IPC).
#[tauri::command]
fn shell_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .on_window_event(|window, event| {
            // Files dropped on the window: allow exactly those paths, then tell the page,
            // which reads them through the fs plugin (src/io/files.ts, onTauriDrop).
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                let scope = window.app_handle().fs_scope();
                for p in paths {
                    let _ = scope.allow_file(p);
                }
                let _ = window.emit("sawhorse://dropped", paths);
            }
        })
        .invoke_handler(tauri::generate_handler![shell_version])
        .run(tauri::generate_context!())
        .expect("error while running Sawhorse");
}
