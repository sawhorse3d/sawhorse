//! Property tests for moving, turning, scaling and copying sketch geometry (lane 22,
//! ADR-0012), with a hostile generator.
//!
//! Each case draws one to four shapes (rectangles, circles of 6 to 24 segments, open and
//! closed polylines on a 0.25 m grid, some touching or crossing) on the ground or on a
//! tilted plane, then applies one to four `TransformSketch` ops whose selections mix
//! points, edges, whole regions and the whole sketch, and whose maps are built to hurt:
//! moves that land a vertex exactly on another vertex or on a point of an edge (also within
//! the merge distance of it), turns by 0, 180, 360 and -360 degrees, by 1e-7 degrees and by
//! 179.99999, about the plane's normal, about axes in the plane and about a skew axis,
//! moves straight off the plane and slightly off it (under and over the planarity
//! tolerance), uniform scales (mirrors and a thousandth included), copies and arrays.
//!
//! After every accepted op:
//! - the model validates (every sketch's graph and regions: no crossing, overlapping,
//!   repeated or zero-length edge, no vertex on an edge it does not end, simple rings with
//!   the right winding, anchors inside), and `testing::check_model` passes;
//! - undo restores the hash before it and redo the hash after it (one undo step);
//! - every live region that is not pinched pushes into a solid whose volume is its area
//!   times the distance (to 1e-9, relative);
//! - the model saves to a snapshot and opens to the same hash;
//! - a rigid map in the plane of a whole sketch keeps its vertex, edge and region counts
//!   and its region areas, and a whole sketch lifted off its plane leaves it empty.
//!
//! A refusal must be typed (never the validator's) and leave the document as it was. At
//! the end the whole sequence goes through `testing::check_sequence` (undo / redo at every
//! step, log replay, determinism).
//!
//! `PROPTEST_CASES` sets the number of cases (default 256); `PROPTEST_RNG_SEED` the seed.

use std::f64::consts::PI;

use glam::{DVec2, DVec3};
use proptest::prelude::*;
use sawhorse_kernel::Document;
use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome};
use sawhorse_kernel::plane::Plane;
use sawhorse_kernel::testing::{check_model, check_sequence};
use sawhorse_kernel::tolerance::{PLANARITY, POINT_MERGE};
use sawhorse_kernel::transform::Xform;

#[derive(Debug, Clone)]
enum Shape {
    Rect([i32; 2], [i32; 2]),
    Circle([i32; 2], u8, u8),
    Line(Vec<[i32; 2]>, bool),
}

#[derive(Debug, Clone)]
enum Map {
    /// Vertex i onto vertex j (in the plane, exactly).
    Land(u8, u8),
    /// Vertex i onto the point at `t` along edge e, nudged `nudge` across it.
    OnEdge(u8, u8, f64, f64),
    /// Along the plane by grid steps.
    Slide(i32, i32),
    /// Along the plane and `n` off it.
    Lift(i32, i32, f64),
    /// About the plane's normal through vertex i, degrees.
    Spin(u8, f64),
    /// About an axis in the plane (0: u, 1: v) or a skew axis (2), through vertex i.
    Tilt(u8, u8, f64),
    /// Uniform scale about vertex i.
    Scale(u8, f64),
}

#[derive(Debug, Clone)]
struct Move {
    /// Which sketch (among those with edges).
    sketch: u8,
    points: Vec<u8>,
    edges: Vec<u8>,
    regions: Vec<u8>,
    /// Every edge of the sketch.
    all: bool,
    map: Map,
    copy: bool,
    count: u32,
}

fn shape() -> impl Strategy<Value = Shape> {
    let c = || -6i32..=6;
    prop_oneof![
        3 => ((c(), c()), (1i32..=6, 1i32..=6)).prop_map(|(a, s)| Shape::Rect([a.0, a.1], [s.0, s.1])),
        2 => ((c(), c()), 1u8..=6, prop_oneof![Just(6u8), Just(12), Just(24)])
            .prop_map(|(a, r, n)| Shape::Circle([a.0, a.1], r, n)),
        2 => (prop::collection::vec((c(), c()).prop_map(|(x, y)| [x, y]), 2..6), any::<bool>())
            .prop_map(|(p, closed)| Shape::Line(p, closed)),
    ]
}

fn angle() -> impl Strategy<Value = f64> {
    prop_oneof![
        2 => Just(0.0),
        3 => Just(180.0),
        2 => Just(360.0),
        1 => Just(-360.0),
        3 => Just(90.0),
        3 => Just(45.0),
        2 => Just(1e-7),
        1 => Just(1e-4),
        1 => Just(179.99999),
        3 => -360.0..360.0f64,
    ]
}

