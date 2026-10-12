//! Transforms, copies, arrays, moving part of a solid, severance (lane 4).
//!
//! - Move / Rotate / Scale are one op, `Transform` (an affine map): object-level, face ids
//!   and drawn edges kept, validated; mirrors stay outward; a flattening scale is refused.
//! - `Copy` and `Array` make new objects (new ids and display numbers, same face ids).
//! - `TransformGeometry` moves part of a solid: planar faces follow, a bent face is refused
//!   (typed, naming the face) unless autofold folds it.
//! - A push that cuts all the way through a board leaves two solids (severance by
//!   connectivity), each keeping its face ids.
//! - Property: any transform followed by its inverse restores the model hash exactly.

use std::collections::BTreeSet;
use std::f64::consts::{FRAC_PI_2, PI};

use proptest::prelude::*;
use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::transform::Xform;
use sawhorse_kernel::{DVec3, KernelError, TransformError};

const GROUND: u64 = 1;

fn apply(m: &mut Model, op: Op) -> OpOutcome {
    m.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

/// A w x d x h box at (x, y) on the ground; returns its object id.
fn box_at(m: &mut Model, x: f64, y: f64, w: f64, d: f64, h: f64) -> u64 {
    let out = apply(
        m,
        Op::AddRectangle {
            sketch: GROUND,
            x,
            y,
            w,
            h: d,
        },
    );
    let OpOutcome::SketchEdited { regions } = out else {
        panic!("{out:?}")
    };
    let out = apply(
        m,
        Op::Extrude {
            sketch: GROUND,
            region: regions[0],
            distance: h,
        },
    );
    let OpOutcome::ObjectCreated { object } = out else {
        panic!("{out:?}")
    };
    object
}

fn face_ids(m: &Model, o: u64) -> Vec<u64> {
    m.object(o).unwrap().brep().persistent_face_ids()
}

fn bounds(m: &Model, o: u64) -> (DVec3, DVec3) {
    m.object(o).unwrap().bounds()
}

fn near(a: DVec3, b: DVec3) -> bool {
    a.distance(b) < 1e-12
}

fn transform(objects: Vec<u64>, xform: Xform) -> Op {
    Op::Transform { objects, xform }
}

fn refused(m: &mut Model, op: Op) -> TransformError {
    let before = m.hash();
    let e = m.apply(&op).expect_err("should be refused");
    assert_eq!(m.hash(), before, "a refusal changes nothing");
    match e {
        KernelError::Transform(t) => t,
        other => panic!("not a transform refusal: {other}"),
    }
}

#[test]
fn move_keeps_face_ids_and_volume() {
    let mut m = Model::with_ground_sketch();
    let o = box_at(&mut m, 0.0, 0.0, 1.0, 0.5, 0.25);
    let ids = face_ids(&m, o);
    let out = apply(
        &mut m,
        transform(vec![o], Xform::translation(DVec3::new(2.0, -1.0, 0.5))),
    );
    assert_eq!(out, OpOutcome::Transformed { objects: vec![o] });
    assert_eq!(
        face_ids(&m, o),
        ids,
        "object-level: every face keeps its id"
    );
    let (lo, hi) = bounds(&m, o);
    assert!(near(lo, DVec3::new(2.0, -1.0, 0.5)) && near(hi, DVec3::new(3.0, -0.5, 0.75)));
    assert!((m.stats().volume_m3 - 0.125).abs() < 1e-12);
    // Stored planes moved with it: pushing the moved cap still works and keeps its id.
    apply(
        &mut m,
        Op::PushPull {
            object: o,
            face: 2,
            distance: 0.25,
        },
    );
    assert!(near(bounds(&m, o).1, DVec3::new(3.0, -0.5, 1.0)));
}

#[test]
fn rotate_and_scale_and_mirror() {
    let mut m = Model::with_ground_sketch();
    let o = box_at(&mut m, 0.0, 0.0, 2.0, 1.0, 0.5);
    apply(
        &mut m,
        transform(vec![o], Xform::rotation(DVec3::ZERO, DVec3::Z, FRAC_PI_2)),
    );
    let (lo, hi) = bounds(&m, o);
    assert!(near(lo, DVec3::new(-1.0, 0.0, 0.0)) && near(hi, DVec3::new(0.0, 2.0, 0.5)));
    // Non-uniform scale: volume follows the determinant.
    apply(
        &mut m,
        transform(
            vec![o],
            Xform::scaling(DVec3::ZERO, DVec3::new(1.0, 1.5, 2.0)),
        ),
    );
    assert!((m.stats().volume_m3 - 3.0).abs() < 1e-12);
    // A mirror stays outward-facing (positive volume, valid winding).
    apply(
        &mut m,
        transform(
            vec![o],
            Xform::scaling(DVec3::ZERO, DVec3::new(-1.0, 1.0, 1.0)),
        ),
    );
    assert!((m.stats().volume_m3 - 3.0).abs() < 1e-12);
    assert!(m.stats().watertight);
    let (lo, hi) = bounds(&m, o);
    assert!(
        near(lo, DVec3::ZERO) && near(hi, DVec3::new(1.0, 3.0, 1.0)),
        "{lo} {hi}"
    );
    // Flattening is refused, typed; so are bad numbers and missing objects.
    assert_eq!(
        refused(
            &mut m,
            transform(
                vec![o],
                Xform::scaling(DVec3::ZERO, DVec3::new(1.0, 0.0, 1.0))
            )
        ),
        TransformError::Singular
    );
    let mut nan = Xform::IDENTITY;
    nan.linear[1][1] = f64::NAN;
    assert_eq!(
        refused(&mut m, transform(vec![o], nan)),
        TransformError::BadTransform
    );
    assert_eq!(
        refused(&mut m, transform(vec![], Xform::IDENTITY)),
        TransformError::NothingSelected
    );
    assert_eq!(
        refused(&mut m, transform(vec![o, 99], Xform::IDENTITY)),
        TransformError::NoSuchObject(99)
    );
    assert_eq!(
        refused(
            &mut m,
            transform(vec![o], Xform::translation(DVec3::new(2e4, 0.0, 0.0)))
        ),
        TransformError::BadTransform
    );
}

#[test]
fn transforming_a_tilted_drawn_solid_keeps_its_drawn_edges() {
    // A box on a tilted sketch with an imprint pulled out into a boss: rotate and scale it.
    let mut m = Model::new();
    apply(
        &mut m,
        Op::NewSketch {
            origin: [0.3, -0.2, 0.1],
            normal: [0.3, -0.5, 0.8],
        },
    );
    apply(
        &mut m,
        Op::AddRectangle {
            sketch: 1,
            x: -0.5,
            y: -0.25,
            w: 1.2,
            h: 0.8,
        },
    );
    apply(
        &mut m,
        Op::Extrude {
            sketch: 1,
            region: 1,
            distance: 0.6,
        },
    );
    let cap = m.object(1).unwrap().brep().to_polys();
    let fi = m.object(1).unwrap().face_ordinal(2).unwrap();
    let ring: Vec<DVec3> = cap.faces[fi].outer.iter().map(|&i| cap.points[i]).collect();
    let c = ring.iter().copied().sum::<DVec3>() / ring.len() as f64;
    let inner: Vec<[f64; 3]> = ring.iter().map(|&p| c.lerp(p, 0.5).to_array()).collect();
    apply(
        &mut m,
        Op::Imprint {
            object: 1,
            face: 2,
            points: inner,
        },
    );
    let user_before = m
        .object(1)
        .unwrap()
        .brep()
        .edges()
        .filter(|(_, e)| e.user)
        .count();
    let volume = m.stats().volume_m3;
    let x = Xform::rotation(DVec3::new(1.0, 1.0, 0.0), DVec3::new(0.2, 0.9, 0.4), 0.7)
        .after(&Xform::scaling(DVec3::ZERO, DVec3::new(1.5, 1.5, 1.5)));
    apply(&mut m, transform(vec![1], x));
    let o = m.object(1).unwrap();
    assert_eq!(
        o.brep().edges().filter(|(_, e)| e.user).count(),
        user_before
    );
    assert!((m.stats().volume_m3 - volume * 3.375).abs() < 1e-9);
    // The imprint is still a face that pushes.
    apply(
        &mut m,
        Op::PushPull {
            object: 1,
            face: 7,
            distance: 0.05,
        },
    );
}

#[test]
fn copies_and_arrays_are_new_objects() {
    let mut m = Model::with_ground_sketch();
    let o = box_at(&mut m, 0.0, 0.0, 0.5, 0.5, 0.5);
    apply(
        &mut m,
        Op::SetTag {
            object: o,
            tag: Some("Legs".into()),
        },
    );
    let ids = face_ids(&m, o);
    let out = apply(
        &mut m,
        Op::Copy {
            objects: vec![o],
            xform: Xform::translation(DVec3::new(1.0, 0.0, 0.0)),
        },
    );
    let OpOutcome::Copied { objects } = out else {
        panic!("{out:?}")
    };
    assert_eq!(objects.len(), 1);
    let c = objects[0];
    assert_ne!(c, o);
    assert_eq!(face_ids(&m, c), ids, "a copy keeps the face ids");
    assert_eq!(m.object(c).unwrap().tag(), Some("Legs"));
    assert_eq!(m.object(c).unwrap().number(), 2);
    assert!(near(bounds(&m, o).0, DVec3::ZERO), "the original stays");
    // An array: three copies at 1, 2 and 3 steps.
    let out = apply(
        &mut m,
        Op::Array {
            objects: vec![o],
            xform: Xform::translation(DVec3::new(0.0, 0.75, 0.0)),
            count: 3,
        },
    );
    let OpOutcome::Copied { objects } = out else {
        panic!("{out:?}")
    };
    assert_eq!(objects.len(), 3);
    for (k, id) in objects.iter().enumerate() {
        assert!(near(
            bounds(&m, *id).0,
            DVec3::new(0.0, 0.75 * (k + 1) as f64, 0.0)
        ));
    }
    let numbers: Vec<u64> = m.objects().iter().map(|(_, o)| o.number()).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5]);
    assert!((m.stats().volume_m3 - 5.0 * 0.125).abs() < 1e-12);
    // A rotated array (a "/4" of a half turn): copies at 45, 90 and 135 degrees about a post.
    let step = Xform::rotation(DVec3::new(-1.0, 0.0, 0.0), DVec3::Z, PI)
        .divided(4)
        .unwrap();
    let out = apply(
        &mut m,
        Op::Array {
            objects: vec![o],
            xform: step,
            count: 3,
        },
    );
    let OpOutcome::Copied { objects } = out else {
        panic!("{out:?}")
    };
    assert_eq!(objects.len(), 3);
    assert!(m.stats().watertight);
    // Overlapping copies are fine: solids never weld.
    apply(
        &mut m,
        Op::Copy {
            objects: vec![o],
            xform: Xform::translation(DVec3::new(0.25, 0.0, 0.0)),
        },
    );
    for bad in [0, 1001] {
        assert_eq!(
            refused(
                &mut m,
                Op::Array {
                    objects: vec![o],
                    xform: Xform::IDENTITY,
                    count: bad
                }
            ),
            TransformError::BadCount
        );
    }
}

