//! The shortest push/pull, extrude and offset (lane 53, #66).
//!
//! The shortest distance is MIN_EXTRUDE (0.01 mm, the merge distance). An extrude of exactly
//! that was refused as a kernel bug: its walls are exactly the merge distance tall, so the
//! validator saw a vertex on the base face (on the ground) or, once the corners were placed
//! in world coordinates and rounded, a zero-length wall (500 mm up); 1000 mm up the rounding
//! went the other way and it extruded. The same rounding reaches a hair over the minimum.
//! Now an extrude of exactly the minimum is refused on every plane ("an extrude must be
//! more than 0.01 mm"), and any push/pull, extrude or offset within a nanometre over it that
//! fails as the minimum makes things fail, while twice the minimum goes through, is refused
//! as "too short here", never a kernel bug and never a refusal about degenerate geometry or
//! paths crossing a face's boundary; under the minimum the refusals say "at least 0.01 mm"
//! as before (a plain push/pull of exactly 0.01 mm goes through). Everything longer is as it
//! was, and any other failure at the minimum keeps its own message.

use proptest::prelude::*;
use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::offset::OffsetSource;
use sawhorse_kernel::tolerance::{MIN_EXTRUDE, MIN_EXTRUDE_SLACK};
use sawhorse_kernel::{Document, KernelError, OffsetError, PushError};

const UNDER: KernelError = KernelError::Push(PushError::ZeroDistance);
const EXTRUDE_SHORT: KernelError = KernelError::Push(PushError::ExtrudeTooShort);
const HERE: KernelError = KernelError::Push(PushError::TooShortHere);
const OFFSET_UNDER: KernelError = KernelError::Offset(OffsetError::ZeroDistance);
const OFFSET_HERE: KernelError = KernelError::Offset(OffsetError::TooShortHere);

/// Which op is judged, for the refusals it may give.
#[derive(Clone, Copy)]
enum Kind {
    Extrude,
    Push,
    Offset,
}

impl Kind {
    /// The one refusal this op may give at `d` near the minimum.
    fn refusal(self, d: f64) -> KernelError {
        let a = d.abs();
        match self {
            Kind::Extrude if a <= MIN_EXTRUDE => EXTRUDE_SHORT,
            Kind::Push if a < MIN_EXTRUDE => UNDER,
            Kind::Offset if a < MIN_EXTRUDE => OFFSET_UNDER,
            Kind::Offset => OFFSET_HERE,
            _ => HERE,
        }
    }
}

fn sketch(m: &mut Model, origin: [f64; 3], normal: [f64; 3]) -> u64 {
    match m.apply(&Op::NewSketch { origin, normal }).unwrap() {
        OpOutcome::SketchCreated { sketch } => sketch,
        o => panic!("{o:?}"),
    }
}

/// A model with one `w` x `h` rectangle (region 1) on a new sketch on the given plane.
fn with_rect(origin: [f64; 3], normal: [f64; 3], x: f64, w: f64, h: f64) -> (Model, u64) {
    let mut m = Model::new();
    let s = sketch(&mut m, origin, normal);
    m.apply(&Op::AddRectangle {
        sketch: s,
        x,
        y: 0.0,
        w,
        h,
    })
    .unwrap();
    (m, s)
}

fn extrude(s: u64, distance: f64) -> Op {
    Op::Extrude {
        sketch: s,
        region: 1,
        distance,
    }
}

