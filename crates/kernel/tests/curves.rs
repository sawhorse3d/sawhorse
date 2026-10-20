//! Lane 23 (#3, ADR-0013): curves. A circle, an arc or a polygon drawn by its tool is one
//! curve: it splits where other edges cross or end on it (arcs keep their circle), erases
//! whole, explodes into plain edges, moves, turns, scales and copies as one (a copy is a new
//! curve), refuses a lone point moved off it, offsets to a curve of its kind, becomes the
//! rims of a solid pushed from it (soft, smooth walls between its segments), and keeps its
//! id through undo, redo and a `.shz` round trip.

use std::f64::consts::{PI, TAU};

use glam::{DVec2, DVec3};
use sawhorse_kernel::curve::{CurveKind, CurveSpec};
use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome};
use sawhorse_kernel::offset::OffsetSource;
use sawhorse_kernel::transform::Xform;
use sawhorse_kernel::{Document, KernelError, TransformError};

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

fn ring(c: [f64; 2], r: f64, n: usize, a0: f64, sweep: f64, closed: bool) -> Vec<[f64; 3]> {
    let m = if closed { n } else { n + 1 };
    (0..m)
        .map(|k| {
            let a = a0 + sweep * k as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin(), 0.0]
        })
        .collect()
}

fn draw(points: Vec<[f64; 3]>, closed: bool, curve: Option<CurveSpec>) -> Op {
    Op::Draw {
        target: DrawTarget::Sketch { sketch: 1 },
        points,
        closed,
        curve,
    }
}

fn circle(c: [f64; 2], r: f64, n: usize) -> Op {
    draw(
        ring(c, r, n, 0.0, TAU, true),
        true,
        Some(CurveSpec {
            kind: CurveKind::Circle,
            centre: Some([c[0], c[1], 0.0]),
        }),
    )
}

fn polygon(c: [f64; 2], r: f64, n: usize) -> Op {
    draw(
        ring(c, r, n, 0.0, TAU, true),
        true,
        Some(CurveSpec {
            kind: CurveKind::Polygon,
            centre: Some([c[0], c[1], 0.0]),
        }),
    )
}

fn arc(c: [f64; 2], r: f64, n: usize, a0: f64, sweep: f64) -> Op {
    draw(
        ring(c, r, n, a0, sweep, false),
        false,
        Some(CurveSpec {
            kind: CurveKind::Arc,
            centre: Some([c[0], c[1], 0.0]),
        }),
    )
}

fn line(a: [f64; 2], b: [f64; 2]) -> Op {
    draw(vec![[a[0], a[1], 0.0], [b[0], b[1], 0.0]], false, None)
}

/// (id, kind, closed, edges, shown segments) of the ground sketch's curves.
fn curves(m: &Model) -> Vec<(u64, CurveKind, bool, usize, u32)> {
    m.sketch(1)
        .unwrap()
        .curves()
        .map(|(id, c)| (id, c.kind, c.closed, c.edge_count(), c.shown_segments()))
        .collect()
}

fn drawn_curve(out: &OpOutcome) -> u64 {
    match out {
        OpOutcome::Drawn { curve: Some(c), .. } => *c,
        other => panic!("no curve: {other:?}"),
    }
}

fn roundtrip(m: &Model) -> Model {
    let text = serde_json::to_string(&m.snapshot()).unwrap();
    let back = serde_json::from_str(&text).unwrap();
    Model::from_snapshot(&back).unwrap()
}

#[test]
fn a_circle_is_one_curve_with_its_circle() {
    let mut d = Document::with_ground_sketch();
    let before = d.hash();
    let id = drawn_curve(&ok(&mut d, circle([1.0, 2.0], 0.3, 24)));
    assert_eq!(
        curves(d.model()),
        vec![(id, CurveKind::Circle, true, 24, 24)]
    );
    let s = d.model().sketch(1).unwrap();
    let info = s.curve_info(id).unwrap();
    assert!((info.radius.unwrap() - 0.3).abs() < 1e-12);
    assert!((DVec3::from_array(info.centre.unwrap()) - DVec3::new(1.0, 2.0, 0.0)).length() < 1e-12);
    assert!((info.sweep.unwrap().abs() - TAU).abs() < 1e-9);
    // Undo and redo, and a .shz round trip, keep it (and its id).
    let after = d.hash();
    d.undo().unwrap();
    assert_eq!(d.hash(), before);
    assert!(curves(d.model()).is_empty());
    d.redo().unwrap();
    assert_eq!(d.hash(), after);
    let back = roundtrip(d.model());
    assert_eq!(back.hash(), d.model().hash());
    assert_eq!(curves(&back), curves(d.model()));
}

