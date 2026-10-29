//! Lane 25 (ADR-0013 addendum): curves that come in with imported geometry. A `.skp` file
//! says which edges make up a circle, an arc or a welded curve (openskp 1.4.0); an import
//! item carries them (`ImportItem::curves`), and each becomes a rim of the solid it runs along
//! when every segment is an edge of that one solid lying between two different faces, no
//! pair of faces twice, and a round curve's points there lie on one circle. Anything else
//! keeps its edges plain, and the import notes say how many and why. Strokes that are curves
//! (a DXF circle, a `.skp` circle lying loose or bounding a flat face) come in as sketch
//! curves, as in lane 23, and the one note counts both.

use std::f64::consts::TAU;

use glam::DVec3;
use sawhorse_kernel::Document;
use sawhorse_kernel::curve::{CurveKind, CurveSpec};
use sawhorse_kernel::meshbuild::{ImportCurve, ImportItem, build_item};
use sawhorse_kernel::model::{
    ImportDefinition, ImportNode, ImportNodeKind, ImportStroke, Model, Op, OpOutcome,
};
use sawhorse_kernel::testing::check_model;
use sawhorse_kernel::transform::Xform;

const R: f64 = 0.05;
const H: f64 = 0.02;
const N: u32 = 24;

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

fn item(
    name: &str,
    positions: Vec<[f64; 3]>,
    faces: Vec<Vec<Vec<u32>>>,
    curves: Vec<ImportCurve>,
) -> ImportItem {
    ImportItem {
        name: name.into(),
        group: None,
        tag: None,
        material: None,
        positions,
        faces,
        lines: vec![],
        merge_coplanar: false,
        face_labels: vec![],
        face_materials: vec![],
        face_uvs: vec![],
        edge_flags: vec![],
        curves,
    }
}

/// An n-sided prism of radius R from z = 0 to H about (cx, 0): bottom ring 0..n, top ring
/// n..2n, both caps (`open` leaves the top cap off) and n side quads.
fn prism(cx: f64, n: u32, open: bool) -> (Vec<[f64; 3]>, Vec<Vec<Vec<u32>>>) {
    let mut positions = Vec::new();
    for z in [0.0, H] {
        for k in 0..n {
            let a = TAU * f64::from(k) / f64::from(n);
            positions.push([cx + R * a.cos(), R * a.sin(), z]);
        }
    }
    let mut faces: Vec<Vec<Vec<u32>>> = vec![vec![(0..n).rev().collect()]];
    if !open {
        faces.push(vec![(n..2 * n).collect()]);
    }
    for k in 0..n {
        let j = (k + 1) % n;
        faces.push(vec![vec![k, j, n + j, n + k]]);
    }
    (positions, faces)
}

fn curve(kind: CurveKind, points: Vec<u32>, closed: bool) -> ImportCurve {
    ImportCurve {
        kind,
        points,
        closed,
    }
}

fn import(items: Vec<ImportItem>) -> Op {
    Op::Import {
        items,
        strokes: vec![],
        materials: vec![],
        scenes: vec![],
        hidden_tags: vec![],
        definitions: vec![],
        nodes: vec![],
    }
}

fn notes_of(out: &OpOutcome) -> Vec<String> {
    match out {
        OpOutcome::Imported { notes, .. } => notes.clone(),
        other => panic!("not an import: {other:?}"),
    }
}

/// A rim as it reads: (kind, closed, segments, centre, radius).
type Rim = (CurveKind, bool, u32, Option<[f64; 3]>, Option<f64>);

/// Every solid's rims, as they read.
fn rims(m: &Model) -> Vec<Rim> {
    let mut out = Vec::new();
    for (_, o) in m.objects() {
        for &id in o.curves().keys() {
            let i = o.curve_info(id).unwrap();
            out.push((i.kind, i.closed, i.segments, i.centre, i.radius));
        }
    }
    out
}

fn near(a: Option<[f64; 3]>, b: [f64; 3]) -> bool {
    a.is_some_and(|a| DVec3::from_array(a).distance(DVec3::from_array(b)) < 1e-12)
}

