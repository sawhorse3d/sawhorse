//! Offset (lane 4): faces, sketch regions and edge chains, inward and outward, drawn on the
//! solid when the result lies on its faces and into a sketch on the plane otherwise; sharp
//! (mitered) corners; typed refusals.

use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome};
use sawhorse_kernel::offset::OffsetSource;
use sawhorse_kernel::{KernelError, OffsetError};

fn apply(m: &mut Model, op: Op) -> OpOutcome {
    m.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

/// A 1 x 0.5 x 0.2 box on the ground (object 1; top face id 2).
fn board() -> Model {
    let mut m = Model::with_ground_sketch();
    apply(
        &mut m,
        Op::AddRectangle {
            sketch: 1,
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 0.5,
        },
    );
    apply(
        &mut m,
        Op::Extrude {
            sketch: 1,
            region: 1,
            distance: 0.2,
        },
    );
    m
}

fn face_area(m: &Model, object: u64, face: u64) -> f64 {
    let o = m.object(object).unwrap();
    let p = o.brep().to_polys();
    let fi = o.face_ordinal(face).unwrap();
    sawhorse_kernel::boolean::face_signature(&p, fi)
        .unwrap()
        .area
}

#[test]
fn inward_offset_of_a_face_imprints_and_pushes() {
    let mut m = board();
    let out = apply(
        &mut m,
        Op::Offset {
            source: OffsetSource::Face { object: 1, face: 2 },
            distance: -0.05,
        },
    );
    let OpOutcome::Drawn {
        sketch: None,
        inside: Some(inner),
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert!((face_area(&m, 1, inner) - 0.9 * 0.4).abs() < 1e-12);
    assert!(
        (face_area(&m, 1, 2) - (0.5 - 0.36)).abs() < 1e-12,
        "the rim"
    );
    // The tenon: the inner face pulls up.
    apply(
        &mut m,
        Op::PushPull {
            object: 1,
            face: inner,
            distance: 0.1,
        },
    );
    assert!((m.stats().volume_m3 - (0.1 + 0.036)).abs() < 1e-12);
}

#[test]
fn outward_offset_of_a_face_becomes_a_sketch_on_its_plane() {
    let mut m = board();
    let out = apply(
        &mut m,
        Op::Offset {
            source: OffsetSource::Face { object: 1, face: 2 },
            distance: 0.05,
        },
    );
    let OpOutcome::Drawn {
        sketch: Some(s),
        regions,
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_ne!(s, 1, "a new sketch on the top plane, not the ground");
    let sk = m.sketch(s).unwrap();
    assert!((sk.plane().origin.z - 0.2).abs() < 1e-12);
    // One region: the 1.1 x 0.6 rectangle with sharp corners (no rounding).
    assert_eq!(regions.len(), 1);
    let r = sk
        .region(sawhorse_kernel::ids::from_public(regions[0]))
        .unwrap();
    assert!((sk.region_area(r) - 1.1 * 0.6).abs() < 1e-12);
    assert_eq!(r.outer.len(), 4, "mitered corners");
}

#[test]
fn a_region_offsets_inside_its_own_sketch() {
    let mut m = Model::with_ground_sketch();
    apply(
        &mut m,
        Op::AddRectangle {
            sketch: 1,
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
    );
    let out = apply(
        &mut m,
        Op::Offset {
            source: OffsetSource::Region {
                sketch: 1,
                region: 1,
            },
            distance: -0.25,
        },
    );
    let OpOutcome::Drawn {
        sketch: Some(1),
        inside: Some(inner),
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    let sk = m.sketch(1).unwrap();
    assert_eq!(m.stats().regions, 2, "the frame and the inside");
    let r = sk.region(sawhorse_kernel::ids::from_public(inner)).unwrap();
    assert!((sk.region_area(r) - 0.25).abs() < 1e-12);
}

#[test]
fn an_edge_chain_offset_cuts_across_the_face() {
    let mut m = board();
    let faces_before = m.stats().faces;
    // The top's front edge, offset to its left (inward, +y) by 0.1: a cut from side to side.
    let out = apply(
        &mut m,
        Op::Offset {
            source: OffsetSource::Edges {
                points: vec![[0.0, 0.0, 0.2], [1.0, 0.0, 0.2]],
                closed: false,
                normal: [0.0, 0.0, 1.0],
                object: Some(1),
            },
            distance: 0.1,
        },
    );
    assert!(
        matches!(out, OpOutcome::Drawn { sketch: None, .. }),
        "{out:?}"
    );
    assert_eq!(m.stats().faces, faces_before + 1);
    // An open chain that would stop inside the face is refused (a dangling line).
    let mut m = board();
    let e = m
        .apply(&Op::Offset {
            source: OffsetSource::Edges {
                points: vec![[0.2, 0.0, 0.2], [0.6, 0.0, 0.2]],
                closed: false,
                normal: [0.0, 0.0, 1.0],
                object: Some(1),
            },
            distance: 0.1,
        })
        .unwrap_err();
    assert!(e.to_string().contains("start and end on its edges"), "{e}");
}

#[test]
fn a_framed_face_offsets_as_a_whole_shape() {
    let mut m = board();
    // A through hole: imprint a square on the top and push it through the board.
    let out = apply(
        &mut m,
        Op::Draw {
            target: DrawTarget::Face { object: 1, face: 2 },
            points: vec![
                [0.4, 0.15, 0.2],
                [0.6, 0.15, 0.2],
                [0.6, 0.35, 0.2],
                [0.4, 0.35, 0.2],
            ],
            closed: true,
            curve: None,
        },
    );
    let OpOutcome::Drawn {
        inside: Some(sq), ..
    } = out
    else {
        panic!()
    };
    apply(
        &mut m,
        Op::PushPull {
            object: 1,
            face: sq,
            distance: -0.2,
        },
    );
    assert_eq!(m.stats().genus, 1);
    let rim_before = face_area(&m, 1, 2);
    assert!((rim_before - (0.5 - 0.04)).abs() < 1e-12);
    let out = apply(
        &mut m,
        Op::Offset {
            source: OffsetSource::Face { object: 1, face: 2 },
            distance: -0.02,
        },
    );
    let OpOutcome::Drawn {
        inside: Some(inner),
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    // The inner frame: (0.96 x 0.46) minus the hole grown by 0.02 (0.24 x 0.24).
    assert!(
        (face_area(&m, 1, inner) - (0.96 * 0.46 - 0.24 * 0.24)).abs() < 1e-12,
        "{}",
        face_area(&m, 1, inner)
    );
    assert!(m.stats().watertight);
}

#[test]
fn refusals_are_typed_and_change_nothing() {
    let mut m = board();
    let before = m.hash();
    for (d, want) in [
        (0.0, OffsetError::ZeroDistance),
        (f64::NAN, OffsetError::OutOfBounds),
        (-0.3, OffsetError::Vanishes),
    ] {
        let e = m
            .apply(&Op::Offset {
                source: OffsetSource::Face { object: 1, face: 2 },
                distance: d,
            })
            .unwrap_err();
        assert_eq!(e, KernelError::Offset(want));
        assert_eq!(m.hash(), before);
    }
    let e = m
        .apply(&Op::Offset {
            source: OffsetSource::Edges {
                points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.5]],
                closed: false,
                normal: [0.0, 0.0, 1.0],
                object: None,
            },
            distance: 0.1,
        })
        .unwrap_err();
    assert_eq!(e, KernelError::Offset(OffsetError::NotPlanar));
    assert!(
        m.apply(&Op::Offset {
            source: OffsetSource::Face {
                object: 1,
                face: 99
            },
            distance: 0.1,
        })
        .is_err()
    );
}
