//! Sawhorse desktop shell.
//!
//! Deliberately thin: the front end (the same Vite build as the browser demo) runs the
//! Rust kernel as WASM inside the webview (ADR-0001), so geometry never crosses IPC.
//! Native-only concerns live here: file dialogs and file access (lane 6: the dialog and fs
//! plugins), drag-and-drop of files onto the window, the render-credit device key in the
//! OS secure store (lane 8, ADR-0004, `device-key/`) and opening Stripe Checkout in the
//! system browser, showing a plugin's folder (lane 11), and the question before the window
//! closes over unsaved changes (lane 35, `close-guard/`). The .shz file association lands
//! in a later lane.
//!
//! File access is scoped: the fs plugin may only touch paths the user picked in a dialog
//! (the dialog plugin adds them to the scope) or dropped on the window (added below).

use std::sync::Mutex;
use std::time::Instant;

use sawhorse_close_guard::{Guard, Request, Say, Then};
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
/// hands the name back as typed): then the dialog's path comes along as `x-chosen`, and if
/// a file of the final name already exists the dialog never asked about it, so the save is
/// refused with `exists:<name>` unless the request carries `x-replace: yes`, which the page
/// sends after asking. The decision is `sawhorse_safe_save::decide`; the scope is never
/// widened by a save.
#[tauri::command]
fn save_file_atomic(app: AppHandle, request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("save_file_atomic takes the file's bytes as its body".into());
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let path = header("x-path")
        .as_deref()
        .and_then(sawhorse_safe_save::percent_decode)
        .ok_or("save_file_atomic needs the path in its x-path header")?;
    let path = std::path::PathBuf::from(path);
    let chosen = match header("x-chosen") {
        None => None,
        Some(h) => Some(std::path::PathBuf::from(
            sawhorse_safe_save::percent_decode(&h)
                .ok_or("save_file_atomic: x-chosen is not percent-encoded")?,
        )),
    };
    let replace = header("x-replace").as_deref() == Some("yes");
    let scope = app.fs_scope();
    match sawhorse_safe_save::decide(
        &path,
        chosen.as_deref(),
        replace,
        |p| scope.is_allowed(p),
        |p| p.exists(),
        |p| p.is_dir(),
    ) {
        sawhorse_safe_save::Verdict::Write => {
            sawhorse_safe_save::write_atomic(&path, bytes).map_err(|e| e.to_string())
        }
        sawhorse_safe_save::Verdict::NotChosen => Err(format!(
            "{} was not chosen in a Save dialog",
            path.display()
        )),
        sawhorse_safe_save::Verdict::Exists(name) => Err(format!("exists:{name}")),
    }
}

/// The close question's state (lane 35, `sawhorse_close_guard`), with the clock it reads.
struct CloseGuard {
    started: Instant,
    guard: Mutex<Guard>,
}

impl Default for CloseGuard {
    fn default() -> Self {
        CloseGuard {
            started: Instant::now(),
            guard: Mutex::new(Guard::new()),
        }
    }
}

impl CloseGuard {
    /// What to do with a close request. A guard that cannot be read lets the window close:
    /// it is never held open by a fault here.
    fn request(&self) -> Request {
        let now = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match self.guard.lock() {
            Ok(mut g) => g.request(now),
            Err(_) => Request::Close,
        }
    }
}

/// The page's side of the close question (lane 35, #44; src/io/closing.ts): "unsaved" and
/// "saved" as the model changes, then the answer to a `sawhorse://close-requested` event:
/// "keep" (No), "close" (Yes: the window is destroyed) or "failed" (it could not ask: the
/// next close request closes at once); "exit" is File > Exit, an ordinary close request.
/// Whatever the page says or fails to say, a close request two seconds after an unanswered
/// question closes the window (`sawhorse_close_guard`).
#[tauri::command]
fn close_guard(
    window: tauri::Window,
    state: State<'_, CloseGuard>,
    say: String,
) -> Result<(), String> {
    let say = Say::parse(&say).ok_or_else(|| format!("close_guard: no such message {say:?}"))?;
    // The lock is released at the end of this statement: Window::close raises a close
    // request whose handler takes it again.
    let then = state
        .guard
        .lock()
        .map_err(|_| "close guard lock poisoned".to_string())?
        .say(say);
    match then {
        Then::Nothing => Ok(()),
        Then::Destroy => window.destroy().map_err(|e| e.to_string()),
        Then::RequestClose => window.close().map_err(|e| e.to_string()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Device::default())
        .manage(CloseGuard::default())
        .on_window_event(|window, event| match event {
            // Files dropped on the window: allow exactly those paths, then tell the page,
            // which reads them through the fs plugin (src/io/files.ts, onTauriDrop).
            tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                let scope = window.app_handle().fs_scope();
                for p in paths {
                    let _ = scope.allow_file(p);
                }
                let _ = window.emit("sawhorse://dropped", paths);
            }
            // The X, Alt+F4, the taskbar's Close, the red button, Window::close (File > Exit):
            // with unsaved changes the page is asked first (lane 35). The page never registers
            // Tauri's own close-requested listener, which would hold every close until the
            // page destroyed the window.
            tauri::WindowEvent::CloseRequested { api, .. } => {
                match window.state::<CloseGuard>().request() {
                    Request::Close => {}
                    Request::Wait => api.prevent_close(),
                    // Kept open only when the question reached the page's event system.
                    Request::Ask => {
                        if window.emit("sawhorse://close-requested", ()).is_ok() {
                            api.prevent_close();
                        }
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            shell_version,
            device_public_key,
            device_sign,
            open_external,
            open_plugin_folder,
            save_file_atomic,
            close_guard
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sawhorse");
}
