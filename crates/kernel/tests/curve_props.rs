//! Property tests for curves (lane 23, ADR-0013), with a hostile generator.
//!
//! Each case picks a plane (the ground or a tilted one) and runs two to eight steps against
//! the live document: circles and polygons of 3 to 96 segments at tiny, ordinary and huge
//! radii (0.1 mm to 9 km; the tiniest refused as too small), arcs of 0.001 to 359.999
//! degrees either way round with 1 to 48 segments; lines that end on a curve's point, end on
//! a segment's middle (a T), cross one segment, cross two curves at once, run along a
//! segment, or are anywhere on the grid; the Eraser on one segment of a curve (the plain
//! op: the rest becomes polyline pieces) and on a whole curve; Explode Curve; offsets of a
//! region and of a curve's chain; `transform_sketch` of whole curves (and of a region, and
//! of a lone curve point, which must be refused) by every kind of map: moves along the
//! plane, landings of a curve point on another point, turns about the normal (0, 90, 180,
//! 1e-7 degrees and any), tilts about an axis in the plane, uniform scales and mirrors,
//! non-uniform scales, lifts off the plane, as moves, copies and arrays; pushes of regions
//! into solids and of their caps back down flat.
//!
//! After every accepted op the model validates (its curves among the rest: every curve's
//! edges exist and form one chain in order, closed for a circle or a polygon, no edge in two
//! curves, every round curve's points on its circle within `CURVE_FIT`, its angles read from
//! them; every solid's rims chains of edges named once by their face ids) and passes
//! `testing::check_model`; undo restores the hash before and redo the hash after; the model
//! saves to a snapshot (through JSON, as a `.shz` holds it) and opens to the same hash with
//! the same curves. A refusal must be typed (never the validator's) and leave the document
//! as it was. At the end the sequence goes through `testing::check_sequence` (undo / redo at
//! every step, log replay, determinism).
//!
//! `PROPTEST_CASES` sets the number of cases (default 256); `PROPTEST_RNG_SEED` the seed.
//! `CURVE_PROPS_DUMP=<file>` writes the ops accepted so far after each one (the last write
//! of a failing run is its sequence).

use std::f64::consts::{PI, TAU};

use glam::{DVec2, DVec3};
use proptest::prelude::*;
use sawhorse_kernel::Document;
use sawhorse_kernel::curve::{CurveKind, CurveSpec};
use sawhorse_kernel::model::{DrawTarget, Model, Op};
use sawhorse_kernel::offset::OffsetSource;
use sawhorse_kernel::plane::Plane;
use sawhorse_kernel::testing::{check_model, check_sequence};
use sawhorse_kernel::transform::Xform;

/// Radii a curve is drawn at: tiny (refused or barely there), ordinary, huge.
const RADII: [f64; 9] = [1e-4, 3e-4, 2e-3, 0.05, 0.3, 1.0, 2.75, 600.0, 9000.0];

#[derive(Debug, Clone)]
enum Step {
    /// A circle (or a polygon) at grid point `c`, radius pick, segments, turned by `a0`.
    Round {
        c: [i32; 2],
        r: u8,
        n: u8,
        a0: f64,
        polygon: bool,
    },
    /// An arc: centre, radius pick, segments, start, sweep (degrees, signed).
    Arc {
        c: [i32; 2],
        r: u8,
        n: u8,
        a0: f64,
        sweep: f64,
    },
    /// A line relative to a curve: 0 from outside to one of its points, 1 from outside to a
    /// segment's middle (a T), 2 across one segment, 3 across two curves, 4 along a segment,
    /// 5 anywhere on the grid.
    Line {
        how: u8,
        curve: u8,
        k: u8,
        other: u8,
        t: f64,
        grid: [i32; 4],
    },
    /// The plain Eraser on segment `k` of a curve.
    EraseSegment {
        curve: u8,
        k: u8,
    },
    EraseCurve {
        curve: u8,
    },
    Explode {
        curve: u8,
        solid: bool,
    },
    /// Offset a region (or a curve's chain) by `d`.
    Offset {
        region: bool,
        pick: u8,
        d: f64,
    },
    Move {
        curves: Vec<u8>,
        region: Option<u8>,
        lone: Option<(u8, u8)>,
        map: Map,
        copy: bool,
        count: u8,
    },
    Extrude {
        region: u8,
        d: f64,
    },
    /// Push a solid's cap (face 2) by `d` (down to flat when it is minus the height).
    Push {
        object: u8,
        flat: bool,
        d: f64,
    },
}

