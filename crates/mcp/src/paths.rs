//! Where a tool may read and write: `.shz` files under the folder the server started with.
//!
//! A path is relative to that folder. It is refused when it is absolute, holds `..`, does
//! not end in `.shz`, or leads out of the folder through a link (its folder, or the file
//! itself when it is there, resolves outside). Its folder must already exist.

use std::path::{Component, Path, PathBuf};

/// A path a tool may use: where it is on disk, and how the answers name it (relative to the
/// folder, with `/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub full: PathBuf,
    pub shown: String,
    /// The file is there now.
    pub exists: bool,
}

/// The folder the server works in, resolved.
pub fn root(folder: &Path) -> Result<PathBuf, String> {
    let root = std::fs::canonicalize(folder)
        .map_err(|e| format!("cannot use the folder {}: {e}", folder.display()))?;
    if !root.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    Ok(root)
}

/// Resolve `given` under `root` (already resolved), or say why not.
pub fn place(root: &Path, given: &str) -> Result<Place, String> {
    let given = given.trim();
    if given.is_empty() {
        return Err(
            "a path is needed, relative to the server's folder, like \"Bookshelf.shz\"".into(),
        );
    }
    let mut parts = Vec::new();
    for c in Path::new(given).components() {
        match c {
            Component::Normal(p) => parts.push(
                p.to_str()
                    .ok_or_else(|| format!("{given}: the path is not valid text"))?
                    .to_string(),
            ),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!(
                    "{given}: \"..\" is not allowed; paths stay inside the server's folder"
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "{given}: paths are relative to the server's folder, not absolute"
                ));
            }
        }
    }
    let Some(name) = parts.pop() else {
        return Err(format!("{given}: the path names no file"));
    };
    if !name.to_ascii_lowercase().ends_with(".shz") || name.len() <= 4 {
        return Err(format!("{given}: only .shz files are read and written"));
    }
    let mut dir = root.to_path_buf();
    for p in &parts {
        dir.push(p);
    }
    let dir = std::fs::canonicalize(&dir).map_err(|_| {
        format!("{given}: the folder it names is not there (folders are not made for you)")
    })?;
    if !dir.starts_with(root) || !dir.is_dir() {
        return Err(format!("{given}: it leads out of the server's folder"));
    }
    let full = dir.join(&name);
    let exists = std::fs::symlink_metadata(&full).is_ok();
    if exists {
        let real = std::fs::canonicalize(&full)
            .map_err(|_| format!("{given}: a link to a file that is not there"))?;
        if !real.starts_with(root) {
            return Err(format!("{given}: it leads out of the server's folder"));
        }
        if !real.is_file() {
            return Err(format!("{given}: not a file"));
        }
    }
    parts.push(name);
    Ok(Place {
        full,
        shown: parts.join("/"),
        exists,
    })
}
