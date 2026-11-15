//! The server's folder rule and its save on Windows (issue #14): drives, UNC and device
//! paths, junctions and links, streams and device names, case, long paths, a start folder
//! given in other forms, and files held open. They run on Windows only (CI's Windows job).
//! Where what Windows does is not certain, a test asserts what must hold whatever it does
//! (nothing read or written outside the start folder, the file at the target whole, no
//! temporary file left, the server answering) and prints what happened, so the log shows it.
#![cfg(windows)]

mod common;

use std::collections::BTreeMap;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{
    Session, answers, board, folder, greeting, hash_of, names, run_binary, temporary_files,
    tool_call,
};
use serde_json::json;

/// A test's own folder, and the links it made there. On drop every link is removed as a
/// link (`remove_dir` for a junction or a folder link, `remove_file` for a file link), and
/// only then the folder, so nothing is ever deleted through a link.
struct Scratch {
    base: PathBuf,
    links: Vec<(PathBuf, bool)>,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        Scratch {
            base: folder(name),
            links: Vec::new(),
        }
    }

    /// A junction at `link` to the folder `target` (`mklink /J`, which needs no privilege).
    fn junction(&mut self, link: &Path, target: &Path) {
        let out = Command::new("cmd")
            .raw_arg(format!(
                "/C mklink /J \"{}\" \"{}\"",
                link.display(),
                target.display()
            ))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "mklink /J {} {}: {}{}",
            link.display(),
            target.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        self.links.push((link.to_path_buf(), true));
    }

    /// A folder link, or false (printed) where the system refuses to make one.
    fn folder_link(&mut self, link: &Path, target: &Path) -> bool {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => {
                self.links.push((link.to_path_buf(), true));
                true
            }
            Err(e) => {
                eprintln!(
                    "no folder link could be made here ({e}); the cases through one are left out"
                );
                false
            }
        }
    }

    /// A file link, or false (printed) where the system refuses to make one.
    fn file_link(&mut self, link: &Path, target: &Path) -> bool {
        match std::os::windows::fs::symlink_file(target, link) {
            Ok(()) => {
                self.links.push((link.to_path_buf(), false));
                true
            }
            Err(e) => {
                eprintln!(
                    "no file link could be made here ({e}); the cases through one are left out"
                );
                false
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for (link, is_folder) in self.links.iter().rev() {
            let gone = if *is_folder {
                std::fs::remove_dir(link)
            } else {
                std::fs::remove_file(link)
            };
            if let Err(e) = gone {
                eprintln!("could not remove the link {}: {e}", link.display());
            }
        }
        if self
            .links
            .iter()
            .all(|(l, _)| std::fs::symlink_metadata(l).is_err())
        {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }
}

/// Everything under `dir`: each name with its bytes (a folder, a link or a junction as
/// `None`; a link or junction is listed, never entered).
fn snapshot(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut out = BTreeMap::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        for entry in std::fs::read_dir(dir.join(&rel)).unwrap() {
            let entry = entry.unwrap();
            let child = rel.join(entry.file_name());
            let kind = entry.file_type().unwrap();
            let key = child.to_string_lossy().into_owned();
            if kind.is_symlink() {
                out.insert(key, None);
            } else if kind.is_dir() {
                out.insert(key, None);
                todo.push(child);
            } else {
                out.insert(key, Some(std::fs::read(dir.join(&child)).unwrap()));
            }
        }
    }
    out
}

/// `path` as text without the `\\?\` a resolved path starts with.
fn plain(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => text,
    }
}

/// The usual layout: `base/root` (the start folder, with `sub/` and `keep.shz`) and
/// `base/away` beside it, holding `far.shz`, a copy of a model, which nothing may touch.
fn layout(name: &str) -> (Scratch, PathBuf, PathBuf, Session) {
    let scratch = Scratch::new(name);
    let root = scratch.base.join("root");
    let away = scratch.base.join("away");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::create_dir_all(&away).unwrap();
    let mut s = Session::new(&root);
    s.ok("add_board", board("Shelf", 0.0));
    s.ok("save_model", json!({"path": "keep.shz"}));
    std::fs::copy(root.join("keep.shz"), away.join("far.shz")).unwrap();
    (scratch, root, away, s)
}

