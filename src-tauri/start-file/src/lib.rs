//! Which of the arguments the desktop app was started with, if any, is a file to open (lane 37,
//! #45).
//!
//! Before lane 37 a double-click on a `.shz` in Windows Explorer could not open Sawhorse: the
//! installers declared no file association and the shell never looked at its arguments. Now
//! the Windows installers associate `.shz` with Sawhorse, Windows starts it with the file's
//! path as an argument (`Sawhorse.exe "C:\...\shelf.shz"`), and so does a path typed after the
//! program's name on any system. The shell (`src-tauri/src/lib.rs`) reads its arguments once
//! at start and asks [`pick`] which one, if any, names the file to open.
//!
//! The rules, the same on every system:
//!
//! - An argument that starts with `-` is an option (macOS has passed `-psn_...` to programs
//!   started from the Finder), never a file, and is skipped. A file whose name starts with `-`
//!   is given as `./-name.shz`.
//! - An empty argument, and one that is not valid Unicode, is skipped: the page names the
//!   file by its path as text, so a path it cannot write down is never handed to it.
//! - A path is taken only when its extension is one File > Open takes ([`EXTENSIONS`], in any
//!   case: `SHELF.SHZ` too). Anything else is skipped.
//! - A relative path is joined to the folder the app was started in; one that cannot be made
//!   absolute (no such folder, or a Windows drive-relative `C:shelf.shz`) is skipped. Nothing
//!   else is done to the path: it is not canonicalized, and no folder or pattern is made from
//!   it.
//! - The caller says whether a path is an existing regular file (`is_file`); the first path
//!   that is one is the file to open, and every later one is not opened. One start opens at
//!   most one file: Windows starts the program once for each file selected in Explorer, so
//!   each of those files opens in its own window.
//!
//! What the shell does with the answer (adding that one path to the fs scope with
//! `allow_file`, and handing it to the page once) is in `src-tauri/src/lib.rs`. The path is
//! only ever read through the fs plugin: it is never run, never passed to a shell and never
//! used to build a command.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The extensions File > Open takes, lower case, without the dot: a `.shz`, and every import
/// type. The page's own list is `ALL_EXTENSIONS` in `src/io/files.ts`; `src/io/startup.test.ts`
/// fails when the two differ.
pub const EXTENSIONS: &[&str] = &[
    "shz", "skp", "stl", "3mf", "glb", "gltf", "obj", "dxf", "svg",
];

/// Whether File > Open takes a file of this name (its extension, in any case).
pub fn opens(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// The arguments (those after the program's own name) that name a file File > Open takes, in
/// their order, each made absolute against `cwd`. Options, empty arguments, arguments that are
/// not valid Unicode, other extensions and paths that cannot be made absolute are left out.
pub fn candidates<I, S>(args: I, cwd: Option<&Path>) -> Vec<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    args.into_iter()
        .filter_map(|a| {
            let a: OsString = a.into();
            let text = a.to_str()?;
            if text.is_empty() || text.starts_with('-') {
                return None;
            }
            let path = Path::new(text);
            if !opens(path) {
                return None;
            }
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                let cwd = cwd.filter(|c| c.is_absolute())?;
                cwd.join(path)
            };
            path.is_absolute().then_some(path)
        })
        .collect()
}