#[test]
fn a_document_without_curves_keeps_its_hash() {
    // The same strokes without the curve field hash as they always did: curves are hashed
    // only when a sketch has had one.
    let mut a = Document::with_ground_sketch();
    ok(
        &mut a,
        draw(ring([0.0, 0.0], 0.3, 24, 0.0, TAU, true), true, None),
    );
    let plain = a.hash();
    let mut b = Document::with_ground_sketch();
    ok(&mut b, circle([0.0, 0.0], 0.3, 24));
    assert_ne!(b.hash(), plain);
    assert!(curves(a.model()).is_empty());
    // Its snapshot has no "curves" or "next_curve" keys.
    let text = serde_json::to_string(&a.model().snapshot()).unwrap();
    assert!(
        !text.contains("curves") && !text.contains("next_curve"),
        "{text}"
    );
}

#[test]
fn a_line_across_a_circle_makes_two_arcs_on_its_circle() {
    let mut d = Document::with_ground_sketch();
    let id = drawn_curve(&ok(&mut d, circle([0.0, 0.0], 1.0, 24)));
    ok(&mut d, line([-2.0, 0.1], [2.0, 0.1]));
    let cs = curves(d.model());
    assert_eq!(cs.len(), 2, "{cs:?}");
    assert!(cs.iter().all(|c| c.1 == CurveKind::Arc && !c.2));
    assert_eq!(cs[0].0, id, "the first piece keeps the id");
    // Every chord point and the arcs' angles agree with the circle.
    let s = d.model().sketch(1).unwrap();
    let total: f64 = cs
        .iter()
        .map(|c| s.curve(c.0).unwrap().round.unwrap().sweep.abs())
        .sum();
    assert!((total - TAU).abs() < 1e-9, "{total}");
    assert_eq!(
        cs.iter().map(|c| c.3).sum::<usize>(),
        26,
        "24 segments, two split"
    );
    d.model().validate().unwrap();
}

#[test]
fn one_line_ending_on_a_circle_leaves_it_whole_two_split_it() {
    let mut d = Document::with_ground_sketch();
    ok(&mut d, circle([0.0, 0.0], 1.0, 24));
    // To a vertex of the circle (angle 0).
    ok(&mut d, line([2.0, 0.0], [1.0, 0.0]));
    assert_eq!(curves(d.model())[0].1, CurveKind::Circle);
    // A T-junction on a segment's middle: still the circle, with a 25th edge.
    let mid = [
        0.5 * (1.0 + (TAU / 24.0).cos()) * 1.0,
        0.5 * (TAU / 24.0).sin(),
    ];
    let _ = mid;
    let a = TAU / 24.0 * 6.0;
    let b = TAU / 24.0 * 7.0;
    let m = [0.5 * (a.cos() + b.cos()), 0.5 * (a.sin() + b.sin())];
    ok(&mut d, line([0.0, 3.0], m));
    let cs = curves(d.model());
    assert_eq!(cs.len(), 2, "two junctions: two arcs {cs:?}");
    assert!(cs.iter().all(|c| c.1 == CurveKind::Arc));
    assert_eq!(cs.iter().map(|c| c.3).sum::<usize>(), 25);
    d.model().validate().unwrap();
}

