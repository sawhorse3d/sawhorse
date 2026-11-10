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

/// `path` as a stem and the extension the page could have added to it: 1 to 10 ASCII
/// letters or digits (the rule the plugins' `file.save` keeps). `cabinet.shz` is
/// (`cabinet`, `shz`), `cabinet.v2.shz` is (`cabinet.v2`, `shz`); `cabinet`, `.shz`,
/// `part.tar-gz` and `part.a2345678901` are nothing.
pub fn added_extension(path: &Path) -> Option<(PathBuf, String)> {
    let ext = path.extension()?.to_str()?;
    if ext.is_empty() || ext.len() > 10 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some((path.with_extension(""), ext.to_string()))
}

/// What the shell does with a save ([`decide`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Write the file.
    Write,
    /// The path was not chosen in a dialog, nor made from one by adding the type's extension.
    NotChosen,
    /// A file of the final name exists and nobody has been asked about it: the page asks
    /// ("cabinet.shz already exists. Replace it?") and sends the save again with consent.
    Exists(String),
}

/// Decide a save without touching the shell or the disk. `path` is the file to write.
/// `chosen`, when the page added the file type's extension (GTK's Save dialog hands the name
/// back as typed; Windows and macOS add the filter's), is the path the dialog returned, so
/// `path` must be it plus one extension [`added_extension`] accepts. `replace` is the page's
/// word that the person agreed to replace an existing file. `allowed` is the fs scope's
/// answer (paths dialogs returned and files dropped on the window), `exists` and `is_dir` the
/// file system's.
///
/// With a `chosen` path the scope's answer for `path` itself counts for nothing: a file
/// saved this way before, or opened through File > Open, is in the scope, and typing its
/// stem in a later Save As would otherwise replace it with no question (the dialog asked
/// about the typed name, not this one). The scope is never widened by a save.
pub fn decide(
    path: &Path,
    chosen: Option<&Path>,
    replace: bool,
    allowed: impl Fn(&Path) -> bool,
    exists: impl Fn(&Path) -> bool,
    is_dir: impl Fn(&Path) -> bool,
) -> Verdict {
    let Some(chosen) = chosen else {
        // The dialog returned this path as it is, and asked about replacing itself.
        return if allowed(path) {
            Verdict::Write
        } else {
            Verdict::NotChosen
        };
    };
    let follows = added_extension(path).is_some_and(|(stem, _)| stem == chosen);
    if !follows || !allowed(chosen) || is_dir(chosen) {
        return Verdict::NotChosen;
    }
    if exists(path) && !replace {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Verdict::Exists(name);
    }
    Verdict::Write
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

#[cfg(test)]
mod deciding {
    use super::{added_extension, decide, Verdict};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    #[test]
    fn an_extension_the_page_could_have_added_is_1_to_10_letters_or_digits() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(
            added_extension(Path::new("/h/cabinet.shz")),
            Some((p("/h/cabinet"), "shz".into()))
        );
        assert_eq!(
            added_extension(Path::new("/h/cabinet.v2.shz")),
            Some((p("/h/cabinet.v2"), "shz".into()))
        );
        assert_eq!(
            added_extension(Path::new("/h/Cabinet.SHZ")),
            Some((p("/h/Cabinet"), "SHZ".into()))
        );
        assert_eq!(
            added_extension(Path::new("/h/part.a234567890")),
            Some((p("/h/part"), "a234567890".into()))
        );
        for none in [
            "/h/cabinet",
            "/h/.shz",
            "/h/part.a2345678901",
            "/h/part.tar-gz",
            "/h/part.sh z",
            "/h/part.shz.",
        ] {
            assert_eq!(added_extension(Path::new(none)), None, "{none}");
        }
    }

    /// A scope (what dialogs returned and drops allowed), the files on disk, the folders.
    struct World {
        scope: BTreeSet<PathBuf>,
        files: BTreeSet<PathBuf>,
        dirs: BTreeSet<PathBuf>,
    }

    impl World {
        fn new() -> World {
            World {
                scope: BTreeSet::new(),
                files: BTreeSet::new(),
                dirs: BTreeSet::new(),
            }
        }
        fn decide(&self, path: &str, chosen: Option<&str>, replace: bool) -> Verdict {
            decide(
                Path::new(path),
                chosen.map(Path::new),
                replace,
                |p| self.scope.contains(p),
                |p| self.files.contains(p),
                |p| self.dirs.contains(p),
            )
        }
        /// The dialog returned `p`: it is in the scope now.
        fn dialog(&mut self, p: &str) {
            self.scope.insert(PathBuf::from(p));
        }
        /// A save that the shell wrote.
        fn wrote(&mut self, p: &str) {
            self.files.insert(PathBuf::from(p));
        }
    }

    #[test]
    fn a_path_the_dialog_returned_as_it_is_writes_when_in_the_scope() {
        let mut w = World::new();
        assert_eq!(w.decide("/h/cabinet.shz", None, false), Verdict::NotChosen);
        w.dialog("/h/cabinet.shz");
        assert_eq!(w.decide("/h/cabinet.shz", None, false), Verdict::Write);
        // Replacing is the dialog's own question there.
        w.wrote("/h/cabinet.shz");
        assert_eq!(w.decide("/h/cabinet.shz", None, false), Verdict::Write);
    }

    #[test]
    fn an_added_extension_writes_a_new_file_and_asks_about_an_existing_one() {
        let mut w = World::new();
        w.dialog("/h/cabinet");
        assert_eq!(
            w.decide("/h/cabinet.shz", Some("/h/cabinet"), false),
            Verdict::Write
        );
        w.wrote("/h/cabinet.shz");
        assert_eq!(
            w.decide("/h/cabinet.shz", Some("/h/cabinet"), false),
            Verdict::Exists("cabinet.shz".into())
        );
        assert_eq!(
            w.decide("/h/cabinet.shz", Some("/h/cabinet"), true),
            Verdict::Write
        );
    }

    #[test]
    fn a_file_saved_this_way_before_is_asked_about_again() {
        // The review's first case: Save As "cabinet" wrote cabinet.shz; later, with another
        // model, Save As "cabinet" again. GTK did not ask (no file is named "cabinet"), and the
        // scope's answer for cabinet.shz must not stand in for the question.
        let mut w = World::new();
        w.dialog("/h/cabinet");
        w.wrote("/h/cabinet.shz");
        w.dialog("/h/cabinet.shz"); // as if a save had widened the scope, or the file was opened
        assert_eq!(
            w.decide("/h/cabinet.shz", Some("/h/cabinet"), false),
            Verdict::Exists("cabinet.shz".into())
        );
    }

    #[test]
    fn a_file_opened_or_dropped_is_asked_about_too() {
        // The review's second case: cabinet.shz came in through File > Open (so it is in the
        // scope), then Save As "cabinet" with a different model.
        let mut w = World::new();
        w.dialog("/h/cabinet.shz");
        w.wrote("/h/cabinet.shz");
        w.dialog("/h/cabinet");
        assert_eq!(
            w.decide("/h/cabinet.shz", Some("/h/cabinet"), false),
            Verdict::Exists("cabinet.shz".into())
        );
        assert_eq!(
            w.decide("/h/cabinet.shz", Some("/h/cabinet"), true),
            Verdict::Write
        );
    }

    #[test]
    fn the_chosen_path_must_be_the_dialogs_a_file_and_the_stem_of_the_path() {
        let mut w = World::new();
        // Not returned by a dialog.
        assert_eq!(
            w.decide("/h/cabinet.shz", Some("/h/cabinet"), false),
            Verdict::NotChosen
        );
        // A folder the person picked (Load unpacked folder) makes nothing writable beside it.
        w.dialog("/h/plans");
        w.dirs.insert(PathBuf::from("/h/plans"));
        assert_eq!(
            w.decide("/h/plans.shz", Some("/h/plans"), false),
            Verdict::NotChosen
        );
        // The path must be the chosen one plus one extension under the rule.
        w.dialog("/h/cabinet");
        assert_eq!(
            w.decide("/h/other.shz", Some("/h/cabinet"), false),
            Verdict::NotChosen
        );
        assert_eq!(
            w.decide("/h/cabinet.tar-gz", Some("/h/cabinet"), false),
            Verdict::NotChosen
        );
        assert_eq!(
            w.decide("/h/cabinet.v2.shz", Some("/h/cabinet"), false),
            Verdict::NotChosen
        );
        assert_eq!(
            w.decide("/h/cabinet", Some("/h/cabinet"), false),
            Verdict::NotChosen
        );
        // No chain: cabinet.shz written through "cabinet" does not make cabinet.shz.txt writable.
        w.wrote("/h/cabinet.shz");
        assert_eq!(
            w.decide("/h/cabinet.shz.txt", Some("/h/cabinet.shz"), false),
            Verdict::NotChosen
        );
    }
}