fn map() -> impl Strategy<Value = Map> {
    let step = || -8i32..=8;
    prop_oneof![
        4 => (any::<u8>(), any::<u8>()).prop_map(|(i, j)| Map::Land(i, j)),
        3 => (
            any::<u8>(),
            any::<u8>(),
            prop_oneof![Just(0.5), Just(0.25), 0.0..1.0f64],
            prop_oneof![3 => Just(0.0), 1 => Just(POINT_MERGE * 0.5), 1 => Just(POINT_MERGE * 1.5)],
        )
            .prop_map(|(i, e, t, n)| Map::OnEdge(i, e, t, n)),
        3 => (step(), step()).prop_map(|(u, v)| Map::Slide(u, v)),
        3 => (
            step(),
            step(),
            prop_oneof![Just(0.25), Just(-0.5), Just(1.0), Just(PLANARITY * 0.1), Just(PLANARITY * 3.0), Just(POINT_MERGE * 2.0)],
        )
            .prop_map(|(u, v, n)| Map::Lift(u, v, n)),
        4 => (any::<u8>(), angle()).prop_map(|(i, a)| Map::Spin(i, a)),
        3 => (0u8..3, any::<u8>(), angle()).prop_map(|(k, i, a)| Map::Tilt(k, i, a)),
        1 => (any::<u8>(), prop_oneof![Just(2.0), Just(0.5), Just(-1.0), Just(1e-3)]).prop_map(|(i, f)| Map::Scale(i, f)),
    ]
}

fn mv() -> impl Strategy<Value = Move> {
    let picks = || prop::collection::vec(any::<u8>(), 0..3);
    (
        any::<u8>(),
        picks(),
        picks(),
        picks(),
        prop::bool::weighted(0.2),
        map(),
        prop::bool::weighted(0.25),
        prop_oneof![4 => Just(1u32), 1 => 2u32..4],
    )
        .prop_map(
            |(sketch, points, edges, regions, all, map, copy, count)| Move {
                sketch,
                points,
                edges,
                regions,
                all,
                map,
                copy,
                count: if copy { count } else { 1 },
            },
        )
}

fn plane() -> impl Strategy<Value = ([f64; 3], [f64; 3])> {
    prop_oneof![
        3 => Just(([0.0; 3], [0.0, 0.0, 1.0])),
        1 => Just(([0.0, 0.0, 0.5], [0.0, 1.0, 1.0])),
        1 => Just(([0.25, -0.5, 0.0], [0.3, -0.5, 0.8])),
        1 => Just(([0.0; 3], [1.0, 0.0, 0.0])),
    ]
}

/// The world points of a shape on `plane`.
fn shape_points(s: &Shape, plane: &Plane) -> (Vec<[f64; 3]>, bool) {
    let g = |x: i32, y: i32| DVec2::new(f64::from(x) * 0.25, f64::from(y) * 0.25);
    let w = |p: DVec2| plane.to_world(p).to_array();
    match s {
        Shape::Rect(a, sz) => {
            let (o, d) = (g(a[0], a[1]), g(sz[0], sz[1]));
            (
                vec![
                    w(o),
                    w(o + DVec2::new(d.x, 0.0)),
                    w(o + d),
                    w(o + DVec2::new(0.0, d.y)),
                ],
                true,
            )
        }
        Shape::Circle(c, r, n) => {
            let (c, r) = (g(c[0], c[1]), f64::from(*r) * 0.125);
            let pts = (0..*n)
                .map(|k| {
                    let a = 2.0 * PI * f64::from(k) / f64::from(*n);
                    w(c + DVec2::new(r * a.cos(), r * a.sin()))
                })
                .collect();
            (pts, true)
        }
        Shape::Line(p, closed) => (p.iter().map(|q| w(g(q[0], q[1]))).collect(), *closed),
    }
}

/// A selection as the op names it: points, edges, regions.
type Picked = (Vec<[f64; 3]>, Vec<[[f64; 3]; 2]>, Vec<u64>);

fn pick<T: Copy>(items: &[T], k: u8) -> Option<T> {
    (!items.is_empty()).then(|| items[k as usize % items.len()])
}

/// What one sketch holds, in world terms.
struct View {
    id: u64,
    plane: Plane,
    verts: Vec<DVec3>,
    edges: Vec<[DVec3; 2]>,
    regions: Vec<u64>,
}

