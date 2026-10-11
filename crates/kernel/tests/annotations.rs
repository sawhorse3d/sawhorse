//! Guides and dimensions (lane 3): undoable annotations in the document. Guides are
//! inferable construction lines and points; dimensions record two points and where their
//! line is drawn. Neither is ever part of a solid.

use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::{Document, DocumentError, KernelError};

#[test]
fn guides_and_dimensions_are_undoable_and_hashed() {
    let mut d = Document::with_ground_sketch();
    let empty = d.hash();
    let out = d
        .apply(&Op::AddGuide {
            point: [0.0, 0.0, 0.5461],
            dir: Some([0.0, 2.0, 0.0]),
        })
        .unwrap();
    assert_eq!(out, OpOutcome::GuideAdded { guide: 1 });
    let g = d.model().guides()[0].1;
    assert_eq!(g.dir.to_array(), [0.0, 1.0, 0.0], "directions are unit");
    assert_ne!(d.hash(), empty);
    d.apply(&Op::AddGuide {
        point: [1.0, 1.0, 0.0],
        dir: None,
    })
    .unwrap();
    let out = d
        .apply(&Op::AddDimension {
            a: [0.0, 0.0, 0.0],
            b: [0.9144, 0.0, 0.0],
            offset: [0.0, -0.1, 0.0],
        })
        .unwrap();
    assert_eq!(out, OpOutcome::DimensionAdded { dimension: 1 });
    assert!((d.model().dimensions()[0].1.length() - 0.9144).abs() < 1e-15);
    // Undo takes them off one by one; redo puts them back; the log replays.
    d.undo().unwrap();
    assert!(d.model().dimensions().is_empty());
    d.redo().unwrap();
    let full = d.hash();
    assert_eq!(Document::replay(d.log()).unwrap().hash(), full);
    d.apply(&Op::EraseGuide { guide: 2 }).unwrap();
    assert_eq!(d.model().guides().len(), 1);
    d.apply(&Op::DeleteGuides).unwrap();
    d.apply(&Op::EraseDimension { dimension: 1 }).unwrap();
    assert_eq!(d.hash(), empty, "back to an empty document");
}

#[test]
fn bad_annotations_are_typed_refusals() {
    let mut m = Model::with_ground_sketch();
    let before = m.hash();
    for op in [
        Op::AddGuide {
            point: [f64::NAN, 0.0, 0.0],
            dir: None,
        },
        Op::AddGuide {
            point: [0.0; 3],
            dir: Some([0.0; 3]),
        },
        Op::AddDimension {
            a: [1.0, 1.0, 1.0],
            b: [1.0, 1.0, 1.0],
            offset: [0.0; 3],
        },
        Op::EraseGuide { guide: 7 },
        Op::EraseDimension { dimension: 7 },
        Op::DeleteGuides,
    ] {
        let e = m.apply(&op).unwrap_err();
        assert!(
            matches!(
                e,
                KernelError::Document(
                    DocumentError::BadAnnotation
                        | DocumentError::NoSuchAnnotation(_)
                        | DocumentError::NoGuides
                )
            ),
            "{op:?}: {e}"
        );
    }
    assert_eq!(m.hash(), before);
}

#[test]
fn documents_without_annotations_keep_their_hashes() {
    // The golden files pin this too; a direct check that the annotation block is optional.
    let mut a = Model::with_ground_sketch();
    a.apply(&Op::AddRectangle {
        sketch: 1,
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    })
    .unwrap();
    let h = a.hash();
    a.apply(&Op::AddGuide {
        point: [0.0; 3],
        dir: None,
    })
    .unwrap();
    a.apply(&Op::DeleteGuides).unwrap();
    assert_eq!(a.hash(), h);
}
