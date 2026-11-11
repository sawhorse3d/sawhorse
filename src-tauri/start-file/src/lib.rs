//! Which of the arguments the desktop app was started with, if any, is a file to open (lane 37,
//! #45; lane 39, #52).
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
//!   case: `SHELF.SHZ` too), both as it was given and once it is cleaned (below). Anything
//!   else is skipped, and so is a path that ends in a separator or in a `.` (`box.shz\`,
//!   `box.shz\.`): the system reads it as a folder, and cleaning would make it the file.
//! - A relative path is joined to the folder the app was started in; one that cannot be made
//!   absolute (no such folder, or a Windows drive-relative `C:shelf.shz`) is skipped.
//! - The absolute path is then cleaned on its text alone ([`clean`]): a `.` is dropped and a
//!   `..` takes away the name before it, so `..\box.shz` given in `C:\models\two words` is
//!   `C:\models\box.shz`. Before lane 39 the `..` stayed in the path, and the fs plugin the
//!   page reads the file through refuses any path that holds a `..` (tauri's `SafePathBuf`),
//!   so the app started on an empty model with the plugin's raw error in the status line
//!   (#52). A `..` that would climb above the root, or above a Windows drive or share, makes
//!   the argument no file at all, and so does a `.` or a `..` after a Windows `\\?\` prefix,
//!   where Windows itself gives them no meaning. Cleaning reads nothing from the disk, and the
//!   path is never canonicalized (that resolves links, and on Windows it gives a `\\?\` path
//!   that other code mishandles). So when a folder named before a `..` is a link, the cleaned
//!   path names a file beside the link, not the one the system would reach through it: given
//!   `/home/maker/plans/../shelf.shz` with `plans` a link to `/mnt/share/plans`, the file
//!   opened is `/home/maker/shelf.shz`.
//! - The caller says whether a cleaned path is an existing regular file (`is_file`); the
//!   first cleaned path that is one is the file to open, and every later one is not opened.
//!   One start opens at most one file: Windows starts the program once for each file selected
//!   in Explorer, so each of those files opens in its own window. Nothing else is made from
//!   the path: no folder, no pattern.
//!
//! What the shell does with the answer (adding that one cleaned path to the fs scope with
//! `allow_file`, and handing it to the page once) is in `src-tauri/src/lib.rs`. The path is
//! only ever read through the fs plugin: it is never run, never passed to a shell and never
//! used to build a command.
//!
//! The tests run on Linux (in CI and in WSL), where paths are Unix paths. The Windows forms
//! (a drive, a share, `\\?\`, backslashes) go through the same code, written over
//! [`Component`] so that each form is one of its cases, but the tests for them
//! (`#[cfg(windows)]`, below) are run by no job yet.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

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

/// `path` with every `.` dropped and every `..` taking away the name before it, worked out
/// from the path's text alone: nothing is read from the disk and no link is followed. `None`
/// when a `..` has no name before it to take away (it would climb above the root, above a
/// Windows drive or share, or out of the start of a relative path), and when the path has a
/// Windows `\\?\` prefix and holds a `.` or a `..`, which Windows does not resolve there. The
/// answer holds no `.` and no `..`.
pub fn clean(path: &Path) -> Option<PathBuf> {
    let mut kept: Vec<Component<'_>> = Vec::new();
    // After a `\\?\` prefix Windows takes every name literally, `.` and `..` too.
    let mut literal = false;
    for part in path.components() {
        match part {
            Component::Prefix(prefix) => {
                literal = prefix.kind().is_verbatim();
                kept.push(part);
            }
            Component::RootDir | Component::Normal(_) => kept.push(part),
            Component::CurDir | Component::ParentDir if literal => return None,
            Component::CurDir => {}
            Component::ParentDir => match kept.last() {
                Some(Component::Normal(_)) => {
                    kept.pop();
                }
                // The root, a drive or a share, or nothing at all: there is no name to take.
                _ => return None,
            },
        }
    }
    Some(kept.into_iter().collect())
}

