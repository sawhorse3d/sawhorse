//! Op-sequence fuzzer (proptest-driven) over the whole document: sketch strokes with
//! merging / T-junctions / crossings / overlaps / erasing, rectangles, polylines, extrude,
//! general push/pull (both ways, through the far side), split_face, imprint, erasing
//! (object edges and faces, sketch regions), batches, delete, groups, tags, and undo / redo
//! interleaved anywhere.
//!
//! Generated ops are *intents* ([`Fuzz`]) resolved against the live document as the
//! sequence runs: "an edge of some face of some solid", "a region of some sketch", "a
//! rectangle at these fractions of a face". That keeps most ops meaningful (a random id
//! would almost always be refused) while still feeding garbage on purpose (NaN, huge,
//! sub-tolerance, unknown ids). The resolved, concrete op list is what gets checked by
//! [`check_sequence`] and, on failure, written to `tests/repro/pending-<name>.json`
//! (rename it, fix the kernel, commit both; `tests/repro.rs` replays it forever).
//!
//! `cargo test --test sequences` runs a quick pass. The long run, seeded so shards can run
//! in parallel: `FUZZ_CASES=6250 FUZZ_SEED=1 cargo test --release --test sequences --
//! --ignored long_fuzz --nocapture` (see docs/DEVELOPMENT.md for the 8-shard recipe).

use std::path::PathBuf;

use glam::{DVec2, DVec3};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use sawhorse_kernel::Document;
use sawhorse_kernel::model::Op;
use sawhorse_kernel::testing::{check_sequence, write_repro};

fn grid() -> impl Strategy<Value = f64> {
    prop_oneof![
        6 => (-8i32..8).prop_map(|k| f64::from(k) * 0.25),
        2 => -3.0..3.0f64,
        1 => Just(1e-6),
        1 => Just(0.25 + 4e-6), // inside POINT_MERGE of a grid line
    ]
}

fn distance() -> impl Strategy<Value = f64> {
    prop_oneof![
        6 => (-8i32..8).prop_map(|k| f64::from(k) * 0.25),
        3 => -3.0..3.0f64,
        2 => (-12i32..-4).prop_map(|k| f64::from(k) * 0.5), // push through
        1 => Just(0.0),
        1 => Just(5e-6),
        1 => Just(2e4),
        1 => Just(f64::NAN),
        1 => Just(f64::INFINITY),
    ]
}

fn normal() -> impl Strategy<Value = [f64; 3]> {
    prop_oneof![
        3 => Just([0.0, 0.0, 1.0]),
        1 => Just([1.0, 0.0, 0.0]),
        1 => Just([0.0, 1.0, -1.0]),
        1 => Just([0.0, 0.0, 0.0]),
        2 => (-1.0..1.0f64, -1.0..1.0f64, -1.0..1.0f64).prop_map(|(x, y, z)| [x, y, z]),
    ]
}

/// Eighths in (0, 1): positions along edges and across faces.
fn frac() -> impl Strategy<Value = f64> {
    prop_oneof![4 => (1i32..8).prop_map(|k| f64::from(k) / 8.0), 1 => 0.0..1.0f64]
}

/// An op intent; `pick` fields choose among what exists (modulo the count).
#[derive(Debug, Clone)]
enum Fuzz {
    NewSketch([f64; 3], [f64; 3]),
    Rect(u8, f64, f64, f64, f64),
    Segment(u8, [f64; 2], [f64; 2]),
    Erase(u8, [f64; 2], [f64; 2]),
    Polyline(u8, Vec<[f64; 2]>, bool),
    Extrude(u8, u8, f64),
    Push(u8, u8, f64),
    Split {
        object: u8,
        face: u8,
        e1: u8,
        t1: f64,
        e2: u8,
        t2: f64,
        bend: f64,
    },
    Imprint {
        object: u8,
        face: u8,
        at: [f64; 2],
        size: [f64; 2],
        triangle: bool,
    },
    /// Erase edge `edge` of face `face` of an object (mostly refused unless it was drawn).
    EraseEdge(u8, u8, u8),
    EraseFace(u8, u8),
    EraseRegion(u8, u8),
    /// Two simple intents resolved against the same state, applied as one batch.
    Batch(Vec<Fuzz>),
    Delete(u8),
    Group(Vec<u8>),
    Tag(u8, Option<u8>),
    TagVisible(u8, bool),
    Undo,
    Redo,
    /// Raw op with whatever ids it has (mostly refused).
    Raw(Op),
}

