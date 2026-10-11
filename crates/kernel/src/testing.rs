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

use crate::document::Document;
use crate::mesh::view_mesh;
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
    for (i, o) in m.objects() {
        if !o.is_watertight() {
            return Err(format!("object {i} is not watertight"));
        }
        let v = o.volume();
        if v.is_nan() || v <= 0.0 {
            return Err(format!("object {i} has non-positive volume {v}"));
        }
    }
    let mesh = view_mesh(m).map_err(|e| format!("view mesh: {e}"))?;
    if mesh.indices.len() % 3 != 0 || mesh.face_ids.len() * 3 != mesh.indices.len() {
        return Err("render mesh index/face-id mismatch".into());
    }
    Ok(())
}

/// Run `ops` (any commands, undo and redo included) on an empty document and check
/// every invariant after each one. `Err` describes the first violation, naming the op.
///
/// Beyond [`check_model`]: a refused command changes nothing (state hash, op log, undo
/// depth); after an accepted edit, undo restores the previous state exactly and redo
/// returns to the new one; at the end, replaying the document's own log reproduces its
/// state and log hashes, and a second run from scratch lands on the same state.
pub fn check_sequence(ops: &[Op]) -> Result<RunSummary, String> {
    let mut d = Document::new();
    let mut summary = RunSummary::default();
    for (i, op) in ops.iter().enumerate() {
        let before = d.hash();
        let (log, depth) = (d.log().len(), d.depth());
        let attempt = catch_unwind(AssertUnwindSafe(|| {
            let mut next = d.clone();
            let r = next.apply(op);
            (next, r)
        }));
        match attempt {
            Err(p) => return Err(format!("op {i} {op:?} panicked: {}", panic_message(p))),
            Ok((next, Ok(_))) => {
                check_model(next.model()).map_err(|e| format!("op {i} {op:?}: {e}"))?;
                if !matches!(op, Op::Undo | Op::Redo) {
                    let mut probe = next.clone();
                    probe
                        .undo()
                        .map_err(|e| format!("op {i} {op:?}: undo failed: {e}"))?;
                    if probe.hash() != before {
                        return Err(format!("op {i} {op:?}: undo did not restore the state"));
                    }
                    probe
                        .redo()
                        .map_err(|e| format!("op {i} {op:?}: redo failed: {e}"))?;
                    if probe.hash() != next.hash() {
                        return Err(format!("op {i} {op:?}: redo did not return to the state"));
                    }
                }
                d = next;
                summary.accepted += 1;
            }
            Ok((next, Err(e))) => {
                if e.is_bug() {
                    return Err(format!("op {i} {op:?}: validator rejected the result: {e}"));
                }
                if next.hash() != before
                    || d.hash() != before
                    || next.log().len() != log
                    || next.depth() != depth
                {
                    return Err(format!(
                        "op {i} {op:?}: refused ({e}) but the document changed"
                    ));
                }
                summary.refused += 1;
            }
        }
    }
    // The log is the history: replaying it lands on the same state and the same log.
    let replayed =
        Document::replay(d.log()).map_err(|(i, e)| format!("log replay refused entry {i}: {e}"))?;
    if replayed.hash() != d.hash() || replayed.log_hash() != d.log_hash() {
        return Err(format!(
            "log replay diverged: {} vs {}",
            replayed.hash_hex(),
            d.hash_hex()
        ));
    }
    // Determinism: a second run of the raw commands lands on the same document.
    let mut again = Document::new();
    for op in ops {
        let _ = again.apply(op);
    }
    if again.hash() != d.hash() {
        return Err(format!(
            "non-deterministic: {} vs {}",
            again.hash_hex(),
            d.hash_hex()
        ));
    }
    summary.objects = d.model().objects().len();
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
                sketch: 1,
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            Op::AddRectangle {
                sketch: 1,
                x: 0.5,
                y: 0.5,
                w: 1.0,
                h: 1.0,
            }, // crossing: splits into three regions
            Op::Extrude {
                sketch: 1,
                region: 1,
                distance: 0.5,
            },
            Op::PushPull {
                object: 1,
                face: 1,
                distance: 0.25,
            },
        ];
        let s = check_sequence(&ops).unwrap();
        assert_eq!((s.accepted, s.refused, s.objects), (5, 0, 1));
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

// ---- boolean differential cases (tests/booleans/cases.json) ----------------------------
//
// The same case file drives the Rust test (`tests/booleans.rs`, hand-computed expectations)
// and the Node harness (`scripts/boolean-diff.mjs`), which runs each case through the WASM
// kernel and through npm `manifold-3d` (the C++ Manifold) on the very same operand meshes.

