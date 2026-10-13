//! Lane 15: the cut list and the nesting from parts that are not watertight solids.
//!
//! Most hand-made SketchUp furniture arrives as groups of open faces. Such a group (or
//! component instance) is still a part: the heal is tried first, and if the faces still do
//! not close, the part is sized from the minimal box around them and flagged. Loose faces
//! outside any group are counted, never listed; Group loose faces turns each piece of them
//! into a part. Any part can be dropped from the totals and the nesting.

use sawhorse_kernel::Document;
use sawhorse_kernel::cutlist::{CutList, CutOptions, M_PER_IN, PartRef};
use sawhorse_kernel::meshbuild::ImportItem;
use sawhorse_kernel::model::{ImportNode, ImportNodeKind, Model, Op, OpOutcome};
use sawhorse_kernel::nesting::NestOptions;

const IN: f64 = M_PER_IN;

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

/// A box `size` inches from `at`, as a `.skp`-style item: six quads, shared corners.
/// `skip` leaves those faces out (an open box), by index: 0 bottom, 1 top, 2 front, 3 back,
/// 4 left, 5 right.
fn box_item(name: &str, at: [f64; 3], size: [f64; 3], skip: &[usize]) -> ImportItem {
    let c = |i: u32| {
        [
            (at[0] + if i & 1 != 0 { size[0] } else { 0.0 }) * IN,
            (at[1] + if i & 2 != 0 { size[1] } else { 0.0 }) * IN,
            (at[2] + if i & 4 != 0 { size[2] } else { 0.0 }) * IN,
        ]
    };
    let quads: [[u32; 4]; 6] = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    ImportItem {
        name: name.into(),
        group: None,
        tag: None,
        material: None,
        positions: (0..8).map(c).collect(),
        faces: quads
            .iter()
            .enumerate()
            .filter(|(i, _)| !skip.contains(i))
            .map(|(_, q)| vec![q.to_vec()])
            .collect(),
        lines: vec![],
        merge_coplanar: false,
        face_labels: vec![],
        face_materials: vec![],
        face_uvs: vec![],
    }
}

fn group(name: &str, items: Vec<ImportItem>) -> ImportNode {
    ImportNode {
        name: Some(name.into()),
        tag: None,
        hidden: false,
        kind: ImportNodeKind::Group {
            items,
            children: vec![],
        },
    }
}

fn import(items: Vec<ImportItem>, nodes: Vec<ImportNode>) -> Op {
    Op::Import {
        items,
        strokes: vec![],
        materials: vec![],
        scenes: vec![],
        hidden_tags: vec![],
        definitions: vec![],
        nodes,
    }
}

fn list(d: &Document) -> CutList {
    d.model().cut_list(&CutOptions::default())
}

fn inches(v: [f64; 3]) -> [f64; 3] {
    v.map(|x| (x / IN * 1e6).round() / 1e6)
}

#[test]
fn an_open_box_in_a_group_is_a_part_sized_from_its_bounds() {
    let mut d = Document::new();
    // Five faces of a 3/4 x 5 x 30 in box: the top left off.
    ok(
        &mut d,
        import(
            vec![],
            vec![group(
                "Rail",
                vec![box_item("Rail", [0.0; 3], [5.0, 30.0, 0.75], &[1])],
            )],
        ),
    );
    assert_eq!(d.model().meshes().len(), 1, "the open box stays a mesh");
    assert_eq!(d.model().meshes()[0].1.faces.len(), 5);
    let l = list(&d);
    assert_eq!(l.rows.len(), 1, "{:?}", l.rows);
    let r = &l.rows[0];
    assert_eq!(r.name, "Rail");
    assert!(r.not_solid && r.included && !r.shaped);
    assert_eq!(inches(r.size), [0.75, 5.0, 30.0]);
    assert_eq!(r.steps, [12, 80, 480]);
    assert!(!r.sheet);
    // Board feet on the nominal 4/4 the part comes from (lane 15).
    assert_eq!(r.board_feet, 1.0 * 5.0 * 30.0 / 144.0);
    assert_eq!(l.totals.not_solid, 1);
    assert_eq!(l.totals.loose_faces, 0);
    assert!(matches!(r.target, PartRef::Node(_)));
    // Nesting places it like any other part.
    let n = d.model().nest(&NestOptions::default());
    assert_eq!(n.totals.parts_in, 1);
    assert_eq!(n.totals.placed, 1, "{:?}", n.unplaced);
    sawhorse_kernel::testing::check_cut_list(d.model()).unwrap();
    sawhorse_kernel::testing::check_nesting(d.model()).unwrap();
}

