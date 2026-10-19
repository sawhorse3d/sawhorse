//! Moving, turning, scaling and copying sketch geometry (lane 22, issue #2, ADR-0012): the
//! `TransformSketch` op behind Move, Rotate and Scale on circles, rectangles, polygons,
//! arcs and lines that have not been pushed into a solid.
//!
//! One test per rule: a whole region translates (its id kept); a point or an edge deforms
//! its region and the neighbours' edges follow; a shared edge moves both regions; a move
//! off the plane is projected for part of a shape and lifts a whole shape onto the plane it
//! lands on; landing on other geometry welds and splits the way drawing does (no duplicate
//! or zero-length edge, no bow-tie); turns in the plane and about other axes, by 0, 180 and
//! 360 degrees; copies and arrays are independent geometry; refusals are typed and leave
//! the document alone; a moved region pushes into a solid of exactly its area times the
//! distance; a moved sketch saves and opens unchanged; undo is one step.

use std::f64::consts::PI;

use glam::{DVec2, DVec3};
use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome};
use sawhorse_kernel::nodes::Item;
use sawhorse_kernel::transform::Xform;
use sawhorse_kernel::{Document, KernelError, SketchError, StructureError, TransformError};

const IN: f64 = 0.0254;

fn doc(ops: &[Op]) -> Document {
    let mut d = Document::with_ground_sketch();
    for (i, op) in ops.iter().enumerate() {
        d.apply(op).unwrap_or_else(|e| panic!("op {i} {op:?}: {e}"));
    }
    d
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

/// A circle of `n` segments, as the Circle tool draws it (a closed stroke on the ground).
fn circle(c: [f64; 2], r: f64, n: usize) -> Op {
    let points = (0..n)
        .map(|k| {
            let a = 2.0 * PI * k as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin(), 0.0]
        })
        .collect();
    Op::Draw {
        target: DrawTarget::Sketch { sketch: 1 },
        points,
        closed: true,
    }
}

fn op(
    sketch: u64,
    points: &[[f64; 3]],
    edges: &[[[f64; 3]; 2]],
    regions: &[u64],
    xform: Xform,
) -> Op {
    Op::TransformSketch {
        sketch,
        points: points.to_vec(),
        edges: edges.to_vec(),
        regions: regions.to_vec(),
        xform,
        copy: false,
        count: 1,
    }
}

fn copy_op(sketch: u64, regions: &[u64], edges: &[[[f64; 3]; 2]], xform: Xform, count: u32) -> Op {
    Op::TransformSketch {
        sketch,
        points: Vec::new(),
        edges: edges.to_vec(),
        regions: regions.to_vec(),
        xform,
        copy: true,
        count,
    }
}

fn shift(x: f64, y: f64, z: f64) -> Xform {
    Xform::translation(DVec3::new(x, y, z))
}

fn turn(about: [f64; 3], axis: DVec3, deg: f64) -> Xform {
    Xform::rotation(DVec3::from_array(about), axis, deg.to_radians())
}

/// Sketch `id`'s vertices (sketch frame), rounded to the nanometre and sorted.
fn verts(m: &Model, id: u64) -> Vec<(i64, i64)> {
    let mut v: Vec<(i64, i64)> = m
        .sketch(id)
        .unwrap()
        .vertices()
        .map(|(_, p)| ((p.x * 1e9).round() as i64, (p.y * 1e9).round() as i64))
        .collect();
    v.sort();
    v
}

fn q(x: f64, y: f64) -> (i64, i64) {
    ((x * 1e9).round() as i64, (y * 1e9).round() as i64)
}

