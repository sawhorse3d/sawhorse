//! A stroke longer than a sketch may hold edges (lane 53, #65).
//!
//! The sketch's limit (MAX_SKETCH_EDGES, 20,000) is checked when the edit is done, and
//! inserting a stroke costs more than its length, so a stroke the limit will refuse used to
//! hold the kernel for seconds first (the issue: 24,000 points, 4.5 s in the WASM kernel,
//! then "a sketch would hold 23999 edges; at most 20000"). A stroke whose count is certain
//! without inserting it (`Sketch::edges_after_clear_polyline`) is now refused before it is
//! inserted, with the same refusal, when it is the last op of the edit; every other stroke
//! goes in as before.

use sawhorse_kernel::model::{DrawTarget, Op};
use sawhorse_kernel::{Document, KernelError};
use std::sync::mpsc;
use std::time::Duration;

/// The issue's stroke: an open zigzag on the ground, a millimetre a step.
fn zigzag(n: usize) -> Vec<[f64; 3]> {
    (0..n)
        .map(|k| [0.001 * k as f64, 0.0005 * (k % 2) as f64, 0.0])
        .collect()
}

fn draw(points: Vec<[f64; 3]>) -> Op {
    Op::Draw {
        target: DrawTarget::Plane {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        },
        points,
        closed: false,
        curve: None,
    }
}

