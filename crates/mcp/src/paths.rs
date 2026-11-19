//! Where a tool may read and write: `.shz` files under the folder the server started with.
//!
//! A path is relative to that folder. It is refused when it is absolute, holds `..`, does
//! not end in `.shz`, or leads out of the folder through a link (its folder, or the file
//! itself when it is there, resolves outside). Its folder must already exist. A save also
//! checks the file it writes, before moving it into place (`written_inside`, #118).
//!
//! A name that means something else on Windows is refused on every system, so a path
//! names the same file everywhere: one holding `:` (a drive, or a stream of another file),
//! `\` outside Windows, a control character or one of `<>"|?*`, ending in a dot or a
//! space, or a device name such as `CON`, `NUL`, `COM1` or `LPT1` before its first dot.

use std::path::{Component, Path, PathBuf};

/// Why `name` (one folder or file name of a path) is refused, if it is.
fn odd_name(name: &str) -> Option<&'static str> {
    if name.chars().any(|c| c.is_control()) {
        return Some("a control character");
    }
    if name.contains(':') {
        return Some("a \":\"");
    }
    if name.contains('\\') {
        return Some("a \"\\\" (folders are separated by \"/\")");
    }
    if name.contains(['<', '>', '"', '|', '?', '*']) {
        return Some("one of < > \" | ? *");
    }
    if name.ends_with(['.', ' ']) {
        return Some("a dot or a space at its end");
    }
    let stem = name.split('.').next().unwrap_or("").trim_end();
    let stem = stem.to_ascii_uppercase();
    let numbered = |p: &str| {
        stem.strip_prefix(p).is_some_and(|d| {
            d.len() == 1 && d.as_bytes()[0].is_ascii_digit()
                || ["\u{b9}", "\u{b2}", "\u{b3}"].contains(&d)
        })
    };
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || numbered("COM")
        || numbered("LPT")
    {
        return Some("a device name of Windows");
    }
    None
}

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
            Component::Normal(p) => {
                let p = p
                    .to_str()
                    .ok_or_else(|| format!("{given}: the path is not valid text"))?;
                if let Some(why) = odd_name(p) {
                    return Err(format!(
                        "{}: the name {p:?} holds {why}, which is refused so that a path names the same file on every system",
                        given.escape_debug()
                    ));
                }
                parts.push(p.to_string());
            }
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

/// Refuse `written`, a file just made for the path `given`, when it resolves outside `root`
/// (already resolved), with the words `place` gives for a path out of the folder (#118).
/// `place` resolves a new file's folder, not the file. Through a link the file server
/// follows (a Samba share on Windows), the folder resolves where it was asked and only a
/// file in it shows where it really is, so a save checks the file it wrote, before moving it.
pub fn written_inside(root: &Path, written: &Path, given: &str) -> Result<(), String> {
    let given = given.trim();
    let real = std::fs::canonicalize(written)
        .map_err(|e| format!("{given}: cannot find the file just written: {e}"))?;
    if !real.starts_with(root) {
        return Err(format!("{given}: it leads out of the server's folder"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("sawhorse-mcp-tests")
            .join(format!("paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// #118: a file written under the folder is checked where it really is. One that
    /// resolves outside is refused with the words `place` gives for a path out of the folder.
    #[test]
    fn a_written_file_is_checked_where_it_really_is() {
        let base = folder("written");
        let (inside, outside) = (base.join("root"), base.join("away"));
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let root = root(&inside).unwrap();
        std::fs::write(inside.join("in.shz"), b"x").unwrap();
        std::fs::write(outside.join("out.shz"), b"x").unwrap();
        assert_eq!(
            written_inside(&root, &inside.join("in.shz"), "in.shz"),
            Ok(())
        );
        assert_eq!(
            written_inside(&root, &outside.join("out.shz"), " out/Escaped.shz "),
            Err("out/Escaped.shz: it leads out of the server's folder".to_string())
        );
        // A file that is not there (removed under it) is refused too, never taken as inside.
        assert!(written_inside(&root, &inside.join("gone.shz"), "gone.shz").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