#[test]
fn a_box_with_a_tiny_gap_heals_and_is_not_flagged() {
    let mut item = box_item("Block", [0.0; 3], [3.0, 3.0, 3.0], &[]);
    // One corner 50 micrometres off on the top face only: the import's 10 micrometre weld
    // leaves the box open (a mesh), the cut list's 0.1 mm heal closes it.
    item.positions.push([3.0 * IN + 5e-5, 3.0 * IN, 3.0 * IN]);
    let top = 1;
    let corner = item.faces[top][0].iter().position(|&v| v == 7).unwrap();
    item.faces[top][0][corner] = 8;
    let mut d = Document::new();
    ok(&mut d, import(vec![], vec![group("Block", vec![item])]));
    assert_eq!(
        d.model().meshes().len(),
        1,
        "a gap the import does not weld"
    );
    let l = list(&d);
    assert_eq!(l.rows.len(), 1);
    assert!(!l.rows[0].not_solid, "the heal closes a 50 micrometre gap");
    sawhorse_kernel::testing::check_cut_list(d.model()).unwrap();
}

#[test]
fn loose_faces_are_counted_not_listed_and_group_loose_makes_them_parts() {
    let mut d = Document::new();
    // Two open boxes drawn loose at the top level, one item (one mesh), like a bench file
    // drawn without groups.
    let a = box_item("benches", [0.0; 3], [1.0, 4.0, 20.0], &[1]);
    let b = box_item("benches", [10.0, 0.0, 0.0], [1.5, 6.0, 30.0], &[0]);
    let mut both = a.clone();
    let off = both.positions.len() as u32;
    both.positions.extend(b.positions);
    both.faces.extend(
        b.faces
            .iter()
            .map(|f| vec![f[0].iter().map(|v| v + off).collect()]),
    );
    ok(&mut d, import(vec![both], vec![]));
    let l = list(&d);
    assert!(l.rows.is_empty(), "loose faces are never parts");
    assert_eq!(l.totals.loose_faces, 10, "the header counts them");
    sawhorse_kernel::testing::check_cut_list(d.model()).unwrap();
    let before = d.hash();

    let out = ok(&mut d, Op::GroupLoose { meshes: vec![] });
    let OpOutcome::LooseGrouped { groups } = out else {
        panic!("{out:?}")
    };
    assert_eq!(groups.len(), 2, "one group per edge-connected piece");
    let l = list(&d);
    assert_eq!(l.totals.loose_faces, 0);
    let names: Vec<&str> = l.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["benches 1", "benches 2"]);
    assert!(l.rows.iter().all(|r| r.not_solid));
    assert_eq!(inches(l.rows[0].size), [1.0, 4.0, 20.0]);
    assert_eq!(inches(l.rows[1].size), [1.5, 6.0, 30.0]);
    sawhorse_kernel::testing::check_cut_list(d.model()).unwrap();
    sawhorse_kernel::testing::check_nesting(d.model()).unwrap();
    // Nothing moved: the same faces, in two meshes now.
    let faces: usize = d.model().meshes().iter().map(|(_, m)| m.faces.len()).sum();
    assert_eq!(faces, 10);

    // One undo step.
    d.undo().unwrap();
    assert_eq!(d.hash(), before);
    // Nothing left to group: a typed refusal, nothing changed.
    d.redo().unwrap();
    let h = d.hash();
    assert!(d.apply(&Op::GroupLoose { meshes: vec![] }).is_err());
    assert_eq!(d.hash(), h);
}

