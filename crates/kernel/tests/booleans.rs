//! Boolean differential suite (ADR-0001): every case in `tests/booleans/cases.json` runs
//! through the kernel's boolean pipeline (triangulate, manifold-rust, re-polygonise,
//! coplanar merge, validate) and must match hand-computed volume, surface area, shell
//! count, genus and, where given, face / hole / user-edge counts. `scripts/boolean-diff.mjs`
//! runs the same file through the WASM kernel and npm `manifold-3d` for a differential.

use std::path::PathBuf;

use sawhorse_kernel::testing::{BoolCase, check_case, run_boolean_case};

#[test]
fn boolean_cases_match_hand_computed_results() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/booleans/cases.json");
    let cases: Vec<BoolCase> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(cases.len() >= 20, "at least 20 cases");
    let mut failures = Vec::new();
    for c in &cases {
        match run_boolean_case(c) {
            Ok(r) => match check_case(c, &r) {
                Err(e) => failures.push(format!("{}: {e}", c.name)),
                Ok(()) => println!(
                    "ok {:<38} volume {:>8.4} area {:>8.4} shells {} genus {} faces {:>2}{}",
                    c.name,
                    r.volume,
                    r.area,
                    r.shells,
                    r.genus,
                    r.faces,
                    r.refused
                        .as_deref()
                        .map(|e| format!(" (refused: {e})"))
                        .unwrap_or_default()
                ),
            },
            Err(e) => failures.push(format!("{}: could not run: {e}", c.name)),
        }
    }
    assert!(
        failures.is_empty(),
        "{} case(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
