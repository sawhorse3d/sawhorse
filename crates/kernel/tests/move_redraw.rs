//! #11, the Move after Enter: a Move, Copy or Array of a whole sketch selection draws every
//! carried edge again with the insertion rules. Drawn with a scan of every vertex and edge
//! for each edge, it grew with the square of the selection (400 circles of 48 segments took
//! about 3.4 s natively). Lane 70: the default run counts the op's work instead of timing
//! it (`testing::sketch_visits`, the same number on every run and machine); the timings,
//! of the kernel's op alone, are ignored tests.

use std::f64::consts::TAU;

use sawhorse_kernel::Document;
use sawhorse_kernel::curve::{CurveKind, CurveSpec};
use sawhorse_kernel::model::{DrawTarget, ImportStroke, Op, OpOutcome};

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{e}"))
}

/// Ring `i` of a grid 20 across, 60 mm apart: centre and its `n` points (radius 25 mm).
fn ring(i: usize, n: usize) -> ([f64; 2], Vec<[f64; 3]>) {
    let c = [(i % 20) as f64 * 0.06, (i / 20) as f64 * 0.06];
    let pts = (0..n)
        .map(|k| {
            let a = TAU * k as f64 / n as f64;
            [c[0] + 0.025 * a.cos(), c[1] + 0.025 * a.sin(), 0.0]
        })
        .collect();
    (c, pts)
}

/// `rings` rings of `n` segments on the ground: circles (curves) or plain closed polylines.
fn rings(rings: usize, n: usize, circles: bool) -> Document {
    let mut d = Document::with_ground_sketch();
    for i in 0..rings {
        let (c, points) = ring(i, n);
        ok(
            &mut d,
            Op::Draw {
                target: DrawTarget::Sketch { sketch: 1 },
                points,
                closed: true,
                curve: circles.then_some(CurveSpec {
                    kind: CurveKind::Circle,
                    centre: Some([c[0], c[1], 0.0]),
                }),
            },
        );
    }
    d
}

