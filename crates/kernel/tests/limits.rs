//! Lane 16: the kernel's resource limits (crate::limits), checked where the work they bound
//! starts. Each refusal is typed, quick, and leaves the document as it was.

use std::time::Instant;

use sawhorse_kernel::meshbuild::{ImportItem, build_item};
use sawhorse_kernel::refbatch::{NamedBatch, RefBatchError, apply as apply_batch};
use sawhorse_kernel::{Document, Op};
use serde_json::json;

fn op(v: serde_json::Value) -> Op {
    serde_json::from_value(v).unwrap()
}

fn boxed() -> Document {
    let mut d = Document::with_ground_sketch();
    d.apply(&op(
        json!({"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.1,"h":0.1}),
    ))
    .unwrap();
    d.apply(&op(
        json!({"op":"extrude","sketch":1,"region":1,"distance":0.1}),
    ))
    .unwrap();
    d
}

fn shift(x: f64) -> serde_json::Value {
    json!({"linear": [[1,0,0],[0,1,0],[0,0,1]], "translation": [x, 0, 0]})
}

#[test]
fn an_import_whose_components_double_thirty_times_is_refused_before_it_is_built() {
    let mut d = Document::with_ground_sketch();
    let h = d.hash();
    let tri = json!({"name": "T", "positions": [[0,0,0],[1,0,0],[0,1,0]], "faces": [[[0,1,2]]]});
    // Definition i holds two copies of definition i - 1: definition 29 is 2^29 triangles.
    let mut defs = vec![json!({"name": "D0", "items": [tri]})];
    for i in 1..30 {
        defs.push(json!({"name": format!("D{i}"), "children": [
            {"kind": "instance", "definition": i - 1, "xform": shift(0.0)},
            {"kind": "instance", "definition": i - 1, "xform": shift(1.0)}
        ]}));
    }
    let o = op(
        json!({"op": "import", "items": [], "definitions": defs, "nodes": [{"kind": "instance", "definition": 29, "xform": shift(0.0)}]}),
    );
    let t = Instant::now();
    let e = d.apply(&o).unwrap_err();
    assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
    assert!(e.to_string().contains("one import may bring in"), "{e}");
    assert_eq!(d.hash(), h, "nothing changed");
}

#[test]
fn an_array_that_would_exceed_the_model_is_refused_before_copies_are_made() {
    let mut d = boxed();
    d.apply(&op(
        json!({"op":"array","objects":[1],"xform":shift(0.2),"count":99}),
    ))
    .unwrap();
    let all: Vec<u64> = d.model().object_ids();
    assert_eq!(all.len(), 100);
    let h = d.hash();
    // 100 solids, 1,000 copies of each: 100,000 more, refused at once.
    let t = Instant::now();
    let e = d
        .apply(&Op::Array {
            objects: all,
            xform: serde_json::from_value(shift(0.0)).unwrap(),
            count: 1000,
            nodes: vec![],
        })
        .unwrap_err();
    assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
    assert!(e.to_string().contains("solids and meshes"), "{e}");
    assert_eq!(d.hash(), h);
}

#[test]
fn a_texture_whose_header_says_too_many_pixels_is_refused() {
    let mut d = boxed();
    // A PNG header (no pixel data needed: refused on the header) of 8192 x 8192.
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&8192u32.to_be_bytes());
    png.extend_from_slice(&8192u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    let o = Op::AddTexture {
        path: "assets/huge.png".into(),
        mime: "image/png".into(),
        data: png,
    };
    let e = d.apply(&o).unwrap_err();
    assert!(e.to_string().contains("4096 x 4096"), "{e}");
}

#[test]
fn a_batch_is_held_to_its_op_count_and_its_work_budget() {
    let mut d = Document::with_ground_sketch();
    let rename = |n: usize| -> Vec<serde_json::Value> {
        (0..n)
            .map(|_| json!({"op": "rename_object", "object": 1, "name": "x"}))
            .collect()
    };
    let e = apply_batch(
        &mut d,
        &NamedBatch {
            name: "Many".into(),
            ops: rename(sawhorse_kernel::refbatch::MAX_OPS + 1),
        },
    )
    .unwrap_err();
    assert_eq!(e, RefBatchError::TooMany);
    // A model of 10,001 things: 2,000 ops on it are more work than one batch may do.
    let items: Vec<serde_json::Value> = (0..10_000)
        .map(|i| json!({"name": format!("T{i}"), "positions": [[i as f64 * 0.01, 0, 0],[i as f64 * 0.01 + 0.005, 0, 0],[i as f64 * 0.01, 0.005, 0]], "faces": [[[0,1,2]]]}))
        .collect();
    d.apply(&op(json!({"op": "import", "items": items})))
        .unwrap();
    let h = d.hash();
    let t = Instant::now();
    let e = apply_batch(
        &mut d,
        &NamedBatch {
            name: "Busy".into(),
            ops: rename(2000),
        },
    )
    .unwrap_err();
    assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
    assert!(matches!(e, RefBatchError::TooMuchWork { .. }), "{e}");
    assert_eq!(d.hash(), h);
}

#[test]
fn undo_snapshots_share_what_an_edit_did_not_change() {
    let mut d = boxed();
    d.apply(&op(
        json!({"op":"array","objects":[1],"xform":shift(0.2),"count":20}),
    ))
    .unwrap();
    let before = d.model().clone();
    let ids = before.object_ids();
    // Push one solid's top: only that solid's B-rep is new.
    let (next, _) = before
        .applied(&op(
            json!({"op":"push_pull","object": ids[5],"face":2,"distance":0.05}),
        ))
        .unwrap();
    let shared = ids
        .iter()
        .filter(|&&i| {
            next.object(i)
                .unwrap()
                .brep()
                .shares_with(before.object(i).unwrap().brep())
        })
        .count();
    assert_eq!(
        shared,
        ids.len() - 1,
        "every other solid shares its B-rep with the snapshot"
    );
    // And the history is charged for what it holds that the model does not share.
    let mut d2 = boxed();
    for k in 0..30 {
        d2.apply(&op(
            json!({"op":"rename_object","object":1,"name": format!("N{k}")}),
        ))
        .unwrap();
    }
    assert!(
        d2.history_bytes() < 2 * 1024 * 1024,
        "{}",
        d2.history_bytes()
    );
    assert_eq!(
        d2.depth().0,
        32,
        "two steps to make the box, thirty renames"
    );
}

#[test]
fn twenty_thousand_disjoint_triangles_are_classified_in_linear_time() {
    let mut positions = Vec::new();
    let mut faces = Vec::new();
    for i in 0..20_000u32 {
        let x = f64::from(i % 200) * 0.01;
        let y = f64::from(i / 200) * 0.01;
        positions.extend([[x, y, 0.0], [x + 0.005, y, 0.0], [x, y + 0.005, 0.0]]);
        faces.push(vec![vec![3 * i, 3 * i + 1, 3 * i + 2]]);
    }
    let item = ImportItem {
        name: "Soup".into(),
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
    };
    let t = Instant::now();
    let built = build_item(&item).unwrap();
    assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
    assert!(built.solids.is_empty() && built.loose.is_some());
}