/// Live region areas of sketch `id`, sorted.
fn areas(m: &Model, id: u64) -> Vec<f64> {
    let s = m.sketch(id).unwrap();
    let mut a: Vec<f64> = s.regions().map(|(_, r)| s.region_area(r)).collect();
    a.sort_by(f64::total_cmp);
    a
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

fn region_ids(m: &Model, id: u64) -> Vec<u64> {
    let s = m.sketch(id).unwrap();
    s.regions()
        .map(|(r, _)| sawhorse_kernel::ids::to_public(r))
        .collect()
}

/// Apply, expecting a refusal that leaves the document exactly as it was.
fn refused(d: &mut Document, op: &Op) -> KernelError {
    let (hash, log, depth) = (d.hash(), d.log().len(), d.depth());
    let e = d.apply(op).expect_err("the op should be refused");
    assert!(!e.is_bug(), "a refusal, not a bug: {e}");
    assert_eq!((d.hash(), d.log().len(), d.depth()), (hash, log, depth));
    e
}

/// Undo restores the hash before; redo the hash after (one undo step).
fn one_step(d: &mut Document, op: &Op) -> OpOutcome {
    let before = d.hash();
    let out = d.apply(op).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    let after = d.hash();
    assert_ne!(before, after, "the op changed something");
    d.undo().unwrap();
    assert_eq!(d.hash(), before, "one undo step restores it");
    d.redo().unwrap();
    assert_eq!(d.hash(), after);
    sawhorse_kernel::testing::check_model(d.model()).unwrap();
    out
}

#[test]
fn a_whole_rectangle_moves_and_keeps_its_region() {
    let mut d = doc(&[rect(0.0, 0.0, 2.0, 1.0)]);
    let out = one_step(&mut d, &op(1, &[], &[], &[1], shift(0.5, 0.25, 0.0)));
    assert_eq!(
        out,
        OpOutcome::SketchMoved {
            sketch: 1,
            regions: vec![[1, 1]]
        }
    );
    let m = d.model();
    assert_eq!(
        verts(m, 1),
        vec![q(0.5, 0.25), q(0.5, 1.25), q(2.5, 0.25), q(2.5, 1.25)]
    );
    assert_eq!(region_ids(m, 1), vec![1], "the region keeps its id");
    assert!(close(&areas(m, 1), &[2.0]));
    // The anchor moved with it (still inside, so the next edit keeps the id too).
    let s = m.sketch(1).unwrap();
    let r = s.region(sawhorse_kernel::ids::from_public(1)).unwrap();
    assert!((r.anchor - DVec2::new(1.5, 0.75)).length() < 1.0);
}

#[test]
fn a_circle_moves_by_a_typed_distance() {
    let mut d = doc(&[circle([0.0, 0.0], 0.1, 24)]);
    let before = areas(d.model(), 1);
    let ids = region_ids(d.model(), 1);
    // 36 inches along red, as the Move tool sends a typed distance.
    one_step(&mut d, &op(1, &[], &[], &ids, shift(36.0 * IN, 0.0, 0.0)));
    let m = d.model();
    assert!(close(&areas(m, 1), &before));
    let s = m.sketch(1).unwrap();
    assert_eq!(s.vertex_count(), 24);
    for (_, p) in s.vertices() {
        let r = (p - DVec2::new(36.0 * IN, 0.0)).length();
        assert!((r - 0.1).abs() < 1e-12, "every point on the moved circle");
    }
}

#[test]
fn moving_a_corner_deforms_the_rectangle_and_its_edges_follow() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    // The corner (1, 1) to (1.5, 1.25): the two edges meeting there follow it.
    one_step(
        &mut d,
        &op(1, &[[1.0, 1.0, 0.0]], &[], &[], shift(0.5, 0.25, 0.0)),
    );
    let m = d.model();
    assert_eq!(
        verts(m, 1),
        vec![q(0.0, 0.0), q(0.0, 1.0), q(1.0, 0.0), q(1.5, 1.25)]
    );
    assert_eq!(m.sketch(1).unwrap().edge_count(), 4);
    assert_eq!(region_ids(m, 1), vec![1], "same region, new shape");
    // Shoelace of (0,0) (1,0) (1.5,1.25) (0,1).
    assert!(close(&areas(m, 1), &[1.375]));
}

#[test]
fn a_point_moved_off_the_plane_is_projected_into_it() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    // Up and sideways at once: the corner moves sideways only, the region stays flat.
    one_step(
        &mut d,
        &op(1, &[[1.0, 1.0, 0.0]], &[], &[], shift(0.25, 0.0, 0.75)),
    );
    let m = d.model();
    assert_eq!(m.sketch_ids(), vec![1], "no new sketch for part of a shape");
    assert!(verts(m, 1).contains(&q(1.25, 1.0)));
    // Straight up: nothing would move; refused, typed, nothing changed.
    let e = refused(
        &mut d,
        &op(1, &[[1.25, 1.0, 0.0]], &[], &[], shift(0.0, 0.0, 0.5)),
    );
    assert_eq!(e, KernelError::Transform(TransformError::LeavesSketchPlane));
}

#[test]
fn an_edge_shared_by_two_regions_moves_both() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0), rect(1.0, 0.0, 1.0, 1.0)]);
    let ids = region_ids(d.model(), 1);
    let out = one_step(
        &mut d,
        &op(
            1,
            &[],
            &[[[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]]],
            &[],
            shift(0.25, 0.0, 0.0),
        ),
    );
    let OpOutcome::SketchMoved { regions, .. } = out else {
        panic!("{out:?}")
    };
    assert_eq!(regions.len(), 2, "both regions bound the moved edge");
    let m = d.model();
    assert_eq!(region_ids(m, 1), ids, "both keep their ids");
    assert!(close(&areas(m, 1), &[0.75, 1.25]));
    assert_eq!(m.sketch(1).unwrap().edge_count(), 7);
}