fn fuzz_op() -> impl Strategy<Value = Fuzz> {
    let pt = || (grid(), grid()).prop_map(|(x, y)| [x, y]);
    prop_oneof![
        1 => (prop_oneof![Just([0.0; 3]), (grid(), grid(), grid()).prop_map(|(x, y, z)| [x, y, z])], normal())
            .prop_map(|(o, n)| Fuzz::NewSketch(o, n)),
        5 => (any::<u8>(), grid(), grid(), grid(), grid()).prop_map(|(s, x, y, w, h)| Fuzz::Rect(s, x, y, w, h)),
        4 => (any::<u8>(), pt(), pt()).prop_map(|(s, a, b)| Fuzz::Segment(s, a, b)),
        1 => (any::<u8>(), pt(), pt()).prop_map(|(s, a, b)| Fuzz::Erase(s, a, b)),
        1 => (any::<u8>(), prop::collection::vec(pt(), 2..6), any::<bool>()).prop_map(|(s, p, c)| Fuzz::Polyline(s, p, c)),
        4 => (any::<u8>(), any::<u8>(), distance()).prop_map(|(s, r, d)| Fuzz::Extrude(s, r, d)),
        6 => (any::<u8>(), any::<u8>(), distance()).prop_map(|(o, f, d)| Fuzz::Push(o, f, d)),
        3 => (any::<u8>(), any::<u8>(), any::<u8>(), frac(), any::<u8>(), frac(), prop_oneof![Just(0.0), 0.0..0.6f64])
            .prop_map(|(object, face, e1, t1, e2, t2, bend)| Fuzz::Split { object, face, e1, t1, e2, t2, bend }),
        3 => (any::<u8>(), any::<u8>(), (frac(), frac()), (frac(), frac()), any::<bool>())
            .prop_map(|(object, face, at, size, triangle)| Fuzz::Imprint { object, face, at: [at.0, at.1], size: [size.0, size.1], triangle }),
        2 => (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(o, f, e)| Fuzz::EraseEdge(o, f, e)),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(o, f)| Fuzz::EraseFace(o, f)),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(s, r)| Fuzz::EraseRegion(s, r)),
        1 => prop::collection::vec(
                prop_oneof![
                    (any::<u8>(), grid(), grid(), grid(), grid()).prop_map(|(s, x, y, w, h)| Fuzz::Rect(s, x, y, w, h)),
                    (any::<u8>(), any::<u8>(), distance()).prop_map(|(o, f, d)| Fuzz::Push(o, f, d)),
                    (any::<u8>(), any::<u8>()).prop_map(|(o, f)| Fuzz::EraseFace(o, f)),
                ],
                0..3,
            )
            .prop_map(Fuzz::Batch),
        1 => any::<u8>().prop_map(Fuzz::Delete),
        1 => prop::collection::vec(any::<u8>(), 0..3).prop_map(Fuzz::Group),
        1 => (any::<u8>(), prop::option::of(0u8..3)).prop_map(|(o, t)| Fuzz::Tag(o, t)),
        1 => (0u8..3, any::<bool>()).prop_map(|(t, v)| Fuzz::TagVisible(t, v)),
        3 => Just(Fuzz::Undo),
        2 => Just(Fuzz::Redo),
        1 => (0u64..4, 0u64..6, distance()).prop_map(|(sketch, region, distance)| Fuzz::Raw(Op::Extrude { sketch, region, distance })),
        1 => (0u64..4, 0u64..14, distance()).prop_map(|(object, face, distance)| Fuzz::Raw(Op::PushPull { object, face, distance })),
    ]
}

fn pick<T: Copy>(items: &[T], k: u8) -> Option<T> {
    (!items.is_empty()).then(|| items[k as usize % items.len()])
}

const TAGS: [&str; 3] = ["Legs", "Top", "Jig"];