#[test]
fn a_cylinders_two_circles_come_in_as_its_rims() {
    let mut d = Document::new();
    let before = d.hash();
    let (positions, faces) = prism(0.0, N, false);
    let curves = vec![
        curve(CurveKind::Circle, (0..N).collect(), true),
        curve(CurveKind::Circle, (N..2 * N).rev().collect(), true),
    ];
    let out = ok(
        &mut d,
        import(vec![item("Dowel", positions, faces, curves)]),
    );
    assert_eq!(
        notes_of(&out),
        vec!["2 curves came in as curves".to_string()]
    );
    check_model(d.model()).unwrap();
    let r = rims(d.model());
    assert_eq!(r.len(), 2);
    for (kind, closed, segments, centre, radius) in &r {
        assert_eq!((*kind, *closed, *segments), (CurveKind::Circle, true, N));
        assert!((radius.unwrap() - R).abs() < 1e-12);
        assert!(near(*centre, [0.0, 0.0, 0.0]) || near(*centre, [0.0, 0.0, H]));
    }
    // One undo step; redo and a .shz round trip keep the rims.
    let after = d.hash();
    d.undo().unwrap();
    assert_eq!(d.hash(), before);
    d.redo().unwrap();
    assert_eq!(d.hash(), after);
    let text = serde_json::to_string(&d.model().snapshot()).unwrap();
    let back = Model::from_snapshot(&serde_json::from_str(&text).unwrap()).unwrap();
    assert_eq!(rims(&back), r);
}

#[test]
fn an_arc_and_a_closed_polyline_come_in_as_their_kinds() {
    let mut d = Document::new();
    // Half the top ring as an arc; a hexagonal prism's bottom ring as a closed polyline (a
    // welded curve with no arc record).
    let (mut positions, mut faces) = prism(0.0, N, false);
    let (hex, hex_faces) = prism(0.3, 6, false);
    let base = positions.len() as u32;
    positions.extend(hex);
    faces.extend(hex_faces.into_iter().map(|f| {
        f.into_iter()
            .map(|r| r.into_iter().map(|i| i + base).collect())
            .collect()
    }));
    let curves = vec![
        curve(CurveKind::Arc, (N..N + N / 2 + 1).collect(), false),
        curve(CurveKind::Polyline, (base..base + 6).collect(), true),
    ];
    let out = ok(
        &mut d,
        import(vec![item("Parts", positions, faces, curves)]),
    );
    assert_eq!(
        notes_of(&out),
        vec!["2 curves came in as curves (1 arc, 1 polyline)".to_string()]
    );
    check_model(d.model()).unwrap();
    let mut r: Vec<_> = rims(d.model())
        .into_iter()
        .map(|x| (x.0, x.1, x.2))
        .collect();
    r.sort();
    assert_eq!(
        r,
        vec![
            (CurveKind::Arc, false, N / 2),
            (CurveKind::Polyline, true, 6)
        ]
    );
}

#[test]
fn a_curve_on_a_mesh_on_a_face_or_off_its_circle_keeps_plain_edges() {
    // On an open prism (a mesh): no curve, said so.
    let (positions, faces) = prism(0.0, N, true);
    let b = build_item(&item(
        "Cup",
        positions,
        faces,
        vec![curve(CurveKind::Circle, (0..N).collect(), true)],
    ))
    .unwrap();
    assert!(b.solids.is_empty() && b.loose.is_some());
    assert_eq!(b.curves.not_solid, 1);

    // A circle drawn on the top of a box: every segment lies between the same two faces (the
    // disc and the face around it), so it cannot be keyed (ADR-0013: a circle drawn on a
    // solid's face imprints plain edges).
    let s = 0.2;
    let mut positions = vec![
        [0.0, 0.0, 0.0],
        [s, 0.0, 0.0],
        [s, s, 0.0],
        [0.0, s, 0.0],
        [0.0, 0.0, s],
        [s, 0.0, s],
        [s, s, s],
        [0.0, s, s],
    ];
    let ring: Vec<u32> = (0..N)
        .map(|k| {
            let a = TAU * f64::from(k) / f64::from(N);
            positions.push([0.1 + R * a.cos(), 0.1 + R * a.sin(), s]);
            8 + k
        })
        .collect();
    let mut hole = ring.clone();
    hole.reverse();
    let faces = vec![
        vec![vec![0, 3, 2, 1]],
        vec![vec![4, 5, 6, 7], hole],
        vec![ring.clone()],
        vec![vec![0, 1, 5, 4]],
        vec![vec![1, 2, 6, 5]],
        vec![vec![2, 3, 7, 6]],
        vec![vec![3, 0, 4, 7]],
    ];
    let mut d = Document::new();
    let out = ok(
        &mut d,
        import(vec![item(
            "Box",
            positions,
            faces,
            vec![curve(CurveKind::Circle, ring, true)],
        )]),
    );
    assert_eq!(
        notes_of(&out),
        vec!["1 curve drawn on a face of a solid came in as plain edges".to_string()]
    );
    check_model(d.model()).unwrap();
    assert!(rims(d.model()).is_empty());

    // Said to be a circle, but one point is 1 mm off it: plain edges, never a bent circle.
    // (The point below it moves too, so the side walls stay flat.)
    let (mut positions, faces) = prism(0.0, N, false);
    for i in [5, N + 5] {
        positions[i as usize][0] *= (R + 0.001) / R;
        positions[i as usize][1] *= (R + 0.001) / R;
    }
    let mut d = Document::new();
    let out = ok(
        &mut d,
        import(vec![item(
            "Dowel",
            positions,
            faces,
            vec![curve(CurveKind::Circle, (N..2 * N).collect(), true)],
        )]),
    );
    assert_eq!(
        notes_of(&out),
        vec![
            "1 round curve whose points on the solid are not on one circle came in as plain edges"
                .to_string()
        ]
    );
    assert!(rims(d.model()).is_empty());
}

