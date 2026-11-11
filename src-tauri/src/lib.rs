//! Sawhorse desktop shell.
//!
//! Deliberately thin: the front end (the same Vite build as the browser demo) runs the
//! Rust kernel as WASM inside the webview (ADR-0001), so geometry never crosses IPC.
//! Native-only concerns live here: file dialogs and file access (lane 6: the dialog and fs
//! plugins), drag-and-drop of files onto the window, the render-credit device key in the
//! OS secure store (lane 8, ADR-0004, `device-key/`) and opening Stripe Checkout in the
//! system browser, showing a plugin's folder (lane 11), the question before the window
//! closes over unsaved changes (lane 35, `close-guard/`), and the file the app was started
//! with (lane 37, `start-file/`): a double-click on a `.shz` in Windows Explorer, where the
//! installers associate `.shz` with Sawhorse (`tauri.windows.conf.json`), or a path after the
//! program's name on any system. A second double-click starts a second window, as starting
//! Sawhorse twice does. macOS hands a double-clicked file to an app as an event, not as an
//! argument, so the association is not declared there yet, nor on Linux, where no MIME type
//! for `.shz` exists for a file manager to match. The window's title bar follows the page's
//! title, the model's name (lane 39, `window-title/`), so two windows can be told apart.
//!
//! File access is scoped: the fs plugin may only touch paths the user picked in a dialog
//! (the dialog plugin adds them to the scope), dropped on the window (added below), or the
//! one file the app was started with (added by `start_file`, below, when the page asks).

use std::path::PathBuf;
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

/// The file the app was started with (lane 37, #45), until the page takes it.
struct StartFile(Mutex<Option<PathBuf>>);

impl StartFile {
    /// Read the process's arguments once: the first that names an existing regular file
    /// File > Open takes (`sawhorse_start_file::pick`), or none. The path kept is absolute
    /// and cleaned of every `.` and `..` on its text alone (lane 39, #52): the fs plugin the
    /// page reads it through refuses a path that holds a `..`.
    fn from_args() -> Self {
        let cwd = std::env::current_dir().ok();
        // Asked of the cleaned path. Path::is_file follows a link to what it names: a folder,
        // a missing file or a device is not a regular file.
        let args = std::env::args_os().skip(1);
        let path = sawhorse_start_file::pick(args, cwd.as_deref(), std::path::Path::is_file);
        StartFile(Mutex::new(path))
    }
}

/// The file the app was started with, handed to the page once (src/io/startup.ts): the
/// first call returns its path and every later one nothing, so a reload of the page never
/// opens it again. Exactly that one path is added to the fs scope, the way a dropped file
/// is, and only then: no folder, no pattern, nothing else made from it. It is the cleaned
/// path `sawhorse_start_file::pick` returned, with no `.` or `..` in it, so the fs plugin's
/// reads (which take their path as tauri's `SafePathBuf`) never refuse it for that. The page reads it
/// through the fs plugin as it reads a dropped file. A path that is not valid Unicode is
/// never taken (the page could not name it), and one the scope refuses is not handed over.
#[tauri::command]
fn start_file(app: AppHandle, state: State<'_, StartFile>) -> Option<String> {
    let path = state.0.lock().ok()?.take()?;
    let text = path.to_str()?.to_owned();
    app.fs_scope().allow_file(&path).ok()?;
    Some(text)
}

/// The window's title bar follows the page's title (lane 39, src/io/title.ts): the model's
/// name, with the unsaved mark, as a browser shows it on the tab. The page sends the text it
/// gives `document.title`; `sawhorse_window_title::title` removes control characters and
/// line breaks, keeps at most 200 characters and falls back to "Sawhorse" when nothing is
/// left; the answer is set on the window that asked (the `Window` an app command is given is
/// the calling webview's window) and on no other. The webview is given no core:window
/// permission for this: it can set no other window's title, and nothing else about a window.
#[tauri::command]
fn window_title(window: tauri::Window, title: String) -> Result<(), String> {
    window
        .set_title(&sawhorse_window_title::title(&title))
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Device::default())
        .manage(CloseGuard::default())
        .manage(StartFile::from_args())
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
            close_guard,
            start_file,
            window_title
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sawhorse");
}
