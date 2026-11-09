//! Sawhorse desktop shell.
//!
//! Deliberately thin: the front end (the same Vite build as the browser demo) runs the
//! Rust kernel as WASM inside the webview (ADR-0001), so geometry never crosses IPC.
//! Native-only concerns live here: file dialogs and file access (lane 6: the dialog and fs
//! plugins), drag-and-drop of files onto the window, the render-credit device key in the
//! OS secure store (lane 8, ADR-0004, `device-key/`) and opening Stripe Checkout in the
//! system browser, and showing a plugin's folder (lane 11). The .shz file association lands
//! in a later lane.
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

/// Show a plugin's folder in the system file manager (lane 11, the Plugins pane's "Open
/// folder"). Only a folder that holds a plugin.json is opened, so the page cannot use this
/// to open arbitrary paths.
#[tauri::command]
fn open_plugin_folder(app: AppHandle, path: String) -> Result<(), String> {
    let dir = std::path::PathBuf::from(&path);
    if !dir.is_dir() || !dir.join("plugin.json").is_file() {
        return Err(format!("{path} is not a plugin folder"));
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Save a file without ever leaving it half written (lane 16, `sawhorse-safe-save`): the
/// bytes are the request's raw body, the path a percent-encoded `x-path` header. Only a path
/// in the fs scope (one the person picked in a Save dialog, or opened) is written, or one
/// the page made from such a path by adding the file type's extension (GTK's Save dialog
/// hands the name back as typed): then, if a file of that name already exists, the dialog
/// never asked about it, so the save is refused with `exists:<name>` unless the request
/// carries `x-replace: yes`, which the page sends after asking.
#[tauri::command]
fn save_file_atomic(app: AppHandle, request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("save_file_atomic takes the file's bytes as its body".into());
    };
    let path = request
        .headers()
        .get("x-path")
        .and_then(|v| v.to_str().ok())
        .and_then(sawhorse_safe_save::percent_decode)
        .ok_or("save_file_atomic needs the path in its x-path header")?;
    let path = std::path::PathBuf::from(path);
    let scope = app.fs_scope();
    let direct = scope.is_allowed(&path);
    // The page puts the file type's extension on a name the dialog returned without one
    // (GTK's chooser hands the name back as typed; Windows and macOS add the filter's), so
    // the path the person chose may be this one less its added extension: 1 to 10 letters
    // or digits, and never a folder (`sawhorse_safe_save::without_added_extension`).
    let via_added_extension = !direct
        && sawhorse_safe_save::without_added_extension(&path)
            .is_some_and(|chosen| scope.is_allowed(&chosen));
    if !direct && !via_added_extension {
        return Err(format!(
            "{} was not chosen in a Save dialog",
            path.display()
        ));
    }
    if via_added_extension && path.exists() {
        // The dialog asked about the typed name, not this one: the page asks, then resends.
        let consented = request
            .headers()
            .get("x-replace")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == "yes");
        if !consented {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Err(format!("exists:{name}"));
        }
    }
    sawhorse_safe_save::write_atomic(&path, bytes).map_err(|e| e.to_string())?;
    if via_added_extension {
        // The file is now the one chosen: later saves and reads go by its full name.
        let _ = scope.allow_file(&path);
    }
    Ok(())
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
            open_external,
            open_plugin_folder,
            save_file_atomic
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sawhorse");
}