#[derive(Debug, Clone)]
enum Map {
    Slide(i32, i32),
    /// A point of curve i onto a point of the sketch.
    Land(u8, u8),
    Spin(u8, f64),
    Tilt(u8, f64),
    Scale(f64),
    Stretch(f64, f64),
    Mirror,
    Lift(f64),
}

fn grid() -> impl Strategy<Value = i32> {
    -8i32..=8
}

fn step() -> impl Strategy<Value = Step> {
    let c = || (grid(), grid()).prop_map(|(x, y)| [x, y]);
    let angle = || {
        prop_oneof![
            2 => Just(0.0),
            2 => Just(90.0),
            2 => Just(180.0),
            1 => Just(1e-7),
            1 => Just(360.0),
            3 => -360.0..360.0f64,
        ]
    };
    let map = prop_oneof![
        3 => (grid(), grid()).prop_map(|(u, v)| Map::Slide(u, v)),
        3 => (any::<u8>(), any::<u8>()).prop_map(|(i, j)| Map::Land(i, j)),
        3 => (any::<u8>(), angle()).prop_map(|(i, a)| Map::Spin(i, a)),
        1 => (0u8..3, angle()).prop_map(|(k, a)| Map::Tilt(k, a)),
        2 => prop_oneof![Just(2.0), Just(0.5), Just(1e-3), Just(-1.0), Just(3.7)].prop_map(Map::Scale),
        1 => (prop_oneof![Just(2.0), Just(0.5), Just(1.0)], prop_oneof![Just(1.0), Just(3.0)]).prop_map(|(x, y)| Map::Stretch(x, y)),
        1 => Just(Map::Mirror),
        1 => prop_oneof![Just(0.5), Just(-0.25), Just(1e-7)].prop_map(Map::Lift),
    ];
    prop_oneof![
        5 => (c(), any::<u8>(), 3u8..=96, angle(), prop::bool::weighted(0.3))
            .prop_map(|(c, r, n, a0, polygon)| Step::Round { c, r, n, a0, polygon }),
        4 => (
            c(),
            any::<u8>(),
            1u8..=48,
            angle(),
            prop_oneof![
                1 => Just(0.001),
                1 => Just(359.999),
                1 => Just(-359.999),
                1 => Just(180.0),
                4 => -359.999..359.999f64,
            ],
        )
            .prop_map(|(c, r, n, a0, sweep)| Step::Arc { c, r, n, a0, sweep }),
        8 => (0u8..6, any::<u8>(), any::<u8>(), any::<u8>(), prop_oneof![Just(0.5), Just(0.25), 0.0..1.0f64], (grid(), grid(), grid(), grid()))
            .prop_map(|(how, curve, k, other, t, g)| Step::Line { how, curve, k, other, t, grid: [g.0, g.1, g.2, g.3] }),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(curve, k)| Step::EraseSegment { curve, k }),
        2 => any::<u8>().prop_map(|curve| Step::EraseCurve { curve }),
        1 => (any::<u8>(), prop::bool::weighted(0.3)).prop_map(|(curve, solid)| Step::Explode { curve, solid }),
        2 => (any::<bool>(), any::<u8>(), prop_oneof![Just(0.05), Just(-0.05), Just(0.2), Just(-0.01), -0.5..0.5f64])
            .prop_map(|(region, pick, d)| Step::Offset { region, pick, d }),
        6 => (
            prop::collection::vec(any::<u8>(), 0..3),
            prop::option::weighted(0.2, any::<u8>()),
            prop::option::weighted(0.1, (any::<u8>(), any::<u8>())),
            map,
            prop::bool::weighted(0.3),
            prop_oneof![4 => Just(1u8), 1 => 2u8..4],
        )
            .prop_map(|(curves, region, lone, map, copy, count)| Step::Move {
                curves,
                region,
                lone,
                map,
                copy,
                count: if copy { count } else { 1 },
            }),
        3 => (any::<u8>(), prop_oneof![Just(0.5), Just(-0.3), Just(0.001)]).prop_map(|(region, d)| Step::Extrude { region, d }),
        2 => (any::<u8>(), any::<bool>(), prop_oneof![Just(0.25), Just(-0.1)]).prop_map(|(object, flat, d)| Step::Push { object, flat, d }),
    ]
}

