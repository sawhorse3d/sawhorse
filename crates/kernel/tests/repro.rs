//! Replays every sequence in `tests/repro/*.json` through the fuzzer's invariants.
//! A file lands here when the fuzzer finds a bug; it stays forever as a regression test.

use std::path::PathBuf;

use sawhorse_kernel::testing::{Repro, check_sequence};

#[test]
fn replay_all_repros() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("repro");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/repro exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut failures = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let repro: Repro =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        if let Err(e) = check_sequence(&repro.ops) {
            failures.push(format!("{}: {e}", f.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} repro(s) still fail:\n{}",
        failures.len(),
        failures.join("\n")
    );
    println!("{} repro sequence(s) pass", files.len());
}
