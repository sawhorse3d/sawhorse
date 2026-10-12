//! The import op: solids where the geometry is a valid solid, non-solid meshes (with a note)
//! where it is not, strokes into sketches, materials, groups, tags and scenes, all as one
//! undoable step that replays identically from the log.

use sawhorse_kernel::meshbuild::ImportItem;
use sawhorse_kernel::model::{ImportStroke, MaterialDef, SceneDef};
use sawhorse_kernel::{Document, DocumentError, KernelError, Op, OpOutcome};

/// An axis box as six quads (shared vertices, outward), SketchUp-style polygons.
fn quad_box(name: &str, min: [f64; 3], s: [f64; 3]) -> ImportItem {
    let c = |i: usize| {
        [
            min[0] + if i & 1 != 0 { s[0] } else { 0.0 },
            min[1] + if i & 2 != 0 { s[1] } else { 0.0 },
            min[2] + if i & 4 != 0 { s[2] } else { 0.0 },
        ]
    };
    ImportItem {
        name: name.into(),
        group: None,
        tag: None,
        material: None,
        positions: (0..8).map(c).collect(),
        faces: [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [1, 3, 7, 5],
            [0, 4, 6, 2],
        ]
        .iter()
        .map(|q| vec![q.to_vec()])
        .collect(),
        lines: vec![],
        merge_coplanar: false,
        face_labels: vec![],
    }
}

fn import_op() -> Op {
    let mut leg = quad_box("Leg", [0.0, 0.0, 0.0], [0.05, 0.05, 0.7]);
    leg.group = Some("Table".into());
    leg.tag = Some("Legs".into());
    leg.material = Some("Walnut".into());
    let mut top = quad_box("Top", [0.0, 0.0, 0.7], [0.8, 0.5, 0.03]);
    top.group = Some("Table".into());
    let mut open = quad_box("Lid", [2.0, 0.0, 0.0], [0.3, 0.3, 0.3]);
    open.faces.pop();
    open.lines = vec![[0, 7]];
    Op::Import {
        items: vec![leg, top, open],
        strokes: vec![
            ImportStroke {
                points: vec![[-1.0, -1.0, 0.0], [-0.5, -1.0, 0.0], [-0.5, -0.5, 0.0]],
                closed: true,
            },
            ImportStroke {
                points: vec![
                    [0.0, 0.0, 0.0],
                    [0.0, 1.0, 1.0],
                    [1.0, 0.0, 3.0],
                    [5.0, 5.0, 0.0],
                ],
                closed: false,
            },
        ],
        materials: vec![MaterialDef {
            name: "Walnut".into(),
            color: [90, 60, 40, 255],
            texture: Some("walnut.png".into()),
        }],
        scenes: vec![SceneDef {
            name: "Front".into(),
            eye: [0.4, -3.0, 0.8],
            target: [0.4, 0.25, 0.4],
            up: [0.0, 0.0, 1.0],
            fov: 35.0,
            parallel: false,
            hidden_tags: vec![],
        }],
        hidden_tags: vec!["Legs".into()],
    }
}