#[test]
fn erasing_a_curve_takes_it_whole_and_one_segment_leaves_a_polyline() {
    let mut d = Document::with_ground_sketch();
    let id = drawn_curve(&ok(&mut d, circle([0.0, 0.0], 1.0, 24)));
    ok(&mut d, line([-2.0, 0.0], [2.0, 0.0]));
    let cs = curves(d.model());
    let other = cs.iter().find(|c| c.0 != id).unwrap().0;
    let before = d.hash();
    ok(
        &mut d,
        Op::EraseCurve {
            sketch: 1,
            curve: id,
        },
    );
    let cs = curves(d.model());
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].0, other, "only that arc went");
    // The line stays, in its three pieces: the points where it met the circle are points
    // of the other arc, so they stay.
    let s = d.model().sketch(1).unwrap();
    assert_eq!(
        s.edge_count(),
        12 + 3,
        "12 arc edges and the line's 3 pieces"
    );
    d.undo().unwrap();
    assert_eq!(d.hash(), before);
    // The plain Eraser op on one segment: the rest stays one curve, a polyline.
    let mut d = Document::with_ground_sketch();
    ok(&mut d, circle([0.0, 0.0], 1.0, 24));
    let p = ring([0.0, 0.0], 1.0, 24, 0.0, TAU, true);
    ok(
        &mut d,
        Op::EraseSegment {
            sketch: 1,
            a: [p[0][0], p[0][1]],
            b: [p[1][0], p[1][1]],
        },
    );
    assert_eq!(
        curves(d.model()),
        vec![(1, CurveKind::Polyline, false, 23, 23)]
    );
    assert!(matches!(
        d.apply(&Op::EraseCurve {
            sketch: 1,
            curve: 9
        }),
        Err(KernelError::Sketch(_))
    ));
}

#[test]
fn explode_leaves_plain_edges() {
    let mut d = Document::with_ground_sketch();
    let id = drawn_curve(&ok(&mut d, circle([0.0, 0.0], 1.0, 24)));
    let out = ok(
        &mut d,
        Op::ExplodeCurve {
            sketch: Some(1),
            object: None,
            curve: id,
        },
    );
    assert_eq!(out, OpOutcome::CurveExploded { edges: 24 });
    assert!(curves(d.model()).is_empty());
    assert_eq!(d.model().sketch(1).unwrap().edge_count(), 24);
    assert_eq!(d.model().sketch(1).unwrap().region_count(), 1);
    // The counter is kept (the next curve does not reuse the id) and saved.
    let id2 = drawn_curve(&ok(&mut d, circle([5.0, 0.0], 1.0, 12)));
    assert!(id2 > id);
    let back = roundtrip(d.model());
    assert_eq!(back.hash(), d.model().hash());
}

#[test]
fn polygons_and_arcs_are_their_kinds() {
    let mut d = Document::with_ground_sketch();
    let p = drawn_curve(&ok(&mut d, polygon([0.0, 0.0], 1.0, 6)));
    let a = drawn_curve(&ok(&mut d, arc([5.0, 0.0], 1.0, 12, 0.25, 2.0)));
    let cs = curves(d.model());
    assert_eq!(cs[0], (p, CurveKind::Polygon, true, 6, 6));
    assert_eq!(cs[1], (a, CurveKind::Arc, false, 12, 12));
    let r = d
        .model()
        .sketch(1)
        .unwrap()
        .curve(a)
        .unwrap()
        .round
        .unwrap();
    assert!((r.start - 0.25).abs() < 1e-12 && (r.sweep - 2.0).abs() < 1e-9);
    // A polygon split by a line: polylines.
    ok(&mut d, line([-2.0, 0.1], [2.0, 0.1]));
    assert!(
        curves(d.model())
            .iter()
            .filter(|c| c.0 != a)
            .all(|c| c.1 == CurveKind::Polyline)
    );
}

#[test]
fn bad_curve_specs_are_refused() {
    let mut d = Document::with_ground_sketch();
    // An open circle, a closed arc, no centre, the wrong centre.
    let pts = ring([0.0, 0.0], 1.0, 24, 0.0, TAU, true);
    let spec = |kind, centre| Some(CurveSpec { kind, centre });
    for op in [
        draw(pts.clone(), false, spec(CurveKind::Circle, Some([0.0; 3]))),
        draw(pts.clone(), true, spec(CurveKind::Arc, Some([0.0; 3]))),
        draw(pts.clone(), true, spec(CurveKind::Circle, None)),
        draw(
            pts.clone(),
            true,
            spec(CurveKind::Circle, Some([0.1, 0.0, 0.0])),
        ),
        draw(
            pts.clone(),
            true,
            spec(CurveKind::Circle, Some([0.0, 0.0, 0.5])),
        ),
    ] {
        let h = d.hash();
        assert!(d.apply(&op).is_err(), "{op:?}");
        assert_eq!(d.hash(), h);
    }
}

