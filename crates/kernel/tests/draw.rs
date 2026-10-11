//! The Draw op (lane 3): strokes into sketches, onto planes (find-or-create the sketch),
//! and onto a solid's face plane, where pieces that run from edge to edge become cuts and a
//! closed stroke inside one face becomes an imprint. The dado test is the bookshelf's.

use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome};
use sawhorse_kernel::{FaceEditError, KernelError, SketchError};

const IN: f64 = 0.0254;

fn run(ops: &[Op]) -> Model {
    let mut m = Model::with_ground_sketch();
    for (i, op) in ops.iter().enumerate() {
        m.apply(op).unwrap_or_else(|e| panic!("op {i} {op:?}: {e}"));
    }
    m
}

/// A board `w x d x h` (inches) standing on the ground at the origin: object 1.
fn board(w: f64, d: f64, h: f64) -> Model {
    run(&[
        Op::AddRectangle {
            sketch: 1,
            x: 0.0,
            y: 0.0,
            w: w * IN,
            h: d * IN,
        },
        Op::Extrude {
            sketch: 1,
            region: 1,
            distance: h * IN,
        },
    ])
}

/// Persistent id of object 1's face whose outward normal is `n` (the first one).
fn face_facing(m: &Model, n: [f64; 3]) -> u64 {
    let o = m.object(1).unwrap();
    let p = o.brep().to_polys();
    let fi = (0..p.faces.len())
        .find(|&i| {
            let pl = p.plane(i).unwrap();
            pl.normal.dot(glam::DVec3::from_array(n)) > 0.999
        })
        .unwrap();
    o.face_id_at(fi).unwrap()
}

fn on_face(object: u64, face: u64) -> DrawTarget {
    DrawTarget::Face { object, face }
}

fn p(x: f64, y: f64, z: f64) -> [f64; 3] {
    [x * IN, y * IN, z * IN]
}