#[test]
fn moving_onto_a_neighbour_welds_the_coincident_edge() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0), rect(2.0, 0.0, 1.0, 1.0)]);
    let b = d
        .model()
        .sketch(1)
        .unwrap()
        .region_at(DVec2::new(2.5, 0.5))
        .map(sawhorse_kernel::ids::to_public)
        .unwrap();
    one_step(&mut d, &op(1, &[], &[], &[b], shift(-1.0, 0.0, 0.0)));
    let m = d.model();
    let s = m.sketch(1).unwrap();
    assert_eq!(s.edge_count(), 7, "the shared edge is one edge, not two");
    assert_eq!(s.vertex_count(), 6);
    assert!(close(&areas(m, 1), &[1.0, 1.0]));
}

#[test]
fn moving_across_another_shape_splits_both_at_the_crossings() {
    let mut d = doc(&[rect(0.0, 0.0, 2.0, 2.0), rect(5.0, 0.0, 2.0, 2.0)]);
    let b = d
        .model()
        .sketch(1)
        .unwrap()
        .region_at(DVec2::new(6.0, 1.0))
        .map(sawhorse_kernel::ids::to_public)
        .unwrap();
    one_step(&mut d, &op(1, &[], &[], &[b], shift(-4.0, 1.0, 0.0)));
    let m = d.model();
    // Exactly what drawing the second square there gives: three regions 3, 1, 3.
    assert!(close(&areas(m, 1), &[1.0, 3.0, 3.0]));
    assert_eq!(m.sketch(1).unwrap().vertex_count(), 10);
}

#[test]
fn a_corner_moved_onto_its_neighbour_merges_and_the_edge_goes() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    // (1, 0) onto (0, 0): the bottom edge shrinks to nothing; a triangle is left.
    one_step(
        &mut d,
        &op(1, &[[1.0, 0.0, 0.0]], &[], &[], shift(-1.0, 0.0, 0.0)),
    );
    let m = d.model();
    let s = m.sketch(1).unwrap();
    assert_eq!((s.vertex_count(), s.edge_count()), (3, 3));
    assert!(close(&areas(m, 1), &[0.5]));
    s.check().unwrap();
}

#[test]
fn a_move_that_would_make_a_bow_tie_splits_it_into_two_regions() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    // (1, 1) to (-0.5, 1): the edge from (1, 0) now crosses the left edge.
    one_step(
        &mut d,
        &op(1, &[[1.0, 1.0, 0.0]], &[], &[], shift(-1.5, 0.0, 0.0)),
    );
    let m = d.model();
    let s = m.sketch(1).unwrap();
    s.check().unwrap();
    assert_eq!(s.region_count(), 2, "two triangles, never a bow-tie");
    for (_, r) in s.regions() {
        let (o, _) = s.region_rings(r);
        assert_eq!(o.len(), 3);
    }
    // Triangle (0,0) (1,0) (0,2/3) and triangle (0,2/3) (-0.5,1) (0,1).
    assert!(close(&areas(m, 1), &[1.0 / 12.0, 1.0 / 3.0]));
}

#[test]
fn turning_a_region_in_its_plane() {
    let mut d = doc(&[rect(-0.5, -0.5, 1.0, 1.0)]);
    one_step(
        &mut d,
        &op(1, &[], &[], &[1], turn([0.0; 3], DVec3::Z, 45.0)),
    );
    let m = d.model();
    let h = 0.5f64.sqrt();
    assert_eq!(
        verts(m, 1),
        vec![q(-h, 0.0), q(0.0, -h), q(0.0, h), q(h, 0.0)]
    );
    assert_eq!(region_ids(m, 1), vec![1]);
    assert!(close(&areas(m, 1), &[1.0]));
    // 180 degrees about a corner: a half turn in the plane.
    one_step(
        &mut d,
        &op(1, &[], &[], &[1], turn([h, 0.0, 0.0], DVec3::Z, 180.0)),
    );
    assert!(verts(d.model(), 1).contains(&q(2.0 * h + h, 0.0)));
    // 0 and 360 degrees move nothing: refused, nothing changed.
    for deg in [0.0, 360.0, -360.0] {
        let e = refused(
            &mut d,
            &op(1, &[], &[], &[1], turn([0.0; 3], DVec3::Z, deg)),
        );
        assert_eq!(e, KernelError::Transform(TransformError::NothingToMove));
    }
}