/// Drive, drive-relative, rooted, UNC, verbatim and device paths are absolute paths and are
/// refused for open_model and save_model, as are ".." in any spelling; nothing outside the
/// start folder is read or written, and "\" and mixed separators that stay inside work.
#[test]
fn drive_unc_device_and_verbatim_paths_and_dot_dot_are_refused() {
    let (scratch, root, away, mut s) = layout("absolute");
    let far = std::fs::canonicalize(away.join("far.shz")).unwrap();
    let far_plain = plain(&far);
    let keep_plain = plain(&std::fs::canonicalize(root.join("keep.shz")).unwrap());
    eprintln!("outside file: {} ({far_plain})", far.display());
    let probe = "sawhorse-fence-probe.shz";
    let cwd = std::env::current_dir().unwrap();
    // Where a drive-relative or rooted path would land if it were followed; none of these
    // are there before (or the check says so) and none may be there after.
    let landing: Vec<PathBuf> = vec![
        PathBuf::from(format!("C:{probe}")),
        PathBuf::from(format!(r"C:\{probe}")),
        PathBuf::from(format!("D:/{probe}")),
        PathBuf::from(format!("/{probe}")),
        cwd.join(probe),
        scratch.base.join(probe),
    ];
    let landed = |paths: &[PathBuf]| -> Vec<bool> { paths.iter().map(|p| p.exists()).collect() };
    let landed_before = landed(&landing);
    eprintln!("probe paths there before: {landed_before:?}");
    let away_before = snapshot(&away);
    let base_before = names(&scratch.base);
    let root_before = snapshot(&root);

    let absolute = [
        format!("C:{probe}"),
        format!("c:{probe}"),
        format!("C:sub\\{probe}"),
        format!("D:/{probe}"),
        format!("D:\\{probe}"),
        format!("/{probe}"),
        format!("\\{probe}"),
        format!("\\sub\\{probe}"),
        r"\\server\share\x.shz".to_string(),
        "//server/share/x.shz".to_string(),
        r"\\?\UNC\server\share\x.shz".to_string(),
        far.to_string_lossy().into_owned(),
        format!(r"\\?\{far_plain}"),
        format!(r"\\?\{}", far_plain.replace('\\', "/")),
        format!(r"\\?\{keep_plain}"),
        far_plain.clone(),
        far_plain.replace('\\', "/"),
        keep_plain.clone(),
        format!(r"\\.\{far_plain}"),
        r"\\.\NUL".to_string(),
        "//./NUL".to_string(),
        r"\\.\C:\x.shz".to_string(),
    ];
    for path in &absolute {
        for tool in ["open_model", "save_model"] {
            let e = s.refused(tool, json!({"path": path}));
            eprintln!("{tool} {path:?}: {e}");
            assert!(e.contains("absolute"), "{tool} {path:?}: {e}");
        }
    }
    let dot_dot = [
        format!("..\\{probe}"),
        format!("..\\away\\{probe}"),
        "..\\away\\far.shz".to_string(),
        format!("sub\\..\\..\\{probe}"),
        format!("a\\..\\..\\{probe}"),
        "sub/..\\keep.shz".to_string(),
        "sub\\../keep.shz".to_string(),
        format!(".\\..\\{probe}"),
        format!("sub/./..\\..\\away/{probe}"),
    ];
    for path in &dot_dot {
        for tool in ["open_model", "save_model"] {
            let e = s.refused(tool, json!({"path": path}));
            eprintln!("{tool} {path:?}: {e}");
            assert!(e.contains("\"..\" is not allowed"), "{tool} {path:?}: {e}");
        }
    }
    // Nothing moved: outside, beside the folder, inside it, or where a drive-relative or
    // rooted path would have gone.
    assert_eq!(snapshot(&away), away_before);
    assert_eq!(names(&scratch.base), base_before);
    assert_eq!(snapshot(&root), root_before);
    assert_eq!(landed(&landing), landed_before);
    // "\" and mixed separators that stay inside are folders, and answers name them with "/".
    s.ok("new_model", json!({}));
    s.ok("add_board", board("Top", 0.0));
    let hash = s.hash();
    for (path, shown) in [
        ("sub\\in.shz", "sub/in.shz"),
        (".\\sub/./in2.shz", "sub/in2.shz"),
        ("sub\\\\in3.shz", "sub/in3.shz"),
    ] {
        let saved = s.ok("save_model", json!({"path": path}));
        assert_eq!(saved["path"], shown, "{saved}");
        let file = saved["file"].as_str().unwrap();
        assert!(!file.contains('/'), "{saved}");
        let name = shown.rsplit('/').next().unwrap();
        assert_eq!(
            hash_of(&std::fs::read(root.join("sub").join(name)).unwrap()),
            hash
        );
        assert_eq!(
            s.ok("open_model", json!({"path": path}))["path"],
            shown,
            "{path}"
        );
    }
    assert!(temporary_files(&root.join("sub")).is_empty());
    assert_eq!(snapshot(&away), away_before);
    s.answers();
}