/// The maintainer's checks of [`decide`] (lane 32, pull request #29): the shell as it now
/// behaves, every save the page makes, and the edges of the rule.
#[cfg(test)]
mod deciding_in_the_shell {
    use super::{decide, write_atomic, Verdict};
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// The shell with a scope: what the dialogs returned, the files opened or dropped. A save
    /// writes the file and never touches the scope, as `save_file_atomic` does now.
    #[derive(Default)]
    struct Shell {
        scope: BTreeSet<PathBuf>,
        files: BTreeSet<PathBuf>,
        dirs: BTreeSet<PathBuf>,
    }

    impl Shell {
        /// A Save or Open dialog returned `p`, or `p` was dropped on the window.
        fn allow(&mut self, p: &str) {
            self.scope.insert(PathBuf::from(p));
        }
        fn file(&mut self, p: &str) {
            self.files.insert(PathBuf::from(p));
        }
        fn verdict(&self, path: &str, chosen: Option<&str>, replace: bool) -> Verdict {
            decide(
                Path::new(path),
                chosen.map(Path::new),
                replace,
                |p| self.scope.contains(p),
                |p| self.files.contains(p) || self.dirs.contains(p),
                |p| self.dirs.contains(p),
            )
        }
        /// What `save_file_atomic` does with a request: write on `Write`, nothing else.
        fn save(&mut self, path: &str, chosen: Option<&str>, replace: bool) -> Verdict {
            let v = self.verdict(path, chosen, replace);
            if v == Verdict::Write {
                self.file(path);
            }
            v
        }
    }