/// Turn an intent into a concrete op against the current document.
fn resolve(f: &Fuzz, d: &Document) -> Op {
    let m = d.model();
    let sketches = m.sketch_ids();
    let objects = m.object_ids();
    let sketch = |k: u8| pick(&sketches, k).unwrap_or(1);
    let object = |k: u8| pick(&objects, k).unwrap_or(1);
    // Outer ring (world) and plane of a face, if it exists.
    // Faces are addressed by persistent id: pick the k-th face, send its id.
    let face = |o: u64, k: u8| -> Option<(u64, Vec<DVec3>, sawhorse_kernel::plane::Plane)> {
        let ob = m.object(o)?;
        let p = ob.brep().to_polys();
        if p.faces.is_empty() {
            return None;
        }
        let fi = k as usize % p.faces.len();
        let ring = p.faces[fi].outer.iter().map(|&i| p.points[i]).collect();
        Some((ob.face_id_at(fi)?, ring, p.plane(fi)?))
    };
    match f {
        Fuzz::NewSketch(o, n) => Op::NewSketch {
            origin: *o,
            normal: *n,
        },
        Fuzz::Rect(s, x, y, w, h) => Op::AddRectangle {
            sketch: sketch(*s),
            x: *x,
            y: *y,
            w: *w,
            h: *h,
        },
        Fuzz::Segment(s, a, b) => Op::InsertSegment {
            sketch: sketch(*s),
            a: *a,
            b: *b,
        },
        Fuzz::Erase(s, a, b) => Op::EraseSegment {
            sketch: sketch(*s),
            a: *a,
            b: *b,
        },
        Fuzz::Polyline(s, p, closed) => Op::AddPolyline {
            sketch: sketch(*s),
            points: p.clone(),
            closed: *closed,
        },
        Fuzz::Extrude(s, r, distance) => {
            let sid = sketch(*s);
            let regions: Vec<u64> = m
                .sketch(sid)
                .map(|sk| {
                    sk.regions()
                        .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
                        .collect()
                })
                .unwrap_or_default();
            Op::Extrude {
                sketch: sid,
                region: pick(&regions, *r).unwrap_or(1),
                distance: *distance,
            }
        }
        Fuzz::Push(o, k, distance) => {
            let oid = object(*o);
            let face = m
                .object(oid)
                .and_then(|ob| {
                    let n = ob.brep().face_count().max(1);
                    ob.face_id_at(*k as usize % n)
                })
                .unwrap_or(1);
            Op::PushPull {
                object: oid,
                face,
                distance: *distance,
            }
        }
        Fuzz::Split {
            object: o,
            face: k,
            e1,
            t1,
            e2,
            t2,
            bend,
        } => {
            let oid = object(*o);
            let Some((fi, ring, _)) = face(oid, *k) else {
                return Op::SplitFace {
                    object: oid,
                    face: 0,
                    path: vec![],
                };
            };
            let n = ring.len();
            let (i, j) = (
                *e1 as usize % n,
                (*e1 as usize + 1 + *e2 as usize % (n - 1)) % n,
            );
            let a = ring[i].lerp(ring[(i + 1) % n], *t1);
            let b = ring[j].lerp(ring[(j + 1) % n], *t2);
            let centre = ring.iter().copied().sum::<DVec3>() / n as f64;
            let mut path = vec![a.to_array()];
            if *bend > 0.0 {
                path.push(a.lerp(b, 0.5).lerp(centre, *bend).to_array());
            }
            path.push(b.to_array());
            Op::SplitFace {
                object: oid,
                face: fi,
                path,
            }
        }
        Fuzz::Imprint {
            object: o,
            face: k,
            at,
            size,
            triangle,
        } => {
            let oid = object(*o);
            let Some((fi, ring, plane)) = face(oid, *k) else {
                return Op::Imprint {
                    object: oid,
                    face: 0,
                    points: vec![],
                };
            };
            let q: Vec<DVec2> = ring.iter().map(|&p| plane.to_local(p)).collect();
            let lo = q
                .iter()
                .copied()
                .fold(DVec2::splat(f64::INFINITY), DVec2::min);
            let hi = q
                .iter()
                .copied()
                .fold(DVec2::splat(f64::NEG_INFINITY), DVec2::max);
            let span = hi - lo;
            let p0 = lo + span * DVec2::new(at[0], at[1]) * 0.8 + span * 0.05;
            let p1 = p0 + span * DVec2::new(size[0], size[1]) * 0.4;
            let corners: Vec<DVec2> = if *triangle {
                vec![p0, DVec2::new(p1.x, p0.y), DVec2::new(p0.x, p1.y)]
            } else {
                vec![p0, DVec2::new(p1.x, p0.y), p1, DVec2::new(p0.x, p1.y)]
            };
            Op::Imprint {
                object: oid,
                face: fi,
                points: corners
                    .iter()
                    .map(|&c| plane.to_world(c).to_array())
                    .collect(),
            }
        }
        Fuzz::EraseEdge(o, k, e) => {
            let oid = object(*o);
            match face(oid, *k) {
                Some((_, ring, _)) => {
                    let i = *e as usize % ring.len();
                    Op::EraseEdge {
                        object: oid,
                        a: ring[i].to_array(),
                        b: ring[(i + 1) % ring.len()].to_array(),
                    }
                }
                None => Op::EraseEdge {
                    object: oid,
                    a: [0.0; 3],
                    b: [1.0, 0.0, 0.0],
                },
            }
        }
        Fuzz::EraseFace(o, k) => {
            let oid = object(*o);
            Op::EraseFace {
                object: oid,
                face: face(oid, *k).map_or(0, |(id, _, _)| id),
            }
        }
        Fuzz::EraseRegion(s, r) => {
            let sid = sketch(*s);
            let regions: Vec<u64> = m
                .sketch(sid)
                .map(|sk| {
                    sk.regions()
                        .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
                        .collect()
                })
                .unwrap_or_default();
            Op::EraseRegion {
                sketch: sid,
                region: pick(&regions, *r).unwrap_or(1),
            }
        }
        Fuzz::Batch(fs) => Op::Batch {
            ops: fs.iter().map(|f| resolve(f, d)).collect(),
        },
        Fuzz::Delete(o) => Op::DeleteObject { object: object(*o) },
        Fuzz::Group(ks) => Op::CreateGroup {
            name: "Fuzz".into(),
            objects: ks.iter().map(|&k| object(k)).collect(),
        },
        Fuzz::Tag(o, t) => Op::SetTag {
            object: object(*o),
            tag: t.map(|t| TAGS[t as usize].to_string()),
        },
        Fuzz::TagVisible(t, v) => Op::SetTagVisible {
            tag: TAGS[*t as usize].to_string(),
            visible: *v,
        },
        Fuzz::Undo => Op::Undo,
        Fuzz::Redo => Op::Redo,
        Fuzz::Raw(op) => op.clone(),
    }
}