#[test]
fn a_tiny_turn_still_moves_and_stays_valid() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0), rect(1.0, 0.0, 1.0, 1.0)]);
    let ids = region_ids(d.model(), 1);
    one_step(
        &mut d,
        &op(1, &[], &[], &[ids[0]], turn([0.0; 3], DVec3::Z, 1e-4)),
    );
    let s = d.model().sketch(1).unwrap();
    s.check().unwrap();
    assert_eq!(s.region_count(), 2);
}

#[test]
fn a_whole_shape_turned_upright_goes_to_the_sketch_on_its_new_plane() {
    let mut d = doc(&[rect(0.0, 0.0, 2.0, 1.0)]);
    // A quarter turn about the red axis: the rectangle stands up in the x-z plane.
    let out = one_step(
        &mut d,
        &op(1, &[], &[], &[1], turn([0.0; 3], DVec3::X, 90.0)),
    );
    let m = d.model();
    assert_eq!(m.sketch_ids().len(), 2, "a new sketch on the upright plane");
    let up = m.sketch_ids()[1];
    assert_eq!(
        out,
        OpOutcome::SketchMoved {
            sketch: up,
            regions: vec![[up, region_ids(m, up)[0]]]
        }
    );
    assert_eq!(
        m.sketch(1).unwrap().edge_count(),
        0,
        "nothing left on the ground"
    );
    assert!(close(&areas(m, up), &[2.0]));
    let s = m.sketch(up).unwrap();
    for (_, p) in s.vertices() {
        let w = s.plane().to_world(p);
        assert!(w.y.abs() < 1e-12, "in the x-z plane: {w}");
    }
}

#[test]
fn part_of_a_shape_turned_off_its_plane_is_projected() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    // The right edge turned 60 degrees about the red axis through the origin: its ends
    // would rise; projected, they move to y = cos 60 times what they were.
    one_step(
        &mut d,
        &op(
            1,
            &[],
            &[[[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]]],
            &[],
            turn([0.0; 3], DVec3::X, 60.0),
        ),
    );
    let m = d.model();
    assert_eq!(m.sketch_ids(), vec![1]);
    assert!(verts(m, 1).contains(&q(1.0, 0.5)));
}

#[test]
fn lifting_a_whole_shape_and_putting_it_back() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    one_step(&mut d, &op(1, &[], &[], &[1], shift(0.0, 0.0, 0.5)));
    let up = *d.model().sketch_ids().last().unwrap();
    assert_ne!(up, 1);
    let s = d.model().sketch(up).unwrap();
    assert!((s.plane().origin.z - 0.5).abs() < 1e-12);
    let r = region_ids(d.model(), up);
    // Back down: into the ground sketch that is already there.
    let out = one_step(&mut d, &op(up, &[], &[], &r, shift(0.0, 0.0, -0.5)));
    let OpOutcome::SketchMoved { sketch, regions } = out else {
        panic!()
    };
    assert_eq!((sketch, regions[0][0]), (1, 1), "back in the ground sketch");
    assert!(close(&areas(d.model(), 1), &[1.0]));
    assert_eq!(d.model().sketch(up).unwrap().edge_count(), 0);
}

#[test]
fn a_shape_lifted_off_a_neighbour_leaves_the_neighbour_whole() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0), rect(1.0, 0.0, 1.0, 1.0)]);
    let m = d.model();
    let b = m
        .sketch(1)
        .unwrap()
        .region_at(DVec2::new(1.5, 0.5))
        .map(sawhorse_kernel::ids::to_public)
        .unwrap();
    let a = region_ids(m, 1).into_iter().find(|&r| r != b).unwrap();
    one_step(&mut d, &op(1, &[], &[], &[b], shift(0.0, 0.0, 0.25)));
    let m = d.model();
    let g = m.sketch(1).unwrap();
    assert_eq!(region_ids(m, 1), vec![a], "the neighbour keeps its id");
    assert_eq!(g.edge_count(), 4, "and its shared edge");
    let up = *m.sketch_ids().last().unwrap();
    assert!(close(&areas(m, up), &[1.0]));
}