#[test]
fn moving_part_of_a_solid() {
    let mut m = Model::with_ground_sketch();
    let o = box_at(&mut m, 0.0, 0.0, 1.0, 1.0, 1.0);
    // The top edge at x = 1 slides over by 0.5: a sloped box, every face still planar.
    let edge = vec![[1.0, 0.0, 1.0], [1.0, 1.0, 1.0]];
    let out = apply(
        &mut m,
        Op::TransformGeometry {
            object: o,
            points: edge,
            xform: Xform::translation(DVec3::new(0.5, 0.0, 0.0)),
            autofold: false,
        },
    );
    let OpOutcome::GeometryMoved { faces } = out else {
        panic!("{out:?}")
    };
    assert_eq!(
        faces.len(),
        4,
        "top, right, front and back changed: {faces:?}"
    );
    assert!((m.stats().volume_m3 - 1.25).abs() < 1e-12);
    // The whole top face up by 0.5 (a face moved along its normal is a push).
    let top: Vec<[f64; 3]> = vec![
        [0.0, 0.0, 1.0],
        [1.5, 0.0, 1.0],
        [1.5, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    apply(
        &mut m,
        Op::TransformGeometry {
            object: o,
            points: top,
            xform: Xform::translation(DVec3::new(0.0, 0.0, 0.5)),
            autofold: false,
        },
    );
    assert!((m.stats().volume_m3 - 1.875).abs() < 1e-12);
    let ids = face_ids(&m, o);
    // One corner up: the top would bend. Refused, naming the face...
    let corner = || Op::TransformGeometry {
        object: o,
        points: vec![[0.0, 0.0, 1.5]],
        xform: Xform::translation(DVec3::new(0.0, 0.0, 0.25)),
        autofold: false,
    };
    let e = refused(&mut m, corner());
    assert!(
        matches!(e, TransformError::WouldBend { face } if ids.contains(&face)),
        "{e:?}"
    );
    // ...unless autofold: the bent faces fold into triangles, the rest keep their ids.
    let Op::TransformGeometry {
        object,
        points,
        xform,
        ..
    } = corner()
    else {
        unreachable!()
    };
    let out = apply(
        &mut m,
        Op::TransformGeometry {
            object,
            points,
            xform,
            autofold: true,
        },
    );
    assert!(matches!(out, OpOutcome::GeometryMoved { .. }));
    assert!(m.stats().faces > 6, "folded");
    assert!(m.stats().watertight);
    let kept: BTreeSet<u64> = face_ids(&m, o).into_iter().collect();
    assert!(ids.iter().all(|id| kept.contains(id)), "no face id lost");
    // Typed refusals: no vertex there; nothing; pulling a corner through the solid.
    assert_eq!(
        refused(
            &mut m,
            Op::TransformGeometry {
                object: o,
                points: vec![[0.3, 0.3, 0.3]],
                xform: Xform::translation(DVec3::X),
                autofold: true
            }
        ),
        TransformError::NoSuchVertex
    );
    assert_eq!(
        refused(
            &mut m,
            Op::TransformGeometry {
                object: o,
                points: vec![],
                xform: Xform::translation(DVec3::X),
                autofold: true
            }
        ),
        TransformError::NothingToMove
    );
    let e = refused(
        &mut m,
        Op::TransformGeometry {
            object: o,
            points: vec![[0.0, 0.0, 0.0]],
            xform: Xform::translation(DVec3::new(2.5, 2.5, 3.0)),
            autofold: true,
        },
    );
    assert!(
        matches!(
            e,
            TransformError::SelfIntersects | TransformError::InsideOut | TransformError::NotPlanar
        ),
        "{e:?}"
    );
}

#[test]
fn a_through_cut_severs_the_board_into_two_solids() {
    let mut m = Model::with_ground_sketch();
    let o = box_at(&mut m, 0.0, 0.0, 1.0, 0.3, 0.05);
    // A strip across the top from edge to edge, pushed all the way through.
    let out = apply(
        &mut m,
        Op::Draw {
            target: sawhorse_kernel::model::DrawTarget::Face { object: o, face: 2 },
            points: vec![
                [0.4, -0.1, 0.05],
                [0.6, -0.1, 0.05],
                [0.6, 0.4, 0.05],
                [0.4, 0.4, 0.05],
            ],
            closed: true,
        },
    );
    let OpOutcome::Drawn { inside, .. } = out else {
        panic!("{out:?}")
    };
    let strip = inside.expect("the strip");
    let before: BTreeSet<u64> = face_ids(&m, o).into_iter().collect();
    let out = apply(
        &mut m,
        Op::PushPull {
            object: o,
            face: strip,
            distance: -0.05,
        },
    );
    let OpOutcome::Severed { objects } = out else {
        panic!("{out:?}")
    };
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0], o, "the original keeps its id");
    assert_eq!(m.objects().len(), 2);
    let st = m.stats();
    assert!(st.watertight);
    assert!((st.volume_m3 - 0.8 * 0.3 * 0.05).abs() < 1e-12);
    for &p in &objects {
        let ob = m.object(p).unwrap();
        assert_eq!(ob.piece_count(), 1);
        assert!((ob.volume() - 0.4 * 0.3 * 0.05).abs() < 1e-12);
        // Each piece keeps the ids of its faces that survived.
        let ids: BTreeSet<u64> = ob.brep().persistent_face_ids().into_iter().collect();
        assert!(
            ids.intersection(&before).count() >= 3,
            "{ids:?} vs {before:?}"
        );
    }
    let a: BTreeSet<u64> = face_ids(&m, objects[0]).into_iter().collect();
    assert!(
        a.contains(&1),
        "the piece holding the smallest face id keeps the object"
    );
    assert_eq!(m.object(objects[1]).unwrap().number(), 2);
    // Undo-free check: severing again is refused only for real reasons; the pieces push.
    apply(
        &mut m,
        Op::PushPull {
            object: objects[1],
            face: 2,
            distance: 0.01,
        },
    );
}