/// Resolve a whole sequence (each intent against the document the earlier ops built).
/// A panic while resolving (a kernel bug hit before `check_sequence` runs) stops the
/// sequence there and is returned with the ops so far, so it still becomes a repro.
fn concrete(intents: &[Fuzz], tally: &mut Tally) -> (Vec<Op>, Option<String>) {
    let mut d = Document::new();
    let mut ops = vec![Op::NewSketch {
        origin: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
    }];
    let _ = d.apply(&ops[0]);
    for f in intents {
        let op = resolve(f, &d);
        ops.push(op.clone());
        let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.apply(&op)));
        if let Ok(r) = &step {
            tally.record(&op, r);
        }
        if let Err(p) = step {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "non-string panic".into());
            let at = ops.len() - 1;
            return (ops, Some(format!("op {at} {op:?} panicked: {msg}")));
        }
    }
    (ops, None)
}

fn repro_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("repro")
}

/// Per-op-kind accepted / refused counts, and the most common refusal reasons.
#[derive(Default)]
struct Tally {
    kinds: std::collections::BTreeMap<String, (usize, usize)>,
    reasons: std::collections::BTreeMap<String, usize>,
}

impl Tally {
    fn record(
        &mut self,
        op: &Op,
        r: &Result<sawhorse_kernel::OpOutcome, sawhorse_kernel::KernelError>,
    ) {
        let json = serde_json::to_value(op).unwrap_or_default();
        let kind = json["op"].as_str().unwrap_or("?").to_string();
        let e = self.kinds.entry(kind.clone()).or_default();
        match r {
            Ok(_) => e.0 += 1,
            Err(err) => {
                e.1 += 1;
                let reason: String = err.to_string().chars().take(70).collect();
                *self.reasons.entry(format!("{kind}: {reason}")).or_default() += 1;
            }
        }
    }