/// Streams ("a.shz:x", "a.shz::$DATA"), device names (CON, NUL, AUX, COM1, LPT1, CONIN$,
/// as a file or a folder, in any case) and names ending in a dot are refused for open and
/// save, and no stream or file is made for them. A space at the end of the whole path is
/// trimmed, as for every path, and names the file without it.
#[test]
fn streams_device_names_and_trailing_dots_are_refused_in_any_case() {
    let (_scratch, root, away, mut s) = layout("names");
    let away_before = snapshot(&away);
    let root_before = snapshot(&root);
    let refused = [
        "a.shz:x",
        "a.shz:x.shz",
        "a.shz::$DATA",
        "keep.shz:x",
        "keep.shz:x.shz",
        "keep.shz::$DATA",
        "keep.shz:x:$DATA",
        "KEEP.SHZ:Zone.Identifier",
        "sub:x.shz",
        "CON.shz",
        "con.shz",
        "Con.Shz",
        "cOn.tar.shz",
        "NUL.shz",
        "nul.shz",
        "nUl.SHZ",
        "AUX.shz",
        "aux.shz",
        "Aux.shz",
        "PRN.shz",
        "prn.shz",
        "COM1.shz",
        "com1.shz",
        "Com9.Shz",
        "LPT1.shz",
        "lpt1.shz",
        "lPt3.shz",
        "CONIN$.shz",
        "conin$.shz",
        "ConOut$.shz",
        "CON/a.shz",
        "con/a.shz",
        "Nul/a.shz",
        "sub/aux/a.shz",
        "sub\\COM1\\a.shz",
        "a.shz.",
        "A.SHZ.",
        "a.shz..",
        "a.shz . ",
        "sub./a.shz",
        "Sub./a.shz",
        "sub /a.shz",
    ];
    for path in refused {
        for tool in ["open_model", "save_model"] {
            let e = s.refused(tool, json!({"path": path}));
            eprintln!("{tool} {path:?}: {e}");
            assert!(
                e.contains("same file on every system"),
                "{tool} {path:?}: {e}"
            );
        }
    }
    assert_eq!(snapshot(&root), root_before);
    assert_eq!(snapshot(&away), away_before);
    // No stream was made on a file there, or on one that is not.
    for stream in ["keep.shz:x", "keep.shz:x.shz", "a.shz:x"] {
        assert!(!root.join(stream).exists(), "{stream}");
    }
    // A space after the whole path is trimmed: "a.shz " is a.shz.
    let saved = s.ok("save_model", json!({"path": "a.shz "}));
    assert_eq!(saved["path"], "a.shz");
    assert_eq!(names(&root), ["a.shz", "keep.shz", "sub"]);
    // Names close to those are files of their own.
    for path in [
        "console.shz",
        "CON1.shz",
        "nullable.shz",
        "com.shz",
        "lpt10.shz",
    ] {
        s.ok("save_model", json!({"path": path}));
        s.ok("open_model", json!({"path": path}));
        assert!(root.join(path).is_file(), "{path}");
    }
    assert!(temporary_files(&root).is_empty());
    s.answers();
}