#[test]
fn a_dropped_part_is_listed_but_not_counted_or_nested() {
    let mut d = Document::new();
    ok(
        &mut d,
        import(
            vec![],
            vec![
                group(
                    "Seat",
                    vec![box_item("Seat", [0.0; 3], [12.0, 40.0, 1.0], &[1])],
                ),
                group(
                    "Cushion",
                    vec![box_item(
                        "Cushion",
                        [0.0, 0.0, 2.0],
                        [12.0, 40.0, 3.0],
                        &[0],
                    )],
                ),
            ],
        ),
    );
    let l = list(&d);
    assert_eq!(l.rows.len(), 2);
    let all_bf = l.totals.board_feet;
    let before = l.hash.clone();
    let cushion = l.rows.iter().find(|r| r.name == "Cushion").unwrap().target;
    ok(
        &mut d,
        Op::SetInclude {
            part: cushion,
            include: false,
        },
    );
    let l = list(&d);
    let c = l.rows.iter().find(|r| r.name == "Cushion").unwrap();
    assert!(!c.included, "still listed, marked");
    assert_eq!(l.totals.excluded, 1);
    assert_eq!(l.totals.pieces, 1);
    let seat_bf = l.rows.iter().find(|r| r.name == "Seat").unwrap().board_feet;
    assert_eq!(l.totals.board_feet, seat_bf);
    assert!(all_bf > seat_bf);
    assert_ne!(l.hash, before);
    let n = d.model().nest(&NestOptions::default());
    assert_eq!(n.totals.parts_in, 1, "the dropped part is not nested");
    assert!(
        n.pieces
            .iter()
            .flat_map(|p| &p.placements)
            .all(|p| p.name == "Seat")
    );
    sawhorse_kernel::testing::check_cut_list(d.model()).unwrap();
    sawhorse_kernel::testing::check_nesting(d.model()).unwrap();
    // Taking it back is the old list exactly (the attribute is gone, not "true").
    ok(
        &mut d,
        Op::SetInclude {
            part: cushion,
            include: true,
        },
    );
    assert_eq!(list(&d).hash, before);
    // Saved and read back.
    ok(
        &mut d,
        Op::SetInclude {
            part: cushion,
            include: false,
        },
    );
    let json = serde_json::to_string(&d.model().snapshot()).unwrap();
    assert!(json.contains("\"include\":false"), "{json}");
    let back = Model::from_snapshot(&serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(back.hash(), d.model().hash());
    assert_eq!(back.cut_list(&CutOptions::default()).totals.excluded, 1);
}

/// The golden `cutlist-mixed.json` replays exactly this: one solid part and one part that is
/// not solid, in one document. Written under `UPDATE_GOLDEN=1` (then `--test golden` fills
/// in the hashes); otherwise it must match, so the WASM kernel is checked on flagged parts.
#[test]
fn the_mixed_cut_list_golden_is_this_document() {
    let mut d = Document::new();
    ok(
        &mut d,
        import(
            vec![],
            vec![
                group(
                    "Top",
                    vec![box_item("Top", [0.0; 3], [16.0, 24.0, 0.75], &[])],
                ),
                group(
                    "Apron",
                    vec![box_item(
                        "Apron",
                        [0.0, 0.0, -4.0],
                        [14.0, 0.75, 4.0],
                        &[0, 1],
                    )],
                ),
            ],
        ),
    );
    let l = list(&d);
    assert_eq!(l.rows.len(), 2);
    let top = l.rows.iter().find(|r| r.name == "Top").unwrap();
    let apron = l.rows.iter().find(|r| r.name == "Apron").unwrap();
    assert!(!top.not_solid && apron.not_solid);
    assert_eq!(inches(apron.size), [0.75, 4.0, 14.0]);
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cutlist-mixed.json");
    let ops = serde_json::to_value(d.log()).unwrap();
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        let mut g: serde_json::Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| {
                serde_json::json!({
                    "name": "cutlist-mixed",
                    "hash": "",
                    "stats": {"objects": 0, "faces": 0, "edges": 0, "vertices": 0, "genus": 0, "watertight": true}
                })
            });
        g["description"] = "Lane 15: a table top (a closed box: a solid part) and an apron imported as an open box with no top or bottom (a mesh in a group: a part flagged not solid, sized from its bounds), in one document. Its cut_list and nesting hashes prove WASM lists and nests flagged parts as native does.".into();
        g["ops"] = ops;
        std::fs::write(&path, serde_json::to_string_pretty(&g).unwrap() + "\n").unwrap();
    } else {
        let g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            g["ops"], ops,
            "golden/cutlist-mixed.json is not this document (UPDATE_GOLDEN=1)"
        );
    }
}