    fn exists(name: &str) -> Verdict {
        Verdict::Exists(name.to_string())
    }

    #[test]
    fn a_second_save_as_of_the_same_typed_name_is_asked_about() {
        // Linux, GTK: Save As "cabinet" twice in one session, a different model each time.
        let mut s = Shell::default();
        s.allow("/h/cabinet");
        assert_eq!(
            s.save("/h/cabinet.shz", Some("/h/cabinet"), false),
            Verdict::Write
        );
        assert!(
            !s.scope.contains(Path::new("/h/cabinet.shz")),
            "a save never widens the scope"
        );
        s.allow("/h/cabinet"); // the second dialog returns the same typed name
        assert_eq!(
            s.save("/h/cabinet.shz", Some("/h/cabinet"), false),
            exists("cabinet.shz")
        );
        // No on the question: nothing is sent again. Yes: the same request with consent.
        assert_eq!(
            s.save("/h/cabinet.shz", Some("/h/cabinet"), true),
            Verdict::Write
        );
    }

    #[test]
    fn a_file_opened_or_dropped_then_saved_as_its_bare_name_is_asked_about() {
        for how in ["opened", "dropped"] {
            let mut s = Shell::default();
            s.file("/h/cabinet.shz");
            s.allow("/h/cabinet.shz"); // File > Open, or a drop: both allow the file
            s.allow("/h/cabinet"); // Save As, typing "cabinet"
            assert_eq!(
                s.save("/h/cabinet.shz", Some("/h/cabinet"), false),
                exists("cabinet.shz"),
                "{how}"
            );
            // That file's own Ctrl+S (its handle has no chosen path) writes as it always did.
            assert_eq!(
                s.save("/h/cabinet.shz", None, false),
                Verdict::Write,
                "{how}"
            );
        }
    }

    #[test]
    fn no_chain_from_a_save_or_from_an_opened_file() {
        let mut s = Shell::default();
        s.allow("/h/cabinet");
        assert_eq!(
            s.save("/h/cabinet.shz", Some("/h/cabinet"), false),
            Verdict::Write
        );
        // The file just written is not in the scope, so it cannot be the next chosen path.
        for chosen in ["/h/cabinet.shz", "/h/cabinet"] {
            assert_eq!(
                s.save("/h/cabinet.shz.txt", Some(chosen), false),
                Verdict::NotChosen,
                "{chosen}"
            );
            assert_eq!(
                s.save("/h/cabinet.shz.txt", Some(chosen), true),
                Verdict::NotChosen,
                "{chosen} with consent"
            );
        }
        assert_eq!(
            s.save("/h/cabinet.shz.txt", None, false),
            Verdict::NotChosen
        );
        assert_eq!(s.save("/h/cabinet.shz.txt", None, true), Verdict::NotChosen);
    }