#[test]
fn an_island_in_a_lifted_hole_goes_along_and_a_dangling_stroke_stays() {
    let mut d = doc(&[
        rect(0.0, 0.0, 4.0, 4.0),
        rect(1.0, 1.0, 2.0, 2.0),
        Op::InsertSegment {
            sketch: 1,
            a: [4.0, 4.0],
            b: [5.0, 5.0],
        },
    ]);
    let m = d.model();
    let big = m
        .sketch(1)
        .unwrap()
        .region_at(DVec2::new(0.5, 0.5))
        .map(sawhorse_kernel::ids::to_public)
        .unwrap();
    one_step(&mut d, &op(1, &[], &[], &[big], shift(0.0, 0.0, 1.0)));
    let m = d.model();
    let g = m.sketch(1).unwrap();
    assert_eq!(g.region_count(), 0);
    assert_eq!(g.edge_count(), 1, "the stroke stays where it was drawn");
    let up = *m.sketch_ids().last().unwrap();
    assert!(
        close(&areas(m, up), &[4.0, 12.0]),
        "the frame and its island"
    );
}

#[test]
fn copies_are_independent_and_weld_where_they_touch() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    let out = one_step(&mut d, &copy_op(1, &[1], &[], shift(3.0, 0.0, 0.0), 1));
    let OpOutcome::SketchCopied { regions } = out else {
        panic!()
    };
    assert_eq!(regions.len(), 1);
    assert_ne!(regions[0][1], 1, "a new region");
    let m = d.model();
    assert!(verts(m, 1).contains(&q(0.0, 0.0)), "the original stays");
    assert!(close(&areas(m, 1), &[1.0, 1.0]));
    assert_eq!(m.sketch(1).unwrap().edge_count(), 8);
    // A copy right beside the original shares its edge (one edge, not two).
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    one_step(&mut d, &copy_op(1, &[1], &[], shift(1.0, 0.0, 0.0), 1));
    assert_eq!(d.model().sketch(1).unwrap().edge_count(), 7);
    // An array: x3.
    let mut d = doc(&[circle([0.0, 0.0], 0.1, 24)]);
    let r = region_ids(d.model(), 1);
    let out = one_step(&mut d, &copy_op(1, &r, &[], shift(0.5, 0.0, 0.0), 3));
    let OpOutcome::SketchCopied { regions } = out else {
        panic!()
    };
    assert_eq!(regions.len(), 3);
    assert_eq!(d.model().sketch(1).unwrap().region_count(), 4);
    // An edge copy is a loose line; a point alone copies nothing.
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    one_step(
        &mut d,
        &copy_op(
            1,
            &[],
            &[[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]],
            shift(0.0, -1.0, 0.0),
            1,
        ),
    );
    assert_eq!(d.model().sketch(1).unwrap().edge_count(), 5);
    let lone = Op::TransformSketch {
        sketch: 1,
        points: vec![[0.0, 0.0, 0.0]],
        edges: vec![],
        regions: vec![],
        xform: shift(1.0, 1.0, 0.0),
        copy: true,
        count: 1,
    };
    let e = refused(&mut d, &lone);
    assert_eq!(e, KernelError::Transform(TransformError::NothingToMove));
    // A copy onto itself is nothing.
    let e = refused(&mut d, &copy_op(1, &[1], &[], shift(0.0, 0.0, 0.0), 1));
    assert_eq!(e, KernelError::Transform(TransformError::NothingToMove));
}

#[test]
fn copies_lifted_off_the_plane_go_to_the_sketch_there() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    one_step(&mut d, &copy_op(1, &[1], &[], shift(0.0, 0.0, 0.25), 2));
    let m = d.model();
    assert_eq!(m.sketch_ids().len(), 3, "the ground and two copies' planes");
    assert!(close(&areas(m, 1), &[1.0]), "the original stays");
    for &s in &m.sketch_ids()[1..] {
        assert!(close(&areas(m, s), &[1.0]));
    }
    // A turned array whose copies land on several planes.
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    one_step(
        &mut d,
        &copy_op(1, &[1], &[], turn([0.0; 3], DVec3::X, 45.0), 3),
    );
    assert_eq!(d.model().sketch_ids().len(), 4);
}