/// The one file to open at start: the first of [`candidates`] that `is_file` says is an
/// existing regular file (the shell passes `Path::is_file`, which follows a link to what it
/// names). `None` when there is none: the app starts on an empty model, as before.
pub fn pick<I, S>(args: I, cwd: Option<&Path>, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    candidates(args, cwd).into_iter().find(|p| is_file(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute folder on the system the tests run on.
    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\work")
        } else {
            PathBuf::from("/work")
        }
    }

    fn at(rel: &str) -> String {
        root().join(rel).to_str().unwrap().to_string()
    }

    /// Every path is an existing file.
    fn any(_: &Path) -> bool {
        true
    }

    fn pick_all(args: &[&str]) -> Option<PathBuf> {
        pick(args.iter().map(|a| a.to_string()), Some(&root()), any)
    }

    #[test]
    fn no_arguments_open_nothing() {
        assert_eq!(pick(Vec::<String>::new(), Some(&root()), any), None);
        assert!(candidates(Vec::<String>::new(), None).is_empty());
    }

    #[test]
    fn a_shz_path_is_the_file_exactly_as_given() {
        let shelf = at("shelf.shz");
        assert_eq!(pick_all(&[&shelf]), Some(PathBuf::from(&shelf)));
    }

    #[test]
    fn options_are_skipped_never_taken_as_files() {
        let shelf = at("shelf.shz");
        for opt in [
            "-psn_0_12345",
            "--open=shelf.shz",
            "-shelf.shz",
            "--",
            "-",
            "--flag",
        ] {
            assert_eq!(pick_all(&[opt]), None, "{opt}");
            assert_eq!(
                pick_all(&[opt, &shelf]),
                Some(PathBuf::from(&shelf)),
                "{opt}"
            );
        }
        // A file whose name starts with "-" is given with its folder.
        assert_eq!(pick_all(&["./-odd.shz"]), Some(root().join("./-odd.shz")));
    }

    #[test]
    fn the_extension_is_matched_in_any_case() {
        for name in ["SHELF.SHZ", "Shelf.Shz", "chair.SKP", "part.Stl", "box.3MF"] {
            let p = at(name);
            assert_eq!(pick_all(&[&p]), Some(PathBuf::from(&p)), "{name}");
        }
    }

    #[test]
    fn a_path_with_spaces_is_one_argument() {
        let p = at("My Projects/a shelf for the hall.shz");
        assert_eq!(pick_all(&[&p]), Some(PathBuf::from(&p)));
    }

    #[test]
    fn a_non_ascii_path_is_kept_as_it_is() {
        for name in ["Möbel/Regal für Jörg.shz", "家具/本棚.shz", "étagère.SHZ"] {
            let p = at(name);
            assert_eq!(pick_all(&[&p]), Some(PathBuf::from(&p)), "{name}");
        }
    }

    #[test]
    fn a_relative_path_is_joined_to_the_starting_folder() {
        assert_eq!(pick_all(&["shelf.shz"]), Some(root().join("shelf.shz")));
        assert_eq!(
            pick_all(&["plans/shelf.shz"]),
            Some(root().join("plans/shelf.shz"))
        );
        // Not canonicalized: the path is what was typed, joined to the folder.
        assert_eq!(
            pick_all(&["../shelf.shz"]),
            Some(root().join("../shelf.shz"))
        );
        // No starting folder (or a relative one): a relative path cannot be made absolute.
        assert_eq!(pick(["shelf.shz"], None, any), None);
        assert_eq!(pick(["shelf.shz"], Some(Path::new("work")), any), None);
        // An absolute path does not need the folder.
        let shelf = at("shelf.shz");
        assert_eq!(
            pick([shelf.clone()], None, any),
            Some(PathBuf::from(&shelf))
        );
    }

    #[test]
    fn extensions_file_open_does_not_take_are_skipped() {
        for name in [
            "notes.txt",
            "plugin.zip",
            "shelf.shz.bak",
            "shelf",
            "shelf.",
            "setup.exe",
            "run.bat",
            "shz",
            ".shz",
        ] {
            assert_eq!(pick_all(&[&at(name)]), None, "{name}");
            assert_eq!(pick_all(&[name]), None, "{name}");
        }
    }

    #[test]
    fn the_pieces_of_an_unquoted_program_path_are_skipped() {
        // The setup .exe registers `<install folder>\<program>.exe "%1"` with the program's
        // own path unquoted, so when the install folder has a space in it the program's name
        // reaches Sawhorse split at the space: its first piece is the program's name (never
        // looked at) and the rest arrives as an argument before the file. That piece names an
        // .exe, so it is skipped, and the file after it is the one opened.
        let shelf = at("shelf.shz");
        for piece in [
            r"Smith\AppData\Local\Sawhorse\sawhorse.exe",
            r"Files\Sawhorse\sawhorse.exe",
            "Files/Sawhorse/sawhorse",
        ] {
            assert_eq!(pick_all(&[piece]), None, "{piece}");
            assert_eq!(
                pick_all(&[piece, &shelf]),
                Some(PathBuf::from(&shelf)),
                "{piece}"
            );
        }
    }

    #[test]
    fn an_empty_argument_is_skipped() {
        assert_eq!(pick_all(&[""]), None);
        let shelf = at("shelf.shz");
        assert_eq!(pick_all(&["", &shelf]), Some(PathBuf::from(&shelf)));
    }

    #[test]
    fn of_several_files_only_the_first_is_opened() {
        let (a, b, c) = (at("a.shz"), at("b.stl"), at("c.shz"));
        assert_eq!(pick_all(&[&a, &b, &c]), Some(PathBuf::from(&a)));
        assert_eq!(
            candidates([a.clone(), b.clone(), c.clone()], Some(&root())),
            vec![PathBuf::from(&a), PathBuf::from(&b), PathBuf::from(&c)]
        );
    }

    #[test]
    fn only_an_existing_regular_file_is_opened() {
        let (gone, dir, real) = (at("gone.shz"), at("folder.shz"), at("real.shz"));
        let is_file = |p: &Path| p == Path::new(&real);
        // Missing, or a folder with a .shz name: not a file, so the next one is looked at.
        assert_eq!(pick([gone.clone()], None, is_file), None);
        assert_eq!(pick([dir.clone()], None, is_file), None);
        assert_eq!(
            pick([gone, dir, real.clone()], None, is_file),
            Some(PathBuf::from(&real))
        );
    }

    #[test]
    fn the_answer_is_exactly_one_path_and_never_a_pattern_or_a_folder() {
        // A name with glob characters is still one file's name (the shell's allow_file
        // escapes it), and nothing above it is ever returned.
        let odd = at("[draft] *?.shz");
        let got = pick_all(&[&odd]).unwrap();
        assert_eq!(got, PathBuf::from(&odd));
        assert!(opens(&got));
        assert_ne!(got, root());
    }

    #[test]
    fn the_list_is_lower_case_without_dots_and_has_the_document_first() {
        assert_eq!(EXTENSIONS[0], "shz");
        for e in EXTENSIONS {
            assert!(!e.is_empty() && !e.starts_with('.'), "{e}");
            assert_eq!(e.to_ascii_lowercase(), *e, "{e}");
        }
        assert!(opens(Path::new("x.gltf")) && opens(Path::new("x.glb")));
        assert!(!opens(Path::new("x.zip")));
    }

    #[cfg(unix)]
    #[test]
    fn an_argument_that_is_not_unicode_is_skipped() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(b"/work/\xffshelf.shz".to_vec());
        assert_eq!(pick([bad.clone()], None, any), None);
        let shelf = at("shelf.shz");
        assert_eq!(
            pick([bad, OsString::from(&shelf)], None, any),
            Some(PathBuf::from(&shelf))
        );
    }
}