/// The arguments (those after the program's own name) that name a file File > Open takes, in
/// their order, each made absolute against `cwd` and then cleaned ([`clean`]). Options, empty
/// arguments, arguments that are not valid Unicode, other extensions, paths that cannot be
/// made absolute and paths whose `..` climbs too far are left out.
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
            // A path that ends in a separator or in a `.` names a folder as the system reads
            // it (`box.shz\`, `box.shz\.`); cleaning would drop that ending and name the file,
            // so such a path is no candidate.
            if text.ends_with(std::path::is_separator)
                || text.rsplit(std::path::is_separator).next() == Some(".")
            {
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
            if !path.is_absolute() {
                return None;
            }
            let path = clean(&path)?;
            (path.is_absolute() && opens(&path)).then_some(path)
        })
        .collect()
}

/// The one file to open at start: the first of [`candidates`] (cleaned paths) that `is_file`
/// says is an existing regular file (the shell passes `Path::is_file`, which follows a link to
/// what it names). `None` when there is none: the app starts on an empty model, as before.
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
    use std::cell::RefCell;

    /// An absolute folder on the system the tests run on.
    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\work")
        } else {
            PathBuf::from("/work")
        }
    }

    /// `rel`, its pieces split at `/`, under the root, written with the system's separator.
    fn at(rel: &str) -> String {
        let p = rel.split('/').fold(root(), |p, piece| p.join(piece));
        p.to_str().unwrap().to_string()
    }

    /// Every path is an existing file.
    fn any(_: &Path) -> bool {
        true
    }

    /// What the fs plugin would refuse in a path the page names (tauri-plugin-fs 2.7.0): its
    /// `SafeFilePath` takes the page's text as a URL when it parses as one with a scheme
    /// longer than one letter (a drive letter is not taken for one), and otherwise as a
    /// `SafePathBuf` (tauri 2.12.3), which refuses any `..` component. Also checked here: no
    /// `.` or `..` anywhere in the text, split as the system splits a path.
    fn refused_by_the_fs_plugin(path: &Path) -> Option<String> {
        let text = path.to_str().expect("a candidate is valid Unicode");
        if path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Some(format!("{text}: a . or .. component"));
        }
        if text
            .split(std::path::is_separator)
            .any(|s| s == "." || s == "..")
        {
            return Some(format!("{text}: a . or .. in its text"));
        }
        // A URL scheme: a letter, then letters, digits, +, - or ., then a colon (the url crate
        // skips leading spaces and control characters first).
        let t = text.trim_start_matches(|c: char| c <= ' ');
        let scheme: String = t
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || "+-.".contains(*c))
            .collect();
        if scheme.len() > 1
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && t[scheme.len()..].starts_with(':')
        {
            return Some(format!("{text}: read as a URL"));
        }
        None
    }

    /// The answer, after checking that the fs plugin would take it as a path.
    fn taken(got: Option<PathBuf>) -> Option<PathBuf> {
        if let Some(p) = &got {
            assert!(p.is_absolute(), "{}", p.display());
            assert!(opens(p), "{}", p.display());
            if let Some(why) = refused_by_the_fs_plugin(p) {
                panic!("the fs plugin would refuse {why}");
            }
        }
        got
    }

    fn pick_all(args: &[&str]) -> Option<PathBuf> {
        taken(pick(args.iter().map(|a| a.to_string()), Some(&root()), any))
    }

    /// From a folder inside the root, as in #52 ("two words", a name with a space).
    fn pick_in(cwd: &str, args: &[&str]) -> Option<PathBuf> {
        let cwd = root().join(cwd);
        taken(pick(args.iter().map(|a| a.to_string()), Some(&cwd), any))
    }

    #[test]
    fn no_arguments_open_nothing() {
        assert_eq!(pick(Vec::<String>::new(), Some(&root()), any), None);
        assert!(candidates(Vec::<String>::new(), None).is_empty());
    }

    #[test]
    fn a_shz_path_is_the_file_exactly_as_given() {
        let shelf = at("shelf.shz");
        let got = pick_all(&[&shelf]).unwrap();
        assert_eq!(got, PathBuf::from(&shelf));
        assert_eq!(got.to_str(), Some(shelf.as_str()));
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
        // A file whose name starts with "-" is given with its folder; the "." is dropped.
        let odd = pick_all(&["./-odd.shz"]).unwrap();
        assert_eq!(odd.to_str(), Some(at("-odd.shz").as_str()));
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
        // No starting folder (or a relative one): a relative path cannot be made absolute.
        assert_eq!(pick(["shelf.shz"], None, any), None);
        assert_eq!(pick(["shelf.shz"], Some(Path::new("work")), any), None);
        // An absolute path does not need the folder.
        let shelf = at("shelf.shz");
        assert_eq!(
            taken(pick([shelf.clone()], None, any)),
            Some(PathBuf::from(&shelf))
        );
    }

    #[test]
    fn a_dotdot_in_a_relative_path_takes_away_the_folder_before_it() {
        // #52: `sawhorse-desktop.exe ..\box.shz` from a folder beside the model.
        let got = pick_in("two words", &["../box.shz"]).unwrap();
        assert_eq!(got.to_str(), Some(at("box.shz").as_str()));
        // From the root folder itself, `..` takes away the root's own name.
        let up = pick_all(&["../shelf.shz"]).unwrap();
        assert_eq!(
            Some(up.as_path()),
            root().parent().map(|p| p.join("shelf.shz")).as_deref()
        );
        // In the middle of a relative path.
        let got = pick_in("two words", &["plans/../box.shz"]).unwrap();
        assert_eq!(got.to_str(), Some(at("two words/box.shz").as_str()));
    }

    #[test]
    fn a_dotdot_in_an_absolute_path_is_cleaned_the_same_way() {
        let got = pick_all(&[&at("plans/../shelf.shz")]).unwrap();
        assert_eq!(got.to_str(), Some(at("shelf.shz").as_str()));
        let got = pick_all(&[&at("a/b/c/../../d/shelf.shz")]).unwrap();
        assert_eq!(got.to_str(), Some(at("a/d/shelf.shz").as_str()));
        // Without a starting folder too: an absolute path never needs one.
        let got = taken(pick([at("plans/../shelf.shz")], None, any)).unwrap();
        assert_eq!(got.to_str(), Some(at("shelf.shz").as_str()));
    }

    #[test]
    fn several_dotdots_in_a_row() {
        let got = pick_in("a/b/c", &["../../../shelf.shz"]).unwrap();
        assert_eq!(got.to_str(), Some(at("shelf.shz").as_str()));
        let got = pick_in("a/b", &["../../x/../y/../../work/shelf.shz"]);
        assert_eq!(got.unwrap().to_str(), Some(at("shelf.shz").as_str()));
        let got = pick_all(&[&at("a/b/../../c/shelf.shz")]).unwrap();
        assert_eq!(got.to_str(), Some(at("c/shelf.shz").as_str()));
    }

    #[test]
    fn a_dotdot_that_climbs_above_the_root_is_no_file() {
        let top = if cfg!(windows) { r"C:\" } else { "/" };
        // At the very start of an absolute path.
        for arg in [
            format!("{top}../shelf.shz"),
            format!("{top}../../shelf.shz"),
            format!("{top}../work/shelf.shz"),
        ] {
            assert_eq!(pick_all(&[&arg]), None, "{arg}");
            assert_eq!(pick([arg.clone()], None, any), None, "{arg}");
            assert!(candidates([arg.clone()], None).is_empty(), "{arg}");
        }
        // From a folder: one `..` too many.
        assert_eq!(pick_all(&["../../shelf.shz"]), None);
        assert_eq!(pick_in("a", &["../../../shelf.shz"]), None);
        assert_eq!(pick_all(&[&at("../../shelf.shz")]), None);
        // The next argument is still looked at.
        let shelf = at("shelf.shz");
        assert_eq!(
            pick_all(&["../../../../nowhere.shz", &shelf]),
            Some(PathBuf::from(&shelf))
        );
    }

    #[test]
    fn dot_components_are_dropped() {
        for (arg, want) in [
            ("./shelf.shz", "shelf.shz"),
            ("plans/./shelf.shz", "plans/shelf.shz"),
            ("././plans/././shelf.shz", "plans/shelf.shz"),
            ("./plans/../shelf.shz", "shelf.shz"),
        ] {
            let got = pick_all(&[arg]).unwrap();
            assert_eq!(got.to_str(), Some(at(want).as_str()), "{arg}");
        }
        let got = pick_all(&[&at("./plans/./shelf.shz")]).unwrap();
        assert_eq!(got.to_str(), Some(at("plans/shelf.shz").as_str()));
    }

    #[test]
    fn a_dotdot_after_the_file_name_is_no_file() {
        // `a.shz/..` names the folder the file is in, which is not a file File > Open takes.
        for arg in [
            "a.shz/..".to_string(),
            at("a.shz/.."),
            at("plans/a.shz/.."),
            // A name after the file, then `..`: as given it has no extension, so it is
            // skipped, rather than taken as `a.shz` (the system would refuse to look inside a
            // file).
            "a.shz/x/..".to_string(),
            at("a.shz/x.shz/.."),
        ] {
            assert_eq!(pick_all(&[&arg]), None, "{arg}");
            assert!(candidates([arg.clone()], Some(&root())).is_empty(), "{arg}");
        }
        // A path ending in a separator or a `.` names a folder as the system reads it: the
        // cleaning would drop the ending and make it the file, so it is skipped too.
        for arg in [
            "a.shz/".to_string(),
            "a.shz/.".to_string(),
            "a.shz//".to_string(),
            "./a.shz/./".to_string(),
            at("a.shz/"),
            at("plans/a.shz/."),
        ] {
            assert_eq!(pick_all(&[&arg]), None, "{arg}");
            assert!(candidates([arg.clone()], Some(&root())).is_empty(), "{arg}");
        }
        // The same file named plainly still opens.
        let got = pick_all(&["a.shz"]).unwrap();
        assert_eq!(got.to_str(), Some(at("a.shz").as_str()));
    }

    #[test]
    fn the_existing_file_check_sees_only_the_cleaned_path() {
        let seen = RefCell::new(Vec::<PathBuf>::new());
        let box_shz = PathBuf::from(at("box.shz"));
        let is_file = |p: &Path| {
            seen.borrow_mut().push(p.to_path_buf());
            p == box_shz
        };
        let cwd = root().join("two words");
        let got = pick(
            ["../gone.shz", "./../box.shz", "../later.shz"],
            Some(&cwd),
            is_file,
        );
        assert_eq!(taken(got), Some(box_shz.clone()));
        // Asked about the first two (cleaned), never about the third.
        let seen = seen.into_inner();
        assert_eq!(seen, vec![PathBuf::from(at("gone.shz")), box_shz]);
        for p in &seen {
            assert_eq!(refused_by_the_fs_plugin(p), None);
        }
    }

    #[test]
    fn cleaning_reads_nothing_and_needs_nothing_to_exist() {
        let p = root().join("no/such/folder/../../file.shz");
        assert_eq!(clean(&p), Some(root().join("no/file.shz")));
        assert_eq!(
            clean(&root().join("a/./b/../c")).unwrap().to_str(),
            Some(at("a/c").as_str())
        );
        // A relative path is cleaned too, but a `..` has nothing before it to take.
        assert_eq!(clean(Path::new("a/../b.shz")), Some(PathBuf::from("b.shz")));
        assert_eq!(clean(Path::new("../b.shz")), None);
        assert_eq!(clean(Path::new("a/../../b.shz")), None);
        // The root on its own stays the root.
        let top = if cfg!(windows) { r"C:\" } else { "/" };
        assert_eq!(clean(Path::new(top)), Some(PathBuf::from(top)));
        assert_eq!(clean(&root().join("..")), Some(PathBuf::from(top)));
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
        // Cleaning never turns a name File > Open does not take into one it does.
        assert_eq!(pick_all(&["notes.txt/../shelf"]), None);
        assert_eq!(pick_all(&["shelf.shz/../notes.txt"]), None);
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
            taken(pick([gone, dir, real.clone()], None, is_file)),
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
        // A `..` never makes the answer a folder.
        assert_eq!(pick_all(&[&at("plans/..")]), None);
        assert_eq!(pick_all(&[".."]), None);
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

    #[test]
    fn the_fs_plugin_check_here_refuses_what_the_plugin_refuses() {
        // The check every answer above goes through, against what the plugin refuses.
        assert!(refused_by_the_fs_plugin(&root().join("../x.shz")).is_some());
        assert!(refused_by_the_fs_plugin(Path::new("https://example.com/x.shz")).is_some());
        assert!(refused_by_the_fs_plugin(Path::new("file:/x.shz")).is_some());
        assert_eq!(refused_by_the_fs_plugin(&root().join("x.shz")), None);
        assert_eq!(refused_by_the_fs_plugin(Path::new("C:/x.shz")), None);
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

    #[cfg(unix)]
    #[test]
    fn a_backslash_is_part_of_a_name_on_unix() {
        // On Linux and macOS `..\box.shz` is one name with backslashes in it, not a `..`.
        let got = pick_all(&[r"..\box.shz"]).unwrap();
        assert_eq!(got.to_str(), Some(at(r"..\box.shz").as_str()));
    }

    // The Windows forms. These are run by no job yet: CI and WSL test on Linux.

    #[cfg(windows)]
    fn win(cwd: &str, arg: &str) -> Option<String> {
        taken(pick([arg], Some(Path::new(cwd)), any)).map(|p| p.to_str().unwrap().to_string())
    }

    #[cfg(windows)]
    #[test]
    fn windows_a_drive() {
        let cwd = r"C:\models\two words";
        assert_eq!(
            win(cwd, r"..\box.shz").as_deref(),
            Some(r"C:\models\box.shz")
        );
        assert_eq!(win(cwd, r"..\..\box.shz").as_deref(), Some(r"C:\box.shz"));
        assert_eq!(win(cwd, r"..\..\..\box.shz"), None);
        assert_eq!(win(cwd, r"C:\..\box.shz"), None);
        assert_eq!(
            win(cwd, r"C:\a\.\b\..\box.shz").as_deref(),
            Some(r"C:\a\box.shz")
        );
        assert_eq!(win(cwd, "C:/a/../box.shz").as_deref(), Some(r"C:\box.shz"));
        assert_eq!(
            win(cwd, r".\box.shz").as_deref(),
            Some(r"C:\models\two words\box.shz")
        );
        // Drive-relative: cannot be made absolute.
        assert_eq!(win(cwd, r"C:box.shz"), None);
        assert_eq!(win(cwd, r"C:..\box.shz"), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_a_share() {
        let cwd = r"\\server\share\models\two words";
        assert_eq!(
            win(cwd, r"..\box.shz").as_deref(),
            Some(r"\\server\share\models\box.shz")
        );
        assert_eq!(win(cwd, r"..\..\..\box.shz"), None);
        assert_eq!(win(cwd, r"\\server\share\..\box.shz"), None);
        assert_eq!(
            win(cwd, r"\\server\share\a\..\box.shz").as_deref(),
            Some(r"\\server\share\box.shz")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_a_verbatim_prefix() {
        let cwd = r"C:\models";
        assert_eq!(
            win(cwd, r"\\?\C:\models\box.shz").as_deref(),
            Some(r"\\?\C:\models\box.shz")
        );
        assert_eq!(win(cwd, r"\\?\C:\models\..\box.shz"), None);
        assert_eq!(win(cwd, r"\\?\C:\models\.\box.shz"), None);
        // A relative path joined to a `\\?\` folder: Rust's own join already resolves its
        // `.` and `..` (PathBuf::push does so for a verbatim path), so nothing is left to
        // refuse.
        assert_eq!(
            win(r"\\?\C:\models", r"..\box.shz").as_deref(),
            Some(r"\\?\C:\box.shz")
        );
    }
}