#[test]
fn a_curve_that_names_no_point_or_is_the_wrong_shape_is_refused() {
    let (positions, faces) = prism(0.0, N, false);
    for (c, why) in [
        (
            curve(CurveKind::Circle, vec![0, 1, 99], true),
            "names a point that is not there",
        ),
        (
            curve(CurveKind::Circle, (0..N).collect(), false),
            "closed curve",
        ),
        (
            curve(CurveKind::Arc, (0..4).collect(), true),
            "closed curve",
        ),
        (
            curve(CurveKind::Polyline, vec![0, 1, 0], false),
            "one point twice",
        ),
        (curve(CurveKind::Polyline, vec![3], false), "too few points"),
    ] {
        let err = build_item(&item("Bad", positions.clone(), faces.clone(), vec![c])).unwrap_err();
        assert!(err.to_string().contains(why), "{err}");
    }
}

#[test]
fn a_components_rims_reach_every_copy_and_settle_after_an_edit() {
    let mut d = Document::new();
    let (positions, faces) = prism(0.0, N, false);
    let def = ImportDefinition {
        name: "Knob".into(),
        items: vec![item(
            "Knob",
            positions,
            faces,
            vec![curve(CurveKind::Circle, (N..2 * N).collect(), true)],
        )],
        children: vec![],
        sections: vec![],
    };
    let at = |x: f64| ImportNode {
        name: None,
        tag: None,
        hidden: false,
        sections: vec![],
        kind: ImportNodeKind::Instance {
            definition: 0,
            xform: Xform {
                linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                translation: [x, 0.0, 0.0],
            },
        },
    };
    let out = ok(
        &mut d,
        Op::Import {
            items: vec![],
            strokes: vec![],
            materials: vec![],
            scenes: vec![],
            hidden_tags: vec![],
            definitions: vec![def],
            nodes: vec![at(0.0), at(0.5)],
        },
    );
    // Counted once, for the definition.
    assert_eq!(notes_of(&out), vec!["1 curve came in as curve".to_string()]);
    check_model(d.model()).unwrap();
    let r = rims(d.model());
    assert_eq!(r.len(), 2, "one rim on each copy");
    assert!(near(r[0].3, [0.0, 0.0, H]) || near(r[1].3, [0.0, 0.0, H]));
    assert!(near(r[0].3, [0.5, 0.0, H]) || near(r[1].3, [0.5, 0.0, H]));
    // Pull the top of the first copy up: both copies grow, and the rims ride up with the
    // caps (their keys name the cap and each wall, which the push keeps).
    let OpOutcome::Imported { groups, .. } = out else {
        panic!()
    };
    ok(
        &mut d,
        Op::SetContext {
            node: Some(groups[0]),
        },
    );
    let inside: Vec<u64> = d
        .model()
        .graph()
        .objects
        .iter()
        .filter(|o| o.parent == Some(groups[0]))
        .map(|o| o.id)
        .collect();
    let solid = inside[0];
    let top = d
        .model()
        .object(solid)
        .unwrap()
        .brep()
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.99)
        .map(|(_, f)| f.id)
        .unwrap();
    ok(
        &mut d,
        Op::PushPull {
            object: solid,
            face: top,
            distance: H,
        },
    );
    ok(&mut d, Op::LeaveContext);
    check_model(d.model()).unwrap();
    let r = rims(d.model());
    assert_eq!(r.len(), 2, "still one rim on each copy");
    for x in &r {
        assert_eq!((x.0, x.2), (CurveKind::Circle, N));
        assert!((x.3.unwrap()[2] - 2.0 * H).abs() < 1e-12, "{x:?}");
    }
}