/// Junctions and folder links inside the start folder that lead out are refused, so is a
/// file link out of it, a link to a file that is not there and a junction to such a
/// junction; nothing outside is written. A junction or link whose target is inside the
/// folder is followed: paths.rs checks where a path resolves, not whether it passes a link,
/// so a link that stays inside is a way to a folder or file inside. A save over a file link
/// or a hard link replaces it and never writes through it.
#[test]
fn links_and_junctions_out_are_refused_and_those_inside_are_followed() {
    let (mut scratch, root, away, mut s) = layout("links");
    std::fs::copy(away.join("far.shz"), away.join("hard-away.shz")).unwrap();
    std::fs::hard_link(away.join("hard-away.shz"), root.join("hard.shz")).unwrap();
    s.ok("save_model", json!({"path": "first.shz"}));
    let first = std::fs::read(root.join("first.shz")).unwrap();

    scratch.junction(&root.join("jdoor"), &away);
    scratch.junction(&root.join("jchain"), &root.join("jdoor"));
    scratch.junction(&root.join("jin"), &root.join("sub"));
    let folder_links = scratch.folder_link(&root.join("ldoor"), &away)
        && scratch.folder_link(&root.join("lin"), &root.join("sub"));
    let file_links = scratch.file_link(&root.join("far.shz"), &away.join("far.shz"))
        && scratch.file_link(&root.join("gone.shz"), &away.join("missing.shz"))
        && scratch.file_link(&root.join("soft.shz"), &root.join("first.shz"));
    eprintln!("folder links made: {folder_links}; file links made: {file_links}");
    let away_before = snapshot(&away);
    let base_before = names(&scratch.base);

    let mut out = vec![
        "jdoor/far.shz",
        "jdoor/new.shz",
        "jdoor\\new.shz",
        "jchain/far.shz",
        "jchain/new.shz",
    ];
    if folder_links {
        out.extend(["ldoor/far.shz", "ldoor/new.shz"]);
    }
    if file_links {
        out.push("far.shz");
    }
    for path in out {
        for tool in ["open_model", "save_model"] {
            let e = s.refused(tool, json!({"path": path}));
            eprintln!("{tool} {path:?}: {e}");
            assert!(e.contains("leads out"), "{tool} {path:?}: {e}");
        }
    }
    if file_links {
        // A link to a file that is not there: nothing to open, and nothing is ever made
        // where it points. (paths.rs refuses a save there too, when the system reports the
        // link itself as there; a save reported done must have replaced the link.)
        let e = s.refused("open_model", json!({"path": "gone.shz"}));
        eprintln!("open_model \"gone.shz\": {e}");
        let (failed, answer) = s.call("save_model", json!({"path": "gone.shz"}));
        eprintln!(
            "save_model \"gone.shz\": {} ({answer})",
            if failed { "refused" } else { "done" }
        );
        assert!(!away.join("missing.shz").exists());
        if !failed {
            let kind = std::fs::symlink_metadata(root.join("gone.shz")).unwrap();
            assert!(!kind.file_type().is_symlink(), "{answer}");
        }
    }
    assert_eq!(snapshot(&away), away_before);
    assert_eq!(names(&scratch.base), base_before);
    assert!(!away.join("new.shz").exists());

    // Links that stay inside are followed.
    s.ok("add_board", board("Top", 400.0));
    let hash = s.hash();
    let mut inside = vec![("jin/x.shz", "x.shz")];
    if folder_links {
        inside.push(("lin/y.shz", "y.shz"));
    }
    for (path, name) in inside {
        let saved = s.ok("save_model", json!({"path": path}));
        eprintln!("save_model {path:?}: {saved}");
        assert_eq!(saved["path"], path);
        // The folder is resolved, so "file" names it where it is.
        let file = saved["file"].as_str().unwrap();
        assert!(file.ends_with(&format!(r"\root\sub\{name}")), "{saved}");
        assert_eq!(
            hash_of(&std::fs::read(root.join("sub").join(name)).unwrap()),
            hash
        );
        assert_eq!(
            s.ok("open_model", json!({"path": path}))["model_hash"],
            hash.as_str()
        );
    }
    assert!(temporary_files(&root.join("sub")).is_empty());

    // A save over a hard link to a file outside, and over a link to a file inside: the file
    // behind it keeps its bytes whatever happens; a save reported done put the new file
    // in the link's place.
    let mut over = vec!["hard.shz"];
    if file_links {
        over.push("soft.shz");
    }
    for path in over {
        let (failed, answer) = s.call("save_model", json!({"path": path}));
        let kind = std::fs::symlink_metadata(root.join(path))
            .unwrap()
            .file_type();
        eprintln!(
            "save over {path}: {} ({answer}); it is a link after: {}",
            if failed { "refused" } else { "done" },
            kind.is_symlink()
        );
        assert_eq!(
            std::fs::read(away.join("hard-away.shz")).unwrap(),
            away_before["hard-away.shz"].clone().unwrap()
        );
        assert_eq!(std::fs::read(root.join("first.shz")).unwrap(), first);
        if !failed {
            assert!(!kind.is_symlink(), "{path}");
            assert_eq!(hash_of(&std::fs::read(root.join(path)).unwrap()), hash);
        }
    }
    assert_eq!(snapshot(&away), away_before);
    assert!(temporary_files(&root).is_empty(), "{:?}", names(&root));
    s.answers();
}

