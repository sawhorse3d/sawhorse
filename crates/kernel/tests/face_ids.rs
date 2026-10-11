//! Persistent face ids (lane 3): faces carry ids that survive edits, so the UI addresses
//! faces by id, never by ordinal.
//!
//! - extrude numbers base 1, cap 2, walls 3.. in face order;
//! - the exact (topological) push path keeps every id, including the moved face's, and
//!   gives side walls fresh ids;
//! - split_face: the first piece keeps the id; imprint: the sub-face gets a fresh id;
//! - the boolean path inherits ids through Manifold's per-triangle face id (the swept
//!   prism's cap carries the pushed face's id) and settles duplicates and renumbered faces
//!   by plane + centroid + area (`sawhorse_kernel::boolean::resolve_face_ids`);
//! - ids are never reused, even after the face they named is gone.

use std::collections::BTreeSet;

use proptest::prelude::*;
use sawhorse_kernel::DVec3;
use sawhorse_kernel::boolean::{
    BoolOp, ID_MATCH_DISTANCE, combine, default_engine, face_signature, resolve_face_ids,
};
use sawhorse_kernel::brep::{PolyFace, Polys};
use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::push::prism;

fn run(ops: &[Op]) -> Model {
    let mut m = Model::with_ground_sketch();
    for (i, op) in ops.iter().enumerate() {
        m.apply(op).unwrap_or_else(|e| panic!("op {i} {op:?}: {e}"));
    }
    m
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Op {
    Op::AddRectangle {
        sketch: 1,
        x,
        y,
        w,
        h,
    }
}

fn extrude(d: f64) -> Op {
    Op::Extrude {
        sketch: 1,
        region: 1,
        distance: d,
    }
}

fn push(face: u64, d: f64) -> Op {
    Op::PushPull {
        object: 1,
        face,
        distance: d,
    }
}

fn imprint_top(z: f64) -> Op {
    Op::Imprint {
        object: 1,
        face: 2,
        points: vec![
            [0.5, 0.25, z],
            [1.5, 0.25, z],
            [1.5, 0.75, z],
            [0.5, 0.75, z],
        ],
    }
}

fn ids(m: &Model) -> Vec<u64> {
    m.object(1).unwrap().brep().persistent_face_ids()
}

/// The face with id `id`: its plane's normal and offset along it.
fn plane_of(m: &Model, id: u64) -> Option<(DVec3, f64)> {
    let o = m.object(1)?;
    let fi = o.face_ordinal(id)?;
    let p = o.brep().to_polys().plane(fi)?;
    Some((p.normal, p.normal.dot(p.origin)))
}

#[test]
fn extrude_numbers_faces_in_order() {
    let m = run(&[rect(0.0, 0.0, 2.0, 1.0), extrude(1.0)]);
    assert_eq!(ids(&m), vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(m.object(1).unwrap().next_face_id(), 7);
    let (n, off) = plane_of(&m, 2).unwrap();
    assert!(n.z > 0.99 && (off - 1.0).abs() < 1e-12, "id 2 is the cap");
    // The view mesh names faces by id.
    let mesh = sawhorse_kernel::mesh::view_mesh(&m).unwrap();
    let parts: Vec<u64> = mesh.parts.iter().map(|p| p.index).collect();
    assert_eq!(parts, vec![1, 2, 3, 4, 5, 6]);
}

#[test]
fn exact_push_path_keeps_ids_and_gives_walls_new_ones() {
    let mut m = run(&[rect(0.0, 0.0, 2.0, 1.0), extrude(1.0), imprint_top(1.0)]);
    assert_eq!(ids(&m), vec![1, 2, 3, 4, 5, 6, 7]);
    // Pull the imprint up: a boss. The sub-face keeps id 7; four new walls 8..=11.
    assert_eq!(m.apply(&push(7, 0.3)).unwrap(), OpOutcome::FaceMoved);
    let after: BTreeSet<u64> = ids(&m).into_iter().collect();
    assert_eq!(after, (1..=11).collect::<BTreeSet<u64>>());
    let (n, off) = plane_of(&m, 7).unwrap();
    assert!(
        n.z > 0.99 && (off - 1.3).abs() < 1e-12,
        "the boss top is still id 7"
    );
    // Push it back flush: the walls vanish (coplanar merge keeps the user-drawn imprint
    // edges), id 7 is on the top again, and ids 8..=11 are gone for good.
    m.apply(&push(7, -0.3)).unwrap();
    let back: BTreeSet<u64> = ids(&m).into_iter().collect();
    assert_eq!(back, (1..=7).collect::<BTreeSet<u64>>());
    assert_eq!(m.object(1).unwrap().next_face_id(), 12);
    // A new imprint never reuses a retired id.
    let out = m
        .apply(&Op::Imprint {
            object: 1,
            face: 2,
            points: vec![
                [0.1, 0.1, 1.0],
                [0.3, 0.1, 1.0],
                [0.3, 0.2, 1.0],
                [0.1, 0.2, 1.0],
            ],
        })
        .unwrap();
    assert_eq!(out, OpOutcome::Imprinted { face: 12 });
}

#[test]
fn split_keeps_the_id_on_the_first_piece() {
    let mut m = run(&[rect(0.0, 0.0, 1.0, 1.0), extrude(1.0)]);
    let out = m
        .apply(&Op::SplitFace {
            object: 1,
            face: 2,
            path: vec![[0.5, 0.0, 1.0], [0.5, 1.0, 1.0]],
        })
        .unwrap();
    assert_eq!(out, OpOutcome::FaceSplit { faces: vec![2, 7] });
    // Splitting the second piece (a 0.5 x 1 half, whichever side) across: 7 stays, 8 is new.
    let o = m.object(1).unwrap();
    let c = face_signature(&o.brep().to_polys(), o.face_ordinal(7).unwrap())
        .unwrap()
        .centroid;
    let out = m
        .apply(&Op::SplitFace {
            object: 1,
            face: 7,
            path: vec![[c.x - 0.25, 0.5, 1.0], [c.x + 0.25, 0.5, 1.0]],
        })
        .unwrap();
    assert_eq!(out, OpOutcome::FaceSplit { faces: vec![7, 8] });
    // Faces are addressed by id: an id that names nothing is a typed refusal.
    let e = m.apply(&push(99, 0.1)).unwrap_err();
    assert!(e.to_string().contains("no face #99"), "{e}");
    let e = m.apply(&push(0, 0.1)).unwrap_err();
    assert!(!e.is_bug(), "{e}");
}

#[test]
fn boolean_pocket_inherits_the_pushed_face_id() {
    // Run the boolean path by hand (the exact path would take this pocket otherwise):
    // box minus the prism swept by the imprint 0.4 down.
    let m = run(&[rect(0.0, 0.0, 2.0, 1.0), extrude(1.0), imprint_top(1.0)]);
    let o = m.object(1).unwrap();
    let p = o.brep().to_polys();
    let fi = o.face_ordinal(7).unwrap();
    let plane = p.plane(fi).unwrap();
    let engine = default_engine().unwrap();
    let res = combine(
        &p,
        &prism(&p, fi, &plane, -0.4),
        BoolOp::Difference,
        engine.as_ref(),
    )
    .unwrap()
    .unwrap();
    let floor = res
        .faces
        .iter()
        .position(|f| f.id == Some(7))
        .expect("the pocket floor carries id 7");
    let sig = face_signature(&res, floor).unwrap();
    assert!((sig.centroid - DVec3::new(1.0, 0.5, 0.6)).length() < 1e-9);
    assert!((sig.area - 0.5).abs() < 1e-9);
    // The top (now with a hole) keeps 2, the outer walls and base keep theirs; the pocket
    // walls are new (no id yet: the object hands them fresh ones).
    let got: BTreeSet<u64> = res.faces.iter().filter_map(|f| f.id).collect();
    assert_eq!(got, (1..=7).collect::<BTreeSet<u64>>());
    assert_eq!(res.faces.iter().filter(|f| f.id.is_none()).count(), 4);
}

#[test]
fn resolve_settles_duplicates_and_matches_renumbered_faces() {
    // Two unit squares on z = 0 side by side, both claiming id 5; the source face 5 was the
    // right one. A third square on z = 1 has no id but matches source face 9 within
    // tolerance.
    let sq = |x: f64, z: f64| -> Vec<DVec3> {
        vec![
            DVec3::new(x, 0.0, z),
            DVec3::new(x + 1.0, 0.0, z),
            DVec3::new(x + 1.0, 1.0, z),
            DVec3::new(x, 1.0, z),
        ]
    };
    let mut points = Vec::new();
    let mut faces = Vec::new();
    for (x, z, id) in [(0.0, 0.0, Some(5)), (1.0, 0.0, Some(5)), (0.0, 1.0, None)] {
        let base = points.len();
        points.extend(sq(x, z));
        let mut f = PolyFace::new((base..base + 4).collect(), vec![]);
        f.id = id;
        faces.push(f);
    }
    let mut p = Polys {
        points,
        faces,
        user_edges: Default::default(),
    };
    let src5 = face_signature(&p, 1).unwrap();
    let mut src9 = face_signature(&p, 2).unwrap();
    src9.centroid += DVec3::new(0.5 * ID_MATCH_DISTANCE, 0.0, 0.0);
    resolve_face_ids(&mut p, &[(5, src5), (9, src9)]);
    let got: Vec<Option<u64>> = p.faces.iter().map(|f| f.id).collect();
    assert_eq!(got, vec![None, Some(5), Some(9)]);
    // Beyond tolerance: no match.
    let mut q = p.clone();
    q.faces[2].id = None;
    let mut far = src9;
    far.centroid += DVec3::new(3.0 * ID_MATCH_DISTANCE, 0.0, 0.0);
    resolve_face_ids(&mut q, &[(9, far)]);
    assert_eq!(q.faces[2].id, None);
}

fn normal() -> impl Strategy<Value = [f64; 3]> {
    (-1.0..1.0f64, -1.0..1.0f64, -1.0..1.0f64)
        .prop_filter("non-zero", |(x, y, z)| x * x + y * y + z * z > 0.05)
        .prop_map(|(x, y, z)| [x, y, z])
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Imprint on any face of a box on any plane and push or pull the sub-face by any
    /// amount: unless it went right through, the face with the sub-face's id is the moved
    /// face (its plane offset by exactly the distance); every original id survives.
    #[test]
    fn pushed_face_keeps_its_id(n in normal(), w in 0.5..3.0f64, h in 0.5..3.0f64, d in 0.5..3.0f64,
                                face in 0usize..6, k in prop_oneof![-4.0..-0.01f64, 0.01..2.0f64]) {
        let mut m = Model::replay(&[
            Op::NewSketch { origin: [0.2, 0.1, -0.3], normal: n },
            Op::AddRectangle { sketch: 1, x: 0.0, y: 0.0, w, h },
            Op::Extrude { sketch: 1, region: 1, distance: d },
        ]).unwrap();
        let p = m.object(1).unwrap().brep().to_polys();
        let c: Vec<_> = p.faces[face].outer.iter().map(|&i| p.points[i]).collect();
        let at = |s: f64, t: f64| c[0].lerp(c[1], s).lerp(c[3].lerp(c[2], s), t).to_array();
        m.apply(&Op::Imprint { object: 1, face: face as u64 + 1, points: vec![at(0.3, 0.3), at(0.7, 0.3), at(0.7, 0.7), at(0.3, 0.7)] }).unwrap();
        let (n0, off0) = plane_of(&m, 7).unwrap();
        let depth = {
            let pp = m.object(1).unwrap().brep().to_polys();
            -pp.points.iter().map(|&q| n0.dot(q) - off0).fold(0.0f64, f64::min)
        };
        match m.apply(&push(7, k)) {
            Ok(OpOutcome::FaceMoved) => {
                let now: BTreeSet<u64> = ids(&m).into_iter().collect();
                if k > -depth + 1e-6 {
                    let (n1, off1) = plane_of(&m, 7).expect("the pushed face keeps id 7");
                    prop_assert!(n1.dot(n0) > 1.0 - 1e-9);
                    prop_assert!((off1 - off0 - k).abs() < 1e-7, "offset {} vs {}", off1 - off0, k);
                    for id in 1..=6u64 {
                        prop_assert!(now.contains(&id), "original face {} survives", id);
                    }
                } else {
                    prop_assert!(!now.contains(&7), "pushed through: the sub-face is gone");
                }
                prop_assert!(now.iter().all(|&i| i < m.object(1).unwrap().next_face_id()));
            }
            Ok(other) => prop_assert!(false, "unexpected {:?}", other),
            Err(e) => prop_assert!(!e.is_bug(), "validator bug: {}", e),
        }
    }
}
