//! Follow Me (lane 4): profiles swept along straight and polyline paths, open and closed,
//! as new solids (region profiles) or joined to / cut from their solid (face profiles),
//! with exact volumes (profile area x the length of the path its centroid takes), a
//! moulding routed around the top of a box, a slot that severs a block, and typed refusals.

use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome};
use sawhorse_kernel::sweep::Profile;
use sawhorse_kernel::{KernelError, SweepError};

fn apply(m: &mut Model, op: Op) -> OpOutcome {
    m.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

/// A w x d x h box at the origin (object 1). Walls: 3 front (y = 0), 4 right (x = w),
/// 5 back (y = d), 6 left end (x = 0); 1 base, 2 top.
fn block(w: f64, d: f64, h: f64) -> Model {
    let mut m = Model::with_ground_sketch();
    apply(
        &mut m,
        Op::AddRectangle {
            sketch: 1,
            x: 0.0,
            y: 0.0,
            w,
            h: d,
        },
    );
    apply(
        &mut m,
        Op::Extrude {
            sketch: 1,
            region: 1,
            distance: h,
        },
    );
    m
}

fn face_area(m: &Model, object: u64, face: u64) -> f64 {
    let o = m.object(object).unwrap();
    let p = o.brep().to_polys();
    sawhorse_kernel::boolean::face_signature(&p, o.face_ordinal(face).unwrap())
        .unwrap()
        .area
}

#[test]
fn a_region_swept_along_an_l_makes_a_mitered_solid() {
    let mut m = Model::with_ground_sketch();
    // A sketch on the plane x = 0 (its frame: u = +green, v = +blue).
    apply(
        &mut m,
        Op::NewSketch {
            origin: [0.0; 3],
            normal: [1.0, 0.0, 0.0],
        },
    );
    apply(
        &mut m,
        Op::AddRectangle {
            sketch: 2,
            x: 0.0,
            y: 0.0,
            w: 0.1,
            h: 0.1,
        },
    );
    let out = apply(
        &mut m,
        Op::FollowMe {
            profile: Profile::Region {
                sketch: 2,
                region: 1,
            },
            path: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            closed: false,
        },
    );
    assert!(matches!(out, OpOutcome::ObjectCreated { .. }), "{out:?}");
    let st = m.stats();
    assert!(st.watertight);
    assert!(
        (st.volume_m3 - 0.01 * 1.9).abs() < 1e-12,
        "{}",
        st.volume_m3
    );
    assert_eq!(st.regions, 0, "the region became the solid");
}

#[test]
fn a_moulding_routed_around_the_top_of_a_box() {
    let mut m = block(1.0, 0.5, 0.3);
    // Cut the end face's top front corner: a 0.05 triangle.
    let out = apply(
        &mut m,
        Op::Draw {
            target: DrawTarget::Face { object: 1, face: 6 },
            points: vec![[0.0, 0.05, 0.3], [0.0, 0.0, 0.25]],
            closed: false,
            curve: None,
        },
    );
    let OpOutcome::Drawn { faces, .. } = out else {
        panic!("{out:?}")
    };
    let corner = *faces
        .iter()
        .find(|&&f| (face_area(&m, 1, f) - 0.00125).abs() < 1e-12)
        .expect("the corner triangle");
    // The path: the top face's edge, all the way round.
    let out = apply(
        &mut m,
        Op::FollowMe {
            profile: Profile::Face {
                object: 1,
                face: corner,
            },
            path: vec![
                [0.0, 0.0, 0.3],
                [1.0, 0.0, 0.3],
                [1.0, 0.5, 0.3],
                [0.0, 0.5, 0.3],
            ],
            closed: true,
        },
    );
    assert_eq!(
        out,
        OpOutcome::Swept {
            objects: vec![1],
            cut: true
        }
    );
    let st = m.stats();
    assert!(st.watertight);
    // Removed: the triangle's area times its centroid's path (the top rectangle inset by
    // a third of the leg each way).
    let inset = 0.05 / 3.0;
    let removed = 0.00125 * (2.0 * (1.0 - 2.0 * inset) + 2.0 * (0.5 - 2.0 * inset));
    assert!(
        (st.volume_m3 - (0.15 - removed)).abs() < 1e-12,
        "{} vs {}",
        st.volume_m3,
        0.15 - removed
    );
}

#[test]
fn the_profile_can_sit_at_any_corner_of_a_closed_path() {
    // The same chamfer at the far end (x = 1, face 4), the path listed from the near
    // corner: the sweep starts on the segment the profile sits against.
    let mut m = block(1.0, 0.5, 0.3);
    let out = apply(
        &mut m,
        Op::Draw {
            target: DrawTarget::Face { object: 1, face: 4 },
            points: vec![[1.0, 0.05, 0.3], [1.0, 0.0, 0.25]],
            closed: false,
            curve: None,
        },
    );
    let OpOutcome::Drawn { faces, .. } = out else {
        panic!("{out:?}")
    };
    let corner = *faces
        .iter()
        .find(|&&f| (face_area(&m, 1, f) - 0.00125).abs() < 1e-12)
        .expect("the corner triangle");
    apply(
        &mut m,
        Op::FollowMe {
            profile: Profile::Face {
                object: 1,
                face: corner,
            },
            path: vec![
                [0.0, 0.0, 0.3],
                [1.0, 0.0, 0.3],
                [1.0, 0.5, 0.3],
                [0.0, 0.5, 0.3],
            ],
            closed: true,
        },
    );
    let inset = 0.05 / 3.0;
    let removed = 0.00125 * (2.0 * (1.0 - 2.0 * inset) + 2.0 * (0.5 - 2.0 * inset));
    assert!((m.stats().volume_m3 - (0.15 - removed)).abs() < 1e-12);
}

#[test]
fn a_face_swept_out_of_its_solid_adds_an_arm() {
    let mut m = block(1.0, 1.0, 0.2);
    let out = apply(
        &mut m,
        Op::Imprint {
            object: 1,
            face: 2,
            points: vec![
                [0.45, 0.45, 0.2],
                [0.55, 0.45, 0.2],
                [0.55, 0.55, 0.2],
                [0.45, 0.55, 0.2],
            ],
        },
    );
    let OpOutcome::Imprinted { face } = out else {
        panic!("{out:?}")
    };
    let out = apply(
        &mut m,
        Op::FollowMe {
            profile: Profile::Face { object: 1, face },
            path: vec![[0.5, 0.5, 0.2], [0.5, 0.5, 0.5], [1.0, 0.5, 0.5]],
            closed: false,
        },
    );
    assert_eq!(
        out,
        OpOutcome::Swept {
            objects: vec![1],
            cut: false
        }
    );
    // Up 0.3 then over 0.5 along the centroid: 0.01 x 0.8 added.
    assert!((m.stats().volume_m3 - (0.2 + 0.008)).abs() < 1e-12);
    assert!(m.stats().watertight);
}

#[test]
fn a_slot_through_a_block_severs_it() {
    let mut m = block(1.0, 0.6, 0.2);
    // Two lines down the end face make a 0.1 wide strip, full height.
    for y in [0.25, 0.35] {
        apply(
            &mut m,
            Op::Draw {
                target: DrawTarget::Face { object: 1, face: 6 },
                points: vec![[0.0, y, 0.2], [0.0, y, 0.0]],
                closed: false,
                curve: None,
            },
        );
    }
    let strip = m
        .object(1)
        .unwrap()
        .brep()
        .persistent_face_ids()
        .into_iter()
        .find(|&f| (face_area(&m, 1, f) - 0.02).abs() < 1e-12)
        .expect("the strip");
    let out = apply(
        &mut m,
        Op::FollowMe {
            profile: Profile::Face {
                object: 1,
                face: strip,
            },
            path: vec![[0.0, 0.3, 0.0], [1.0, 0.3, 0.0]],
            closed: false,
        },
    );
    let OpOutcome::Swept { objects, cut: true } = out else {
        panic!("{out:?}")
    };
    assert_eq!(objects.len(), 2, "the slot cut the block in two");
    assert_eq!(m.objects().len(), 2);
    assert!((m.stats().volume_m3 - (0.12 - 0.02)).abs() < 1e-12);
}

#[test]
fn refusals_are_typed_and_change_nothing() {
    let mut m = block(1.0, 0.5, 0.3);
    let before = m.hash();
    let sweep = |path: Vec<[f64; 3]>, closed: bool| Op::FollowMe {
        profile: Profile::Face { object: 1, face: 6 },
        path,
        closed,
    };
    let cases = [
        // Along the end face: the profile lies along the path.
        (
            sweep(vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]], false),
            SweepError::ProfileAlongPath,
        ),
        // The whole end face through the whole length: nothing left.
        (
            sweep(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], false),
            SweepError::RemovesEverything,
        ),
        (
            sweep(
                vec![[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [0.2, 0.0, 0.0]],
                false,
            ),
            SweepError::PathFoldsBack,
        ),
        (
            sweep(vec![[0.0, 0.0, 0.0]], false),
            SweepError::PathTooShort,
        ),
        // A turn far too tight for a 0.5 deep profile.
        (
            sweep(
                vec![[0.0, 0.0, 0.0], [0.05, 0.0, 0.0], [0.05, 1.0, 0.0]],
                false,
            ),
            SweepError::SelfIntersects,
        ),
    ];
    for (op, want) in cases {
        let e = m.apply(&op).unwrap_err();
        assert_eq!(e, KernelError::Sweep(want), "{op:?}");
        assert_eq!(m.hash(), before);
    }
}
