//! Shared invariants for the op-sequence fuzzer, the repro replays and cargo-fuzz.
//!
//! [`check_sequence`] runs a sequence of ops from an empty model and checks, after every
//! op:
//! - an accepted op leaves a model that validates (topology, watertight solids, sketch
//!   graph), whose solids have positive volume, and that tessellates for rendering;
//! - a refused op returns a typed refusal (never a validator "bug" error) and leaves the
//!   model bit-for-bit unchanged (same hash);
//! - nothing panics (in debug builds the per-mutation validator panics on corruption);
//!
//! and finally that replaying the same sequence again gives the same hash (determinism).
//! Failing sequences are written as JSON by [`write_repro`] and replayed forever after by
//! `tests/repro.rs`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::mesh::render_mesh;
use crate::model::{Model, Op};

/// What a clean run looked like (for fuzzer statistics).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunSummary {
    pub accepted: usize,
    pub refused: usize,
    pub objects: usize,
}

fn panic_message(p: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic".to_string()
    }
}

/// Invariants of any committed model.
pub fn check_model(m: &Model) -> Result<(), String> {
    m.validate().map_err(|e| format!("validate: {e}"))?;
    for (i, o) in m.objects().iter().enumerate() {
        if !o.is_watertight() {
            return Err(format!("object {i} is not watertight"));
        }
        let v = o.volume();
        if v.is_nan() || v <= 0.0 {
            return Err(format!("object {i} has non-positive volume {v}"));
        }
    }
    let mesh = render_mesh(m).map_err(|e| format!("render mesh: {e}"))?;
    if mesh.indices.len() % 3 != 0 || mesh.face_ids.len() * 3 != mesh.indices.len() {
        return Err("render mesh index/face-id mismatch".into());
    }
    Ok(())
}

/// Run `ops` from an empty model and check every invariant. `Err` describes the first
/// violation, naming the op index.
pub fn check_sequence(ops: &[Op]) -> Result<RunSummary, String> {
    let mut m = Model::new();
    let mut summary = RunSummary::default();
    for (i, op) in ops.iter().enumerate() {
        let before = m.hash();
        let attempt = catch_unwind(AssertUnwindSafe(|| {
            let mut next = m.clone();
            let r = next.apply(op);
            (next, r)
        }));
        match attempt {
            Err(p) => return Err(format!("op {i} {op:?} panicked: {}", panic_message(p))),
            Ok((next, Ok(_))) => {
                check_model(&next).map_err(|e| format!("op {i} {op:?}: {e}"))?;
                m = next;
                summary.accepted += 1;
            }
            Ok((next, Err(e))) => {
                if e.is_bug() {
                    return Err(format!("op {i} {op:?}: validator rejected the result: {e}"));
                }
                if next.hash() != before || m.hash() != before {
                    return Err(format!(
                        "op {i} {op:?}: refused ({e}) but the model changed"
                    ));
                }
                summary.refused += 1;
            }
        }
    }
    // Determinism: a second run lands on the same model.
    let mut again = Model::new();
    for op in ops {
        let _ = again.apply(op);
    }
    if again.hash() != m.hash() {
        return Err(format!(
            "non-deterministic: {} vs {}",
            again.hash_hex(),
            m.hash_hex()
        ));
    }
    summary.objects = m.objects().len();
    Ok(summary)
}

/// A repro file: the failing sequence plus context.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Repro {
    pub name: String,
    pub error: String,
    pub ops: Vec<Op>,
}

/// Write a failing sequence to `dir/<name>.json` (overwriting: when proptest shrinks, the
/// last write is the minimal case).
#[cfg(not(target_arch = "wasm32"))]
pub fn write_repro(
    dir: &std::path::Path,
    name: &str,
    error: &str,
    ops: &[Op],
) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{name}.json"));
    let repro = Repro {
        name: name.to_string(),
        error: error.to_string(),
        ops: ops.to_vec(),
    };
    let json = serde_json::to_string_pretty(&repro).map_err(std::io::Error::other)?;
    std::fs::write(&path, json + "\n")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_sequence_passes() {
        let ops = vec![
            Op::NewSketch {
                origin: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            },
            Op::AddRectangle {
                sketch: 0,
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            Op::AddRectangle {
                sketch: 0,
                x: 0.5,
                y: 0.5,
                w: 1.0,
                h: 1.0,
            }, // refused: crossing
            Op::Extrude {
                sketch: 0,
                region: 0,
                distance: 0.5,
            },
            Op::PushPull {
                object: 0,
                face: 1,
                distance: 0.25,
            },
        ];
        let s = check_sequence(&ops).unwrap();
        assert_eq!((s.accepted, s.refused, s.objects), (4, 1, 1));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn repro_round_trip() {
        let dir = std::env::temp_dir().join(format!("sawhorse-repro-{}", std::process::id()));
        let ops = vec![Op::NewSketch {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        }];
        let path = write_repro(&dir, "pending-test", "example", &ops).unwrap();
        let back: Repro = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.ops, ops);
        assert_eq!(back.error, "example");
        std::fs::remove_dir_all(&dir).ok();
    }
}