#[test]
fn a_moved_region_pushes_into_a_solid_of_its_area_times_the_distance() {
    let (w, h, depth) = (0.6096, 0.4064, 0.01905);
    let mut d = doc(&[rect(0.0, 0.0, w, h)]);
    one_step(&mut d, &op(1, &[], &[], &[1], shift(0.3, -0.2, 0.0)));
    one_step(
        &mut d,
        &op(1, &[], &[], &[1], turn([0.3, -0.2, 0.0], DVec3::Z, 30.0)),
    );
    let out = d
        .apply(&Op::Extrude {
            sketch: 1,
            region: 1,
            distance: depth,
        })
        .unwrap();
    let OpOutcome::ObjectCreated { object } = out else {
        panic!()
    };
    let v = d.model().object(object).unwrap().volume();
    assert!(
        (v - w * h * depth).abs() < 1e-12,
        "{v} vs {}",
        w * h * depth
    );
    // A deformed region pushes to its own area too.
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    one_step(
        &mut d,
        &op(1, &[[1.0, 1.0, 0.0]], &[], &[], shift(0.5, 0.25, 0.0)),
    );
    d.apply(&Op::Extrude {
        sketch: 1,
        region: 1,
        distance: 0.5,
    })
    .unwrap();
    let v = d.model().objects()[0].1.volume();
    assert!((v - 1.375 * 0.5).abs() < 1e-12);
    // The cut list sees the solid like any other.
    sawhorse_kernel::testing::check_cut_list(d.model()).unwrap();
}

#[test]
fn a_lifted_region_pushes_too() {
    let mut d = doc(&[rect(0.0, 0.0, 0.5, 0.25)]);
    one_step(
        &mut d,
        &op(1, &[], &[], &[1], turn([0.0; 3], DVec3::Y, -90.0)),
    );
    let up = *d.model().sketch_ids().last().unwrap();
    let r = region_ids(d.model(), up)[0];
    d.apply(&Op::Extrude {
        sketch: up,
        region: r,
        distance: 0.1,
    })
    .unwrap();
    let v = d.model().objects()[0].1.volume();
    assert!((v - 0.5 * 0.25 * 0.1).abs() < 1e-12);
}

#[test]
fn moved_sketches_save_and_open_unchanged() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0), circle([3.0, 0.0], 0.25, 24)]);
    d.apply(&op(1, &[[1.0, 1.0, 0.0]], &[], &[], shift(0.25, 0.5, 0.0)))
        .unwrap();
    d.apply(&op(1, &[], &[], &[1], shift(0.0, 0.0, 0.75)))
        .unwrap();
    let json = serde_json::to_string(&d.model().snapshot()).unwrap();
    let back = Model::from_snapshot(&serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(back.hash(), d.model().hash());
    // The op itself round-trips through the log's JSON (defaults left out).
    let o = copy_op(1, &[1], &[], shift(1.0, 0.0, 0.0), 3);
    let text = serde_json::to_string(&o).unwrap();
    assert!(text.contains("\"op\":\"transform_sketch\""));
    assert!(!text.contains("points"), "{text}");
    assert_eq!(serde_json::from_str::<Op>(&text).unwrap(), o);
    let bare: Op = serde_json::from_str(
        r#"{"op":"transform_sketch","sketch":1,"regions":[1],"xform":{"linear":[[1,0,0],[0,1,0],[0,0,1]],"translation":[1,0,0]}}"#,
    )
    .unwrap();
    assert!(matches!(
        bare,
        Op::TransformSketch {
            copy: false,
            count: 1,
            ..
        }
    ));
    // The log replays to the same document.
    let again = Document::replay(d.log()).unwrap();
    assert_eq!(again.hash(), d.hash());
}

