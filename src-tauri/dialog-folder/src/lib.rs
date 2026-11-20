//! The folder a desktop Open or Save dialog starts in (#71, #80).
//!
//! The page gave the dialogs no folder: Save was given a bare name (`Untitled.shz`) and Open
//! nothing. The system's chooser then started where it chose. On Linux that is GTK's: its
//! Save chooser starts in the process's working directory, which for an AppImage is the
//! mount it runs from (`/tmp/.mount_Sawhor...`, read-only and gone when the app closes), and
//! its Open chooser starts in Recent.
//!
//! Now the page keeps the folder of the last file it opened or saved and names it to the
//! shell before each dialog, after the folder of the model's own file for Save As. The shell's
//! `dialog_folder` command (`src-tauri/src/lib.rs`) hands them to [`choose`], which is the
//! whole rule: on Linux, the folders the page named, then the system's Documents folder, then
//! the home folder, and of those the first the dialog can start in. On Windows and macOS it
//! answers nothing, and the system's dialog starts where it starts.
//!
//! - A folder is taken only when its path is absolute and holds no `..`. The page writes a
//!   folder down as text cut from a file's path; one that is not a whole path names nothing,
//!   and a `..` would go into the path the dialog hands back, which the fs plugin the page
//!   reads files through refuses (tauri's `SafePathBuf`).
//! - It is taken only when it can be used now ([`usable`]: a folder whose contents this user
//!   can list). One deleted, renamed, on a drive no longer there, or closed to this user is
//!   passed over, so the dialog never falls back to the working directory because of it and
//!   never opens on a folder it cannot show.
//! - A path that is not Unicode is passed over: the page is answered in text.
//! - The order given is the order of choice, and the first taken wins.
//! - With none, there is no answer and the dialog is given no folder, as before.
//!
//! The page may name at most [`MAX_NAMED`] folders, each at most [`MAX_LEN`] bytes
//! ([`named`]): it has two to name, and nothing it sends is trusted to be short.

use std::path::{Component, Path, PathBuf};

/// The most folders the page may name: the model's own and the last one used, with room.
pub const MAX_NAMED: usize = 4;
/// The longest folder path taken from the page, in bytes (Linux's PATH_MAX).
pub const MAX_LEN: usize = 4096;

/// The folders the page named, as paths: the first [`MAX_NAMED`] of them, without any that
/// is empty or longer than [`MAX_LEN`].
pub fn named(folders: Vec<String>) -> impl Iterator<Item = PathBuf> {
    folders
        .into_iter()
        .take(MAX_NAMED)
        .filter(|f| !f.is_empty() && f.len() <= MAX_LEN)
        .map(PathBuf::from)
}

/// Whether a dialog may be started in a folder written this way: an absolute path with no
/// `..` in it.
fn whole(folder: &Path) -> bool {
    folder.is_absolute() && !folder.components().any(|c| c == Component::ParentDir)
}

/// The first of `candidates`, in order, that is a whole path and usable now. `usable` is
/// asked only about whole paths, and about none after the one taken.
pub fn start<I>(candidates: I, usable: impl Fn(&Path) -> bool) -> Option<PathBuf>
where
    I: IntoIterator<Item = Option<PathBuf>>,
{
    candidates
        .into_iter()
        .flatten()
        .find(|folder| whole(folder) && usable(folder))
}

/// Whether a dialog can start in `folder` now: it is a folder, and this user can list what
/// is in it. A file, a folder that is gone and a folder this user may not read are not.
/// The folder is opened and closed; no entry of it is read.
pub fn usable(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok()
}