/// A start folder given as a junction, with a separator at its end, and by its 8.3 short
/// name where the volume has one: each is the folder it names, and the rule holds there.
/// (Relative start folders are in server.rs, with "\" forms on Windows.)
#[test]
fn a_start_folder_given_as_a_junction_with_a_separator_or_a_short_name_works() {
    let mut scratch = Scratch::new("start");
    let real = scratch.base.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let real_resolved = std::fs::canonicalize(&real).unwrap();
    let junction = scratch.base.join("jstart");
    scratch.junction(&junction, &real);
    let temp = std::env::temp_dir();
    eprintln!(
        "the temporary folder is {} ({}an 8.3 short name in it)",
        temp.display(),
        if temp.to_string_lossy().contains('~') {
            ""
        } else {
            "no "
        }
    );
    let long = scratch.base.join("longfoldername-eight-dot-three");
    std::fs::create_dir_all(&long).unwrap();
    let short = scratch.base.join("LONGFO~1");
    let short_is_there =
        short.is_dir() && std::fs::canonicalize(&short).ok() == std::fs::canonicalize(&long).ok();
    eprintln!(
        "the 8.3 short name {} {}",
        short.display(),
        if short_is_there {
            "names the long folder"
        } else {
            "is not there (the volume makes none); that case is left out"
        }
    );
    let mut starts: Vec<(String, PathBuf)> = vec![
        (junction.to_string_lossy().into_owned(), real.clone()),
        (format!("{}\\", junction.display()), real.clone()),
        (format!("{}\\", real.display()), real.clone()),
        (format!("{}/", real.display()), real.clone()),
        (format!("{}\\.", real.display()), real.clone()),
        (plain(&real_resolved).replace('\\', "/"), real.clone()),
        (real_resolved.to_string_lossy().into_owned(), real.clone()),
    ];
    if short_is_there {
        starts.push((short.to_string_lossy().into_owned(), long.clone()));
    }
    for (n, (given, lands)) in starts.iter().enumerate() {
        let mut s = Session::new(Path::new(given));
        s.ok("add_board", board("Shelf", 0.0));
        let name = format!("s{n}.shz");
        let saved = s.ok("save_model", json!({"path": name}));
        eprintln!("start folder {given:?}: {saved}");
        let text = saved["file"].as_str().unwrap();
        assert!(text.starts_with(r"\\?\"), "{saved}");
        let file = PathBuf::from(text);
        assert_eq!(
            std::fs::canonicalize(&file).unwrap(),
            std::fs::canonicalize(lands.join(&name)).unwrap()
        );
        assert_eq!(
            hash_of(&std::fs::read(lands.join(&name)).unwrap()),
            s.hash()
        );
        let e = s.refused("save_model", json!({"path": "..\\out.shz"}));
        assert!(e.contains(".."), "{e}");
    }
    // The binary, given the junction and the short name relative to where it starts.
    let mut relative = vec![("jstart", real.clone()), ("jstart\\", real.clone())];
    if short_is_there {
        relative.push(("LONGFO~1", long.clone()));
    }
    for (n, (given, lands)) in relative.into_iter().enumerate() {
        let name = format!("b{n}.shz");
        let [hello, ready] = greeting();
        let requests = [
            hello,
            ready,
            tool_call(1, "add_board", board("Shelf", 0.0)),
            tool_call(2, "save_model", json!({"path": name})),
        ];
        let (status, out) = run_binary(&scratch.base, given, &requests, "\n");
        assert!(status.success(), "{given}");
        let lines = answers(&out);
        assert_eq!(lines.len(), 3, "{given}: {lines:?}");
        assert_eq!(lines[2]["result"]["isError"], false, "{}", lines[2]);
        assert!(lands.join(&name).is_file(), "{given}");
    }
    assert_eq!(
        names(&scratch.base),
        ["jstart", "longfoldername-eight-dot-three", "real"]
    );
    assert!(temporary_files(&real).is_empty());
}

/// A path whose full length is over 260 characters (folders made under the start folder)
/// is saved, opened, and saved over, with no temporary file left.
#[test]
fn a_path_over_260_characters_is_saved_and_opened() {
    let scratch = Scratch::new("long");
    let root = scratch.base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let parts: Vec<String> = (0..5)
        .map(|k| format!("{k}-{}", "folder".repeat(9)))
        .collect();
    let deep = parts.iter().fold(root.clone(), |p, f| p.join(f));
    std::fs::create_dir_all(&deep).unwrap();
    let rel = format!("{}/long.shz", parts.join("/"));
    let full = deep.join("long.shz");
    let length = plain(&full).chars().count();
    eprintln!("the file's full path is {length} characters");
    assert!(length > 260, "{length}");
    let mut s = Session::new(&root);
    s.ok("add_board", board("Shelf", 0.0));
    let saved = s.ok("save_model", json!({"path": rel}));
    assert_eq!(saved["path"], rel.as_str());
    let hash = s.hash();
    assert_eq!(hash_of(&std::fs::read(&full).unwrap()), hash);
    s.ok("add_board", board("Top", 400.0));
    let again = s.hash();
    s.ok("save_model", json!({}));
    assert_eq!(hash_of(&std::fs::read(&full).unwrap()), again);
    s.ok("new_model", json!({}));
    assert_eq!(
        s.ok("open_model", json!({"path": rel}))["model_hash"],
        again.as_str()
    );
    assert_eq!(names(&deep), ["long.shz"]);
}

/// A save over a file another handle holds open, with each share mode: the save is done or
/// refused, and either way the file is whole afterwards (the old one after a refusal, the
/// new one after a save reported done), with no temporary file left. open_model on the held
/// file reads it or says why, and the server answers on.
#[test]
fn a_save_over_a_file_held_open_leaves_one_whole_file_and_no_temporary_file() {
    const SHARE_READ: u32 = 0x1;
    const SHARE_WRITE: u32 = 0x2;
    const SHARE_DELETE: u32 = 0x4;
    let scratch = Scratch::new("held");
    let root = scratch.base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("held.shz");
    let mut s = Session::new(&root);
    s.ok("add_board", board("Shelf", 0.0));
    s.ok("save_model", json!({"path": "held.shz"}));
    let modes: [(&str, Option<u32>, bool); 6] = [
        ("std's own share mode (read, write and delete)", None, false),
        (
            "read, write and delete shared",
            Some(SHARE_READ | SHARE_WRITE | SHARE_DELETE),
            false,
        ),
        (
            "read and write shared, not delete",
            Some(SHARE_READ | SHARE_WRITE),
            false,
        ),
        ("read shared only", Some(SHARE_READ), false),
        ("nothing shared", Some(0), false),
        ("nothing shared, held for writing", Some(0), true),
    ];
    for (n, (what, mode, write)) in modes.into_iter().enumerate() {
        s.ok(
            "add_board",
            board(&format!("Board {n}"), 100.0 * (n as f64 + 1.0)),
        );
        let new = s.hash();
        let old = std::fs::read(&target).unwrap();
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(write);
        if let Some(mode) = mode {
            options.share_mode(mode);
        }
        let handle = options.open(&target).unwrap();
        let (failed, answer) = s.call("save_model", json!({"path": "held.shz"}));
        let mut other = Session::new(&root);
        let (open_failed, opened) = other.call("open_model", json!({"path": "held.shz"}));
        drop(handle);
        eprintln!(
            "held with {what}: save {} ({answer}); open {} ({opened})",
            if failed { "refused" } else { "done" },
            if open_failed { "refused" } else { "done" }
        );
        let now = std::fs::read(&target).unwrap();
        if failed {
            assert_eq!(
                now, old,
                "{what}: a refused save left the old file as it was"
            );
        } else {
            assert_eq!(
                hash_of(&now),
                new,
                "{what}: a save reported done left the new file"
            );
        }
        if !open_failed {
            let read = opened["model_hash"].as_str().unwrap();
            assert!(
                read == hash_of(&old) || read == new,
                "{what}: open read a whole file: {opened}"
            );
        }
        assert!(
            temporary_files(&root).is_empty(),
            "{what}: {:?}",
            names(&root)
        );
        assert_eq!(names(&root), ["held.shz"], "{what}");
        s.answers();
        other.answers();
    }
}

/// save_model's and open_model's "file" is the long `\\?\C:\...` form the system gives for
/// a resolved path, with "\" between folders, and names the file saved.
#[test]
fn the_file_field_is_the_long_resolved_form() {
    let scratch = Scratch::new("file-field");
    let root = scratch.base.join("root");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let mut s = Session::new(&root);
    s.ok("add_board", board("Shelf", 0.0));
    let resolved = std::fs::canonicalize(&root).unwrap();
    eprintln!(
        "start folder {} resolves to {}",
        root.display(),
        resolved.display()
    );
    for (path, rel) in [("f.shz", "f.shz"), ("sub/g.shz", r"sub\g.shz")] {
        let saved = s.ok("save_model", json!({"path": path}));
        let file = saved["file"].as_str().unwrap();
        eprintln!("save_model {path:?}: \"file\" is {file}");
        assert!(file.starts_with(r"\\?\"), "{saved}");
        assert!(!file.contains('/'), "{saved}");
        assert_eq!(file, format!(r"{}\{rel}", resolved.display()), "{saved}");
        assert_eq!(
            std::fs::canonicalize(file).unwrap(),
            std::fs::canonicalize(root.join(path)).unwrap()
        );
        let opened = s.ok("open_model", json!({"path": path}));
        assert_eq!(opened["file"], saved["file"]);
    }
}
