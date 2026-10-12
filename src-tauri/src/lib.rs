//! Sawhorse desktop shell.
//!
//! Deliberately thin: the front end (the same Vite build as the browser demo) runs the
//! Rust kernel as WASM inside the webview (ADR-0001), so geometry never crosses IPC.
//! Native-only concerns live here: file dialogs and file access (lane 6: the dialog and fs
//! plugins), drag-and-drop of files onto the window, the render-credit device key in the
//! OS secure store (lane 8, ADR-0004, `device-key/`) and opening Stripe Checkout in the
//! system browser. The .shz file association lands in a later lane.
//!
//! File access is scoped: the fs plugin may only touch paths the user picked in a dialog
//! (the dialog plugin adds them to the scope) or dropped on the window (added below).

use std::sync::Mutex;

use sawhorse_device_key::{external_url_allowed, DeviceKey, KeyringStore};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_fs::FsExt;
use tauri_plugin_opener::OpenerExt;

/// Reports the shell version to the front end (smoke test for IPC).
#[tauri::command]
fn shell_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The device key, loaded from (or created in) the OS keychain on first use.
#[derive(Default)]
struct Device(Mutex<Option<DeviceKey>>);

fn with_key<T>(
    state: &Device,
    f: impl FnOnce(&DeviceKey) -> Result<T, String>,
) -> Result<T, String> {
    let mut slot = state
        .0
        .lock()
        .map_err(|_| "device key lock poisoned".to_string())?;
    if slot.is_none() {
        let store = KeyringStore::new().map_err(|e| e.to_string())?;
        *slot = Some(DeviceKey::load_or_create(&store).map_err(|e| e.to_string())?);
    }
    f(slot.as_ref().expect("set above"))
}

/// The device's Ed25519 public key, base64url (the private key never leaves Rust).
#[tauri::command]
fn device_public_key(state: State<'_, Device>) -> Result<String, String> {
    with_key(&state, |k| Ok(k.public_key_b64()))
}

/// Sign a render-service request (base64url bytes in, base64url signature out).
#[tauri::command]
fn device_sign(state: State<'_, Device>, message: String) -> Result<String, String> {
    with_key(&state, |k| k.sign_b64(&message).map_err(|e| e.to_string()))
}

/// Open a checkout page in the system browser. Only Stripe Checkout (and the render
/// service's simulated checkout on loopback, in development) is allowed.
#[tauri::command]
fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    if !external_url_allowed(&url) {
        return Err(format!(
            "refusing to open {url}: only checkout pages open from the app"
        ));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Device::default())
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
        .invoke_handler(tauri::generate_handler![
            shell_version,
            device_public_key,
            device_sign,
            open_external
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sawhorse");
}