fn plane() -> impl Strategy<Value = ([f64; 3], [f64; 3])> {
    prop_oneof![
        3 => Just(([0.0; 3], [0.0, 0.0, 1.0])),
        1 => Just(([0.0, 0.0, 0.5], [0.0, 1.0, 1.0])),
        1 => Just(([0.25, -0.5, 0.0], [0.3, -0.5, 0.8])),
    ]
}

fn pick<T: Clone>(items: &[T], k: u8) -> Option<T> {
    (!items.is_empty()).then(|| items[k as usize % items.len()].clone())
}

/// A curve of the sketch as the steps see it: id, kind, closed, world points, centre.
#[derive(Debug, Clone)]
struct Seen {
    id: u64,
    closed: bool,
    pts: Vec<DVec3>,
}

fn seen(m: &Model, sid: u64) -> Vec<Seen> {
    let Some(s) = m.sketch(sid) else {
        return Vec::new();
    };
    s.curves()
        .filter_map(|(id, c)| {
            let info = s.curve_info(id)?;
            Some(Seen {
                id,
                closed: c.closed,
                pts: info.points.iter().map(|p| DVec3::from_array(*p)).collect(),
            })
        })
        .collect()
}

/// The world points of a ring on `plane` (local centre, radius, segments, start, sweep).
fn ring(
    plane: &Plane,
    c: DVec2,
    r: f64,
    n: usize,
    a0: f64,
    sweep: f64,
    closed: bool,
) -> Vec<[f64; 3]> {
    let m = if closed { n } else { n + 1 };
    (0..m)
        .map(|k| {
            let a = a0 + sweep * k as f64 / n as f64;
            plane.to_world(c + r * DVec2::from_angle(a)).to_array()
        })
        .collect()
}

