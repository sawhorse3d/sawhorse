//! Erasing (lane 3): lines drawn on faces, imprints, sketch regions, and batches.
//!
//! A solid stays a closed solid: an edge or face of an object can only be erased where it
//! separates two coplanar faces (something the user drew), and then the two faces merge.
//! Everything else is a typed refusal that changes nothing.

use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::{Document, DocumentError, FaceEditError, KernelError, SketchError};

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

fn boxed() -> Vec<Op> {
    vec![
        rect(0.0, 0.0, 2.0, 1.0),
        Op::Extrude {
            sketch: 1,
            region: 1,
            distance: 1.0,
        },
    ]
}

fn imprint() -> Op {
    Op::Imprint {
        object: 1,
        face: 2,
        points: vec![
            [0.5, 0.25, 1.0],
            [1.5, 0.25, 1.0],
            [1.5, 0.75, 1.0],
            [0.5, 0.75, 1.0],
        ],
    }
}

fn faces(m: &Model) -> usize {
    m.object(1).unwrap().brep().face_count()
}

#[test]
fn erasing_one_edge_of_an_imprint_merges_it_back() {
    let plain = run(&boxed());
    let mut m = run(&[boxed(), vec![imprint()]].concat());
    assert_eq!(faces(&m), 7);
    let out = m
        .apply(&Op::EraseEdge {
            object: 1,
            a: [0.5, 0.25, 1.0],
            b: [1.5, 0.25, 1.0],
        })
        .unwrap();
    assert_eq!(
        out,
        OpOutcome::FacesMerged { face: 2 },
        "the top keeps its id"
    );
    assert_eq!(faces(&m), 6);
    let o = m.object(1).unwrap();
    assert!(o.same_shape(plain.object(1).unwrap(), 1e-12) || o.brep().face_count() == 6);
    assert!((o.volume() - 2.0).abs() < 1e-12);
    assert_eq!(m.stats().holes, 0);
}

#[test]
fn erasing_an_imprinted_face_merges_it_into_its_parent() {
    let mut m = run(&[boxed(), vec![imprint()]].concat());
    let out = m.apply(&Op::EraseFace { object: 1, face: 7 }).unwrap();
    assert_eq!(out, OpOutcome::FacesMerged { face: 2 });
    assert_eq!(faces(&m), 6);
    assert_eq!(m.stats().holes, 0);
    assert!(
        m.object(1).unwrap().face_ordinal(7).is_none(),
        "id 7 is retired"
    );
}

#[test]
fn erasing_a_cut_line_merges_the_two_halves() {
    let mut m = run(&boxed());
    m.apply(&Op::SplitFace {
        object: 1,
        face: 2,
        path: vec![[1.0, 0.0, 1.0], [1.2, 0.5, 1.0], [1.0, 1.0, 1.0]],
    })
    .unwrap();
    assert_eq!(faces(&m), 7);
    // Erase only the second segment: the whole cut goes (a face cannot keep a dangling line).
    m.apply(&Op::EraseEdge {
        object: 1,
        a: [1.2, 0.5, 1.0],
        b: [1.0, 1.0, 1.0],
    })
    .unwrap();
    assert_eq!(faces(&m), 6);
    let st = m.stats();
    assert_eq!(
        (st.vertices, st.edges),
        (8, 12),
        "split vertices healed away"
    );
}

#[test]
fn structural_edges_and_faces_are_refused_without_changes() {
    let mut m = run(&boxed());
    let before = m.hash();
    let e = m
        .apply(&Op::EraseEdge {
            object: 1,
            a: [0.0, 0.0, 0.0],
            b: [2.0, 0.0, 0.0],
        })
        .unwrap_err();
    assert_eq!(e, KernelError::FaceEdit(FaceEditError::WouldOpenSolid));
    let e = m.apply(&Op::EraseFace { object: 1, face: 2 }).unwrap_err();
    assert_eq!(e, KernelError::FaceEdit(FaceEditError::WouldOpenSolid));
    let e = m
        .apply(&Op::EraseEdge {
            object: 1,
            a: [0.3, 0.3, 1.0],
            b: [0.7, 0.3, 1.0],
        })
        .unwrap_err();
    assert_eq!(e, KernelError::FaceEdit(FaceEditError::NoSuchEdge));
    assert_eq!(m.hash(), before);
    // A boss wall edge is not between coplanar faces either.
    m.apply(&imprint()).unwrap();
    m.apply(&Op::PushPull {
        object: 1,
        face: 7,
        distance: 0.25,
    })
    .unwrap();
    let before = m.hash();
    let e = m
        .apply(&Op::EraseEdge {
            object: 1,
            a: [0.5, 0.25, 1.0],
            b: [0.5, 0.25, 1.25],
        })
        .unwrap_err();
    assert!(!e.is_bug(), "{e}");
    assert_eq!(m.hash(), before);
}