/// The issue's numbers: a 400 x 100 mm rectangle on the ground, 500 mm up and 1000 mm up,
/// both ways. Exactly 0.01 mm is refused as too short everywhere and changes nothing;
/// 0.0100001 mm and 0.02 mm extrude everywhere, by exactly that distance.
#[test]
fn the_issue_s_cases() {
    for z in [0.0, 0.5, 1.0] {
        for sign in [1.0, -1.0] {
            let (m, s) = with_rect([0.0, 0.0, z], [0.0, 0.0, 1.0], 0.0, 0.4, 0.1);
            let mut refused = m.clone();
            let r = refused.apply(&extrude(s, sign * 1e-5));
            assert_eq!(r, Err(EXTRUDE_SHORT), "z {z}, sign {sign}");
            assert_eq!(
                r.unwrap_err().to_string(),
                "an extrude must be more than 0.01 mm"
            );
            assert_eq!(refused.hash(), m.hash(), "a refusal changes nothing");
            for d in [1.00001e-5, 2e-5] {
                let mut ok = m.clone();
                let out = ok.apply(&extrude(s, sign * d));
                assert!(
                    matches!(out, Ok(OpOutcome::ObjectCreated { .. })),
                    "{out:?}"
                );
                let o = ok.object(1).unwrap();
                assert!((o.volume() - 0.4 * 0.1 * d).abs() <= 1e-9 * 0.4 * 0.1 * d + 1e-18);
                let (lo, hi) = o.bounds();
                let t = hi.z - lo.z;
                assert!((t - d).abs() < 1e-12, "z {z}: {t} thick for {d}");
            }
        }
    }
}

/// Through the document (what the app and the WASM kernel use): the refusal leaves the
/// undo history and the hash alone.
#[test]
fn the_document_keeps_its_history_on_the_refusal() {
    let mut d = Document::with_ground_sketch();
    d.apply(&Op::AddRectangle {
        sketch: 1,
        x: 0.0,
        y: 0.0,
        w: 0.4,
        h: 0.1,
    })
    .unwrap();
    let (hash, undo) = (d.model().hash(), d.can_undo());
    let e = d.apply(&extrude(1, 1e-5)).unwrap_err();
    assert_eq!(e, EXTRUDE_SHORT);
    assert!(!e.is_bug());
    assert_eq!((d.model().hash(), d.can_undo()), (hash, undo));
}

/// A plain push/pull of exactly the minimum only slides the face: it went through before on
/// every plane tried, and still does, by exactly that distance.
#[test]
fn a_push_of_exactly_the_minimum_still_slides_the_face() {
    for z in [0.0, 0.5, 1.0, 1234.5] {
        let (mut m, s) = with_rect([0.0, 0.0, z], [0.0, 0.0, 1.0], 0.0, 0.4, 0.1);
        m.apply(&extrude(s, 0.1)).unwrap();
        for d in [1e-5, -1e-5] {
            let mut p = m.clone();
            p.apply(&Op::PushPull {
                object: 1,
                face: 2,
                distance: d,
            })
            .unwrap();
            let v = p.object(1).unwrap().volume();
            assert!((v - 0.4 * 0.1 * (0.1 + d)).abs() < 1e-12, "z {z}: {v}");
        }
    }
}

fn normals() -> Vec<[f64; 3]> {
    vec![
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.6, 0.8],
        [0.3, -0.2, 0.9],
        [0.577, 0.577, 0.577],
    ]
}

/// The distances around the minimum: a few doubles under it, exactly it, the next 40
/// doubles over it, steps of 1e-13 m up to 3e-12 m over it (the rounding measured reaches
/// 1.4e-12 m at 10 km), the edge of the band and just past it, and 0.0100001 mm.
fn distances() -> Vec<f64> {
    let mut ds = vec![MIN_EXTRUDE.next_down().next_down(), MIN_EXTRUDE.next_down()];
    let mut d = MIN_EXTRUDE;
    for _ in 0..41 {
        ds.push(d);
        d = d.next_up();
    }
    for k in 1..=30 {
        ds.push(MIN_EXTRUDE + k as f64 * 1e-13);
    }
    ds.push(MIN_EXTRUDE + MIN_EXTRUDE_SLACK);
    ds.push((MIN_EXTRUDE + MIN_EXTRUDE_SLACK).next_up());
    ds.push(1.00001e-5);
    ds
}