/// The op a step means now, or `None` when there is nothing for it to act on.
fn resolve(step: &Step, d: &Document, plane: &Plane, sid: u64) -> Option<Op> {
    let m = d.model();
    let g = |x: i32, y: i32| DVec2::new(f64::from(x) * 0.25, f64::from(y) * 0.25);
    let w = |p: DVec2| plane.to_world(p).to_array();
    let curves = seen(m, sid);
    let draw = |points: Vec<[f64; 3]>, closed: bool, curve: Option<CurveSpec>| Op::Draw {
        target: DrawTarget::Sketch { sketch: sid },
        points,
        closed,
        curve,
    };
    let sk = m.sketch(sid)?;
    let verts: Vec<DVec3> = sk.vertices().map(|(_, p)| plane.to_world(p)).collect();
    Some(match step {
        Step::Round {
            c,
            r,
            n,
            a0,
            polygon,
        } => {
            let (cc, rr) = (g(c[0], c[1]), RADII[*r as usize % RADII.len()]);
            draw(
                ring(plane, cc, rr, *n as usize, a0.to_radians(), TAU, true),
                true,
                Some(CurveSpec {
                    kind: if *polygon {
                        CurveKind::Polygon
                    } else {
                        CurveKind::Circle
                    },
                    centre: Some(w(cc)),
                }),
            )
        }
        Step::Arc { c, r, n, a0, sweep } => {
            let (cc, rr) = (g(c[0], c[1]), RADII[*r as usize % RADII.len()]);
            draw(
                ring(
                    plane,
                    cc,
                    rr,
                    *n as usize,
                    a0.to_radians(),
                    sweep.to_radians(),
                    false,
                ),
                false,
                Some(CurveSpec {
                    kind: CurveKind::Arc,
                    centre: Some(w(cc)),
                }),
            )
        }
        Step::Line {
            how,
            curve,
            k,
            other,
            t,
            grid: q,
        } => {
            let line = |a: DVec3, b: DVec3| draw(vec![a.to_array(), b.to_array()], false, None);
            let Some(c) = pick(&curves, *curve) else {
                return Some(line(
                    plane.to_world(g(q[0], q[1])),
                    plane.to_world(g(q[2], q[3])),
                ));
            };
            let n = c.pts.len();
            let i = *k as usize % n;
            let j = if c.closed || i + 1 < n {
                (i + 1) % n
            } else {
                i.saturating_sub(1)
            };
            let (a, b) = (c.pts[i], c.pts[j]);
            let out = plane.to_world(g(q[0], q[1]));
            match how {
                0 => line(out, a),
                1 => line(out, a.lerp(b, 0.5)),
                2 => {
                    let p = a.lerp(b, *t);
                    let across =
                        plane.normal.cross(b - a).normalize_or_zero() * (b - a).length().max(1e-3);
                    line(p - across, p + across)
                }
                3 => {
                    let o = pick(&curves, *other).unwrap_or_else(|| c.clone());
                    let p = a.lerp(b, *t);
                    let qq = o.pts[*other as usize % o.pts.len()];
                    let dir = qq - p;
                    line(p - dir * 0.37, qq + dir * 0.41)
                }
                4 => line(a, b),
                _ => line(out, plane.to_world(g(q[2], q[3]))),
            }
        }
        Step::EraseSegment { curve, k } => {
            let c = pick(&curves, *curve)?;
            let n = c.pts.len();
            let i = *k as usize % n;
            let j = if c.closed || i + 1 < n {
                (i + 1) % n
            } else {
                i.saturating_sub(1)
            };
            let (a, b) = (plane.to_local(c.pts[i]), plane.to_local(c.pts[j]));
            Op::EraseSegment {
                sketch: sid,
                a: a.to_array(),
                b: b.to_array(),
            }
        }
        Step::EraseCurve { curve } => Op::EraseCurve {
            sketch: sid,
            curve: pick(&curves, *curve).map_or(77, |c| c.id),
        },
        Step::Explode { curve, solid } => {
            if *solid {
                let objs = m.object_ids();
                let o = pick(&objs, *curve)?;
                let ids: Vec<u64> = m.object(o)?.curves().keys().copied().collect();
                Op::ExplodeCurve {
                    sketch: None,
                    object: Some(o),
                    curve: pick(&ids, *curve).unwrap_or(5),
                }
            } else {
                Op::ExplodeCurve {
                    sketch: Some(sid),
                    object: None,
                    curve: pick(&curves, *curve).map_or(77, |c| c.id),
                }
            }
        }
        Step::Offset {
            region,
            pick: k,
            d: dist,
        } => {
            if *region {
                let regions: Vec<u64> = sk
                    .regions()
                    .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
                    .collect();
                Op::Offset {
                    source: OffsetSource::Region {
                        sketch: sid,
                        region: pick(&regions, *k)?,
                    },
                    distance: *dist,
                }
            } else {
                let c = pick(&curves, *k)?;
                Op::Offset {
                    source: OffsetSource::Edges {
                        points: c.pts.iter().map(|p| p.to_array()).collect(),
                        closed: c.closed,
                        normal: plane.normal.to_array(),
                        object: None,
                    },
                    distance: *dist,
                }
            }
        }
        Step::Move {
            curves: cs,
            region,
            lone,
            map,
            copy,
            count,
        } => {
            let ids: Vec<u64> = cs
                .iter()
                .filter_map(|&k| pick(&curves, k).map(|c| c.id))
                .collect();
            let regions: Vec<u64> = match region {
                Some(k) => {
                    let all: Vec<u64> = sk
                        .regions()
                        .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
                        .collect();
                    pick(&all, *k).into_iter().collect()
                }
                None => Vec::new(),
            };
            let points: Vec<[f64; 3]> = match lone {
                Some((c, k)) => pick(&curves, *c)
                    .map(|c| c.pts[*k as usize % c.pts.len()].to_array())
                    .into_iter()
                    .collect(),
                None => Vec::new(),
            };
            let anchor = ids
                .first()
                .and_then(|id| curves.iter().find(|c| c.id == *id))
                .map_or(plane.origin, |c| c.pts[0]);
            let n = plane.normal;
            let xform = match map {
                Map::Slide(u, v) => Xform::translation(
                    plane.u * f64::from(*u) * 0.25 + plane.v * f64::from(*v) * 0.25,
                ),
                Map::Land(i, j) => {
                    let from =
                        pick(&curves, *i).map_or(anchor, |c| c.pts[*i as usize % c.pts.len()]);
                    let to = pick(&verts, *j).unwrap_or(plane.origin);
                    Xform::translation(to - from)
                }
                Map::Spin(i, deg) => {
                    let about = pick(&verts, *i).unwrap_or(anchor);
                    Xform::rotation(about, n, deg.to_radians())
                }
                Map::Tilt(k, deg) => {
                    let axis = match k % 3 {
                        0 => plane.u,
                        1 => plane.v,
                        _ => DVec3::new(0.3, -0.5, 0.8),
                    };
                    Xform::rotation(anchor, axis, deg.to_radians())
                }
                Map::Scale(f) => Xform::scaling(anchor, DVec3::splat(*f)),
                Map::Stretch(x, y) => Xform::scaling(anchor, DVec3::new(*x, *y, 1.0)),
                Map::Mirror => Xform::scaling(anchor, DVec3::new(-1.0, 1.0, 1.0)),
                Map::Lift(h) => Xform::translation(n * *h),
            };
            if ids.is_empty() && regions.is_empty() && points.is_empty() {
                return None;
            }
            Op::TransformSketch {
                sketch: sid,
                points,
                edges: Vec::new(),
                regions,
                curves: ids,
                xform,
                copy: *copy,
                count: u32::from(*count),
            }
        }
        Step::Extrude { region, d: dist } => {
            let regions: Vec<u64> = sk
                .regions()
                .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
                .collect();
            Op::Extrude {
                sketch: sid,
                region: pick(&regions, *region)?,
                distance: *dist,
            }
        }
        Step::Push {
            object,
            flat,
            d: dist,
        } => {
            let objs = m.object_ids();
            let o = pick(&objs, *object)?;
            let (lo, hi) = m.object(o)?.bounds();
            let h = (hi - lo).dot(plane.normal.abs());
            Op::PushPull {
                object: o,
                face: 2,
                distance: if *flat { -h.abs() } else { *dist },
            }
        }
    })
}