#[test]
fn a_disc_imported_as_a_curve_stroke_pulls_into_a_solid_with_rims() {
    // What `.skp` import makes of a SketchUp disc lying alone (src/io/skp.ts): a closed stroke
    // that is a circle, so the sketch finds the face and keeps the circle; pulled, the circle
    // becomes both caps' rims (ADR-0013).
    let mut d = Document::new();
    let pts: Vec<[f64; 3]> = (0..N)
        .map(|k| {
            let a = TAU * f64::from(k) / f64::from(N);
            [R * a.cos(), R * a.sin(), 0.0]
        })
        .collect();
    let out = ok(
        &mut d,
        Op::Import {
            items: vec![],
            strokes: vec![ImportStroke {
                points: pts,
                closed: true,
                curve: Some(CurveSpec {
                    kind: CurveKind::Circle,
                    centre: Some([0.0, 0.0, 0.0]),
                }),
            }],
            materials: vec![],
            scenes: vec![],
            hidden_tags: vec![],
            definitions: vec![],
            nodes: vec![],
        },
    );
    assert_eq!(notes_of(&out), vec!["1 curve came in as curve".to_string()]);
    let OpOutcome::Imported { sketches, .. } = out else {
        panic!()
    };
    let sid = sketches[0];
    let s = d.model().sketch(sid).unwrap();
    let region = s
        .regions()
        .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
        .next()
        .unwrap();
    ok(
        &mut d,
        Op::Extrude {
            sketch: sid,
            region,
            distance: H,
        },
    );
    check_model(d.model()).unwrap();
    let r = rims(d.model());
    assert_eq!(r.len(), 2);
    assert!(
        r.iter()
            .all(|x| x.0 == CurveKind::Circle && x.2 == N && (x.4.unwrap() - R).abs() < 1e-12)
    );
}

#[test]
fn strokes_and_rims_are_counted_in_one_note_by_kind() {
    let mut d = Document::new();
    let (positions, faces) = prism(0.0, N, false);
    let ring = |cx: f64| -> Vec<[f64; 3]> {
        (0..12)
            .map(|k| {
                let a = TAU * f64::from(k) / 12.0;
                [cx + R * a.cos(), R * a.sin(), 0.0]
            })
            .collect()
    };
    let out = ok(
        &mut d,
        Op::Import {
            items: vec![item(
                "Dowel",
                positions,
                faces,
                vec![curve(CurveKind::Circle, (0..N).collect(), true)],
            )],
            strokes: vec![
                ImportStroke {
                    points: ring(1.0),
                    closed: true,
                    curve: Some(CurveSpec {
                        kind: CurveKind::Circle,
                        centre: Some([1.0, 0.0, 0.0]),
                    }),
                },
                ImportStroke {
                    points: ring(2.0)[..5].to_vec(),
                    closed: false,
                    curve: Some(CurveSpec {
                        kind: CurveKind::Arc,
                        centre: Some([2.0, 0.0, 0.0]),
                    }),
                },
                // Said to be a circle about the wrong centre: drawn as plain edges, counted.
                ImportStroke {
                    points: ring(3.0),
                    closed: true,
                    curve: Some(CurveSpec {
                        kind: CurveKind::Circle,
                        centre: Some([3.1, 0.0, 0.0]),
                    }),
                },
            ],
            materials: vec![],
            scenes: vec![],
            hidden_tags: vec![],
            definitions: vec![],
            nodes: vec![],
        },
    );
    assert_eq!(
        notes_of(&out),
        vec!["3 curves came in as curves (2 circles, 1 arc; 1 as plain edges: their points did not land as drawn)".to_string()]
    );
    check_model(d.model()).unwrap();
}