    #[test]
    fn without_a_chosen_path_the_rule_is_the_scope_alone_as_before() {
        // Windows and macOS (the dialog adds the extension), a name typed with it on Linux,
        // Ctrl+S after an open, a drop or such a Save As, every export and a plugin's
        // file.save: no x-chosen. The answer is the old one, whatever x-replace, the disk or
        // the folders say, and it never asks.
        let paths = [
            "/h/cabinet.shz",
            "/h/cabinet",
            "/h/.shz",
            "/h/part.stl",
            "C:\\Models\\Cabinet.SHZ",
        ];
        for p in paths {
            for allowed in [false, true] {
                for on_disk in [false, true] {
                    for folder in [false, true] {
                        for replace in [false, true] {
                            let v = decide(
                                Path::new(p),
                                None,
                                replace,
                                |_| allowed,
                                |_| on_disk || folder,
                                |_| folder,
                            );
                            let old = if allowed {
                                Verdict::Write
                            } else {
                                Verdict::NotChosen
                            };
                            assert_eq!(
                                v, old,
                                "{p} allowed={allowed} on_disk={on_disk} folder={folder} replace={replace}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_extension_rule_at_its_edges() {
        let mut s = Shell::default();
        s.allow("/h/cabinet");
        let ok = |s: &Shell, p: &str| s.verdict(p, Some("/h/cabinet"), false);
        // Accepted: 1 to 10 ASCII letters or digits, either case.
        for p in [
            "/h/cabinet.shz",
            "/h/cabinet.SHZ",
            "/h/cabinet.Shz",
            "/h/cabinet.3mf",
            "/h/cabinet.7",
            "/h/cabinet.a234567890",
        ] {
            assert_eq!(ok(&s, p), Verdict::Write, "{p}");
        }
        // Refused: empty, a trailing dot, 11 characters, a non-ASCII letter, a separator or a
        // second dot inside, a space, a dash, the same path, another stem.
        for p in [
            "/h/cabinet.",
            "/h/cabinet..",
            "/h/cabinet.a2345678901",
            "/h/cabinet.sh\u{e9}",
            "/h/cabinet.\u{ff53}hz",
            "/h/cabinet.s/hz",
            "/h/cabinet.s\\hz",
            "/h/cabinet.s.hz",
            "/h/cabinet.sh z",
            "/h/cabinet.tar-gz",
            "/h/cabinet",
            "/h/cabinet/shz",
            "/h/Cabinet.shz",
            "/h/cabinets.shz",
            "/other/cabinet.shz",
        ] {
            assert_eq!(ok(&s, p), Verdict::NotChosen, "{p}");
        }
    }

    #[test]
    fn hidden_and_dotted_names() {
        let mut s = Shell::default();
        // GTK returns "/h/.shz" for a typed ".shz": a hidden file that already has its
        // extension, so the page adds nothing and the plain rule writes it, as before.
        s.allow("/h/.shz");
        assert_eq!(s.verdict("/h/.shz", None, false), Verdict::Write);
        // Nothing can be made from a folder's own name: "/h/.shz" is not "/h" plus "shz".
        s.allow("/h");
        assert_eq!(s.verdict("/h/.shz", Some("/h"), false), Verdict::NotChosen);
        // A typed ".notes" is a hidden name; the page adds ".shz" and the shell agrees.
        s.allow("/h/.notes");
        assert_eq!(
            s.verdict("/h/.notes.shz", Some("/h/.notes"), false),
            Verdict::Write
        );
        // A typed "cabinet." gets ".shz" after its dot; the stem is the typed name exactly.
        s.allow("/h/cabinet.");
        assert_eq!(
            s.verdict("/h/cabinet..shz", Some("/h/cabinet."), false),
            Verdict::Write
        );
        // A name with a dot of its own: "cabinet.v2" plus ".shz".
        s.allow("/h/cabinet.v2");
        assert_eq!(
            s.verdict("/h/cabinet.v2.shz", Some("/h/cabinet.v2"), false),
            Verdict::Write
        );
    }

    #[test]
    fn the_chosen_path_must_be_in_the_scope_and_not_a_folder_whatever_else_holds() {
        let mut s = Shell::default();
        // The final path in the scope does not stand in for the chosen one.
        s.allow("/h/cabinet.shz");
        assert_eq!(
            s.verdict("/h/cabinet.shz", Some("/h/cabinet"), false),
            Verdict::NotChosen
        );
        assert_eq!(
            s.verdict("/h/cabinet.shz", Some("/h/cabinet"), true),
            Verdict::NotChosen
        );
        // A folder picked in a dialog (Load unpacked folder) or dropped is in the scope too.
        s.allow("/h/plans");
        s.dirs.insert(PathBuf::from("/h/plans"));
        for replace in [false, true] {
            assert_eq!(
                s.verdict("/h/plans.shz", Some("/h/plans"), replace),
                Verdict::NotChosen
            );
        }
    }

    #[test]
    fn consent_is_the_only_way_past_an_existing_file() {
        let mut s = Shell::default();
        s.allow("/h/cabinet");
        s.file("/h/cabinet.shz");
        assert_eq!(
            s.verdict("/h/cabinet.shz", Some("/h/cabinet"), false),
            exists("cabinet.shz")
        );
        assert_eq!(
            s.verdict("/h/cabinet.shz", Some("/h/cabinet"), true),
            Verdict::Write
        );
        // The name in the refusal is the file's name only, never its folder.
        s.allow("/home/me/Shop Projects/Bench v2");
        s.file("/home/me/Shop Projects/Bench v2.shz");
        assert_eq!(
            s.verdict(
                "/home/me/Shop Projects/Bench v2.shz",
                Some("/home/me/Shop Projects/Bench v2"),
                false
            ),
            exists("Bench v2.shz")
        );
        // A file named as typed (GTK asked about that one) is left alone and changes nothing.
        let mut t = Shell::default();
        t.allow("/h/cabinet");
        t.file("/h/cabinet");
        assert_eq!(
            t.save("/h/cabinet.shz", Some("/h/cabinet"), false),
            Verdict::Write
        );
        assert!(t.files.contains(Path::new("/h/cabinet")));
    }

    #[test]
    fn windows_style_paths_as_the_function_sees_them() {
        // On Windows the dialog adds the extension, so these are the page's own fallback; on
        // Linux and macOS a backslash is a character of the name. Either way the final path
        // must be the chosen one plus one extension.
        let mut s = Shell::default();
        s.allow("C:\\Models\\cabinet");
        assert_eq!(
            s.verdict(
                "C:\\Models\\cabinet.shz",
                Some("C:\\Models\\cabinet"),
                false
            ),
            Verdict::Write
        );
        assert_eq!(
            s.verdict(
                "C:\\Models\\cabinet.SHZ",
                Some("C:\\Models\\cabinet"),
                false
            ),
            Verdict::Write
        );
        assert_eq!(
            s.verdict("C:\\Models\\other.shz", Some("C:\\Models\\cabinet"), false),
            Verdict::NotChosen
        );
        assert_eq!(
            s.verdict("C:\\Models\\cabinet.shz", Some("C:\\Models\\Other"), false),
            Verdict::NotChosen
        );
    }

    #[test]
    fn on_the_real_disk_with_the_shells_own_questions() {
        // The shell passes Path::exists and Path::is_dir; here they answer for a real folder.
        let d =
            std::env::temp_dir().join(format!("sawhorse-safe-save-decide-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("plans")).unwrap();
        let scope: BTreeSet<PathBuf> = [d.join("cabinet"), d.join("plans"), d.join("shelf")]
            .into_iter()
            .collect();
        let real = |path: &Path, chosen: &Path, replace: bool| {
            decide(
                path,
                Some(chosen),
                replace,
                |p| scope.contains(p),
                |p| p.exists(),
                |p| p.is_dir(),
            )
        };
        let cabinet = d.join("cabinet.shz");
        assert_eq!(real(&cabinet, &d.join("cabinet"), false), Verdict::Write);
        write_atomic(&cabinet, b"first model").unwrap();
        assert_eq!(
            real(&cabinet, &d.join("cabinet"), false),
            exists("cabinet.shz")
        );
        assert_eq!(real(&cabinet, &d.join("cabinet"), true), Verdict::Write);
        write_atomic(&cabinet, b"second model").unwrap();
        assert_eq!(fs::read(&cabinet).unwrap(), b"second model");
        assert_eq!(fs::read(d.join("cabinet.shz.bak")).unwrap(), b"first model");
        // The picked folder makes nothing beside it writable.
        assert_eq!(
            real(&d.join("plans.shz"), &d.join("plans"), true),
            Verdict::NotChosen
        );
        // A folder already holding the final name: asked about, and Yes then fails to write
        // and leaves the folder and what is in it as they were.
        fs::create_dir_all(d.join("shelf.shz")).unwrap();
        fs::write(d.join("shelf.shz").join("keep.txt"), b"kept").unwrap();
        assert_eq!(
            real(&d.join("shelf.shz"), &d.join("shelf"), false),
            exists("shelf.shz")
        );
        assert!(write_atomic(&d.join("shelf.shz"), b"model").is_err());
        assert_eq!(
            fs::read(d.join("shelf.shz").join("keep.txt")).unwrap(),
            b"kept"
        );
        assert!(!d.join(".shelf.shz.saving").exists());
        let _ = fs::remove_dir_all(&d);
    }
}