/// One operand: a box or a prism (outer ring, optional holes, between two heights), with
/// an optional loop imprinted on its top face (user-drawn edges to carry through).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SolidSpec {
    #[serde(rename = "box", default, skip_serializing_if = "Option::is_none")]
    pub cuboid: Option<[f64; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prism: Option<PrismSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imprint_top: Option<Vec<[f64; 2]>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PrismSpec {
    pub outer: Vec<[f64; 2]>,
    #[serde(default)]
    pub holes: Vec<Vec<[f64; 2]>>,
    pub z0: f64,
    pub z1: f64,
}

/// Expected outcome. `refused`: the kernel refuses (typed); `volume`, `area`, `shells` and
/// `genus` then describe what a mesh engine reports.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Expect {
    #[serde(default)]
    pub empty: bool,
    #[serde(default)]
    pub refused: bool,
    pub volume: Option<f64>,
    pub area: Option<f64>,
    pub shells: Option<usize>,
    pub genus: Option<i64>,
    pub faces: Option<usize>,
    pub holes: Option<usize>,
    pub user_edges: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BoolCase {
    pub name: String,
    pub op: crate::boolean::BoolOp,
    pub a: SolidSpec,
    pub b: SolidSpec,
    pub expect: Expect,
}

/// What the kernel produced for a case.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct CaseResult {
    pub empty: bool,
    pub refused: Option<String>,
    pub volume: f64,
    pub area: f64,
    pub shells: usize,
    pub genus: i64,
    pub faces: usize,
    pub holes: usize,
    pub user_edges: usize,
}

/// Build an operand through the ordinary ops (sketch, polyline, extrude, imprint).
pub fn build_solid(s: &SolidSpec) -> Result<crate::object::Object, String> {
    let (outer, holes, z0, z1) = match (&s.cuboid, &s.prism) {
        (Some(b), None) => (
            vec![[b[0], b[1]], [b[3], b[1]], [b[3], b[4]], [b[0], b[4]]],
            Vec::new(),
            b[2],
            b[5],
        ),
        (None, Some(p)) => (p.outer.clone(), p.holes.clone(), p.z0, p.z1),
        _ => return Err("an operand is exactly one of box / prism".into()),
    };
    let mut m = Model::new();
    let e = |e: crate::error::KernelError| e.to_string();
    m.apply(&Op::NewSketch {
        origin: [0.0, 0.0, z0],
        normal: [0.0, 0.0, 1.0],
    })
    .map_err(e)?;
    for ring in std::iter::once(&outer).chain(holes.iter()) {
        m.apply(&Op::AddPolyline {
            sketch: 1,
            points: ring.clone(),
            closed: true,
        })
        .map_err(e)?;
    }
    let s1 = m.sketch(1).ok_or("sketch")?;
    let region = s1
        .regions()
        .max_by(|a, b| s1.region_area(a.1).total_cmp(&s1.region_area(b.1)))
        .map(|(id, _)| crate::ids::to_public(id))
        .ok_or("no region")?;
    m.apply(&Op::Extrude {
        sketch: 1,
        region,
        distance: z1 - z0,
    })
    .map_err(e)?;
    if let Some(pts) = &s.imprint_top {
        m.apply(&Op::Imprint {
            object: 1,
            face: 1,
            points: pts.iter().map(|p| [p[0], p[1], z1]).collect(),
        })
        .map_err(e)?;
    }
    m.object(1).cloned().ok_or_else(|| "no object".into())
}

/// Run a case through `Object::combine` (the push/pull boolean pipeline).
pub fn run_boolean_case(c: &BoolCase) -> Result<CaseResult, String> {
    let a = build_solid(&c.a)?;
    let b = build_solid(&c.b)?;
    match a.combine(&b, c.op) {
        Ok(None) => Ok(CaseResult {
            empty: true,
            ..Default::default()
        }),
        Ok(Some(o)) => {
            let r = o.validate().map_err(|e| e.to_string())?;
            Ok(CaseResult {
                empty: false,
                refused: None,
                volume: o.volume(),
                area: o.surface_area(),
                shells: r.shells.len(),
                genus: r.shells.iter().map(|s| s.genus.unwrap_or(0)).sum(),
                faces: r.faces,
                holes: r.holes,
                user_edges: o.brep().edges().filter(|(_, e)| e.user).count(),
            })
        }
        Err(e) => Ok(CaseResult {
            refused: Some(e.to_string()),
            ..Default::default()
        }),
    }
}

/// Check a result against a case's expectations; `Err` lists every mismatch.
pub fn check_case(c: &BoolCase, r: &CaseResult) -> Result<(), String> {
    let x = &c.expect;
    let mut bad = Vec::new();
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs().max(1.0);
    if x.refused != r.refused.is_some() {
        bad.push(format!(
            "refused: expected {}, got {:?}",
            x.refused, r.refused
        ));
    }
    if x.refused {
        return if bad.is_empty() {
            Ok(())
        } else {
            Err(bad.join("; "))
        };
    }
    if x.empty != r.empty {
        bad.push(format!("empty: expected {}, got {}", x.empty, r.empty));
    }
    if let Some(v) = x.volume
        && !near(r.volume, v)
    {
        bad.push(format!("volume {} (expected {v})", r.volume));
    }
    if let Some(v) = x.area
        && !near(r.area, v)
    {
        bad.push(format!("area {} (expected {v})", r.area));
    }
    for (what, got, want) in [
        ("shells", r.shells as i64, x.shells.map(|v| v as i64)),
        ("genus", r.genus, x.genus),
        ("faces", r.faces as i64, x.faces.map(|v| v as i64)),
        ("holes", r.holes as i64, x.holes.map(|v| v as i64)),
        (
            "user edges",
            r.user_edges as i64,
            x.user_edges.map(|v| v as i64),
        ),
    ] {
        if let Some(w) = want
            && got != w
        {
            bad.push(format!("{what} {got} (expected {w})"));
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad.join("; "))
    }
}