fn view(m: &Model, id: u64) -> View {
    let s = m.sketch(id).expect("sketch");
    let plane = *s.plane();
    let at = |v| plane.to_world(s.vertex(v).expect("vertex"));
    View {
        id,
        plane,
        verts: s.vertices().map(|(_, p)| plane.to_world(p)).collect(),
        edges: s.edges().map(|(_, [a, b])| [at(a), at(b)]).collect(),
        regions: s
            .regions()
            .map(|(r, _)| sawhorse_kernel::ids::to_public(r))
            .collect(),
    }
}

/// The xform a map stands for in this sketch (None when the sketch is too empty for it).
fn xform_of(map: &Map, v: &View) -> Option<(Xform, bool)> {
    let n = v.plane.normal;
    let vert = |i: u8| pick(&v.verts, i);
    // The bool: rigid within the plane.
    Some(match map {
        Map::Land(i, j) => (Xform::translation(vert(*j)? - vert(*i)?), true),
        Map::OnEdge(i, e, t, nudge) => {
            let [a, b] = pick(&v.edges, *e)?;
            let across = n.cross(b - a).normalize_or_zero();
            let to = a.lerp(b, *t) + across * *nudge;
            (Xform::translation(to - vert(*i)?), true)
        }
        Map::Slide(du, dv) => (
            Xform::translation(
                v.plane.u * f64::from(*du) * 0.125 + v.plane.v * f64::from(*dv) * 0.125,
            ),
            true,
        ),
        Map::Lift(du, dv, dn) => (
            Xform::translation(
                v.plane.u * f64::from(*du) * 0.125 + v.plane.v * f64::from(*dv) * 0.125 + n * *dn,
            ),
            false,
        ),
        Map::Spin(i, deg) => (Xform::rotation(vert(*i)?, n, deg.to_radians()), true),
        Map::Tilt(k, i, deg) => {
            let axis = match k {
                0 => v.plane.u,
                1 => v.plane.v,
                _ => DVec3::new(0.3, -0.5, 0.8),
            };
            (Xform::rotation(vert(*i)?, axis, deg.to_radians()), false)
        }
        Map::Scale(i, f) => {
            let c = vert(*i)?;
            (Xform::scaling(c, DVec3::splat(*f)), false)
        }
    })
}

fn counts_and_areas(m: &Model, id: u64) -> (usize, usize, usize, Vec<f64>) {
    let s = m.sketch(id).expect("sketch");
    let mut a: Vec<f64> = s.regions().map(|(_, r)| s.region_area(r)).collect();
    a.sort_by(f64::total_cmp);
    (s.vertex_count(), s.edge_count(), s.region_count(), a)
}

/// Every live region that is not pinched pushes into a solid of its area times 0.25.
fn regions_push_true(d: &Document) -> Result<(), String> {
    let m = d.model();
    for id in m.sketch_ids() {
        let s = m.sketch(id).expect("sketch");
        for (r, reg) in s.regions() {
            if reg.pinched {
                continue;
            }
            let area = s.region_area(reg);
            let mut probe = d.clone();
            let region = sawhorse_kernel::ids::to_public(r);
            match probe.apply(&Op::Extrude {
                sketch: id,
                region,
                distance: 0.25,
            }) {
                Ok(OpOutcome::ObjectCreated { object }) => {
                    let v = probe.model().object(object).expect("solid").volume();
                    let want = area * 0.25;
                    // Relative 1e-9, plus what the shoelace sums themselves lose: a region
                    // a few metres from the origin sums products of size ~10 m2 to an area
                    // that can be 1e-9 m2 (a thousandth-scale copy), so both the area and
                    // the volume carry ~1e-14 of rounding whatever the region's size.
                    if (v - want).abs() > 1e-9 * want + 1e-12 {
                        return Err(format!(
                            "region {region} of sketch {id}: volume {v}, area x distance {want}"
                        ));
                    }
                }
                Ok(o) => return Err(format!("extrude gave {o:?}")),
                Err(e) if e.is_bug() => return Err(format!("extruding region {region}: {e}")),
                Err(_) => {}
            }
        }
    }
    Ok(())
}