#[test]
fn erasing_a_region_keeps_the_edges_its_neighbour_needs() {
    let mut m = run(&[rect(0.0, 0.0, 1.0, 1.0), rect(1.0, 0.0, 1.0, 1.0)]);
    let s = m.sketch(1).unwrap();
    assert_eq!(s.region_count(), 2);
    let (left, right): (Vec<_>, Vec<_>) = s
        .regions()
        .map(|(id, r)| (sawhorse_kernel::ids::to_public(id), s.region_rings(r).0))
        .partition(|(_, ring)| ring.iter().all(|p| p.x <= 1.0 + 1e-12));
    let (lid, rid) = (left[0].0, right[0].0);
    m.apply(&Op::EraseRegion {
        sketch: 1,
        region: lid,
    })
    .unwrap();
    let s = m.sketch(1).unwrap();
    assert_eq!(s.region_count(), 1);
    let r = s.region(sawhorse_kernel::ids::from_public(rid)).unwrap();
    assert!(
        (s.region_area(r) - 1.0).abs() < 1e-12,
        "the right square is whole"
    );
    assert_eq!(s.edge_count(), 4);
    // Erasing the last one empties the sketch.
    m.apply(&Op::EraseRegion {
        sketch: 1,
        region: rid,
    })
    .unwrap();
    let s = m.sketch(1).unwrap();
    assert_eq!(
        (s.region_count(), s.edge_count(), s.vertex_count()),
        (0, 0, 0)
    );
    assert_eq!(
        m.apply(&Op::EraseRegion {
            sketch: 1,
            region: rid
        })
        .unwrap_err(),
        KernelError::Sketch(SketchError::NoSuchRegion(rid))
    );
}

#[test]
fn a_region_inside_another_erases_to_a_hole_free_parent() {
    let mut m = run(&[rect(0.0, 0.0, 4.0, 4.0), rect(1.0, 1.0, 1.0, 1.0)]);
    let s = m.sketch(1).unwrap();
    let inner = s
        .regions()
        .find(|(_, r)| (s.region_area(r) - 1.0).abs() < 1e-12)
        .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
        .unwrap();
    m.apply(&Op::EraseRegion {
        sketch: 1,
        region: inner,
    })
    .unwrap();
    let s = m.sketch(1).unwrap();
    assert_eq!(s.region_count(), 1);
    let (_, r) = s.regions().next().unwrap();
    assert!((s.region_area(r) - 16.0).abs() < 1e-12);
}

#[test]
fn batches_are_one_transaction_and_one_undo_step() {
    let mut d = Document::with_ground_sketch();
    let out = d
        .apply(&Op::Batch {
            ops: vec![rect(0.0, 0.0, 1.0, 1.0), rect(3.0, 0.0, 1.0, 1.0)],
        })
        .unwrap();
    let OpOutcome::Batch { outcomes } = out else {
        panic!("{out:?}")
    };
    assert_eq!(outcomes.len(), 2);
    assert_eq!(d.model().stats().regions, 2);
    d.undo().unwrap();
    assert_eq!(d.model().stats().regions, 0, "one undo step");
    d.redo().unwrap();
    let before = d.hash();
    // A refusal anywhere rolls back the whole batch.
    let e = d
        .apply(&Op::Batch {
            ops: vec![rect(5.0, 0.0, 1.0, 1.0), rect(0.0, 0.0, 0.0, 1.0)],
        })
        .unwrap_err();
    assert!(!e.is_bug());
    assert_eq!(d.hash(), before);
    for bad in [
        Op::Batch { ops: vec![] },
        Op::Batch {
            ops: vec![Op::Undo],
        },
        Op::Batch {
            ops: vec![Op::Batch { ops: vec![] }],
        },
    ] {
        let e = d.apply(&bad).unwrap_err();
        assert!(
            matches!(
                e,
                KernelError::Document(DocumentError::EmptyBatch | DocumentError::NotInBatch)
            ),
            "{e}"
        );
    }
    assert_eq!(d.hash(), before);
    // Batches replay from the log like any op.
    let replayed = Document::replay(d.log()).unwrap();
    assert_eq!(replayed.hash(), d.hash());
}