#[test]
fn a_rectangle_across_a_face_cuts_a_strip_and_pushes_into_a_dado() {
    // A bookshelf side: 3/4 x 11 1/4 x 72 in. Its inner face is x = 3/4.
    let mut m = board(0.75, 11.25, 72.0);
    let inner = face_facing(&m, [1.0, 0.0, 0.0]);
    let before = m.stats();
    let out = m
        .apply(&Op::Draw {
            target: on_face(1, inner),
            points: vec![
                p(0.75, 0.0, 21.5),
                p(0.75, 11.25, 21.5),
                p(0.75, 11.25, 22.25),
                p(0.75, 0.0, 22.25),
            ],
            closed: true,
        })
        .unwrap();
    let OpOutcome::Drawn {
        faces,
        sketch,
        inside,
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(sketch, None);
    assert_eq!(faces.len(), 3, "the inner face, the strip, the piece above");
    let st = m.stats();
    assert_eq!(st.faces, before.faces + 2);
    assert!(st.watertight);
    // The strip is the face of area 11.25 x 0.75 in.
    let o = m.object(1).unwrap();
    let pp = o.brep().to_polys();
    let strip = faces
        .iter()
        .copied()
        .find(|&id| {
            let fi = o.face_ordinal(id).unwrap();
            let sig = sawhorse_kernel::boolean::face_signature(&pp, fi).unwrap();
            (sig.area - 11.25 * 0.75 * IN * IN).abs() < 1e-12
        })
        .expect("a 11 1/4 x 3/4 strip");
    assert_eq!(
        inside,
        Some(strip),
        "the outcome names the strip the rectangle encloses"
    );
    // Push it 3/8 in: a dado.
    m.apply(&Op::PushPull {
        object: 1,
        face: strip,
        distance: -0.375 * IN,
    })
    .unwrap();
    let st = m.stats();
    assert!(st.watertight);
    assert_eq!((st.shells, st.genus), (1, 0));
    let full = 0.75 * 11.25 * 72.0;
    let dado = 11.25 * 0.75 * 0.375;
    assert!(
        (st.volume_m3 / IN.powi(3) - (full - dado)).abs() < 1e-6,
        "{}",
        st.volume_m3 / IN.powi(3)
    );
}

#[test]
fn a_line_from_edge_to_edge_splits_and_a_dangling_line_is_refused() {
    let mut m = board(24.0, 16.0, 12.0);
    let before = m.hash();
    let e = m
        .apply(&Op::Draw {
            target: on_face(1, 2),
            points: vec![p(10.0, 0.0, 12.0), p(10.0, 8.0, 12.0)],
            closed: false,
        })
        .unwrap_err();
    assert_eq!(e, KernelError::FaceEdit(FaceEditError::Dangling));
    assert_eq!(m.hash(), before);
    let out = m
        .apply(&Op::Draw {
            target: on_face(1, 2),
            points: vec![p(10.0, 0.0, 12.0), p(10.0, 8.0, 12.0), p(14.0, 16.0, 12.0)],
            closed: false,
        })
        .unwrap();
    assert!(matches!(out, OpOutcome::Drawn { ref faces, .. } if faces.len() == 2));
    assert_eq!(m.stats().faces, 7);
}

#[test]
fn a_closed_stroke_inside_a_face_imprints_and_one_sticking_out_cuts() {
    let mut m = board(24.0, 16.0, 12.0);
    m.apply(&Op::Draw {
        target: on_face(1, 2),
        points: vec![
            p(4.0, 4.0, 12.0),
            p(8.0, 4.0, 12.0),
            p(8.0, 8.0, 12.0),
            p(4.0, 8.0, 12.0),
        ],
        closed: true,
    })
    .unwrap();
    let st = m.stats();
    assert_eq!((st.faces, st.holes), (7, 1), "an imprint");
    // A rectangle hanging over the front edge: its inside part is one L-shaped... U-shaped
    // cut from the edge, around, back to the edge; the overhang is dropped.
    m.apply(&Op::Draw {
        target: on_face(1, 2),
        points: vec![
            p(12.0, -2.0, 12.0),
            p(16.0, -2.0, 12.0),
            p(16.0, 3.0, 12.0),
            p(12.0, 3.0, 12.0),
        ],
        closed: true,
    })
    .unwrap();
    let st = m.stats();
    assert_eq!(st.faces, 8);
    assert!(st.watertight);
    // Strokes entirely off the face, or off its plane, are refused.
    let before = m.hash();
    for (pts, why) in [
        (vec![p(30.0, 2.0, 12.0), p(34.0, 2.0, 12.0)], "off the face"),
        (
            vec![p(10.0, 0.0, 12.5), p(10.0, 16.0, 12.5)],
            "off the plane",
        ),
    ] {
        let e = m.apply(&Op::Draw {
            target: on_face(1, 2),
            points: pts,
            closed: false,
        });
        assert!(e.is_err(), "{why}");
        assert!(!e.unwrap_err().is_bug());
    }
    assert_eq!(m.hash(), before);
}

#[test]
fn a_stroke_crosses_every_face_on_the_plane() {
    // Split the top in two, then draw one line across both halves: both are cut.
    let mut m = board(24.0, 16.0, 12.0);
    m.apply(&Op::Draw {
        target: on_face(1, 2),
        points: vec![p(12.0, 0.0, 12.0), p(12.0, 16.0, 12.0)],
        closed: false,
    })
    .unwrap();
    assert_eq!(m.stats().faces, 7);
    let out = m
        .apply(&Op::Draw {
            target: on_face(1, 2),
            points: vec![p(0.0, 8.0, 12.0), p(24.0, 8.0, 12.0)],
            closed: false,
        })
        .unwrap();
    assert!(
        matches!(out, OpOutcome::Drawn { ref faces, .. } if faces.len() == 4),
        "{out:?}"
    );
    assert_eq!(m.stats().faces, 9);
}

#[test]
fn planes_find_or_create_their_sketch() {
    let mut m = Model::with_ground_sketch();
    // The ground plane (facing either way) is sketch 1.
    let out = m
        .apply(&Op::Draw {
            target: DrawTarget::Plane {
                origin: [5.0, 5.0, 0.0],
                normal: [0.0, 0.0, -1.0],
            },
            points: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            closed: true,
        })
        .unwrap();
    assert!(
        matches!(out, OpOutcome::Drawn { sketch: Some(1), ref regions, .. } if regions.len() == 1),
        "{out:?}"
    );
    // A horizontal plane 21 1/2 in up: a new sketch, reused by the next stroke on it.
    let at = 21.5 * IN;
    let shelf = |x0: f64| Op::Draw {
        target: DrawTarget::Plane {
            origin: [0.0, 0.0, at],
            normal: [0.0, 0.0, 1.0],
        },
        points: vec![
            [x0, 0.0, at],
            [x0 + 1.0, 0.0, at],
            [x0 + 1.0, 0.3, at],
            [x0, 0.3, at],
        ],
        closed: true,
    };
    let out = m.apply(&shelf(0.0)).unwrap();
    let OpOutcome::Drawn {
        sketch: Some(s2), ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_ne!(s2, 1);
    let out = m.apply(&shelf(2.0)).unwrap();
    assert!(matches!(out, OpOutcome::Drawn { sketch: Some(s), .. } if s == s2));
    assert_eq!(m.sketches().len(), 2);
    // Into a sketch by id; points off its plane are refused.
    let e = m
        .apply(&Op::Draw {
            target: DrawTarget::Sketch { sketch: 1 },
            points: vec![[0.0, 0.0, 0.1], [1.0, 0.0, 0.1]],
            closed: false,
        })
        .unwrap_err();
    assert_eq!(e, KernelError::Sketch(SketchError::OffPlane));
    // Extruding the floating rectangle makes a floating solid (a shelf).
    let s = m.sketch(s2).unwrap();
    let region = sawhorse_kernel::ids::to_public(s.regions().next().unwrap().0);
    m.apply(&Op::Extrude {
        sketch: s2,
        region,
        distance: 0.75 * IN,
    })
    .unwrap();
    let st = m.stats();
    assert!(st.watertight);
    assert!((st.volume_m3 - 0.3 * 0.75 * IN).abs() < 1e-12);
}

#[test]
fn draw_ops_round_trip_through_json() {
    let op = Op::Draw {
        target: DrawTarget::Face { object: 1, face: 2 },
        points: vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]],
        closed: false,
    };
    let json = serde_json::to_string(&op).unwrap();
    assert!(
        json.contains(r#""target":{"on":"face","object":1,"face":2}"#),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
}
