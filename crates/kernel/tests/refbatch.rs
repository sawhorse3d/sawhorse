//! Plugin batches against the goldens (lane 11, ADR-0009): a batch that builds what a
//! golden builds lands on the golden's hashes, so a plugin's edit is exactly the edit the
//! built-in tools make.

use std::path::PathBuf;

use sawhorse_kernel::Document;
use sawhorse_kernel::cutlist::CutOptions;
use sawhorse_kernel::refbatch::{NamedBatch, apply};
use serde_json::{Value, json};

fn golden(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn batch(name: &str, ops: Value) -> NamedBatch {
    NamedBatch {
        name: name.into(),
        ops: ops.as_array().unwrap().clone(),
    }
}

#[test]
fn a_batch_that_draws_and_pulls_a_box_hashes_as_the_built_in_box() {
    let g = golden("spike_box");
    // The ground sketch (id 1), as the app starts; then the rectangle and the extrusion in
    // one batch, the extrusion naming the region the rectangle made.
    let mut d = Document::with_ground_sketch();
    apply(
        &mut d,
        &batch(
            "Box",
            json!([
                {"op": "add_rectangle", "sketch": 1, "x": 0.0, "y": 0.0, "w": 0.6096, "h": 0.4064},
                {"op": "extrude", "sketch": 1, "region": {"$ref": 0, "path": "regions.0"}, "distance": 0.3048}
            ]),
        ),
    )
    .unwrap();
    assert_eq!(d.model().hash_hex(), g["hash"].as_str().unwrap());
    assert_eq!(
        d.model().cut_list(&CutOptions::default()).hash,
        g["cut_list"].as_str().unwrap()
    );
    // One undo step back to the empty ground sketch.
    assert_eq!(d.depth().0, 1);
    d.undo().unwrap();
    assert_eq!(d.model().objects().len(), 0);
}

#[test]
fn the_golden_ops_as_one_batch_land_on_the_golden_hash() {
    // Every golden built from plain edits (no history, no navigation): its ops after the
    // ground sketch, sent as one plugin batch, give the golden's model hash.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden");
    let mut tried = 0;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    for f in files {
        let g: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let ops = g["ops"].as_array().unwrap();
        let plain = ops.iter().skip(1).all(|o| {
            !matches!(
                o["op"].as_str(),
                Some("undo" | "redo" | "batch" | "set_context" | "leave_context" | "restore")
            )
        });
        if !plain || ops.len() < 2 || ops[0]["op"] != "new_sketch" {
            continue;
        }
        let mut d = Document::new();
        d.apply(&serde_json::from_value(ops[0].clone()).unwrap())
            .unwrap();
        let r = apply(
            &mut d,
            &NamedBatch {
                name: "golden".into(),
                ops: ops[1..].to_vec(),
            },
        );
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let r = r.unwrap_or_else(|e| panic!("{name}: {e}"));
        let _ = r;
        assert_eq!(
            d.model().hash_hex(),
            g["hash"].as_str().unwrap(),
            "{name}: batch hash differs from the golden"
        );
        tried += 1;
    }
    assert!(tried >= 5, "only {tried} goldens were plain enough");
}

#[test]
fn a_component_of_named_parts_reads_as_a_cut_list() {
    let mut d = Document::with_ground_sketch();
    let item = |name: &str, size: [f64; 3]| {
        let p = |x: f64, y: f64, z: f64| json!([x * size[0], y * size[1], z * size[2]]);
        json!({
            "name": name,
            "positions": [p(0.,0.,0.), p(1.,0.,0.), p(0.,1.,0.), p(1.,1.,0.), p(0.,0.,1.), p(1.,0.,1.), p(0.,1.,1.), p(1.,1.,1.)],
            "faces": [[[0,2,3,1]], [[4,5,7,6]], [[0,1,5,4]], [[2,6,7,3]], [[0,4,6,2]], [[1,3,7,5]]]
        })
    };
    let t = 0.01905;
    let place = |x: f64, z: f64| json!({"linear": [[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]], "translation": [x, 0.0, z]});
    let (out, _) = apply(
        &mut d,
        &batch(
            "Cabinet",
            json!([
                {"op": "import",
                 "definitions": [
                    {"name": "Side", "items": [item("Side", [t, 0.3, 0.9])]},
                    {"name": "Shelf", "items": [item("Shelf", [0.5, 0.3, t])]},
                    {"name": "Cabinet", "items": [], "children": [
                        {"kind": "instance", "definition": 0, "xform": place(0.0, 0.0), "name": "Left side"},
                        {"kind": "instance", "definition": 0, "xform": place(0.5 + t, 0.0), "name": "Right side"},
                        {"kind": "instance", "definition": 1, "xform": place(t, 0.3), "name": "Shelf 1"},
                        {"kind": "instance", "definition": 1, "xform": place(t, 0.6), "name": "Shelf 2"}
                    ]}
                 ],
                 "nodes": [{"kind": "instance", "definition": 2, "xform": place(0.0, 0.0), "name": "Base cabinet"}]
                },
                {"op": "rename", "item": {"node": {"$ref": 0, "path": "groups.0"}}, "name": "Base cabinet A"}
            ]),
        ),
    )
    .unwrap();
    let s = serde_json::to_value(&out).unwrap();
    assert_eq!(s["outcomes"][1]["kind"], "renamed");
    let c = d.model().cut_list(&CutOptions::default());
    let rows: Vec<(String, u32)> = c
        .rows
        .iter()
        .map(|r| (r.name.clone(), r.qty as u32))
        .collect();
    assert!(rows.contains(&("Side".to_string(), 2)), "{rows:?}");
    assert!(rows.contains(&("Shelf".to_string(), 2)), "{rows:?}");
    let g = d.model().graph();
    let labels: Vec<&str> = g.nodes.iter().map(|n| n.label.as_str()).collect();
    for want in [
        "Base cabinet A",
        "Left side",
        "Right side",
        "Shelf 1",
        "Shelf 2",
    ] {
        assert!(labels.contains(&want), "{labels:?}");
    }
}