#[test]
fn severed_pieces_stay_in_their_group() {
    let mut m = Model::with_ground_sketch();
    let o = box_at(&mut m, 0.0, 0.0, 1.0, 0.3, 0.05);
    apply(
        &mut m,
        Op::CreateGroup {
            name: "Board".into(),
            objects: vec![o],
        },
    );
    let out = apply(
        &mut m,
        Op::Draw {
            target: sawhorse_kernel::model::DrawTarget::Face { object: o, face: 2 },
            points: vec![
                [0.5, -0.1, 0.05],
                [0.6, -0.1, 0.05],
                [0.6, 0.4, 0.05],
                [0.5, 0.4, 0.05],
            ],
            closed: true,
        },
    );
    let OpOutcome::Drawn { inside, .. } = out else {
        panic!()
    };
    let out = apply(
        &mut m,
        Op::PushPull {
            object: o,
            face: inside.unwrap(),
            distance: -0.2,
        },
    );
    let OpOutcome::Severed { objects } = out else {
        panic!("{out:?}")
    };
    assert_eq!(m.groups()[0].1.objects, objects);
}

fn rigid_or_scale() -> impl Strategy<Value = Xform> {
    let v = || {
        (-4i32..4, -4i32..4, -4i32..4)
            .prop_map(|(x, y, z)| DVec3::new(x as f64, y as f64, z as f64) * 0.25)
    };
    prop_oneof![
        v().prop_map(Xform::translation),
        (v(), (-1.0..1.0f64, -1.0..1.0f64, 0.1..1.0f64), -6.0..6.0f64)
            .prop_map(|(o, (x, y, z), a)| Xform::rotation(o, DVec3::new(x, y, z), a)),
        (
            v(),
            (0.25..4.0f64, 0.25..4.0f64, 0.25..4.0f64),
            any::<bool>()
        )
            .prop_map(|(o, (x, y, z), mirror)| Xform::scaling(
                o,
                DVec3::new(if mirror { -x } else { x }, y, z)
            )),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any transform followed by its inverse restores the model hash (positions, topology,
    /// face ids, drawn edges, tags, everything hashed).
    #[test]
    fn transform_then_inverse_restores_hash(
        boxes in prop::collection::vec((-8i32..8, -8i32..8, 1i32..6, 1i32..6, 1i32..6), 1..3),
        x in rigid_or_scale(),
    ) {
        let mut m = Model::with_ground_sketch();
        let mut ids = Vec::new();
        for (k, (bx, by, w, d, h)) in boxes.into_iter().enumerate() {
            // Spread the boxes out so their footprints never touch.
            let x0 = f64::from(bx) * 0.25 + k as f64 * 10.0;
            ids.push(box_at(&mut m, x0, f64::from(by) * 0.25, f64::from(w) * 0.25, f64::from(d) * 0.25, f64::from(h) * 0.25));
        }
        let before = m.hash();
        m.apply(&transform(ids.clone(), x)).unwrap();
        m.apply(&transform(ids, x.inverse())).unwrap();
        prop_assert_eq!(m.hash(), before);
    }
}
