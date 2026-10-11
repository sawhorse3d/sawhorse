//! Property tests for the Object tier's face edits (ADR-0001, lane 2).
//!
//! - split_face then merging the pieces (dissolving the cut) is the identity, on any face of
//!   a box on any plane;
//! - imprint then merging is the identity;
//! - every accepted edit leaves a valid, watertight solid with the same volume and an Euler
//!   characteristic of 2; every refused edit leaves the model bit-for-bit unchanged
//!   (transactional), with a typed error, never a validator bug.

use std::collections::BTreeSet;

use proptest::prelude::*;
use sawhorse_kernel::brep::{Brep, Polys};
use sawhorse_kernel::faceops::merge_coplanar;
use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::validate::validate;
use sawhorse_kernel::{DVec2, DVec3};

fn normal() -> impl Strategy<Value = [f64; 3]> {
    prop_oneof![
        3 => Just([0.0, 0.0, 1.0]),
        1 => Just([1.0, 0.0, 0.0]),
        3 => (-1.0..1.0f64, -1.0..1.0f64, -1.0..1.0f64)
            .prop_filter("non-degenerate normal", |(x, y, z)| x * x + y * y + z * z > 1e-2)
            .prop_map(|(x, y, z)| [x, y, z]),
    ]
}

fn size() -> impl Strategy<Value = f64> {
    0.05..4.0f64
}

/// A box on a plane: returns the model (object id 1).
fn boxed(n: [f64; 3], w: f64, h: f64, d: f64) -> Model {
    Model::replay(&[
        Op::NewSketch {
            origin: [0.3, -0.2, 0.1],
            normal: n,
        },
        Op::AddRectangle {
            sketch: 1,
            x: -0.5,
            y: 0.25,
            w,
            h,
        },
        Op::Extrude {
            sketch: 1,
            region: 1,
            distance: d,
        },
    ])
    .unwrap()
}

/// Faces as sets of quantised position rings, canonical rotation, holes sorted.
type Canon = BTreeSet<(Vec<(i64, i64, i64)>, Vec<Vec<(i64, i64, i64)>>)>;

fn canon(p: &Polys) -> Canon {
    let q = |i: usize| {
        let v = p.points[i];
        (
            (v.x * 1e8).round() as i64,
            (v.y * 1e8).round() as i64,
            (v.z * 1e8).round() as i64,
        )
    };
    let ring = |r: &Vec<usize>| {
        let pts: Vec<_> = r.iter().map(|&i| q(i)).collect();
        let s = (0..pts.len()).min_by_key(|&i| pts[i]).unwrap();
        pts[s..]
            .iter()
            .chain(pts[..s].iter())
            .copied()
            .collect::<Vec<_>>()
    };
    p.faces
        .iter()
        .map(|f| {
            let mut holes: Vec<_> = f.holes.iter().map(ring).collect();
            holes.sort();
            (ring(&f.outer), holes)
        })
        .collect()
}

/// Corners of face `f` of object 1 (outer loop, world).
fn corners(m: &Model, f: usize) -> Vec<DVec3> {
    let p = m.object(1).unwrap().brep().to_polys();
    p.faces[f].outer.iter().map(|&i| p.points[i]).collect()
}

fn check_solid(m: &Model, volume: f64) -> Result<(), TestCaseError> {
    let o = m.object(1).unwrap();
    let r = o
        .validate()
        .map_err(|e| TestCaseError::fail(e.to_string()))?;
    prop_assert!(r.watertight);
    prop_assert_eq!(r.shells.len(), 1);
    prop_assert_eq!(r.shells[0].euler, 2, "V - E + F - H");
    prop_assert!((o.volume() - volume).abs() <= 1e-9 * volume.max(1.0));
    Ok(())
}

fn merged(m: &Model) -> Canon {
    let mut p = m.object(1).unwrap().brep().to_polys();
    merge_coplanar(&mut p, false);
    let b = Brep::build(&p).unwrap();
    validate(&b).unwrap();
    canon(&p)
}