fn run(p: ([f64; 3], [f64; 3]), steps: &[Step]) -> Result<(), TestCaseError> {
    let mut d = Document::with_ground_sketch();
    let mut ops: Vec<Op> = Vec::new();
    let plane = Plane::new(DVec3::from_array(p.0), DVec3::from_array(p.1))
        .map_err(|e| TestCaseError::fail(e.to_string()))?;
    let sid = if p.1 == [0.0, 0.0, 1.0] && p.0 == [0.0; 3] {
        1
    } else {
        let op = Op::NewSketch {
            origin: p.0,
            normal: p.1,
        };
        d.apply(&op)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        ops.push(op);
        d.model().sketch_ids()[1]
    };
    let plane = *d.model().sketch(sid).map_or(&plane, |s| s.plane());
    for step in steps {
        let Some(op) = resolve(step, &d, &plane, sid) else {
            continue;
        };
        let before = d.hash();
        let snap = (d.log().len(), d.depth());
        // Only points selected, one of them a curve's: the op must be refused.
        let lone_on_curve = match &op {
            Op::TransformSketch {
                points,
                edges,
                regions,
                curves,
                ..
            } if edges.is_empty() && regions.is_empty() && curves.is_empty() => {
                let s = d.model().sketch(sid);
                points.iter().any(|p| {
                    s.and_then(|s| s.vertex_at(plane.to_local(DVec3::from_array(*p))))
                        .is_some_and(|v| s.is_some_and(|s| !s.curves_at(v).is_empty()))
                })
            }
            _ => false,
        };
        match d.apply(&op) {
            Err(e) => {
                prop_assert!(
                    !e.is_bug(),
                    "{op:?}: the validator rejected the result: {e}"
                );
                prop_assert_eq!(d.hash(), before, "refused but changed: {:?}", op);
                prop_assert_eq!((d.log().len(), d.depth()), snap);
            }
            Ok(_) => {
                prop_assert!(!lone_on_curve, "a lone curve point moved: {:?}", op);
                ops.push(op.clone());
                if let Ok(path) = std::env::var("CURVE_PROPS_DUMP") {
                    let _ = std::fs::write(path, serde_json::to_string(&ops).unwrap_or_default());
                }
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
                let text = serde_json::to_string(&d.model().snapshot())
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                let back = Model::from_snapshot(
                    &serde_json::from_str(&text).map_err(|e| TestCaseError::fail(e.to_string()))?,
                )
                .map_err(|e| TestCaseError::fail(format!("snapshot after {op:?}: {e}")))?;
                // What each sketch and each solid holds (a solid in a reused slot gets a new
                // public id on open, which the model hash includes; that predates curves).
                let m = d.model();
                prop_assert_eq!(m.sketch_ids().len(), back.sketch_ids().len());
                for ((_, a), (_, b)) in m.sketches().into_iter().zip(back.sketches()) {
                    prop_assert_eq!(
                        sawhorse_kernel::hash::sketch_hash(a),
                        sawhorse_kernel::hash::sketch_hash(b),
                        "sketch round trip after {:?}",
                        op
                    );
                }
                prop_assert_eq!(m.object_ids().len(), back.object_ids().len());
                for ((_, a), (_, b)) in m.objects().into_iter().zip(back.objects()) {
                    prop_assert_eq!(a.curves(), b.curves());
                    prop_assert_eq!(
                        sawhorse_kernel::hash::object_hash(a),
                        sawhorse_kernel::hash::object_hash(b),
                        "solid round trip after {:?}",
                        op
                    );
                }
                for ((_, s), (_, t)) in d.model().sketches().into_iter().zip(back.sketches()) {
                    let a: Vec<_> = s
                        .curves()
                        .map(|(k, c)| (k, c.kind, c.chain.len()))
                        .collect();
                    let b: Vec<_> = t
                        .curves()
                        .map(|(k, c)| (k, c.kind, c.chain.len()))
                        .collect();
                    prop_assert_eq!(a, b, "curves after the round trip");
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
    let _ = PI;
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        ..ProptestConfig::default()
    })]

    #[test]
    fn curves_keep_every_invariant(
        p in plane(),
        steps in prop::collection::vec(step(), 2..9),
    ) {
        run(p, &steps)?;
    }
}