fn xform_op(curve: u64, x: Xform, copy: bool) -> Op {
    Op::TransformSketch {
        sketch: 1,
        points: vec![],
        edges: vec![],
        regions: vec![],
        curves: vec![curve],
        xform: x,
        copy,
        count: 1,
    }
}

#[test]
fn a_circle_moves_turns_and_scales_as_a_circle() {
    let mut d = Document::with_ground_sketch();
    let id = drawn_curve(&ok(&mut d, circle([0.0, 0.0], 0.3, 24)));
    let out = ok(
        &mut d,
        xform_op(id, Xform::translation(DVec3::new(1.0, 0.5, 0.0)), false),
    );
    assert!(matches!(out, OpOutcome::SketchMoved { ref curves, .. } if curves == &vec![[1, id]]));
    let r = d
        .model()
        .sketch(1)
        .unwrap()
        .curve(id)
        .unwrap()
        .round
        .unwrap();
    assert!((r.centre - DVec2::new(1.0, 0.5)).length() < 1e-12 && (r.radius - 0.3).abs() < 1e-12);
    ok(
        &mut d,
        xform_op(id, Xform::rotation(DVec3::ZERO, DVec3::Z, PI / 2.0), false),
    );
    let r = d
        .model()
        .sketch(1)
        .unwrap()
        .curve(id)
        .unwrap()
        .round
        .unwrap();
    assert!((r.centre - DVec2::new(-0.5, 1.0)).length() < 1e-12);
    ok(
        &mut d,
        xform_op(
            id,
            Xform::scaling(DVec3::new(-0.5, 1.0, 0.0), DVec3::new(2.0, 2.0, 1.0)),
            false,
        ),
    );
    let c = d.model().sketch(1).unwrap().curve(id).unwrap();
    assert_eq!(c.kind, CurveKind::Circle);
    assert!((c.round.unwrap().radius - 0.6).abs() < 1e-12);
    // A mirror keeps it a circle; a non-uniform scale makes it one curve, a polyline.
    ok(
        &mut d,
        xform_op(
            id,
            Xform::scaling(DVec3::new(-0.5, 1.0, 0.0), DVec3::new(-1.0, 1.0, 1.0)),
            false,
        ),
    );
    assert_eq!(curves(d.model())[0].1, CurveKind::Circle);
    ok(
        &mut d,
        xform_op(
            id,
            Xform::scaling(DVec3::new(-0.5, 1.0, 0.0), DVec3::new(2.0, 1.0, 1.0)),
            false,
        ),
    );
    assert_eq!(
        curves(d.model()),
        vec![(id, CurveKind::Polyline, true, 24, 24)]
    );
    d.model().validate().unwrap();
}

#[test]
fn a_copy_is_a_new_curve_and_a_lone_point_is_refused() {
    let mut d = Document::with_ground_sketch();
    let id = drawn_curve(&ok(&mut d, circle([0.0, 0.0], 0.3, 24)));
    let out = ok(
        &mut d,
        xform_op(id, Xform::translation(DVec3::new(2.0, 0.0, 0.0)), true),
    );
    let OpOutcome::SketchCopied { curves: made, .. } = out else {
        panic!("{out:?}")
    };
    assert_eq!(made.len(), 1);
    assert_ne!(made[0][1], id);
    let cs = curves(d.model());
    assert_eq!(cs.len(), 2);
    assert!(cs.iter().all(|c| c.1 == CurveKind::Circle));
    // An array of three.
    let out = ok(
        &mut d,
        Op::TransformSketch {
            sketch: 1,
            points: vec![],
            edges: vec![],
            regions: vec![],
            curves: vec![id],
            xform: Xform::translation(DVec3::new(0.0, 1.0, 0.0)),
            copy: true,
            count: 3,
        },
    );
    assert!(matches!(out, OpOutcome::SketchCopied { ref curves, .. } if curves.len() == 3));
    // A point of a curve alone: refused, naming Explode Curve.
    let h = d.hash();
    let e = d
        .apply(&Op::TransformSketch {
            sketch: 1,
            points: vec![[0.3, 0.0, 0.0]],
            edges: vec![],
            regions: vec![],
            curves: vec![],
            xform: Xform::translation(DVec3::new(0.0, 0.1, 0.0)),
            copy: false,
            count: 1,
        })
        .unwrap_err();
    assert_eq!(e, KernelError::Transform(TransformError::CurvePoint));
    assert!(e.to_string().contains("Explode Curve"));
    assert_eq!(d.hash(), h);
}