#[test]
fn refusals_are_typed_and_change_nothing() {
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    let e = refused(
        &mut d,
        &op(1, &[[0.5, 0.5, 0.0]], &[], &[], shift(1.0, 0.0, 0.0)),
    );
    assert_eq!(e, KernelError::Transform(TransformError::NoSuchSketchPoint));
    let e = refused(
        &mut d,
        &op(1, &[[0.0, 0.0, 0.1]], &[], &[], shift(1.0, 0.0, 0.0)),
    );
    assert_eq!(e, KernelError::Transform(TransformError::NoSuchSketchPoint));
    let e = refused(
        &mut d,
        &op(
            1,
            &[],
            &[[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]]],
            &[],
            shift(1.0, 0.0, 0.0),
        ),
    );
    assert_eq!(e, KernelError::Transform(TransformError::NoSuchSketchEdge));
    let e = refused(&mut d, &op(1, &[], &[], &[9], shift(1.0, 0.0, 0.0)));
    assert_eq!(e, KernelError::Sketch(SketchError::NoSuchRegion(9)));
    let e = refused(&mut d, &op(1, &[], &[], &[], shift(1.0, 0.0, 0.0)));
    assert_eq!(e, KernelError::Transform(TransformError::NothingSelected));
    let e = refused(&mut d, &op(7, &[], &[], &[1], shift(1.0, 0.0, 0.0)));
    assert_eq!(e, KernelError::Sketch(SketchError::NoSuchSketch(7)));
    let e = refused(&mut d, &op(1, &[], &[], &[1], shift(f64::NAN, 0.0, 0.0)));
    assert_eq!(e, KernelError::Transform(TransformError::BadTransform));
    // The far corner would land past 10 km (in the plane, and lifted off it).
    let e = refused(&mut d, &op(1, &[], &[], &[1], shift(9999.5, 0.0, 0.0)));
    assert_eq!(e, KernelError::Sketch(SketchError::OutOfBounds));
    let e = refused(&mut d, &op(1, &[], &[], &[1], shift(9999.5, 0.0, 1.0)));
    assert_eq!(e, KernelError::Sketch(SketchError::OutOfBounds));
    let mut moved = op(1, &[], &[], &[1], shift(1.0, 0.0, 0.0));
    if let Op::TransformSketch { count, .. } = &mut moved {
        *count = 2;
    }
    let e = refused(&mut d, &moved);
    assert_eq!(e, KernelError::Transform(TransformError::BadCount));
    let e = refused(&mut d, &copy_op(1, &[1], &[], shift(1.0, 0.0, 0.0), 0));
    assert_eq!(e, KernelError::Transform(TransformError::BadCount));
    // A region already pushed into a solid is not sketch geometry any more.
    d.apply(&rect(2.0, 0.0, 1.0, 1.0)).unwrap();
    d.apply(&Op::Extrude {
        sketch: 1,
        region: 1,
        distance: 0.5,
    })
    .unwrap();
    let e = refused(&mut d, &op(1, &[], &[], &[1], shift(1.0, 0.0, 0.0)));
    assert!(
        matches!(
            e,
            KernelError::Sketch(SketchError::NoSuchRegion(1) | SketchError::RegionConsumed(1))
        ),
        "{e}"
    );
}

#[test]
fn sketches_outside_the_open_group_do_not_move() {
    let mut d = doc(&[
        rect(0.0, 0.0, 1.0, 1.0),
        rect(3.0, 0.0, 1.0, 1.0),
        Op::Extrude {
            sketch: 1,
            region: 2,
            distance: 0.5,
        },
        Op::MakeGroup {
            items: vec![Item::Object(1)],
            name: None,
        },
    ]);
    let g = d.model().context_nodes()[0];
    d.apply(&Op::SetContext { node: Some(g) }).unwrap();
    let e = refused(&mut d, &op(1, &[], &[], &[1], shift(1.0, 0.0, 0.0)));
    assert_eq!(e, KernelError::Structure(StructureError::OutsideContext));
    d.apply(&Op::LeaveContext).unwrap();
    one_step(&mut d, &op(1, &[], &[], &[1], shift(0.0, 2.0, 0.0)));
}

#[test]
fn a_selection_spanning_shapes_moves_them_together() {
    // Two separate rectangles and a loose line, all at once: everything translates.
    let mut d = doc(&[
        rect(0.0, 0.0, 1.0, 1.0),
        rect(2.0, 0.0, 1.0, 1.0),
        Op::InsertSegment {
            sketch: 1,
            a: [0.0, 3.0],
            b: [3.0, 3.0],
        },
    ]);
    let rs = region_ids(d.model(), 1);
    one_step(
        &mut d,
        &op(
            1,
            &[],
            &[[[0.0, 3.0, 0.0], [3.0, 3.0, 0.0]]],
            &rs,
            shift(10.0, 0.0, 0.0),
        ),
    );
    let v = verts(d.model(), 1);
    assert!(v.iter().all(|p| p.0 >= q(10.0, 0.0).0), "{v:?}");
    // The loose line alone lifts off its plane whole.
    one_step(
        &mut d,
        &op(
            1,
            &[],
            &[[[10.0, 3.0, 0.0], [13.0, 3.0, 0.0]]],
            &[],
            shift(0.0, 0.0, 1.0),
        ),
    );
    assert_eq!(d.model().sketch(1).unwrap().edge_count(), 8);
}

#[test]
fn a_mirror_and_a_scale_of_sketch_geometry() {
    // Scale x2 about a corner: the rectangle's area quadruples.
    let mut d = doc(&[rect(0.0, 0.0, 1.0, 1.0)]);
    one_step(
        &mut d,
        &op(
            1,
            &[],
            &[],
            &[1],
            Xform::scaling(DVec3::ZERO, DVec3::new(2.0, 2.0, 1.0)),
        ),
    );
    assert!(close(&areas(d.model(), 1), &[4.0]));
    // A mirror across x = 3: still a valid counter-clockwise region.
    one_step(
        &mut d,
        &op(
            1,
            &[],
            &[],
            &[1],
            Xform::scaling(DVec3::new(3.0, 0.0, 0.0), DVec3::new(-1.0, 1.0, 1.0)),
        ),
    );
    assert!(verts(d.model(), 1).contains(&q(6.0, 0.0)));
    assert!(close(&areas(d.model(), 1), &[4.0]));
}