#[test]
fn import_makes_solids_meshes_strokes_groups_and_is_one_undo_step() {
    let mut d = Document::with_ground_sketch();
    let before = d.hash();
    let out = d.apply(&import_op()).unwrap();
    let OpOutcome::Imported {
        objects,
        meshes,
        groups,
        sketches,
        notes,
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(objects, vec![1, 2]);
    assert_eq!(meshes, vec![1]);
    assert_eq!(groups.len(), 1);
    assert_eq!(
        sketches,
        vec![1],
        "the triangle went onto the ground sketch"
    );
    assert!(
        notes.iter().any(|n| n.contains("Lid: kept as a mesh")),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("skipped 1 stroke")),
        "{notes:?}"
    );
    let m = d.model();
    assert_eq!(m.object(1).unwrap().name(), Some("Leg"));
    assert_eq!(m.object(1).unwrap().material(), Some("Walnut"));
    assert_eq!(m.object(1).unwrap().tag(), Some("Legs"));
    assert_eq!(m.object(2).unwrap().face_ids().len(), 6);
    assert!(!m.is_visible(1), "the Legs tag came in hidden");
    assert!(m.is_visible(2));
    assert_eq!(m.groups()[0].1.name, "Table");
    assert_eq!(m.groups()[0].1.objects, vec![1, 2]);
    let lid = m.mesh(1).unwrap();
    assert_eq!(lid.faces.len(), 5);
    assert_eq!(lid.lines.len(), 1);
    assert!(lid.note.contains("4 open edges"), "{}", lid.note);
    assert_eq!(
        m.materials()["Walnut"].texture.as_deref(),
        Some("walnut.png")
    );
    assert_eq!(m.scenes()[0].name, "Front");
    assert_eq!(m.stats().regions, 1);
    let st = m.stats();
    assert!(st.watertight);
    assert!((st.volume_m3 - (0.05 * 0.05 * 0.7 + 0.8 * 0.5 * 0.03)).abs() < 1e-12);
    // The view mesh shows the visible solid and the mesh (amber in the UI), not the leg.
    let vm = sawhorse_kernel::mesh::view_mesh(m).unwrap();
    assert!(
        vm.parts
            .iter()
            .any(|p| p.kind == sawhorse_kernel::mesh::PartKind::Mesh)
    );
    assert!(
        vm.parts
            .iter()
            .all(|p| p.owner != 1 || p.kind != sawhorse_kernel::mesh::PartKind::Face)
    );
    // Replay, then undo as one step.
    let replayed = Document::replay(d.log()).unwrap();
    assert_eq!(replayed.hash(), d.hash());
    d.undo().unwrap();
    assert_eq!(d.hash(), before);
    d.redo().unwrap();
    assert_eq!(d.hash(), replayed.hash());
    // The imported solid edits like any other: push its top.
    d.apply(&Op::PushPull {
        object: 2,
        face: 2,
        distance: 0.01,
    })
    .unwrap();
    assert_eq!(
        d.model().object(2).unwrap().name(),
        Some("Top"),
        "metadata survives edits"
    );
    d.apply(&Op::DeleteMesh { mesh: 1 }).unwrap();
    assert!(d.model().meshes().is_empty());
}

#[test]
fn refusals_change_nothing() {
    let mut d = Document::with_ground_sketch();
    let h = d.hash();
    let empty = Op::Import {
        items: vec![],
        strokes: vec![],
        materials: vec![],
        scenes: vec![],
        hidden_tags: vec![],
    };
    assert_eq!(
        d.apply(&empty),
        Err(KernelError::Document(DocumentError::NothingImported))
    );
    let mut bad = quad_box("Bad", [0.0; 3], [1.0; 3]);
    bad.faces[0][0][0] = 42;
    assert!(matches!(
        d.apply(&Op::Import {
            items: vec![bad],
            strokes: vec![],
            materials: vec![],
            scenes: vec![],
            hidden_tags: vec![],
        }),
        Err(KernelError::Document(DocumentError::BadImport(_)))
    ));
    assert!(
        d.apply(&Op::SetMaterial {
            object: 1,
            material: Some("Nope".into())
        })
        .is_err()
    );
    assert!(d.apply(&Op::DeleteMesh { mesh: 3 }).is_err());
    assert_eq!(d.hash(), h);
}

#[test]
fn ops_round_trip_through_json() {
    let op = import_op();
    let json = serde_json::to_string(&op).unwrap();
    assert!(json.starts_with(r#"{"op":"import","items":[{"name":"Leg","group":"Table""#));
    let back: Op = serde_json::from_str(&json).unwrap();
    assert_eq!(back, op);
    // Optional fields default.
    let min: Op = serde_json::from_str(
        r#"{"op":"import","items":[{"name":"t","positions":[[0,0,0],[1,0,0],[0,1,0]],"faces":[[[0,1,2]]]}]}"#,
    )
    .unwrap();
    let mut d = Document::with_ground_sketch();
    let out = d.apply(&min).unwrap();
    assert!(matches!(out, OpOutcome::Imported { ref meshes, .. } if meshes.len() == 1));
}
