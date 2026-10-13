//! Saving a file without ever leaving it half written (lane 16).
//!
//! The fs plugin's `writeFile` truncates the file and writes it in place: a crash, a power
//! cut or a full disk halfway leaves a truncated `.shz` (its zip directory is written last)
//! and the previous good copy is gone. [`write_atomic`] instead writes the bytes to a
//! temporary file in the same folder, flushes it to disk, reads it back and compares it
//! byte for byte, keeps the previous file as `<name>.bak`, and only then renames the
//! temporary file over the original (a rename within one folder replaces it whole or not
//! at all). Any failure removes the temporary file and leaves the original as it was.

use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Why a save failed. The original file is unchanged.
#[derive(Debug)]
pub enum SaveError {
    /// The path has no folder or file name to write beside.
    BadPath(String),
    /// Writing, syncing, reading back or renaming failed.
    Io(String),
    /// What was read back is not what was written (a full disk, a failing drive).
    Mismatch,
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::BadPath(p) => write!(f, "cannot save to {p}"),
            SaveError::Io(e) => write!(f, "could not write the file: {e}"),
            SaveError::Mismatch => write!(
                f,
                "the file read back differently from what was written (is the disk full?); the original was kept"
            ),
        }
    }
}

impl std::error::Error for SaveError {}

/// The temporary file and backup next to `path`.
fn siblings(path: &Path) -> Result<(PathBuf, PathBuf), SaveError> {
    let name = path
        .file_name()
        .ok_or_else(|| SaveError::BadPath(path.display().to_string()))?
        .to_string_lossy()
        .into_owned();
    let dir = path.parent().unwrap_or(Path::new("."));
    Ok((
        dir.join(format!(".{name}.saving")),
        dir.join(format!("{name}.bak")),
    ))
}

/// Write `bytes` to `path` atomically (see the module docs). The previous file, if any, is
/// kept as `<name>.bak`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    let (tmp, bak) = siblings(path)?;
    let io = |e: std::io::Error| SaveError::Io(e.to_string());
    let result = (|| {
        {
            let mut f = fs::File::create(&tmp).map_err(io)?;
            f.write_all(bytes).map_err(io)?;
            f.sync_all().map_err(io)?;
        }
        // Read back: the bytes on disk must be the bytes given.
        let mut back = Vec::with_capacity(bytes.len());
        fs::File::open(&tmp)
            .map_err(io)?
            .read_to_end(&mut back)
            .map_err(io)?;
        if back != bytes {
            return Err(SaveError::Mismatch);
        }
        drop(back);
        if path.exists() {
            fs::copy(path, &bak).map_err(io)?;
        }
        fs::rename(&tmp, path).map_err(io)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Percent-decode a path sent from the webview in a header (`encodeURIComponent`).
pub fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sawhorse-safe-save-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn writes_keeps_a_backup_and_leaves_no_temporary_file() {
        let d = dir("basic");
        let p = d.join("Bench.shz");
        write_atomic(&p, b"first").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"first");
        assert!(
            !d.join("Bench.shz.bak").exists(),
            "nothing to back up the first time"
        );
        write_atomic(&p, b"second version").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"second version");
        assert_eq!(fs::read(d.join("Bench.shz.bak")).unwrap(), b"first");
        assert!(!d.join(".Bench.shz.saving").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_save_leaves_the_original() {
        let d = dir("fail");
        let p = d.join("Shelf.shz");
        write_atomic(&p, b"good").unwrap();
        // The temporary file's place is taken by a folder: the write fails, the original stays.
        fs::create_dir_all(d.join(".Shelf.shz.saving")).unwrap();
        assert!(matches!(write_atomic(&p, b"new"), Err(SaveError::Io(_))));
        assert_eq!(fs::read(&p).unwrap(), b"good");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn paths_come_through_percent_encoding() {
        assert_eq!(
            percent_decode("C%3A%5CUsers%5Cme%5CBench%20%C3%A9.shz").as_deref(),
            Some("C:\\Users\\me\\Bench é.shz")
        );
        assert_eq!(percent_decode("bad%zz"), None);
        assert!(siblings(Path::new("/")).is_err());
    }
}