#[test]
fn a_circle_lifted_off_its_plane_is_a_circle_there() {
    let mut d = Document::with_ground_sketch();
    let id = drawn_curve(&ok(&mut d, circle([0.0, 0.0], 0.3, 24)));
    let out = ok(
        &mut d,
        xform_op(id, Xform::rotation(DVec3::ZERO, DVec3::X, PI / 2.0), false),
    );
    let OpOutcome::SketchMoved {
        sketch, curves: c, ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_ne!(sketch, 1);
    assert_eq!(c.len(), 1);
    let s = d.model().sketch(sketch).unwrap();
    let k = s.curve(c[0][1]).unwrap();
    assert_eq!(k.kind, CurveKind::Circle);
    assert!((k.round.unwrap().radius - 0.3).abs() < 1e-12);
    assert!(curves(d.model()).is_empty(), "it left the ground");
}

#[test]
fn a_pushed_circle_has_rims_and_soft_walls() {
    let mut d = Document::with_ground_sketch();
    let out = ok(&mut d, circle([0.0, 0.0], 0.3, 24));
    let OpOutcome::Drawn {
        inside: Some(region),
        ..
    } = out
    else {
        panic!()
    };
    let out = ok(
        &mut d,
        Op::Extrude {
            sketch: 1,
            region,
            distance: 0.5,
        },
    );
    let OpOutcome::ObjectCreated { object } = out else {
        panic!()
    };
    assert!(
        curves(d.model()).is_empty(),
        "the circle went into the solid"
    );
    let o = d.model().object(object).unwrap();
    assert_eq!(o.curves().len(), 2, "base and cap rims");
    for id in o.curves().keys() {
        let info = o.curve_info(*id).unwrap();
        assert_eq!(info.kind, CurveKind::Circle);
        assert_eq!(info.segments, 24);
        assert!((info.radius.unwrap() - 0.3).abs() < 1e-9);
    }
    // The 24 walls meet across soft, smooth edges; the rims stay hard.
    assert_eq!(o.edge_flags().len(), 24);
    assert!(o.edge_flags().values().all(|f| *f == 3));
    // Push the cap further: the rims stay. Copy and save: they stay.
    ok(
        &mut d,
        Op::PushPull {
            object,
            face: 2,
            distance: 0.25,
        },
    );
    assert_eq!(d.model().object(object).unwrap().curves().len(), 2);
    let out = ok(
        &mut d,
        Op::Copy {
            objects: vec![object],
            xform: Xform::translation(DVec3::new(2.0, 0.0, 0.0)),
            nodes: vec![],
        },
    );
    let OpOutcome::Copied { objects, .. } = out else {
        panic!()
    };
    assert_eq!(d.model().object(objects[0]).unwrap().curves().len(), 2);
    let back = roundtrip(d.model());
    assert_eq!(back.hash(), d.model().hash());
    // Scaled out of round, a rim reads as a curve (a polyline).
    ok(
        &mut d,
        Op::Transform {
            objects: vec![object],
            xform: Xform::scaling(DVec3::ZERO, DVec3::new(2.0, 1.0, 1.0)),
            nodes: vec![],
        },
    );
    let o = d.model().object(object).unwrap();
    let info = o.curve_info(*o.curves().keys().next().unwrap()).unwrap();
    assert_eq!(info.kind, CurveKind::Polyline);
    // Explode a rim: one fewer.
    let rim = *o.curves().keys().next().unwrap();
    ok(
        &mut d,
        Op::ExplodeCurve {
            sketch: None,
            object: Some(object),
            curve: rim,
        },
    );
    assert_eq!(d.model().object(object).unwrap().curves().len(), 1);
}

#[test]
fn a_component_carries_its_rims() {
    let mut d = Document::with_ground_sketch();
    let out = ok(&mut d, circle([0.0, 0.0], 0.3, 24));
    let OpOutcome::Drawn {
        inside: Some(region),
        ..
    } = out
    else {
        panic!()
    };
    let OpOutcome::ObjectCreated { object } = ok(
        &mut d,
        Op::Extrude {
            sketch: 1,
            region,
            distance: 0.5,
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::MakeComponent {
            name: "Dowel".into(),
            items: vec![sawhorse_kernel::nodes::Item::Object(object)],
        },
    );
    let m = d.model();
    assert!(m.objects().iter().all(|(_, o)| o.curves().len() == 2));
}

#[test]
fn a_circle_pushed_back_flat_comes_back_a_circle() {
    let mut d = Document::with_ground_sketch();
    let out = ok(&mut d, circle([0.0, 0.0], 0.3, 24));
    let OpOutcome::Drawn {
        inside: Some(region),
        ..
    } = out
    else {
        panic!()
    };
    let OpOutcome::ObjectCreated { object } = ok(
        &mut d,
        Op::Extrude {
            sketch: 1,
            region,
            distance: 0.5,
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::PushPull {
            object,
            face: 2,
            distance: -0.5,
        },
    );
    let cs = curves(d.model());
    assert_eq!(cs.len(), 1, "{cs:?}");
    assert_eq!(cs[0].1, CurveKind::Circle);
}

#[test]
fn an_offset_circle_is_a_circle() {
    let mut d = Document::with_ground_sketch();
    let out = ok(&mut d, circle([0.0, 0.0], 0.3, 24));
    let OpOutcome::Drawn {
        inside: Some(region),
        ..
    } = out
    else {
        panic!()
    };
    let out = ok(
        &mut d,
        Op::Offset {
            source: OffsetSource::Region { sketch: 1, region },
            distance: -0.05,
        },
    );
    let c = drawn_curve(&out);
    let s = d.model().sketch(1).unwrap();
    let k = s.curve(c).unwrap();
    assert_eq!(k.kind, CurveKind::Circle);
    assert!(k.round.unwrap().centre.length() < 1e-9);
    assert!(k.round.unwrap().radius < 0.3);
    // An arc's chain offset: an arc.
    let mut d = Document::with_ground_sketch();
    let a = drawn_curve(&ok(&mut d, arc([0.0, 0.0], 1.0, 12, 0.0, 2.0)));
    let pts = d.model().sketch(1).unwrap().curve_info(a).unwrap().points;
    let out = ok(
        &mut d,
        Op::Offset {
            source: OffsetSource::Edges {
                points: pts,
                closed: false,
                normal: [0.0, 0.0, 1.0],
                object: None,
            },
            distance: 0.1,
        },
    );
    let c = drawn_curve(&out);
    assert_eq!(
        d.model().sketch(1).unwrap().curve(c).unwrap().kind,
        CurveKind::Arc
    );
}

#[test]
fn dxf_circles_import_as_curves() {
    let mut d = Document::with_ground_sketch();
    let pts = ring([0.0, 0.0], 0.5, 48, 0.0, TAU, true);
    let out = ok(
        &mut d,
        Op::Import {
            items: vec![],
            strokes: vec![sawhorse_kernel::model::ImportStroke {
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
    let OpOutcome::Imported { notes, .. } = out else {
        panic!()
    };
    assert!(
        notes.iter().any(|n| n == "1 curve came in as curve"),
        "{notes:?}"
    );
    assert_eq!(curves(d.model())[0].1, CurveKind::Circle);
}

#[test]
fn a_tiny_triangle_pushed_flat_and_pulled_again_keeps_its_rims() {
    // Property-test find (lane 23): a 0.1 mm three-segment circle pushed into a solid, the
    // cap pushed back flat (the circle returns to the sketch) and pulled again. The solid
    // takes the reused slot, so a .shz renumbers it: compare what it holds, not its id.
    let mut d = Document::with_ground_sketch();
    let out = ok(&mut d, circle([0.0, 0.0], 1e-4, 3));
    let OpOutcome::Drawn {
        inside: Some(region),
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    let OpOutcome::ObjectCreated { object } = ok(
        &mut d,
        Op::Extrude {
            sketch: 1,
            region,
            distance: 0.5,
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::PushPull {
            object,
            face: 2,
            distance: -0.5,
        },
    );
    assert_eq!(curves(d.model()).len(), 1, "the circle came back");
    let regions: Vec<u64> = d
        .model()
        .sketch(1)
        .unwrap()
        .regions()
        .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
        .collect();
    ok(
        &mut d,
        Op::Extrude {
            sketch: 1,
            region: regions[0],
            distance: 0.5,
        },
    );
    let back = roundtrip(d.model());
    for ((_, a), (_, b)) in d.model().objects().into_iter().zip(back.objects()) {
        assert_eq!(a.curves(), b.curves());
        assert_eq!(a.curves().len(), 2);
        assert_eq!(
            sawhorse_kernel::hash::object_hash(a),
            sawhorse_kernel::hash::object_hash(b)
        );
    }
}

#[test]
fn a_huge_nonagon_copied_a_hair_turned_still_draws() {
    // Property-test find (lane 23, seed 2026102300): a 9 km nonagon (a curve or not) copied
    // turned 1e-7 degrees about a corner leaves 0.14 m2 slivers 1.5e-5 m wide bent round the
    // other corners, whose correct triangles the area check measured 1.2e-8 off, so the view
    // could not be built (fixed in sawhorse-tessellate: areas to their own size).
    for curve in [false, true] {
        let mut d = Document::with_ground_sketch();
        let pts = ring([0.0, 0.0], 9000.0, 9, 0.0, TAU, true);
        let spec = curve.then_some(CurveSpec {
            kind: CurveKind::Circle,
            centre: Some([0.0; 3]),
        });
        ok(&mut d, draw(pts.clone(), true, spec));
        let r = d
            .model()
            .sketch(1)
            .unwrap()
            .regions()
            .next()
            .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
            .unwrap();
        ok(
            &mut d,
            Op::TransformSketch {
                sketch: 1,
                points: vec![],
                edges: vec![],
                regions: vec![r],
                curves: vec![],
                xform: Xform::rotation(DVec3::from_array(pts[0]), DVec3::Z, 1e-7f64.to_radians()),
                copy: true,
                count: 1,
            },
        );
        sawhorse_kernel::testing::check_model(d.model()).unwrap();
    }
}

#[test]
fn a_huge_67_gon_copied_a_hair_turned_pushes_into_solids() {
    // Property-test find (lane 23, seed 2026102301): a 9 km 67-gon (a curve or not) copied
    // turned 1e-7 degrees about a corner leaves slivers whose tips are 1.3e-8 rad wide;
    // extruded, the validator called the edge leaving a tip a crossing of the other side's
    // wall (a rounding of the test's own point; fixed in `intersect`: an edge from a point of
    // a face's plane meets it nowhere else unless it lies in it).
    for curve in [false, true] {
        let mut d = Document::with_ground_sketch();
        let pts = ring([0.0, 0.0], 9000.0, 67, 0.0, TAU, true);
        let spec = curve.then_some(CurveSpec {
            kind: CurveKind::Circle,
            centre: Some([0.0; 3]),
        });
        ok(&mut d, draw(pts, true, spec));
        let s = d.model().sketch(1).unwrap();
        let verts: Vec<DVec3> = s.vertices().map(|(_, p)| s.plane().to_world(p)).collect();
        let about = verts[71 % verts.len()];
        let r = s
            .regions()
            .next()
            .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
            .unwrap();
        ok(
            &mut d,
            Op::TransformSketch {
                sketch: 1,
                points: vec![],
                edges: vec![],
                regions: if curve { vec![] } else { vec![r] },
                curves: if curve { vec![1] } else { vec![] },
                xform: Xform::rotation(about, DVec3::Z, 1e-7f64.to_radians()),
                copy: true,
                count: 1,
            },
        );
        let regions: Vec<u64> = d
            .model()
            .sketch(1)
            .unwrap()
            .regions()
            .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
            .collect();
        for region in regions {
            let mut e = d.clone();
            ok(
                &mut e,
                Op::Extrude {
                    sketch: 1,
                    region,
                    distance: 0.5,
                },
            );
            sawhorse_kernel::testing::check_model(e.model()).unwrap();
        }
    }
}