fn frac() -> impl Strategy<Value = f64> {
    (1i32..8).prop_map(|k| f64::from(k) / 8.0)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 192, ..ProptestConfig::default() })]

    #[test]
    fn split_then_merge_is_identity(n in normal(), w in size(), h in size(), d in size(), face in 0usize..6,
                                    e1 in 0usize..4, e2 in 1usize..4, t1 in frac(), t2 in frac(), bend in -0.3..0.3f64, on_vertex in any::<bool>()) {
        let mut m = boxed(n, w, h, d);
        let original = canon(&m.object(1).unwrap().brep().to_polys());
        let vol = m.object(1).unwrap().volume();
        let c = corners(&m, face);
        // Two points on different edges of the (convex, rectangular) face, or two corners.
        let e2 = (e1 + e2) % 4;
        let (a, b) = if on_vertex {
            (c[e1], c[e2])
        } else {
            (c[e1].lerp(c[(e1 + 1) % 4], t1), c[e2].lerp(c[(e2 + 1) % 4], t2))
        };
        prop_assume!(a.distance(b) > 1e-3);
        let centre = c.iter().copied().sum::<DVec3>() / 4.0;
        let mid = a.lerp(b, 0.5).lerp(centre, bend.abs());
        let path: Vec<[f64; 3]> = if bend > 0.0 { vec![a.to_array(), mid.to_array(), b.to_array()] } else { vec![a.to_array(), b.to_array()] };
        match m.apply(&Op::SplitFace { object: 1, face: face as u32, path }) {
            Ok(OpOutcome::FaceSplit { faces }) => {
                prop_assert_eq!(faces.len(), 2);
                check_solid(&m, vol)?;
                prop_assert_eq!(merged(&m), original);
            }
            Ok(other) => prop_assert!(false, "unexpected {:?}", other),
            // Neighbouring corners joined straight: the cut would run along an edge.
            Err(e) => {
                let adjacent = (e2 + 1) % 4 == e1 || (e1 + 1) % 4 == e2;
                prop_assert!(!e.is_bug() && on_vertex && adjacent && bend <= 0.0, "{}", e);
            }
        }
    }

    #[test]
    fn imprint_then_merge_is_identity(n in normal(), w in size(), h in size(), d in size(), face in 0usize..6,
                                      u0 in frac(), v0 in frac(), du in 1i32..4, dv in 1i32..4, triangle in any::<bool>()) {
        let mut m = boxed(n, w, h, d);
        let original = canon(&m.object(1).unwrap().brep().to_polys());
        let vol = m.object(1).unwrap().volume();
        let c = corners(&m, face);
        // Bilinear coordinates inside the rectangular face, kept away from its edges.
        let at = |s: f64, t: f64| {
            let s = 0.05 + 0.9 * s.clamp(0.0, 1.0);
            let t = 0.05 + 0.9 * t.clamp(0.0, 1.0);
            c[0].lerp(c[1], s).lerp(c[3].lerp(c[2], s), t)
        };
        let (u1, v1) = ((u0 + f64::from(du) / 16.0).min(1.0), (v0 + f64::from(dv) / 16.0).min(1.0));
        let pts: Vec<[f64; 3]> = if triangle {
            vec![at(u0, v0), at(u1, v0), at(u0, v1)]
        } else {
            vec![at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)]
        }
        .into_iter()
        .map(|p| p.to_array())
        .collect();
        let out = m.apply(&Op::Imprint { object: 1, face: face as u32, points: pts });
        prop_assert!(matches!(out, Ok(OpOutcome::Imprinted { face: 6 })), "{:?}", out);
        check_solid(&m, vol)?;
        let r = m.object(1).unwrap().validate().unwrap();
        prop_assert_eq!((r.faces, r.holes), (7, 1));
        prop_assert_eq!(merged(&m), original);
    }

    #[test]
    fn refused_edits_are_typed_and_transactional(n in normal(), w in size(), h in size(), d in size(), face in 0u32..7,
                                                 pts in prop::collection::vec((-1.5..1.5f64, -1.5..1.5f64), 1..6), imprint in any::<bool>(), lift in prop_oneof![4 => Just(0.0), 1 => -0.01..0.01f64]) {
        let mut m = boxed(n, w, h, d);
        let vol = m.object(1).unwrap().volume();
        let before = m.hash();
        let fi = (face as usize).min(5);
        let c = corners(&m, fi);
        let (o, ux, uy) = (c[0], c[1] - c[0], c[3] - c[0]);
        let nrm = ux.cross(uy).normalize();
        let world: Vec<[f64; 3]> = pts.iter().map(|&(s, t)| (o + ux * s + uy * t + nrm * lift).to_array()).collect();
        let op = if imprint {
            Op::Imprint { object: 1, face, points: world }
        } else {
            Op::SplitFace { object: 1, face, path: world }
        };
        match m.apply(&op) {
            Ok(_) => check_solid(&m, vol)?,
            Err(e) => {
                prop_assert!(!e.is_bug(), "validator bug: {}", e);
                prop_assert_eq!(m.hash(), before);
            }
        }
    }
}

#[test]
fn extruding_consumes_the_region_and_the_face_takes_drawing() {
    let mut m = Model::with_ground_sketch();
    m.apply(&Op::AddRectangle {
        sketch: 1,
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    })
    .unwrap();
    m.apply(&Op::Extrude {
        sketch: 1,
        region: 1,
        distance: 0.5,
    })
    .unwrap();
    let s = m.sketch(1).unwrap();
    assert_eq!(s.region_count(), 0, "the region left the sketch");
    assert_eq!(s.edge_count(), 0, "its edges bounded nothing else");
    // Drawing on the cap is an Object-tier edit.
    let out = m
        .apply(&Op::Imprint {
            object: 1,
            face: 1,
            points: vec![
                [0.25, 0.25, 0.5],
                [0.75, 0.25, 0.5],
                [0.75, 0.75, 0.5],
                [0.25, 0.75, 0.5],
            ],
        })
        .unwrap();
    assert_eq!(out, OpOutcome::Imprinted { face: 6 });
    let out = m
        .apply(&Op::SplitFace {
            object: 1,
            face: 6,
            path: vec![[0.5, 0.25, 0.5], [0.5, 0.75, 0.5]],
        })
        .unwrap();
    assert_eq!(out, OpOutcome::FaceSplit { faces: vec![6, 7] });
    let st = m.stats();
    assert!(st.watertight);
    assert_eq!((st.faces, st.holes, st.euler), (8, 1, 2));
    // The 2D frame is the face's own: a point in the hole region maps back inside it.
    let p = m.object(1).unwrap().brep().to_polys();
    let plane = p.plane(6).unwrap();
    let q: DVec2 = plane.to_local(DVec3::new(0.3, 0.5, 0.5));
    assert!(q.is_finite());
}