fn run(
    plane_pick: ([f64; 3], [f64; 3]),
    shapes: &[Shape],
    moves: &[Move],
) -> Result<(), TestCaseError> {
    let mut d = Document::new();
    let mut ops = vec![Op::NewSketch {
        origin: plane_pick.0,
        normal: plane_pick.1,
    }];
    d.apply(&ops[0]).expect("a sketch");
    let plane = *d.model().sketch(1).expect("sketch 1").plane();
    for s in shapes {
        let (points, closed) = shape_points(s, &plane);
        let op = Op::Draw {
            target: DrawTarget::Sketch { sketch: 1 },
            points,
            closed,
        };
        if d.apply(&op).is_ok() {
            ops.push(op);
        }
    }
    for m in moves {
        let model = d.model();
        let with_edges: Vec<u64> = model
            .sketch_ids()
            .into_iter()
            .filter(|&id| model.sketch(id).is_some_and(|s| s.edge_count() > 0))
            .collect();
        let Some(id) = pick(&with_edges, m.sketch) else {
            break;
        };
        let v = view(model, id);
        let Some((xform, rigid)) = xform_of(&m.map, &v) else {
            continue;
        };
        let (points, edges, regions): Picked = if m.all {
            (
                Vec::new(),
                v.edges
                    .iter()
                    .map(|[a, b]| [a.to_array(), b.to_array()])
                    .collect(),
                Vec::new(),
            )
        } else {
            (
                m.points
                    .iter()
                    .filter_map(|&k| pick(&v.verts, k).map(|p| p.to_array()))
                    .collect(),
                m.edges
                    .iter()
                    .filter_map(|&k| pick(&v.edges, k).map(|[a, b]| [a.to_array(), b.to_array()]))
                    .collect(),
                m.regions
                    .iter()
                    .filter_map(|&k| pick(&v.regions, k))
                    .collect(),
            )
        };
        let op = Op::TransformSketch {
            sketch: v.id,
            points,
            edges,
            regions,
            xform,
            copy: m.copy,
            count: m.count,
        };
        let before = d.hash();
        let shape_before = counts_and_areas(d.model(), v.id);
        let snap = (d.log().len(), d.depth());
        let result = d.apply(&op);
        if std::env::var("SKETCH_MOVE_STATS").is_ok() {
            // One line per op for a tally (`... | sort | uniq -c`): what it was and how it went.
            let how = match &result {
                Ok(OpOutcome::SketchMoved { sketch, .. }) => {
                    if *sketch != v.id { "lifted" } else { "moved" }.to_string()
                }
                Ok(OpOutcome::SketchCopied { .. }) => "copied".to_string(),
                Ok(o) => format!("{o:?}"),
                Err(e) => format!("refused: {e}"),
            };
            eprintln!("STAT {} {how}", if m.all { "all" } else { "part" });
        }
        match result {
            Err(e) => {
                prop_assert!(
                    !e.is_bug(),
                    "{op:?}: the validator rejected the result: {e}"
                );
                prop_assert_eq!(d.hash(), before, "refused but changed: {:?}", op);
                prop_assert_eq!((d.log().len(), d.depth()), snap);
            }
            Ok(_) => {
                ops.push(op.clone());
                check_model(d.model()).map_err(|e| TestCaseError::fail(format!("{op:?}: {e}")))?;
                let after = d.hash();
                let mut probe = d.clone();
                probe
                    .undo()
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                prop_assert_eq!(probe.hash(), before, "undo of {:?}", op);
                probe
                    .redo()
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                prop_assert_eq!(probe.hash(), after, "redo of {:?}", op);
                regions_push_true(&d)
                    .map_err(|e| TestCaseError::fail(format!("after {op:?}: {e}")))?;
                let back = Model::from_snapshot(&d.model().snapshot())
                    .map_err(|e| TestCaseError::fail(format!("snapshot after {op:?}: {e}")))?;
                prop_assert_eq!(
                    back.hash(),
                    d.model().hash(),
                    "snapshot round trip after {:?}",
                    op
                );
                if m.all && !m.copy {
                    let now = counts_and_areas(d.model(), v.id);
                    if rigid {
                        // A rigid map of everything in the sketch: congruent.
                        prop_assert_eq!(
                            (now.0, now.1, now.2),
                            (shape_before.0, shape_before.1, shape_before.2),
                            "{:?}",
                            op
                        );
                        for (a, b) in now.3.iter().zip(&shape_before.3) {
                            prop_assert!(
                                (a - b).abs() <= 1e-9 * b.max(1.0),
                                "areas {:?} vs {:?}",
                                now.3,
                                shape_before.3
                            );
                        }
                    } else if let Map::Lift(_, _, dn) = m.map
                        && dn.abs() > PLANARITY
                    {
                        prop_assert_eq!(now.1, 0, "lifted whole, nothing left: {:?}", op);
                    }
                }
            }
        }
    }
    check_sequence(&ops).map_err(|e| {
        TestCaseError::fail(format!(
            "sequence: {e}\n{}",
            serde_json::to_string(&ops).unwrap_or_default()
        ))
    })?;
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        ..ProptestConfig::default()
    })]

    #[test]
    fn sketch_moves_keep_every_invariant(
        p in plane(),
        shapes in prop::collection::vec(shape(), 1..5),
        moves in prop::collection::vec(mv(), 1..5),
    ) {
        run(p, &shapes, &moves)?;
    }
}
