//! Clean-room guard (docs/CLEANROOM.md): identifiers that sat too close to a copyleft
//! project's names were renamed in lane 2. This test fails if any of them comes back in
//! the Rust sources. The names are assembled at runtime so this file does not match itself.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_files(&p, out);
            }
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

#[test]
fn retired_names_stay_retired() {
    let retired: Vec<String> = [
        ["Push", "Pull", "Error"].concat(),
        ["Sticky", "Error"].concat(),
        ["Render", "Mesh"].concat(),
        ["loop", "_half", "_edges"].concat(),
    ]
    .into();
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    rust_files(&crates, &mut files);
    assert!(files.len() > 10, "found only {} Rust files", files.len());
    let mut hits = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for name in &retired {
            if text.contains(name.as_str()) {
                hits.push(format!("{} contains {name}", f.display()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "retired identifiers found:\n{}",
        hits.join("\n")
    );
}