/// Apply `op` to a document holding a 400 x 100 mm rectangle (in the ground sketch, far
/// from the zigzag) on another thread; the refusal, or a failure after `secs` seconds.
/// Before the fix the 24,000-point stroke took seconds in a release build and the
/// 200,000-point one would take hours; the bound is generous so a loaded machine passes.
fn refused_within(op: Op, secs: u64) -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut d = Document::with_ground_sketch();
        d.apply(&Op::AddRectangle {
            sketch: 1,
            x: -1.0,
            y: -1.0,
            w: 0.4,
            h: 0.1,
        })
        .unwrap();
        let (hash, undo) = (d.hash(), d.can_undo());
        let r = d.apply(&op);
        let _ = tx.send((r, d.hash() == hash && d.can_undo() == undo));
    });
    let (r, unchanged) = rx
        .recv_timeout(Duration::from_secs(secs))
        .unwrap_or_else(|_| panic!("the stroke was not refused within {secs} s"));
    assert!(unchanged, "a refusal changes nothing");
    match r {
        Err(e @ KernelError::Limit(_)) => e.to_string(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_issue_s_zigzag_is_refused_before_it_is_inserted() {
    // The sketch holds the rectangle's 4 edges; the zigzag adds one edge a step.
    for (n, edges) in [
        (24_000, 4 + 23_999),
        (32_000, 4 + 31_999),
        (200_000, 4 + 199_999),
    ] {
        let want = format!("a sketch would hold {edges} edges; at most 20000");
        assert_eq!(refused_within(draw(zigzag(n)), 120), want);
        // The same as the last op of a batch (a plugin's or the MCP server's apply_ops).
        let batch = Op::Batch {
            ops: vec![
                Op::AddGuide {
                    point: [0.0, 0.0, 0.0],
                    dir: None,
                },
                draw(zigzag(n)),
            ],
        };
        assert_eq!(refused_within(batch, 120), want);
        // In the sketch's own frame.
        let poly = Op::AddPolyline {
            sketch: 1,
            points: zigzag(n).iter().map(|p| [p[0], p[1]]).collect(),
            closed: false,
        };
        assert_eq!(refused_within(poly, 120), want);
    }
}

/// Release-only checks of the strokes at and around the limit that must be inserted to be
/// judged (minutes in a debug build): `cargo test --release -p sawhorse-kernel --test
/// stroke_limit -- --ignored`.
#[test]
#[ignore]
fn at_the_limit_and_inside_a_batch() {
    let mut d = Document::with_ground_sketch();
    // 20,001 points make exactly 20,000 edges: allowed.
    d.apply(&draw(zigzag(20_001))).unwrap();
    assert_eq!(d.model().sketch(1).unwrap().edge_count(), 20_000);
    // 20,002 points: one over, refused before it is inserted, with its count.
    let mut d = Document::with_ground_sketch();
    let e = d.apply(&draw(zigzag(20_002))).unwrap_err();
    assert_eq!(
        e.to_string(),
        "a sketch would hold 20001 edges; at most 20000"
    );
    // Not the last op of its batch: inserted, and the ops after it can take edges away. A
    // straight stroke of 24,000 points, then the whole line erased: allowed, as before.
    let line: Vec<[f64; 2]> = (0..24_000).map(|k| [0.001 * k as f64, 0.0]).collect();
    let end = line[line.len() - 1];
    let mut d = Document::with_ground_sketch();
    d.apply(&Op::Batch {
        ops: vec![
            Op::AddPolyline {
                sketch: 1,
                points: line,
                closed: false,
            },
            Op::EraseSegment {
                sketch: 1,
                a: [0.0, 0.0],
                b: end,
            },
        ],
    })
    .unwrap();
    assert_eq!(d.model().sketch(1).unwrap().edge_count(), 0);
    // The refusal after inserting (the stroke not last) says what the early one says.
    let mut d = Document::with_ground_sketch();
    let late = d
        .apply(&Op::Batch {
            ops: vec![
                draw(zigzag(24_000)),
                Op::AddGuide {
                    point: [0.0, 0.0, 0.0],
                    dir: None,
                },
            ],
        })
        .unwrap_err();
    let early = d.apply(&draw(zigzag(24_000))).unwrap_err();
    assert_eq!(late, early);
    assert_eq!(
        early.to_string(),
        "a sketch would hold 23999 edges; at most 20000"
    );
}

/// A polyline of `n` clear points in a sketch's frame (the zigzag).
fn clear_line(n: usize) -> Vec<[f64; 2]> {
    zigzag(n).iter().map(|p| [p[0], p[1]]).collect()
}

/// A model with two sketches: A empty (on the ground), B (a metre up) holding a rectangle.
fn two_sketches() -> (sawhorse_kernel::Model, u64, u64) {
    use sawhorse_kernel::model::OpOutcome;
    let mut m = sawhorse_kernel::Model::new();
    let mut new = |z: f64| match m
        .apply(&Op::NewSketch {
            origin: [0.0, 0.0, z],
            normal: [0.0, 0.0, 1.0],
        })
        .unwrap()
    {
        OpOutcome::SketchCreated { sketch } => sketch,
        o => panic!("{o:?}"),
    };
    let (a, b) = (new(0.0), new(1.0));
    m.apply(&Op::AddRectangle {
        sketch: b,
        x: -5.0,
        y: -5.0,
        w: 1.0,
        h: 1.0,
    })
    .unwrap();
    (m, a, b)
}

/// The review's case: a batch drawing 20,002 clear points into each of two sketches. The
/// limit check reports the first sketch over the limit, A with 20,001 edges, though only B's
/// stroke (the last op) is counted rather than drawn: the error is the one the check gives
/// with both drawn, the same as before this lane.
#[test]
fn two_sketches_over_the_limit_report_the_first() {
    let (m, a, b) = two_sketches();
    let poly = |sketch| Op::AddPolyline {
        sketch,
        points: clear_line(20_002),
        closed: false,
    };
    let mut x = m.clone();
    let e = x.apply(&Op::Batch {
        ops: vec![poly(a), poly(b)],
    });
    assert_eq!(
        e.map(|_| ()),
        Err(KernelError::Limit(
            sawhorse_kernel::limits::LimitError::SketchEdges(20_001)
        ))
    );
    assert_eq!(x.hash(), m.hash());
    // B's stroke first and A's last: A is still the first sketch the check comes to.
    let mut y = m.clone();
    let e2 = y
        .apply(&Op::Batch {
            ops: vec![poly(b), poly(a)],
        })
        .unwrap_err();
    // The same batch with a guide after the strokes, so neither is the last op and both are
    // drawn: the same error.
    let mut z = m.clone();
    let mut ops = vec![poly(b), poly(a)];
    ops.push(Op::AddGuide {
        point: [0.0, 0.0, 0.0],
        dir: None,
    });
    assert_eq!(z.apply(&Op::Batch { ops }).unwrap_err(), e2);
}

/// A batch whose earlier op breaks another limit (one tag past the 10,000 a model may hold)
/// and whose last op is a stroke certain to break the sketch's: refused for whichever the
/// check finds first, exactly as with the stroke drawn.
#[test]
fn another_limit_broken_earlier_in_the_batch_is_reported_as_before() {
    let (m, a, _) = two_sketches();
    let mut ops: Vec<Op> = (0..=sawhorse_kernel::limits::MAX_TAGS)
        .map(|i| Op::CreateTag {
            name: format!("t{i}"),
            color: None,
        })
        .collect();
    ops.push(Op::AddPolyline {
        sketch: a,
        points: clear_line(20_002),
        closed: false,
    });
    let mut x = m.clone();
    let early = x.apply(&Op::Batch { ops: ops.clone() }).unwrap_err();
    ops.push(Op::AddGuide {
        point: [0.0, 0.0, 0.0],
        dir: None,
    });
    let mut y = m.clone();
    let drawn = y.apply(&Op::Batch { ops }).unwrap_err();
    assert_eq!(early, drawn);
    assert!(
        !matches!(
            early,
            KernelError::Limit(sawhorse_kernel::limits::LimitError::SketchEdges(_))
        ),
        "{early}"
    );
}

/// A stroke past the limit that is not the last op of its batch is drawn, and the ops after
/// it can take edges away again: a straight line of 20,101 points (20,100 edges, every one
/// clear) then the whole line erased is allowed, as before. Counting the line instead of
/// drawing it (when it is not last, or for every op of a batch) refuses this batch.
#[test]
fn a_stroke_that_is_not_last_is_drawn() {
    let n = sawhorse_kernel::limits::MAX_SKETCH_EDGES + 101;
    let line: Vec<[f64; 2]> = (0..n).map(|k| [0.001 * k as f64, 0.0]).collect();
    let end = line[n - 1];
    let mut d = Document::with_ground_sketch();
    d.apply(&Op::Batch {
        ops: vec![
            Op::AddPolyline {
                sketch: 1,
                points: line,
                closed: false,
            },
            Op::EraseSegment {
                sketch: 1,
                a: [0.0, 0.0],
                b: end,
            },
        ],
    })
    .unwrap();
    assert_eq!(d.model().sketch(1).unwrap().edge_count(), 0);
}

/// A stroke past the limit with one point off the sketch's plane is refused for that point
/// (as before: the stroke is taken into the sketch's frame first), not counted as clear.
#[test]
fn an_off_plane_stroke_is_refused_for_its_point() {
    let mut pts = zigzag(sawhorse_kernel::limits::MAX_SKETCH_EDGES + 5);
    pts[7][2] = 0.001;
    for target in [
        DrawTarget::Sketch { sketch: 1 },
        DrawTarget::Plane {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        },
    ] {
        let mut d = Document::with_ground_sketch();
        let e = d
            .apply(&Op::Draw {
                target,
                points: pts.clone(),
                closed: false,
                curve: None,
            })
            .unwrap_err();
        assert_eq!(
            e,
            KernelError::Sketch(sawhorse_kernel::SketchError::OffPlane),
            "{e}"
        );
    }
}