/// Everything selected, as the app sends it: circles by curve id, polylines by every edge's
/// two points; moved 100 mm along red (`copy`: `count` copies, 1.3 m apart, clear of the
/// grid and of each other, so a copy adds its edges and splits none).
fn move_all(d: &Document, circles: bool, copy: bool, count: u32) -> Op {
    let s = d.model().sketch(1).unwrap();
    let (curves, edges): (Vec<u64>, Vec<[[f64; 3]; 2]>) = if circles {
        (s.curves().map(|(id, _)| id).collect(), Vec::new())
    } else {
        let at = |v| {
            let p = s.vertex(v).unwrap();
            [p.x, p.y, 0.0]
        };
        (
            Vec::new(),
            s.edges().map(|(_, [a, b])| [at(a), at(b)]).collect(),
        )
    };
    serde_json::from_value(serde_json::json!({
        "op": "transform_sketch",
        "sketch": 1,
        "edges": edges,
        "curves": curves,
        "xform": {
            "linear": [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
            "translation": [if copy { 1.3 } else { 0.1 }, 0, 0]
        },
        "copy": copy,
        "count": count,
    }))
    .unwrap()
}

/// The same rings as [`rings`] made in one import (quicker to make many), as curves or plain.
fn imported(rings: usize, n: usize, circles: bool) -> Document {
    let mut d = Document::with_ground_sketch();
    let strokes = (0..rings)
        .map(|i| {
            let (c, points) = ring(i, n);
            ImportStroke {
                points,
                closed: true,
                curve: circles.then_some(CurveSpec {
                    kind: CurveKind::Circle,
                    centre: Some([c[0], c[1], 0.0]),
                }),
            }
        })
        .collect();
    ok(
        &mut d,
        Op::Import {
            items: vec![],
            strokes,
            materials: vec![],
            scenes: vec![],
            hidden_tags: vec![],
            definitions: vec![],
            nodes: vec![],
        },
    );
    d
}

/// Lane 70: a Move, Copy and Array of 3 of everything, circles (by curve id) and polylines
/// (edge by edge), drawn one ring at a time and imported, four times as many rings costing at
/// most six times the work: the vertices, edges and curve points the sketch's searches look
/// at during the op ([`sawhorse_kernel::testing::sketch_visits`]). A count, not a clock, so
/// it is the same on every run and machine and a loaded one cannot fail it. With every edge
/// drawn again through a scan of the sketch, as before #87, the work grows with the square
/// of the selection, sixteen times for four times the rings, and this fails for every case.
#[test]
fn a_move_copy_or_array_of_everything_counts_work_in_step_with_the_selection() {
    use sawhorse_kernel::testing::sketch_visits;
    let mut table = Vec::new();
    for (what, copy, count, small, big) in [
        ("move", false, 1, 50, 200),
        ("copy", true, 1, 50, 200),
        ("array 3", true, 3, 25, 100),
    ] {
        for circles in [true, false] {
            for drawn in [false, true] {
                let work = [small, big].map(|k| {
                    let mut d = if drawn {
                        rings(k, 24, circles)
                    } else {
                        imported(k, 24, circles)
                    };
                    let op = move_all(&d, circles, copy, count);
                    sketch_visits(|| ok(&mut d, op)).1
                });
                let line = format!(
                    "{what:8} {:9} {:8} {small:3} rings {:9}, {big:3} rings {:9} ({:.2} times)",
                    if circles { "circles" } else { "polylines" },
                    if drawn { "drawn" } else { "imported" },
                    work[0],
                    work[1],
                    work[1] as f64 / work[0] as f64
                );
                eprintln!("{line}");
                table.push((line, work));
            }
        }
    }
    for (line, [a, b]) in &table {
        assert!(*a > 0 && *b <= 6 * *a, "{line}");
    }
}

/// #11: a Move of everything, of circles and of polylines selected edge by edge, and a Copy
/// of circles, four times as many rings costing at most twelve times as long (the fastest of
/// three runs each, and of up to nine while they are not within the bound). With every edge
/// drawn again through scans of the sketch it cost about fifteen to sixteen times. Measured:
/// the code before #87 gives 15.3 to 16.6 times and fails every time; now about 6 to 7.5
/// times on an idle machine, and 10 to 13 times has been seen beside other work. It is a
/// wall-clock comparison: on a heavily loaded machine it can fail, so since lane 70 it is
/// ignored and the count above holds the same growth in the default run (what the count
/// leaves out, the time of the op's other steps, this still times). Release only: `cargo
/// test --release -p sawhorse-kernel --test move_redraw -- --ignored --nocapture`; the
/// seconds are in `the_move_copy_and_array_timings`.
#[test]
#[ignore]
fn a_move_or_copy_of_everything_grows_with_the_selection_not_its_square() {
    for (what, circles, copy, small, big) in [
        ("move, circles", true, false, 200, 800),
        ("move, polylines by their edges", false, false, 200, 800),
        ("copy, circles", true, true, 100, 400),
    ] {
        let docs = [imported(small, 24, circles), imported(big, 24, circles)];
        let ops = [
            move_all(&docs[0], circles, copy, 1),
            move_all(&docs[1], circles, copy, 1),
        ];
        let mut best = [f64::MAX; 2];
        let mut rounds = 0;
        for round in 0..9 {
            if round >= 3 && best[1] <= 12.0 * best[0] {
                break;
            }
            rounds += 1;
            for j in 0..2 {
                let mut d = docs[j].clone();
                let t = std::time::Instant::now();
                ok(&mut d, ops[j].clone());
                best[j] = best[j].min(t.elapsed().as_secs_f64());
            }
        }
        eprintln!(
            "{what}: {small} rings {:.3} s, {big} rings {:.3} s ({:.1} times), fastest of {rounds}",
            best[0],
            best[1],
            best[1] / best[0]
        );
        assert!(best[1] <= 12.0 * best[0], "{what}: {best:?}");
    }
}

/// Seconds for the op on a fresh copy of the rings, the fastest of `runs`, and the model
/// hash after it.
fn time(
    n_rings: usize,
    n: usize,
    circles: bool,
    copy: bool,
    count: u32,
    runs: usize,
) -> (f64, String) {
    let base = rings(n_rings, n, circles);
    let op = move_all(&base, circles, copy, count);
    let mut best = f64::MAX;
    let mut hash = String::new();
    for _ in 0..runs {
        let mut d = rings(n_rings, n, circles);
        let t = std::time::Instant::now();
        ok(&mut d, op.clone());
        best = best.min(t.elapsed().as_secs_f64());
        hash = d.model().hash_hex();
    }
    (best, hash)
}

/// The table on #11, natively: every kind of ring, Move (200 and 400 rings), Copy (100 and
/// 200) and an Array of 3 (50 and 100), each twice as many as the other and within the
/// sketch's 20,000 edges after it. Prints the time and the model hash after the op (to
/// compare with `main`). Release only: `cargo test --release -p sawhorse-kernel --test
/// move_redraw -- --ignored --nocapture`.
#[test]
#[ignore]
fn the_move_copy_and_array_timings() {
    for (what, copy, count, sizes) in [
        ("move", false, 1, [200, 400]),
        ("copy", true, 1, [100, 200]),
        ("array 3", true, 3, [50, 100]),
    ] {
        for circles in [true, false] {
            for n in [24, 48] {
                for k in sizes {
                    let (t, h) = time(k, n, circles, copy, count, 3);
                    eprintln!(
                        "{what:8} {:9} {n} segments, {k}: {:.3} s  hash {h}",
                        if circles { "circles" } else { "polylines" },
                        t
                    );
                }
            }
        }
    }
}
