//! Replays every sequence in `tests/repro/*.json` through the fuzzer's invariants.
//! A file lands here when the fuzzer finds a bug; it stays forever as a regression test.
//!
//! Only named cases are replayed. The fuzzer writes a fresh find to `tests/repro/pending/`
//! (gitignored), which this test never reads, so a find on one machine cannot break the
//! replay for everyone else before it is fixed. A stray `pending-*.json` in this directory
//! itself (a find copied up without being promoted) is skipped with a loud warning: rename
//! it, fix the kernel, commit both (`tests/repro/README.md`).

use std::path::{Path, PathBuf};

use sawhorse_kernel::testing::Repro;

fn repro_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("repro")
}

/// The named cases in `dir` (sorted) and the pending files that were skipped.
fn named_cases(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("tests/repro exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files.into_iter().partition(|p| {
        !p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("pending-"))
    })
}

#[test]
fn replay_all_repros() {
    let (files, pending) = named_cases(&repro_dir());
    for p in &pending {
        eprintln!(
            "WARNING: skipping {} (a pending fuzzer find, not a named regression case): \
             rename it, fix the kernel and commit both, see tests/repro/README.md",
            p.display()
        );
    }
    let mut failures = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let repro: Repro =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        if let Err(e) = repro.check() {
            failures.push(format!("{}: {e}", f.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} repro(s) still fail:\n{}",
        failures.len(),
        failures.join("\n")
    );
    println!(
        "{} repro sequence(s) pass, {} pending skipped",
        files.len(),
        pending.len()
    );
}

/// Pending finds never reach the replay: neither the gitignored `pending/` directory nor a
/// `pending-*.json` left in the repro directory itself.
#[test]
fn pending_finds_are_not_replayed() {
    let dir = std::env::temp_dir().join(format!("sawhorse-repro-skip-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("pending")).unwrap();
    std::fs::write(dir.join("2026-10-12-named.json"), "{}").unwrap();
    std::fs::write(dir.join("pending-long_fuzz_seed1.json"), "not json").unwrap();
    std::fs::write(
        dir.join("pending").join("pending-quick_fuzz.json"),
        "not json",
    )
    .unwrap();
    std::fs::write(dir.join("README.md"), "").unwrap();
    let (named, pending) = named_cases(&dir);
    std::fs::remove_dir_all(&dir).ok();
    let names = |v: &[PathBuf]| -> Vec<String> {
        v.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(names(&named), ["2026-10-12-named.json"]);
    assert_eq!(names(&pending), ["pending-long_fuzz_seed1.json"]);
}

/// Every committed case parses, and a case found with plane snapping off says so (the
/// replay then runs it the same way).
#[test]
fn repro_files_parse_and_keep_their_snapping() {
    let (files, _) = named_cases(&repro_dir());
    assert!(!files.is_empty());
    let mut off = 0;
    for f in &files {
        let r: Repro = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        assert!(!r.ops.is_empty(), "{}: no ops", f.display());
        assert!(
            !r.name.starts_with("pending-"),
            "{}: a named case keeps a pending name inside",
            f.display()
        );
        if r.plane_snap == Some(false) {
            off += 1;
        }
    }
    // The lane-12c finds (2026-10-12, FUZZ_PLANE_SNAP=0) replay with snapping off.
    assert!(off >= 3, "{off} snapping-off cases");
}