/// What one op did at one distance, judged against the rules: never a kernel bug; only the
/// one refusal for its kind and distance; under the minimum (and an extrude at it) refused;
/// past the band, through.
fn judge(what: &str, m: &Model, op: &Op, d: f64, kind: Kind) -> bool {
    let mut mm = m.clone();
    let r = mm.apply(op);
    let extrude = matches!(kind, Kind::Extrude);
    if let Err(e) = &r {
        assert!(!e.is_bug(), "{what} at {d:e}: {e}");
        assert_eq!(
            *e,
            kind.refusal(d),
            "{what} at {d:e}: only the plain refusal at the minimum"
        );
        assert_eq!(
            mm.hash(),
            m.hash(),
            "{what} at {d:e}: a refusal changes nothing"
        );
    }
    let a = d.abs();
    if a < MIN_EXTRUDE || (extrude && a == MIN_EXTRUDE) {
        assert!(r.is_err(), "{what} at {d:e} went through");
    }
    if a > MIN_EXTRUDE + MIN_EXTRUDE_SLACK {
        assert!(r.is_ok(), "{what} at {d:e}: {:?}", r.err());
    }
    r.is_ok()
}

/// The sweep: six planes (level both ways, upright, two sloped, the diagonal) from the
/// origin to 9 km out along their normal, a 0.4 x 0.1 m rectangle and an 8 km long one,
/// both signs, every distance above. Extrude, push/pull the cap, push/pull it with Ctrl
/// (new walls exactly the distance tall), offset the cap (an imprint), offset the region (a
/// sketch).
#[test]
fn every_plane_and_distance_near_the_minimum() {
    let mut refused = 0;
    let mut through = 0;
    for n in normals() {
        for h in [0.0, 0.5, 1.0, 37.0, 1234.5, 4999.0] {
            for (x0, w) in [(0.0, 0.4), (-4000.0, 8000.0)] {
                let o = [h * n[0], h * n[1], h * n[2]];
                let (base, s) = with_rect(o, n, x0, w, 0.1);
                let mut boxed = base.clone();
                boxed.apply(&extrude(s, 0.1)).unwrap();
                for d in distances() {
                    for d in [d, -d] {
                        let cases: [(&str, &Model, Op, Kind); 5] = [
                            ("extrude", &base, extrude(s, d), Kind::Extrude),
                            (
                                "push",
                                &boxed,
                                Op::PushPull {
                                    object: 1,
                                    face: 2,
                                    distance: d,
                                },
                                Kind::Push,
                            ),
                            (
                                "ctrl push",
                                &boxed,
                                Op::PushPullNewFace {
                                    object: 1,
                                    face: 2,
                                    distance: d,
                                },
                                Kind::Push,
                            ),
                            (
                                "offset face",
                                &boxed,
                                Op::Offset {
                                    source: OffsetSource::Face { object: 1, face: 2 },
                                    distance: d,
                                },
                                Kind::Offset,
                            ),
                            (
                                "offset region",
                                &base,
                                Op::Offset {
                                    source: OffsetSource::Region {
                                        sketch: s,
                                        region: 1,
                                    },
                                    distance: d,
                                },
                                Kind::Offset,
                            ),
                        ];
                        for (what, m, op, kind) in cases {
                            let what = format!("{what} n={n:?} h={h} w={w}");
                            if judge(&what, m, &op, d, kind) {
                                through += 1;
                            } else {
                                refused += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    // Both kinds of answer were exercised.
    assert!(refused > 1000 && through > 1000, "{refused} {through}");
}

proptest! {
    /// Any plane within the coordinate limit, any rectangle on it, any distance from a few
    /// doubles under the minimum to 3 nm over it: never a kernel bug, only the plain
    /// refusal, exactly the minimum refused for an extrude, and through past the band.
    #[test]
    fn any_plane_near_the_minimum(
        nx in -1.0f64..1.0, ny in -1.0f64..1.0, nz in -1.0f64..1.0,
        h in -9000.0f64..9000.0,
        x0 in -500.0f64..500.0, w in 0.001f64..400.0, hh in 0.001f64..2.0,
        step in prop_oneof![
            (-3i64..60).prop_map(|k| k as f64),
            (0i64..30_000).prop_map(|k| k as f64 * 1e-4),
        ],
        ulps in any::<bool>(),
        sign in prop_oneof![Just(1.0), Just(-1.0)],
    ) {
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        prop_assume!(len > 0.1);
        let n = [nx / len, ny / len, nz / len];
        let o = [h * n[0], h * n[1], h * n[2]];
        let mut base = Model::new();
        let s = sketch(&mut base, o, n);
        prop_assume!(base
            .apply(&Op::AddRectangle { sketch: s, x: x0, y: 0.0, w, h: hh })
            .is_ok());
        // `step` doubles from the minimum, or `step` x 1e-13 m over it.
        let d = if ulps {
            let mut d = MIN_EXTRUDE;
            let k = step as i64;
            for _ in 0..k.abs() {
                d = if k > 0 { d.next_up() } else { d.next_down() };
            }
            d
        } else {
            MIN_EXTRUDE + step * 1e-13
        };
        let d = sign * d;
        judge("extrude", &base, &extrude(s, d), d, Kind::Extrude);
        let mut boxed = base.clone();
        prop_assume!(boxed.apply(&extrude(s, 0.05)).is_ok());
        for op in [
            Op::PushPull { object: 1, face: 2, distance: d },
            Op::PushPullNewFace { object: 1, face: 2, distance: d },
        ] {
            judge("push", &boxed, &op, d, Kind::Push);
        }
        let off = Op::Offset { source: OffsetSource::Face { object: 1, face: 2 }, distance: d };
        // An inward offset of a rectangle narrower than twice the distance vanishes.
        if hh > 1e-4 && w > 1e-4 {
            judge("offset", &boxed, &off, d, Kind::Offset);
        }
    }
}

/// The texts, each true where it is shown: under the minimum "at least 0.01 mm" (a plain
/// push/pull of exactly 0.01 mm goes through, above), an extrude at it "more than", and a
/// push/pull that cannot be built at the minimum "too short here".
#[test]
fn each_refusal_says_what_is_true_for_it() {
    let (mut m, s) = with_rect([0.0, 0.0, 0.5], [0.0, 0.0, 1.0], 0.0, 0.4, 0.1);
    let mut e = m.clone();
    assert_eq!(
        e.apply(&extrude(s, 5e-6)).unwrap_err().to_string(),
        "an extrude must be more than 0.01 mm"
    );
    m.apply(&extrude(s, 0.1)).unwrap();
    let mut p = m.clone();
    let push = |d: f64| Op::PushPull {
        object: 1,
        face: 2,
        distance: d,
    };
    assert_eq!(
        p.apply(&push(5e-6)).unwrap_err().to_string(),
        "distance must be at least 0.01 mm"
    );
    p.apply(&push(1e-5)).unwrap();
    // 500 mm up, a Ctrl push/pull of exactly 0.01 mm rounds its new walls under 0.01 mm.
    let mut c = m.clone();
    let e = c
        .apply(&Op::PushPullNewFace {
            object: 1,
            face: 2,
            distance: 1e-5,
        })
        .unwrap_err();
    assert_eq!(e, HERE);
    assert!(e.to_string().starts_with("too short here"), "{e}");
    let mut o = m.clone();
    assert_eq!(
        o.apply(&Op::Offset {
            source: OffsetSource::Face { object: 1, face: 2 },
            distance: 5e-6
        })
        .unwrap_err()
        .to_string(),
        "offset distance must be at least 0.01 mm"
    );
}

/// Finding the same inside a raw batch (a plugin's or the MCP server's ops, as the kernel
/// takes them): a Ctrl push/pull and an inward offset at the minimum that cannot be built
/// get "too short here" too, and the batch leaves nothing behind.
#[test]
fn inside_a_batch_too() {
    let guide = Op::AddGuide {
        point: [0.0, 0.0, 0.0],
        dir: None,
    };
    let ctrl = Op::PushPullNewFace {
        object: 1,
        face: 2,
        distance: 1e-5,
    };
    let inward = Op::Offset {
        source: OffsetSource::Face { object: 1, face: 2 },
        distance: -1e-5,
    };
    let mut seen = [0, 0];
    for n in normals() {
        for h in [0.0, 0.5, 1.0, 37.0, 1234.5] {
            let o = [h * n[0], h * n[1], h * n[2]];
            let (mut m, s) = with_rect(o, n, 0.0, 0.4, 0.1);
            m.apply(&extrude(s, 0.1)).unwrap();
            let hash = m.hash();
            for (k, (op, want)) in [(&ctrl, HERE), (&inward, OFFSET_HERE)]
                .into_iter()
                .enumerate()
            {
                // Only where the op alone is refused as too short here (that depends on how
                // the corners round on this plane).
                if m.clone().apply(op) != Err(want.clone()) {
                    continue;
                }
                seen[k] += 1;
                for ops in [
                    vec![guide.clone(), op.clone()],
                    vec![op.clone(), guide.clone()],
                ] {
                    let mut b = m.clone();
                    assert_eq!(
                        b.apply(&Op::Batch { ops }),
                        Err(want.clone()),
                        "{op:?} {n:?} {h}"
                    );
                    assert_eq!(b.hash(), hash, "a refused batch changes nothing");
                }
            }
        }
    }
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
    // An extrude at exactly the minimum in a batch: the extrude's own refusal.
    let (mut b, s) = with_rect([0.0, 0.0, 0.5], [0.0, 0.0, 1.0], 0.0, 0.4, 0.1);
    assert_eq!(
        b.apply(&Op::Batch {
            ops: vec![guide, extrude(s, 1e-5)]
        }),
        Err(EXTRUDE_SHORT)
    );
}

/// Any other failure at the minimum keeps its own message: a push/pull of 0.01 mm of a face
/// that is not there, or of a solid inside a group that is not open, alone and in a batch.
#[test]
fn other_failures_at_the_minimum_keep_their_own_message() {
    let (mut m, s) = with_rect([0.0, 0.0, 0.5], [0.0, 0.0, 1.0], 0.0, 0.4, 0.1);
    m.apply(&extrude(s, 0.1)).unwrap();
    let mut a = m.clone();
    assert_eq!(
        a.apply(&Op::PushPullNewFace {
            object: 1,
            face: 99,
            distance: 1e-5,
        }),
        Err(KernelError::Push(PushError::NoSuchFace(99)))
    );
    m.apply(&Op::CreateGroup {
        name: "G".into(),
        objects: vec![1],
    })
    .unwrap();
    for op in [
        Op::PushPullNewFace {
            object: 1,
            face: 2,
            distance: 1e-5,
        },
        Op::Offset {
            source: OffsetSource::Face { object: 1, face: 2 },
            distance: -1e-5,
        },
    ] {
        let mut g = m.clone();
        let e = g.apply(&op).unwrap_err();
        assert!(e != HERE && e != OFFSET_HERE && !e.is_bug(), "{e}");
        let mut b = m.clone();
        assert_eq!(b.apply(&Op::Batch { ops: vec![op] }), Err(e));
    }
}

/// The extrude's checks stay in main's order: a shape whose cap would be beyond 10 km says so
/// at exactly the minimum too (a sketch on the plane x = 10 km, pulled out by 0.01 mm), and
/// the same shape pulled back in by 0.01 mm is too short.
#[test]
fn beyond_10_km_is_said_before_too_short() {
    let (m, s) = with_rect([1.0e4, 0.0, 0.0], [1.0, 0.0, 0.0], 0.0, 0.4, 0.1);
    let mut out = m.clone();
    assert_eq!(
        out.apply(&extrude(s, 1e-5)),
        Err(KernelError::Push(PushError::OutOfBounds))
    );
    let mut back = m.clone();
    assert_eq!(back.apply(&extrude(s, -1e-5)), Err(EXTRUDE_SHORT));
}

/// A plugin's or the MCP server's batch (`refbatch::apply`, which first applies each op on its
/// own to resolve the placeholders) refuses a Ctrl push/pull at the minimum that cannot be
/// built here as too short here too, naming the op, and changes nothing.
#[test]
fn a_plugin_batch_too() {
    use sawhorse_kernel::refbatch::{NamedBatch, RefBatchError, apply};
    let mut seen = 0;
    for n in normals() {
        for h in [0.0, 0.5, 1.0, 37.0] {
            let o = [h * n[0], h * n[1], h * n[2]];
            let (mut m, s) = with_rect(o, n, 0.0, 0.4, 0.1);
            m.apply(&extrude(s, 0.1)).unwrap();
            let ctrl = Op::PushPullNewFace {
                object: 1,
                face: 2,
                distance: 1e-5,
            };
            if m.clone().apply(&ctrl) != Err(HERE) {
                continue;
            }
            seen += 1;
            let mut d = Document::open(m.snapshot()).unwrap();
            let hash = d.hash();
            let batch = NamedBatch {
                name: "Plugin".into(),
                ops: vec![serde_json::to_value(&ctrl).unwrap()],
            };
            match apply(&mut d, &batch) {
                Err(RefBatchError::Refused {
                    index: 0, error, ..
                }) => assert_eq!(*error, HERE),
                other => panic!("{other:?}"),
            }
            assert_eq!(d.hash(), hash);
        }
    }
    assert!(seen > 0);
}

/// An op at the minimum whose failure is the minimum's kind but which fails at twice the
/// minimum too keeps its own error, alone and in a batch: the doubled op is what says the
/// distance alone is to blame. A face a few hundredths of a millimetre wide, offset inward
/// by 0.01 mm (its loop comes too close to the far edge) and by 0.02 mm (nothing is left).
#[test]
fn an_op_that_fails_at_twice_the_minimum_too_keeps_its_error() {
    let mut seen = 0;
    for k in 0..40 {
        let w = 2.05e-5 + k as f64 * 2.5e-7;
        let (mut m, s) = with_rect([0.0, 0.0, 0.5], [0.0, 0.0, 1.0], 0.0, 0.4, w);
        if m.apply(&extrude(s, 0.1)).is_err() {
            continue;
        }
        let off = |d: f64| Op::Offset {
            source: OffsetSource::Face { object: 1, face: 2 },
            distance: d,
        };
        let (Err(at), Err(_)) = (m.clone().apply(&off(-1e-5)), m.clone().apply(&off(-2e-5))) else {
            continue;
        };
        if !matches!(at, KernelError::FaceEdit(_)) {
            continue;
        }
        seen += 1;
        assert_ne!(at, OFFSET_HERE, "w {w}");
        let guide = Op::AddGuide {
            point: [0.0, 0.0, 0.0],
            dir: None,
        };
        assert_eq!(
            m.clone().apply(&Op::Batch {
                ops: vec![guide, off(-1e-5)]
            }),
            Err(at)
        );
    }
    assert!(seen > 0, "no face that thin failed both ways");
}

/// A band extrude (a hair over 0.01 mm, 500 mm up, where the placed walls round under 0.01
/// mm) builds a solid the validator refuses, and that refusal is judged as every other op's:
/// too short here alone and inside a batch, not a kernel bug.
#[test]
fn a_band_extrude_is_judged_as_every_other_op() {
    let d = MIN_EXTRUDE.next_up();
    let (m, s) = with_rect([0.0, 0.0, 0.5], [0.0, 0.0, 1.0], 0.0, 0.4, 0.1);
    assert_eq!(m.clone().apply(&extrude(s, d)), Err(HERE));
    let guide = Op::AddGuide {
        point: [0.0, 0.0, 0.0],
        dir: None,
    };
    for ops in [
        vec![guide.clone(), extrude(s, d)],
        vec![extrude(s, d), guide],
    ] {
        assert_eq!(m.clone().apply(&Op::Batch { ops }), Err(HERE));
    }
}