/// The property tests' finds (lane 22, `tests/sketch_move_finds/`): op sequences after
/// which a region the sketch held could not be pushed into a valid solid. The sketch kept
/// an edge, or a corner's clearance from another edge, a rounding error over the merge
/// distance; the solid tier measured it again in world coordinates and called it a
/// zero-length edge or a vertex on a face. The sketch now merges at `SKETCH_MERGE`
/// (POINT_MERGE plus the 1 nm noise floor), so every region pushes, to its area times the
/// distance. Each sequence is replayed as recorded (an op the kernel now refuses is skipped,
/// as the fuzzer's replay does).
#[test]
fn the_property_finds_push_into_valid_solids() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sketch_move_finds");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 3);
    for f in files {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let ops: Vec<Op> = serde_json::from_value(v["ops"].clone()).unwrap();
        let mut d = Document::new();
        for op in &ops {
            if let Err(e) = d.apply(op) {
                assert!(!e.is_bug(), "{}: {e}", f.display());
            }
        }
        sawhorse_kernel::testing::check_model(d.model()).unwrap();
        let m = d.model();
        let mut pushed = 0;
        for id in m.sketch_ids() {
            let s = m.sketch(id).unwrap();
            for (r, reg) in s.regions() {
                if reg.pinched {
                    continue;
                }
                let area = s.region_area(reg);
                let mut probe = d.clone();
                let out = probe
                    .apply(&Op::Extrude {
                        sketch: id,
                        region: sawhorse_kernel::ids::to_public(r),
                        distance: 0.25,
                    })
                    .unwrap_or_else(|e| panic!("{}: region {r:?}: {e}", f.display()));
                let OpOutcome::ObjectCreated { object } = out else {
                    panic!("{out:?}")
                };
                let v = probe.model().object(object).unwrap().volume();
                assert!((v - area * 0.25).abs() <= 1e-9 * area * 0.25 + 1e-12);
                pushed += 1;
            }
        }
        assert!(pushed > 0, "{}", f.display());
    }
}

#[test]
fn an_edge_a_rounding_error_over_the_merge_distance_is_too_short() {
    // On the tilted plane of the second find: a stroke whose corners are 1e-5 + 2e-17 m
    // apart was taken (over POINT_MERGE in the sketch's frame) and its region then made a
    // solid with a zero-length edge. It is too short for the sketch now, refused, typed.
    let mut d = Document::new();
    d.apply(&Op::NewSketch {
        origin: [0.0, 0.0, 0.5],
        normal: [0.0, 1.0, 1.0],
    })
    .unwrap();
    let plane = *d.model().sketch(1).unwrap().plane();
    let l = sawhorse_kernel::tolerance::POINT_MERGE + 2e-17;
    let w = |x: f64, y: f64| plane.to_world(DVec2::new(x, y)).to_array();
    let square = |extra: bool| {
        let mut p = vec![w(0.0, 0.0), w(0.5, 0.0), w(0.5, 0.5)];
        if extra {
            p.push(w(0.5 - l, 0.5));
        }
        p.push(w(0.0, 0.5));
        Op::Draw {
            target: DrawTarget::Sketch { sketch: 1 },
            points: p,
            closed: true,
        }
    };
    let e = refused(&mut d, &square(true));
    assert_eq!(e, KernelError::Sketch(SketchError::Degenerate));
    d.apply(&square(false)).unwrap();
    // Moving a corner to a rounding error over the merge distance from its neighbour merges
    // them too: the edge between them goes, and the triangle left pushes.
    let corner = w(0.5, 0.5);
    let to = DVec3::from_array(w(0.5 - l, 0.0)) - DVec3::from_array(corner);
    one_step(&mut d, &op(1, &[corner], &[], &[], Xform::translation(to)));
    let s = d.model().sketch(1).unwrap();
    assert_eq!(s.vertex_count(), 3);
    for (_, [a, b]) in s.edges() {
        let (pa, pb) = (s.vertex(a).unwrap(), s.vertex(b).unwrap());
        assert!(
            plane.to_world(pa).distance(plane.to_world(pb))
                > sawhorse_kernel::tolerance::POINT_MERGE
        );
    }
    d.apply(&Op::Extrude {
        sketch: 1,
        region: 1,
        distance: 0.25,
    })
    .unwrap();
}