    fn report(&self) -> String {
        let mut out = String::from(
            "  op kind          accepted   refused
",
        );
        for (k, (a, r)) in &self.kinds {
            out += &format!(
                "  {k:<16} {a:>8} {r:>9}
"
            );
        }
        let mut top: Vec<(&String, &usize)> = self.reasons.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1));
        out += "  top refusals:
";
        for (reason, n) in top.into_iter().take(40) {
            out += &format!(
                "  {n:>7}  {reason}
"
            );
        }
        out
    }
}

#[derive(Default)]
struct Totals {
    sequences: usize,
    accepted: usize,
    refused: usize,
    objects: usize,
}

fn run(name: &str, cases: u32, max_len: usize, seed: Option<u64>) {
    let config = Config {
        cases,
        failure_persistence: None,
        ..Config::default()
    };
    let mut runner = match seed {
        Some(s) => {
            let mut bytes = [0u8; 32];
            bytes[..8].copy_from_slice(&s.to_le_bytes());
            TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, &bytes))
        }
        None => TestRunner::new(config),
    };
    let totals = std::cell::RefCell::new(Totals::default());
    let tally = std::cell::RefCell::new(Tally::default());
    let started = std::time::Instant::now();
    // Most sequences start from a solid (a rectangle pulled up), so face ops, push/pull and
    // booleans see real geometry; the rest start from an empty ground sketch.
    let size = || prop_oneof![(1i32..9).prop_map(|k| f64::from(k) * 0.25), 0.1..2.5f64];
    let start = prop_oneof![
        3 => (grid(), grid(), size(), size(), size(), any::<bool>()).prop_map(|(x, y, w, h, d, down)| vec![
            Fuzz::Rect(0, x, y, w, h),
            Fuzz::Extrude(0, 0, if down { -d } else { d }),
        ]),
        1 => Just(Vec::new()),
    ];
    let strategy =
        (start, prop::collection::vec(fuzz_op(), 1..max_len)).prop_map(|(mut s, rest)| {
            s.extend(rest);
            s
        });
    let result = runner.run(&strategy, |intents| {
        let (ops, panicked) = concrete(&intents, &mut tally.borrow_mut());
        let checked = match panicked {
            Some(e) => Err(e),
            None => check_sequence(&ops),
        };
        match checked {
            Ok(s) => {
                let mut t = totals.borrow_mut();
                t.sequences += 1;
                t.accepted += s.accepted;
                t.refused += s.refused;
                t.objects += s.objects;
                Ok(())
            }
            Err(e) => {
                let _ = write_repro(&repro_dir(), &format!("pending-{name}"), &e, &ops);
                Err(TestCaseError::fail(e))
            }
        }
    });
    let t = totals.borrow();
    println!(
        "{name}: {} sequences, {} ops accepted, {} refused, {} solids at the end, {:.1}s",
        t.sequences,
        t.accepted,
        t.refused,
        t.objects,
        started.elapsed().as_secs_f64()
    );
    if std::env::var("FUZZ_REPORT").is_ok_and(|v| v == "1") {
        println!("{}", tally.borrow().report());
    }
    if let Err(e) = result {
        panic!(
            "{e}\nminimal failing sequence written to {}",
            repro_dir().join(format!("pending-{name}.json")).display()
        );
    }
}

#[test]
fn quick_fuzz() {
    run("quick_fuzz", 192, 24, None);
}

/// Long run for CI nightlies and before releases. FUZZ_CASES sets the case count,
/// FUZZ_SEED a deterministic seed (one per parallel shard).
#[test]
#[ignore]
fn long_fuzz() {
    let cases = std::env::var("FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    let seed: Option<u64> = std::env::var("FUZZ_SEED").ok().and_then(|v| v.parse().ok());
    let name = match seed {
        Some(s) => format!("long_fuzz_seed{s}"),
        None => "long_fuzz".to_string(),
    };
    run(&name, cases, 48, seed);
}