/// The shell's answer to the page: the folder the next dialog starts in, or nothing.
///
/// `linux` is whether this is the Linux app. Elsewhere the answer is nothing, and neither
/// `system` nor `usable` is called. On Linux the candidates are the folders the page named
/// ([`named`]), then the two `system` gives, Documents and home in that order (either may be
/// missing), and the answer is the first of them [`start`] takes that is also Unicode.
pub fn choose(
    linux: bool,
    folders: Vec<String>,
    system: impl FnOnce() -> [Option<PathBuf>; 2],
    usable: impl Fn(&Path) -> bool,
) -> Option<String> {
    if !linux {
        return None;
    }
    let candidates = named(folders).map(Some).chain(system());
    let folder = start(candidates, |f| f.to_str().is_some() && usable(f))?;
    folder.to_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[cfg(unix)]
    fn some(paths: &[&str]) -> Vec<Option<PathBuf>> {
        paths.iter().map(|p| Some(PathBuf::from(p))).collect()
    }

    /// A disk with exactly these folders on it.
    #[cfg(unix)]
    fn disk<'a>(folders: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
        move |p| folders.iter().any(|f| Path::new(f) == p)
    }

    #[cfg(unix)]
    #[test]
    fn the_first_folder_that_is_there_is_taken_in_the_order_given() {
        let on_disk = ["/home/maker/models", "/home/maker/Documents", "/home/maker"];
        let asked = some(&["/home/maker/models", "/home/maker/Documents", "/home/maker"]);
        assert_eq!(
            start(asked, disk(&on_disk)),
            Some(PathBuf::from("/home/maker/models"))
        );
        let model_first = some(&["/home/maker", "/home/maker/models"]);
        assert_eq!(
            start(model_first, disk(&on_disk)),
            Some(PathBuf::from("/home/maker"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_is_gone_is_passed_over_for_documents_and_then_home() {
        let asked = || {
            some(&[
                "/media/stick/models",
                "/home/maker/Documents",
                "/home/maker",
            ])
        };
        assert_eq!(
            start(asked(), disk(&["/home/maker/Documents", "/home/maker"])),
            Some(PathBuf::from("/home/maker/Documents"))
        );
        assert_eq!(
            start(asked(), disk(&["/home/maker"])),
            Some(PathBuf::from("/home/maker"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_system_folder_the_system_does_not_have_is_no_candidate() {
        // No Documents folder is set up (the resolver gave none): home is next.
        let asked = vec![None, Some(PathBuf::from("/home/maker"))];
        assert_eq!(
            start(asked, disk(&["/home/maker"])),
            Some(PathBuf::from("/home/maker"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn with_no_folder_there_is_no_answer() {
        assert_eq!(start(some(&["/gone", "/also/gone"]), disk(&[])), None);
        assert_eq!(start(Vec::new(), disk(&["/home/maker"])), None);
        assert_eq!(start(vec![None, None], disk(&["/home/maker"])), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_whole_is_never_taken_and_never_asked_about() {
        let asked_about = RefCell::new(Vec::new());
        let everything_is_a_folder = |p: &Path| {
            asked_about.borrow_mut().push(p.to_path_buf());
            true
        };
        let asked = some(&[
            "models",
            "./models",
            "",
            "/home/maker/../other",
            "/home/maker/models/..",
            "/home/maker",
            "/never/asked",
        ]);
        assert_eq!(
            start(asked, everything_is_a_folder),
            Some(PathBuf::from("/home/maker"))
        );
        assert_eq!(*asked_about.borrow(), vec![PathBuf::from("/home/maker")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_is_not_a_folder() {
        // The caller's `usable` says so; `start` takes its word.
        let asked = some(&["/home/maker/shelf.shz", "/home/maker"]);
        assert_eq!(
            start(asked, disk(&["/home/maker"])),
            Some(PathBuf::from("/home/maker"))
        );
    }

    #[test]
    fn the_page_names_a_few_folders_of_sane_length() {
        let many: Vec<String> = (0..10).map(|i| format!("/f{i}")).collect();
        let taken: Vec<PathBuf> = named(many).collect();
        assert_eq!(taken.len(), MAX_NAMED);
        assert_eq!(taken[0], PathBuf::from("/f0"));
        let long = format!("/{}", "a".repeat(MAX_LEN));
        let just = format!("/{}", "a".repeat(MAX_LEN - 1));
        let taken: Vec<PathBuf> =
            named(vec![String::new(), long, just.clone(), "/ok".into()]).collect();
        assert_eq!(taken, vec![PathBuf::from(just), PathBuf::from("/ok")]);
    }

    /// Against the real disk: this crate's own folder is one, its manifest is not, and a
    /// name beside it that nothing has is not.
    #[test]
    fn the_real_disk_is_asked_as_the_shell_asks_it() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(usable(here));
        assert!(!usable(&here.join("Cargo.toml")));
        assert!(!usable(&here.join("no-such-folder")));
        let candidates = vec![
            Some(here.join("no-such-folder")),
            Some(here.join("Cargo.toml")),
            Some(here.to_path_buf()),
        ];
        assert_eq!(start(candidates, usable), Some(here.to_path_buf()));
    }

    /// A folder that is there but closed to this user cannot be listed, so a dialog is not
    /// started in it. Root reads every folder, so as root there is nothing to see.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_folder_this_user_may_not_read_is_not_usable() {
        use std::fs;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let root = fs::metadata("/proc/self")
            .map(|m| m.uid() == 0)
            .unwrap_or(false);
        let closed =
            std::env::temp_dir().join(format!("sawhorse-dialog-folder-{}", std::process::id()));
        fs::create_dir(&closed).unwrap();
        assert!(usable(&closed));
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
        let seen = usable(&closed);
        let taken = start(
            vec![Some(closed.clone()), Some(std::env::temp_dir())],
            usable,
        );
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir(&closed).unwrap();
        if !root {
            assert!(!seen);
            assert_eq!(taken, Some(std::env::temp_dir()));
        }
    }

    const NAMED: &str = "/home/maker/models";
    const DOCUMENTS: &str = "/home/maker/Documents";
    const HOME: &str = "/home/maker";

    fn system() -> [Option<PathBuf>; 2] {
        [Some(PathBuf::from(DOCUMENTS)), Some(PathBuf::from(HOME))]
    }

    #[cfg(unix)]
    #[test]
    fn on_linux_the_page_s_folders_come_first_then_documents_then_home() {
        let page = || vec![NAMED.to_string()];
        assert_eq!(
            choose(true, page(), system, disk(&[NAMED, DOCUMENTS, HOME])),
            Some(NAMED.into())
        );
        assert_eq!(
            choose(true, page(), system, disk(&[DOCUMENTS, HOME])),
            Some(DOCUMENTS.into())
        );
        assert_eq!(
            choose(true, page(), system, disk(&[HOME])),
            Some(HOME.into())
        );
        assert_eq!(choose(true, page(), system, disk(&[])), None);
        // The page named none: the first Save of the first start.
        assert_eq!(
            choose(true, Vec::new(), system, disk(&[NAMED, DOCUMENTS, HOME])),
            Some(DOCUMENTS.into())
        );
        // No Documents folder is set up: home.
        let no_documents = || [None, Some(PathBuf::from(HOME))];
        assert_eq!(
            choose(true, Vec::new(), no_documents, disk(&[DOCUMENTS, HOME])),
            Some(HOME.into())
        );
        // Two named, the model's own first: the order named is the order of choice.
        let two = vec![HOME.to_string(), NAMED.to_string()];
        assert_eq!(
            choose(true, two, system, disk(&[NAMED, DOCUMENTS, HOME])),
            Some(HOME.into())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_used_is_passed_over_wherever_it_is_in_the_order() {
        // `usable` is the caller's word on each folder: gone, a file, or closed to this user.
        let page = || vec![NAMED.to_string()];
        fn not(closed: &'static str) -> impl Fn(&Path) -> bool {
            move |p| p != Path::new(closed)
        }
        assert_eq!(
            choose(true, page(), system, not(NAMED)),
            Some(DOCUMENTS.into())
        );
        assert_eq!(
            choose(true, Vec::new(), system, not(DOCUMENTS)),
            Some(HOME.into())
        );
        assert_eq!(choose(true, page(), system, not(HOME)), Some(NAMED.into()));
    }

    #[test]
    fn off_linux_the_answer_is_nothing_and_nothing_is_looked_at() {
        let looked = RefCell::new(0);
        let answer = choose(
            false,
            vec![NAMED.to_string()],
            || {
                *looked.borrow_mut() += 1;
                system()
            },
            |_| {
                *looked.borrow_mut() += 1;
                true
            },
        );
        assert_eq!(answer, None);
        assert_eq!(*looked.borrow(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_unicode_is_passed_over() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let odd = PathBuf::from(OsString::from_vec(b"/home/mak\xffer/Documents".to_vec()));
        assert!(odd.to_str().is_none());
        let asked_about = RefCell::new(Vec::new());
        let everything = |p: &Path| {
            asked_about.borrow_mut().push(p.to_path_buf());
            true
        };
        let with_odd = || [Some(odd.clone()), Some(PathBuf::from(HOME))];
        assert_eq!(
            choose(true, Vec::new(), with_odd, everything),
            Some(HOME.into())
        );
        assert_eq!(*asked_about.borrow(), vec![PathBuf::from(HOME)]);
    }
}
