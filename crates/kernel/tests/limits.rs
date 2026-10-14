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

/// A PNG that is only its header (the kernel reads no more): `w` x `h` pixels, made distinct
/// by `n` (a byte after the header).
fn png_header(w: u32, h: u32, n: u8) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&w.to_be_bytes());
    png.extend_from_slice(&h.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0, n]);
    png
}

/// The lane 16 verifier's F10: sixteen legal 4096 x 4096 images (a few kilobytes each as
/// PNGs of one colour) were a gigabyte decoded in the browser, and the 512-image limit
/// allowed tens of gigabytes. The pixels of all images together are held to 128 million.
#[test]
fn texture_images_are_held_to_their_pixels_in_all() {
    let mut d = boxed();
    let side = 4096u32;
    let per = u64::from(side) * u64::from(side);
    let fit = (sawhorse_kernel::limits::MAX_TEXTURE_PIXELS_TOTAL / per) as u8;
    assert_eq!(fit, 8);
    for n in 0..fit {
        d.apply(&Op::AddTexture {
            path: format!("assets/t{n}.png"),
            mime: "image/png".into(),
            data: png_header(side, side, n),
        })
        .unwrap();
    }
    let h = d.hash();
    let t = Instant::now();
    let e = d
        .apply(&Op::AddTexture {
            path: "assets/t-over.png".into(),
            mime: "image/png".into(),
            data: png_header(side, side, 99),
        })
        .unwrap_err();
    assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
    assert!(e.to_string().contains("million pixels"), "{e}");
    assert_eq!(d.hash(), h, "nothing changed");
    // A small image still fits beside them: the limit is the pixels, not the count.
    let e = d
        .apply(&Op::AddTexture {
            path: "assets/small.png".into(),
            mime: "image/png".into(),
            data: png_header(64, 64, 7),
        })
        .unwrap_err();
    assert!(
        e.to_string().contains("million pixels"),
        "exactly at the limit: {e}"
    );
    d.undo().unwrap();
    d.apply(&Op::AddTexture {
        path: "assets/small.png".into(),
        mime: "image/png".into(),
        data: png_header(64, 64, 7),
    })
    .unwrap();
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

// ---- lane 16b: guides, dimensions, tags, materials, scenes, section planes, sheets ----------

use sawhorse_kernel::limits::{self as l, LimitError};
use sawhorse_kernel::snapshot::{SnapDimension, SnapGuide};

/// A document holding `guides` guides and `dims` dimensions, restored from a snapshot (the
/// way a file brings them; one op each would take minutes).
fn annotated(guides: usize, dims: usize) -> Document {
    let mut s = boxed().model().snapshot();
    s.guides = (0..guides)
        .map(|i| SnapGuide {
            point: [(i % 1000) as f64 * 1e-3, (i / 1000) as f64 * 1e-3, 0.0],
            dir: Some([0.0, 1.0, 0.0]),
        })
        .collect();
    s.dimensions = (0..dims)
        .map(|i| SnapDimension {
            a: [i as f64 * 1e-4, 0.0, 0.0],
            b: [i as f64 * 1e-4, 1.0, 0.0],
            offset: [0.0, 0.0, 0.1],
        })
        .collect();
    Document::open(s).unwrap()
}

fn count_error(e: &sawhorse_kernel::KernelError) -> Option<(&'static str, usize)> {
    match e {
        sawhorse_kernel::KernelError::Limit(LimitError::Count { what, max, .. }) => {
            Some((what, *max))
        }
        _ => None,
    }
}

#[test]
fn guides_and_dimensions_stop_at_the_model_limit() {
    let mut d = annotated(l::MAX_GUIDES, l::MAX_DIMENSIONS);
    let h = d.hash();
    let t = Instant::now();
    let e = d
        .apply(&op(
            json!({"op":"add_guide","point":[0,0,0.5],"dir":[1,0,0]}),
        ))
        .unwrap_err();
    assert!(t.elapsed().as_millis() < 2000, "{:?}", t.elapsed());
    assert_eq!(count_error(&e), Some(("guides", 50_000)), "{e}");
    assert!(e.to_string().contains("50001 guides; at most 50000"), "{e}");
    let e = d
        .apply(&op(
            json!({"op":"add_dimension","a":[0,0,0],"b":[1,0,0],"offset":[0,0,0.1]}),
        ))
        .unwrap_err();
    assert_eq!(count_error(&e), Some(("dimensions", 50_000)), "{e}");
    assert_eq!(d.hash(), h, "nothing changed");
    // One fewer: the next one goes in.
    let mut d = annotated(l::MAX_GUIDES - 1, 0);
    d.apply(&op(
        json!({"op":"add_guide","point":[0,0,0.5],"dir":[1,0,0]}),
    ))
    .unwrap();
    assert_eq!(d.model().snapshot().guides.len(), l::MAX_GUIDES);
}

#[test]
fn a_snapshot_over_any_count_is_refused_before_it_is_built() {
    let base = boxed().model().snapshot();
    let over = |f: &dyn Fn(&mut sawhorse_kernel::snapshot::Snapshot)| {
        let mut s = base.clone();
        f(&mut s);
        let t = Instant::now();
        let e = Document::open(s).map(|_| ()).unwrap_err();
        assert!(t.elapsed().as_millis() < 2000, "{:?}", t.elapsed());
        count_error(&e).unwrap_or_else(|| panic!("not a count: {e}"))
    };
    let g = SnapGuide {
        point: [0.0; 3],
        dir: None,
    };
    assert_eq!(
        over(&|s| s.guides = vec![g.clone(); l::MAX_GUIDES + 1]).0,
        "guides"
    );
    let dm = SnapDimension {
        a: [0.0; 3],
        b: [1.0, 0.0, 0.0],
        offset: [0.0; 3],
    };
    assert_eq!(
        over(&|s| s.dimensions = vec![dm.clone(); l::MAX_DIMENSIONS + 1]).0,
        "dimensions"
    );
    assert_eq!(
        over(&|s| s.tags = (0..=l::MAX_TAGS).map(|i| (format!("t{i}"), true)).collect()).0,
        "tags"
    );
    assert_eq!(
        over(&|s| s.tag_colors = (0..=l::MAX_TAGS)
            .map(|i| (format!("t{i}"), "#aabbcc".to_string()))
            .collect())
        .0,
        "tag colours"
    );
    let mat = |i: usize| {
        serde_json::from_value(json!({"name": format!("m{i}"), "color": [1, 2, 3, 255]})).unwrap()
    };
    assert_eq!(
        over(&|s| s.materials = (0..=l::MAX_MATERIALS).map(mat).collect()).0,
        "materials"
    );
    let scene = |i: usize| {
        serde_json::from_value(
            json!({"name": format!("s{i}"), "eye": [1,1,1], "target": [0,0,0], "up": [0,0,1]}),
        )
        .unwrap()
    };
    assert_eq!(
        over(&|s| s.scenes = (0..=l::MAX_SCENES).map(scene).collect()).0,
        "scenes"
    );
    let hiding: sawhorse_kernel::model::SceneDef = serde_json::from_value(
        json!({"name": "s", "eye": [1,1,1], "target": [0,0,0], "up": [0,0,1], "hidden_tags": (0..=l::MAX_TAGS).map(|i| format!("t{i}")).collect::<Vec<_>>()}),
    )
    .unwrap();
    assert_eq!(
        over(&|s| s.scenes = vec![hiding.clone()]).0,
        "tags hidden by one scene"
    );
}

#[test]
fn an_import_brings_no_more_materials_scenes_or_hidden_tags_than_the_model_holds() {
    let mut d = boxed();
    let h = d.hash();
    let mats: Vec<_> = (0..=l::MAX_MATERIALS)
        .map(|i| json!({"name": format!("m{i}"), "color": [1, 2, 3, 255]}))
        .collect();
    let t = Instant::now();
    let e = d
        .apply(&op(json!({"op": "import", "materials": mats})))
        .unwrap_err();
    assert!(t.elapsed().as_millis() < 1000, "{:?}", t.elapsed());
    assert_eq!(
        count_error(&e),
        Some(("materials", l::MAX_MATERIALS)),
        "{e}"
    );
    let scenes: Vec<_> = (0..=l::MAX_SCENES)
        .map(|i| json!({"name": format!("s{i}"), "eye": [1,1,1], "target": [0,0,0], "up": [0,0,1]}))
        .collect();
    let e = d
        .apply(&op(json!({"op": "import", "scenes": scenes})))
        .unwrap_err();
    assert_eq!(count_error(&e), Some(("scenes", l::MAX_SCENES)), "{e}");
    let hidden: Vec<_> = (0..=l::MAX_TAGS).map(|i| format!("t{i}")).collect();
    let e = d
        .apply(&op(json!({"op": "import", "hidden_tags": hidden})))
        .unwrap_err();
    assert_eq!(count_error(&e).map(|c| c.1), Some(l::MAX_TAGS), "{e}");
    let e = d
        .apply(&op(json!({"op": "add_scene", "scene": {"name": "S", "eye": [1,1,1], "target": [0,0,0], "up": [0,0,1], "hidden_tags": hidden}})))
        .unwrap_err();
    assert_eq!(
        count_error(&e),
        Some(("tags hidden by one scene", l::MAX_TAGS)),
        "{e}"
    );
    assert_eq!(d.hash(), h, "nothing changed");
    // Materials the model already holds are not counted twice.
    let some: Vec<_> = (0..10)
        .map(|i| json!({"name": format!("m{i}"), "color": [1, 2, 3, 255]}))
        .collect();
    d.apply(&op(json!({"op": "import", "materials": some.clone()})))
        .unwrap();
    d.apply(&op(json!({"op": "import", "materials": some})))
        .unwrap();
}

#[test]
fn the_limits_are_the_documented_numbers() {
    // docs/LIMITS.md states these; src/kernel/limits.ts mirrors them (its test reads this file).
    assert_eq!(
        (
            l::MAX_GUIDES,
            l::MAX_DIMENSIONS,
            l::MAX_TAGS,
            l::MAX_MATERIALS,
            l::MAX_SCENES,
            l::MAX_SECTIONS,
            l::MAX_SHEETS,
            l::MAX_SHEET_ITEMS
        ),
        (50_000, 50_000, 10_000, 4_096, 1_000, 1_000, 200, 2_000)
    );
    assert_eq!(sawhorse_kernel::looks::MAX_TEXTURES, 512);
    const { assert!(sawhorse_kernel::looks::MAX_TEXTURES <= l::MAX_MATERIALS) };
}
