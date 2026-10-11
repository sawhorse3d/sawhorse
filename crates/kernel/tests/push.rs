//! General push/pull (lane 2): every path, with volume, surface area and Euler checks.
//!
//! Paths (see `sawhorse_kernel::push`): collapse, topological (slide + side walls), boolean
//! (union / difference with the swept prism through manifold-rust, re-polygonised).

use proptest::prelude::*;
use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::{KernelError, PushError};

const G: u64 = 1;

fn run(ops: &[Op]) -> Model {
    let mut m = Model::with_ground_sketch();
    for (i, op) in ops.iter().enumerate() {
        m.apply(op).unwrap_or_else(|e| panic!("op {i} {op:?}: {e}"));
    }
    m
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Op {
    Op::AddRectangle {
        sketch: G,
        x,
        y,
        w,
        h,
    }
}

fn extrude(region: u64, d: f64) -> Op {
    Op::Extrude {
        sketch: G,
        region,
        distance: d,
    }
}

/// Push/pull the face with persistent id `face`. On a freshly extruded box the base is 1,
/// the cap 2, the walls 3..=6; an imprint's sub-face is 7.
fn push(face: u64, d: f64) -> Op {
    Op::PushPull {
        object: 1,
        face,
        distance: d,
    }
}

fn imprint_top(z: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> Op {
    Op::Imprint {
        object: 1,
        face: 2,
        points: vec![[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]],
    }
}

struct Shape {
    faces: usize,
    holes: usize,
    shells: usize,
    genus: i64,
    volume: f64,
    area: f64,
}

fn shape(m: &Model) -> Shape {
    let o = m.object(1).expect("object 1");
    let r = o.validate().expect("valid solid");
    assert!(r.watertight);
    for s in &r.shells {
        assert_eq!(
            s.euler % 2,
            0,
            "Euler characteristic must be even for a closed shell"
        );
    }
    Shape {
        faces: r.faces,
        holes: r.holes,
        shells: r.shells.len(),
        genus: r.shells.iter().map(|s| s.genus.unwrap_or(-1)).sum(),
        volume: o.volume(),
        area: o.surface_area(),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

/// A 2 x 1 x 1 box with a 1 x 0.5 imprint centred on its top (object 1, sub-face id 7).
fn boxed_with_imprint() -> Vec<Op> {
    vec![
        rect(0.0, 0.0, 2.0, 1.0),
        extrude(1, 1.0),
        imprint_top(1.0, 0.5, 0.25, 1.5, 0.75),
    ]
}

#[test]
fn cap_pulls_and_pushes_without_new_topology() {
    let mut m = run(&[rect(0.0, 0.0, 1.0, 1.0), extrude(1, 1.0)]);
    assert_eq!(m.apply(&push(2, 0.25)).unwrap(), OpOutcome::FaceMoved);
    let s = shape(&m);
    assert_eq!((s.faces, s.genus), (6, 0));
    assert!(close(s.volume, 1.25) && close(s.area, 2.0 + 4.0 * 1.25));
    m.apply(&push(2, -0.75)).unwrap();
    assert!(close(shape(&m).volume, 0.5));
    // A side wall: its neighbours run along the push, so it slides too.
    m.apply(&push(3, 0.5)).unwrap();
    let s = shape(&m);
    assert_eq!(s.faces, 6);
    assert!(close(s.volume, 0.75));
}

#[test]
fn imprint_pulled_up_grows_side_walls() {
    let mut ops = boxed_with_imprint();
    ops.push(push(7, 0.3));
    let m = run(&ops);
    let s = shape(&m);
    assert_eq!((s.faces, s.holes, s.shells, s.genus), (11, 1, 1, 0));
    assert!(close(s.volume, 2.0 + 0.5 * 0.3));
    assert!(
        close(s.area, 10.0 + 3.0 * 0.3),
        "boss walls: perimeter 3 x 0.3"
    );
}

#[test]
fn imprint_pushed_in_makes_a_pocket_and_flush_restores_the_imprint() {
    let mut ops = boxed_with_imprint();
    ops.push(push(7, -0.4));
    let mut m = run(&ops);
    let s = shape(&m);
    assert_eq!((s.faces, s.holes, s.genus), (11, 1, 0));
    assert!(close(s.volume, 2.0 - 0.5 * 0.4));
    assert!(close(s.area, 10.0 + 0.4 * 3.0));
    // Pull the pocket floor back up flush: the walls vanish (boolean path), but the user's
    // imprint edges are kept, so the top is still a face with a hole plus the sub-face.
    m.apply(&push(7, 0.4)).unwrap();
    let s = shape(&m);
    assert_eq!((s.faces, s.holes, s.genus), (7, 1, 0));
    assert!(close(s.volume, 2.0) && close(s.area, 10.0));
    let p = m.object(1).unwrap().brep().to_polys();
    assert_eq!(p.user_edges.len(), 4, "the imprint survived");
}

#[test]
fn push_through_the_far_side_drills_a_hole() {
    let mut ops = boxed_with_imprint();
    ops.push(push(7, -1.5));
    let m = run(&ops);
    let s = shape(&m);
    assert_eq!((s.faces, s.holes, s.shells, s.genus), (10, 2, 1, 1));
    assert!(close(s.volume, 2.0 - 0.5));
    assert!(close(s.area, 10.0 - 2.0 * 0.5 + 3.0 * 1.0));
    // Exactly to the far side: same hole (the prism's end is coplanar with the bottom).
    let mut ops = boxed_with_imprint();
    ops.push(push(7, -1.0));
    let s = shape(&run(&ops));
    assert_eq!((s.faces, s.genus), (10, 1));
    assert!(close(s.volume, 1.5));
}

#[test]
fn boss_pushed_back_through_the_box() {
    // Pull the imprint up into a boss, then push the boss's top down through everything.
    let mut ops = boxed_with_imprint();
    ops.push(push(7, 0.3));
    ops.push(push(7, -1.5));
    let s = shape(&run(&ops));
    assert_eq!((s.genus, s.shells), (1, 1));
    assert!(close(s.volume, 1.5));
}

#[test]
fn diagonal_half_of_the_top_pulled_up_slides_one_corner() {
    let mut m = run(&[rect(0.0, 0.0, 1.0, 1.0), extrude(1, 1.0)]);
    let out = m
        .apply(&Op::SplitFace {
            object: 1,
            face: 2,
            path: vec![[0.0, 0.0, 1.0], [1.0, 1.0, 1.0]],
        })
        .unwrap();
    assert_eq!(out, OpOutcome::FaceSplit { faces: vec![2, 7] });
    m.apply(&push(7, 0.5)).unwrap();
    let s = shape(&m);
    assert!(close(s.volume, 1.0 + 0.5 * 0.5));
    assert_eq!((s.shells, s.genus), (1, 0));
}

#[test]
fn pulling_into_the_other_arm_fills_the_notch() {
    // U in plan: 3 x 2 outer, 1 x 1 notch from the top edge, extruded 1.
    let mut m = Model::with_ground_sketch();
    m.apply(&Op::AddPolyline {
        sketch: G,
        points: vec![
            [0.0, 0.0],
            [3.0, 0.0],
            [3.0, 2.0],
            [2.0, 2.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ],
        closed: true,
    })
    .unwrap();
    m.apply(&extrude(1, 1.0)).unwrap();
    assert!(close(shape(&m).volume, 5.0));
    // The left arm's inner wall (x = 1, facing +x).
    let p = m.object(1).unwrap().brep().to_polys();
    let face = (0..p.faces.len())
        .find(|&i| {
            let pl = p.plane(i).unwrap();
            pl.normal.x > 0.99
                && (pl.signed_distance(sawhorse_kernel::DVec3::new(1.0, 1.5, 0.5))).abs() < 1e-9
        })
        .unwrap();
    let face = m.object(1).unwrap().face_id_at(face).unwrap();
    // Pull it 1.5 to the right: across the notch and into the right arm (boolean union).
    m.apply(&push(face, 1.5)).unwrap();
    let s = shape(&m);
    assert!(close(s.volume, 6.0), "the notch is filled: {}", s.volume);
    assert_eq!((s.shells, s.genus), (1, 0));
}

#[test]
fn tube_hole_wall_moves_and_pushes_through() {
    let mut m = run(&[
        rect(0.0, 0.0, 4.0, 4.0),
        rect(1.0, 1.0, 2.0, 2.0),
        extrude(1, 1.0),
    ]);
    assert_eq!(shape(&m).genus, 1);
    let p = m.object(1).unwrap().brep().to_polys();
    let wall = (0..p.faces.len())
        .find(|&i| {
            let pl = p.plane(i).unwrap();
            pl.normal.x > 0.99 && pl.origin.x.abs() < 1.0 + 1e-9 && pl.origin.x > 0.5
        })
        .unwrap();
    let wall = m.object(1).unwrap().face_id_at(wall).unwrap();
    m.apply(&push(wall, 0.5)).unwrap();
    assert!(close(shape(&m).volume, 13.0));
    // Pushing the hole wall back through the outer wall opens the ring into a C.
    m.apply(&push(wall, -2.0)).unwrap();
    let s = shape(&m);
    assert_eq!((s.shells, s.genus), (1, 0));
    assert!(close(s.volume, 13.0 - 1.5 * 2.0), "{}", s.volume);
}

#[test]
fn collapse_and_refusals() {
    let mut m = run(&[rect(0.0, 0.0, 1.0, 1.0), extrude(1, 1.0)]);
    let before = m.hash();
    for (op, want) in [
        (push(2, 0.0), PushError::ZeroDistance),
        (push(2, 5e-6), PushError::ZeroDistance),
        (push(2, f64::NAN), PushError::OutOfBounds),
        (push(2, 2e4), PushError::OutOfBounds),
        (push(42, 1.0), PushError::NoSuchFace(42)),
    ] {
        assert_eq!(m.apply(&op), Err(KernelError::Push(want)), "{op:?}");
        assert_eq!(m.hash(), before);
    }
    assert!(
        matches!(m.apply(&push(2, -1.0)).unwrap(), OpOutcome::ObjectCollapsed { ref regions } if regions.len() == 1)
    );
    let mut m = run(&[rect(0.0, 0.0, 1.0, 1.0), extrude(1, 1.0)]);
    assert_eq!(
        m.apply(&push(2, -3.0)).unwrap(),
        OpOutcome::ObjectCollapsed { regions: vec![] },
        "pushed past the base: gone, and nothing lands on the sketch"
    );
}

fn normal() -> impl Strategy<Value = [f64; 3]> {
    prop_oneof![
        2 => Just([0.0, 0.0, 1.0]),
        3 => (-1.0..1.0f64, -1.0..1.0f64, -1.0..1.0f64)
            .prop_filter("non-degenerate normal", |(x, y, z)| x * x + y * y + z * z > 1e-2)
            .prop_map(|(x, y, z)| [x, y, z]),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Imprint on any face of a box on any plane, pull or push the sub-face by any amount:
    /// the result is valid with the right volume (or the whole thing collapses), and pushing
    /// back by the same amount restores the volume.
    #[test]
    fn imprint_push_pull_volumes(n in normal(), w in 0.5..3.0f64, h in 0.5..3.0f64, d in 0.5..3.0f64, face in 0u32..6,
                                 k in prop_oneof![-4.0..-0.01f64, 0.01..2.0f64]) {
        let mut m = Model::replay(&[
            Op::NewSketch { origin: [0.2, 0.1, -0.3], normal: n },
            Op::AddRectangle { sketch: 1, x: 0.0, y: 0.0, w, h },
            Op::Extrude { sketch: 1, region: 1, distance: d },
        ]).unwrap();
        let p = m.object(1).unwrap().brep().to_polys();
        let c: Vec<_> = p.faces[face as usize].outer.iter().map(|&i| p.points[i]).collect();
        let at = |s: f64, t: f64| c[0].lerp(c[1], s).lerp(c[3].lerp(c[2], s), t).to_array();
        m.apply(&Op::Imprint { object: 1, face: u64::from(face) + 1, points: vec![at(0.3, 0.3), at(0.7, 0.3), at(0.7, 0.7), at(0.3, 0.7)] }).unwrap();
        let p = m.object(1).unwrap().brep().to_polys();
        let pl = p.plane(6).unwrap();
        let area: f64 = sawhorse_tessellate::signed_area(&p.faces[6].outer.iter().map(|&i| pl.to_local(p.points[i])).collect::<Vec<_>>());
        let depth = {
            // Thickness of the box behind this face.
            let other = p.points.iter().map(|&q| pl.signed_distance(q)).fold(0.0f64, f64::min);
            -other
        };
        let v0 = m.object(1).unwrap().volume();
        // The sub-face keeps persistent id 7 whichever path the push takes (a pocket's floor
        // on the boolean path inherits it from the swept prism's cap), so it can be pushed back.
        match m.apply(&Op::PushPull { object: 1, face: 7, distance: k }) {
            Ok(OpOutcome::FaceMoved) => {
                let s = shape(&m);
                let expect = if k < -depth { v0 - area * depth } else { v0 + area * k };
                prop_assert!(close(s.volume, expect), "volume {} expected {} (k {}, depth {})", s.volume, expect, k, depth);
                if k > -depth + 1e-6 {
                    // Back by the same amount.
                    let back = m.apply(&Op::PushPull { object: 1, face: 7, distance: -k });
                    prop_assert!(back.is_ok(), "{:?}", back);
                    prop_assert!(close(shape(&m).volume, v0));
                }
            }
            Ok(other) => prop_assert!(false, "unexpected {:?}", other),
            Err(e) => prop_assert!(!e.is_bug(), "validator bug: {}", e),
        }
    }
}

#[test]
fn ctrl_pull_leaves_a_seam_where_the_face_was() {
    let mut m = run(&[rect(0.0, 0.0, 1.0, 1.0), extrude(1, 1.0)]);
    let out = m
        .apply(&Op::PushPullNewFace {
            object: 1,
            face: 2,
            distance: 0.5,
        })
        .unwrap();
    assert_eq!(out, OpOutcome::FaceMoved);
    let s = shape(&m);
    assert!(close(s.volume, 1.5));
    assert_eq!(s.faces, 10, "base, cap, four old walls, four new walls");
    // The cap kept its id and moved up.
    let o = m.object(1).unwrap();
    let p = o.brep().to_polys();
    let cap = p.plane(o.face_ordinal(2).unwrap()).unwrap();
    assert!((cap.origin.z - 1.5).abs() < 1e-12);
    // The seam is user-drawn: erasing one seam edge merges those two walls.
    m.apply(&Op::EraseEdge {
        object: 1,
        a: [0.0, 0.0, 1.0],
        b: [1.0, 0.0, 1.0],
    })
    .unwrap();
    assert_eq!(shape(&m).faces, 9);
    // An inward push with the flag is an ordinary push.
    let mut m = run(&[rect(0.0, 0.0, 1.0, 1.0), extrude(1, 1.0)]);
    m.apply(&Op::PushPullNewFace {
        object: 1,
        face: 2,
        distance: -0.25,
    })
    .unwrap();
    let s = shape(&m);
    assert_eq!(s.faces, 6);
    assert!(close(s.volume, 0.75));
}
