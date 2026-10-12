//! Materials, paint, texture positioning, styles and shadows (lane 10, ADR-0008): face and
//! container paint and what a face shows, paint through splits, imprints, pushes and
//! components, the cut list reading species from the material, textures and their
//! coordinates, styles and shadows in scenes, the snapshot round trip, refusals, and the
//! painted golden (`tests/golden/painted.json`).

use glam::DVec3;
use sawhorse_kernel::Document;
use sawhorse_kernel::cutlist::{CutList, CutOptions, M_PER_IN};
use sawhorse_kernel::looks::{
    EdgeStyle, FaceStyle, PaintTarget, Shadows, StockKind, StockLink, Style, UvXform,
};
use sawhorse_kernel::mesh::{face_uvs, view_mesh};
use sawhorse_kernel::model::{DrawTarget, Op, OpOutcome, SceneDef};
use sawhorse_kernel::nodes::Item;
use sawhorse_kernel::testing::tiny_png;
use sawhorse_kernel::transform::Xform;

const IN: f64 = M_PER_IN;

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

fn refused(d: &mut Document, op: Op) -> String {
    let before = d.hash();
    let e = d.apply(&op).expect_err("refused");
    assert!(!e.is_bug(), "a refusal, not a bug: {e}");
    assert_eq!(d.hash(), before, "a refusal changes nothing");
    e.to_string()
}

/// The ops that draw a board from (x, y, z), `w` along red, `d` along green, `h` up
/// (inches), when the next sketch is `sketch` and its region 1 the rectangle.
fn board_ops(x: f64, y: f64, z: f64, w: f64, d: f64, h: f64) -> Vec<Op> {
    let (x, y, z, w, d, h) = (x * IN, y * IN, z * IN, w * IN, d * IN, h * IN);
    vec![Op::Draw {
        target: DrawTarget::Plane {
            origin: [0.0, 0.0, z],
            normal: [0.0, 0.0, 1.0],
        },
        points: vec![[x, y, z], [x + w, y, z], [x + w, y + d, z], [x, y + d, z]],
        closed: true,
    }]
    .into_iter()
    .chain(std::iter::once(Op::Extrude {
        sketch: 0,
        region: 0,
        distance: h,
    }))
    .collect()
}

/// A board, as `board_ops` draws it. Returns its object id. Faces: 1 bottom, 2 top, 3..6
/// the sides (front, right, back, left).
fn board(doc: &mut Document, x: f64, y: f64, z: f64, w: f64, d: f64, h: f64) -> u64 {
    let ops = board_ops(x, y, z, w, d, h);
    let OpOutcome::Drawn {
        sketch: Some(sketch),
        inside: Some(region),
        ..
    } = ok(doc, ops[0].clone())
    else {
        panic!()
    };
    let Op::Extrude { distance, .. } = ops[1] else {
        panic!()
    };
    match ok(
        doc,
        Op::Extrude {
            sketch,
            region,
            distance,
        },
    ) {
        OpOutcome::ObjectCreated { object } => object,
        o => panic!("{o:?}"),
    }
}

fn define(name: &str, color: [u8; 3], stock: Option<(StockKind, &str)>) -> Op {
    Op::DefineMaterial {
        name: name.into(),
        color: [color[0], color[1], color[2], 255],
        texture: None,
        size: None,
        rotation: 0.0,
        offset: [0.0, 0.0],
        stock: stock.map(|(kind, s)| StockLink {
            kind,
            species: s.into(),
        }),
    }
}

fn paint_faces(object: u64, faces: &[u64], material: Option<&str>) -> Op {
    Op::Paint {
        target: PaintTarget::Faces {
            object,
            faces: faces.to_vec(),
        },
        material: material.map(str::to_string),
    }
}

fn paint_items(items: &[Item], material: Option<&str>) -> Op {
    Op::Paint {
        target: PaintTarget::Items {
            items: items.to_vec(),
        },
        material: material.map(str::to_string),
    }
}

fn shows(d: &Document, object: u64, face: u64) -> Option<String> {
    d.model().face_material(object, face).map(str::to_string)
}

fn list(d: &Document) -> CutList {
    d.model().cut_list(&CutOptions::default())
}

#[test]
fn faces_show_their_own_paint_then_the_solid_then_the_container() {
    let mut d = Document::with_ground_sketch();
    let b = board(&mut d, 0.0, 0.0, 0.0, 10.0, 4.0, 1.0);
    ok(&mut d, define("Walnut", [90, 60, 40], None));
    ok(&mut d, define("Paint", [240, 240, 236], None));
    ok(&mut d, define("Red", [160, 40, 30], None));
    assert_eq!(shows(&d, b, 2), None, "unpainted: the default look");
    ok(&mut d, paint_faces(b, &[2], Some("Walnut")));
    assert_eq!(shows(&d, b, 2).as_deref(), Some("Walnut"));
    assert_eq!(shows(&d, b, 3), None);
    // Painting the solid paints its default; the top keeps its own.
    ok(&mut d, paint_items(&[Item::Object(b)], Some("Paint")));
    assert_eq!(shows(&d, b, 2).as_deref(), Some("Walnut"));
    assert_eq!(shows(&d, b, 3).as_deref(), Some("Paint"));
    // In a group painted red: the solid's own default still wins; without it, the group's.
    let OpOutcome::GroupCreated { group } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(b)],
            name: Some("Box".into()),
        },
    ) else {
        panic!()
    };
    ok(&mut d, paint_items(&[Item::Node(group)], Some("Red")));
    assert_eq!(shows(&d, b, 3).as_deref(), Some("Paint"));
    ok(&mut d, paint_items(&[Item::Object(b)], None));
    assert_eq!(shows(&d, b, 3).as_deref(), Some("Red"));
    assert_eq!(
        shows(&d, b, 2).as_deref(),
        Some("Walnut"),
        "face paint wins"
    );
    // Sampling reads what a face shows.
    assert_eq!(d.model().sample(b, 4).unwrap().as_deref(), Some("Red"));
    assert_eq!(d.model().sample(b, 2).unwrap().as_deref(), Some("Walnut"));
    // The view mesh tells the renderer which material each face shows, and which are its own.
    let m = view_mesh(d.model()).unwrap();
    let top = m
        .parts
        .iter()
        .find(|p| p.owner == b && p.index == 2)
        .unwrap();
    assert_eq!(top.material.as_deref(), Some("Walnut"));
    assert!(top.own);
    assert_eq!(top.color, Some([90, 60, 40, 255]));
    let side = m
        .parts
        .iter()
        .find(|p| p.owner == b && p.index == 3)
        .unwrap();
    assert_eq!(side.material.as_deref(), Some("Red"));
    assert!(!side.own);
    // Taking the paint off the face: back to the group's.
    ok(&mut d, paint_faces(b, &[2], None));
    assert_eq!(shows(&d, b, 2).as_deref(), Some("Red"));
    assert!(d.model().object(b).unwrap().face_paint().is_empty());
    // One undo brings the face paint back.
    d.undo().unwrap();
    assert_eq!(shows(&d, b, 2).as_deref(), Some("Walnut"));
}

#[test]
fn paint_follows_splits_imprints_pushes_and_merges() {
    let mut d = Document::with_ground_sketch();
    let b = board(&mut d, 0.0, 0.0, 0.0, 10.0, 4.0, 1.0);
    ok(&mut d, define("Oak", [200, 170, 120], None));
    ok(&mut d, paint_faces(b, &[2], Some("Oak")));
    // A line across the top splits it: both pieces stay oak.
    let z = 1.0 * IN;
    let out = ok(
        &mut d,
        Op::Draw {
            target: DrawTarget::Face { object: b, face: 2 },
            points: vec![[5.0 * IN, 0.0, z], [5.0 * IN, 4.0 * IN, z]],
            closed: false,
        },
    );
    let OpOutcome::Drawn { faces, .. } = out else {
        panic!("{out:?}")
    };
    assert_eq!(faces.len(), 2);
    for f in &faces {
        assert_eq!(shows(&d, b, *f).as_deref(), Some("Oak"), "piece {f}");
    }
    // An imprint inside one piece is oak too.
    let pts = [[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]].map(|[x, y]| [x * IN, y * IN, z]);
    let out = ok(
        &mut d,
        Op::Draw {
            target: DrawTarget::Face {
                object: b,
                face: faces[0],
            },
            points: pts.to_vec(),
            closed: true,
        },
    );
    let OpOutcome::Drawn {
        inside: Some(sub), ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(shows(&d, b, sub).as_deref(), Some("Oak"));
    // Pushing the imprint down makes a pocket: its floor keeps the paint, the new walls show
    // the default (they are new faces, nothing was said about them).
    let before: Vec<u64> = d.model().object(b).unwrap().brep().persistent_face_ids();
    ok(
        &mut d,
        Op::PushPull {
            object: b,
            face: sub,
            distance: -0.25 * IN,
        },
    );
    assert_eq!(shows(&d, b, sub).as_deref(), Some("Oak"));
    let after = d.model().object(b).unwrap().brep().persistent_face_ids();
    let walls: Vec<u64> = after
        .iter()
        .filter(|f| !before.contains(f))
        .copied()
        .collect();
    assert_eq!(walls.len(), 4);
    for w in walls {
        assert_eq!(shows(&d, b, w), None, "new wall {w}");
    }
    // Paint on faces that went away goes with them: the validator would refuse otherwise.
    let live = d.model().object(b).unwrap().brep().persistent_face_ids();
    for f in d.model().object(b).unwrap().face_paint().keys() {
        assert!(live.contains(f));
    }
}

#[test]
fn component_paint_reaches_every_copy_and_instance_paint_only_its_own() {
    let mut d = Document::with_ground_sketch();
    ok(&mut d, define("Oak", [200, 170, 120], None));
    ok(&mut d, define("Blue", [40, 60, 160], None));
    let b = board(&mut d, 0.0, 0.0, 0.0, 30.0, 11.0, 0.75);
    let OpOutcome::ComponentCreated { instance, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(b)],
            name: "Shelf".into(),
        },
    ) else {
        panic!()
    };
    let OpOutcome::Copied { nodes, .. } = ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![instance],
            xform: Xform::translation(DVec3::new(0.0, 0.0, 12.0 * IN)),
        },
    ) else {
        panic!()
    };
    let copy = nodes[0];
    let solid_of = |d: &Document, node: u64| {
        d.model()
            .objects()
            .into_iter()
            .find(|(_, o)| o.membership().parent() == Some(node))
            .map(|(id, _)| id)
            .unwrap()
    };
    // Inside one copy, paint the top: the other copy's top is oak too.
    ok(
        &mut d,
        Op::SetContext {
            node: Some(instance),
        },
    );
    let mine = solid_of(&d, instance);
    ok(&mut d, paint_faces(mine, &[2], Some("Oak")));
    ok(&mut d, Op::LeaveContext);
    let other = solid_of(&d, copy);
    assert_eq!(shows(&d, other, 2).as_deref(), Some("Oak"));
    // Paint one instance blue (closed: its default): only its unpainted faces turn blue.
    ok(&mut d, paint_items(&[Item::Node(copy)], Some("Blue")));
    assert_eq!(shows(&d, other, 3).as_deref(), Some("Blue"));
    assert_eq!(shows(&d, other, 2).as_deref(), Some("Oak"));
    assert_eq!(shows(&d, mine, 3), None, "the first copy is unpainted");
    // An edit in the first copy re-derives the blue one; it stays blue.
    ok(
        &mut d,
        Op::SetContext {
            node: Some(instance),
        },
    );
    ok(
        &mut d,
        Op::PushPull {
            object: mine,
            face: 2,
            distance: 0.25 * IN,
        },
    );
    ok(&mut d, Op::LeaveContext);
    let other = solid_of(&d, copy);
    assert_eq!(shows(&d, other, 3).as_deref(), Some("Blue"));
    assert_eq!(shows(&d, other, 2).as_deref(), Some("Oak"));
    // Make Unique keeps both.
    ok(&mut d, Op::MakeUnique { node: copy });
    let other = solid_of(&d, copy);
    assert_eq!(shows(&d, other, 3).as_deref(), Some("Blue"));
    assert_eq!(shows(&d, other, 2).as_deref(), Some("Oak"));
    // Round trip through the snapshot: the same model.
    let back = sawhorse_kernel::Model::from_snapshot(&d.model().snapshot()).unwrap();
    assert_eq!(back.hash(), d.model().hash());
}

#[test]
fn the_cut_list_reads_the_species_from_the_material() {
    let mut d = Document::with_ground_sketch();
    ok(
        &mut d,
        define(
            "White oak",
            [205, 178, 136],
            Some((StockKind::Lumber, "White oak")),
        ),
    );
    ok(
        &mut d,
        define(
            "Birch ply",
            [230, 205, 165],
            Some((StockKind::Sheet, "3/4 Baltic birch ply")),
        ),
    );
    let side = board(&mut d, 0.0, 0.0, 0.0, 0.75, 11.25, 30.0);
    ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(side)],
            name: "Side".into(),
        },
    );
    // Every face of the part painted White oak (no stock said on the part): White oak.
    let solid = side;
    let all: Vec<u64> = d
        .model()
        .object(solid)
        .unwrap()
        .brep()
        .persistent_face_ids();
    let inst = d
        .model()
        .object(solid)
        .unwrap()
        .membership()
        .parent()
        .unwrap();
    ok(&mut d, Op::SetContext { node: Some(inst) });
    ok(&mut d, paint_faces(solid, &all, Some("White oak")));
    ok(&mut d, Op::LeaveContext);
    let back = board(&mut d, 1.0, 11.0, 0.0, 28.0, 0.75, 30.0);
    ok(
        &mut d,
        paint_items(&[Item::Object(back)], Some("Birch ply")),
    );
    let l = list(&d);
    let side_row = l.rows.iter().find(|r| r.name == "Side").unwrap();
    assert_eq!(side_row.material.as_deref(), Some("White oak"));
    assert!(!side_row.sheet && !side_row.stock_set);
    // The ply's link says sheet goods, though it is 3/4 in thick and its name says nothing.
    let ply = l.rows.iter().find(|r| r.name.starts_with("Solid")).unwrap();
    assert_eq!(ply.material.as_deref(), Some("3/4 Baltic birch ply"));
    assert!(ply.sheet);
    assert!(
        l.totals
            .lumber
            .iter()
            .any(|t| t.material.as_deref() == Some("White oak"))
    );
    // A stock named on the part wins over the material.
    ok(
        &mut d,
        Op::SetStock {
            part: sawhorse_kernel::cutlist::PartRef::Node(inst),
            stock: Some("Rift white oak".into()),
            sheet: None,
        },
    );
    let l = list(&d);
    let side_row = l.rows.iter().find(|r| r.name == "Side").unwrap();
    assert_eq!(side_row.material.as_deref(), Some("Rift white oak"));
    assert!(side_row.stock_set);
    // Half the faces: whichever covers more area names the part.
    let mut d2 = Document::with_ground_sketch();
    ok(&mut d2, define("Walnut", [90, 60, 40], None));
    let b = board(&mut d2, 0.0, 0.0, 0.0, 10.0, 10.0, 1.0);
    ok(&mut d2, paint_faces(b, &[3], Some("Walnut")));
    assert_eq!(
        list(&d2).rows[0].material,
        None,
        "a small face does not name the part"
    );
    ok(&mut d2, paint_faces(b, &[1, 2], Some("Walnut")));
    assert_eq!(list(&d2).rows[0].material.as_deref(), Some("Walnut"));
}

fn add_texture(path: &str) -> Op {
    Op::AddTexture {
        path: path.into(),
        mime: "image/png".into(),
        data: tiny_png(4, 2, [200, 160, 110]),
    }
}

fn textured(name: &str, path: &str, size_in: [f64; 2]) -> Op {
    Op::DefineMaterial {
        name: name.into(),
        color: [200, 160, 110, 255],
        texture: Some(path.into()),
        size: Some([size_in[0] * IN, size_in[1] * IN]),
        rotation: 0.0,
        offset: [0.0, 0.0],
        stock: Some(StockLink {
            kind: StockKind::Lumber,
            species: "White oak".into(),
        }),
    }
}

#[test]
fn textures_give_coordinates_and_positions_move_them() {
    let mut d = Document::with_ground_sketch();
    let b = board(&mut d, 0.0, 0.0, 0.0, 24.0, 6.0, 1.0);
    ok(&mut d, add_texture("assets/oak.png"));
    ok(&mut d, textured("White oak", "assets/oak.png", [12.0, 6.0]));
    ok(&mut d, paint_items(&[Item::Object(b)], Some("White oak")));
    let m = view_mesh(d.model()).unwrap();
    assert_eq!(m.uvs.len() * 3, m.positions.len() * 2, "two per vertex");
    // The top (24 x 6 in) repeats the 12 x 6 in texture twice along its length: the grain
    // runs along the board.
    let part = m
        .parts
        .iter()
        .position(|p| p.owner == b && p.index == 2)
        .unwrap();
    let verts: Vec<usize> = m
        .indices
        .chunks(3)
        .zip(&m.face_ids)
        .filter(|(_, f)| **f as usize == part)
        .flat_map(|(t, _)| t.iter().map(|&i| i as usize))
        .collect();
    let (mut umin, mut umax, mut vmin, mut vmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for &i in &verts {
        umin = umin.min(m.uvs[2 * i]);
        umax = umax.max(m.uvs[2 * i]);
        vmin = vmin.min(m.uvs[2 * i + 1]);
        vmax = vmax.max(m.uvs[2 * i + 1]);
    }
    assert!(((umax - umin) - 2.0).abs() < 1e-4, "{umin}..{umax}");
    assert!(((vmax - vmin) - 1.0).abs() < 1e-4, "{vmin}..{vmax}");
    // The live preview gives what the op will.
    let pos = UvXform {
        scale: [2.0, 2.0],
        rotation: 90.0,
        offset: [0.5 * IN, 0.0],
    };
    let preview = face_uvs(d.model(), b, 2, &pos).unwrap();
    ok(
        &mut d,
        Op::SetFaceUv {
            object: b,
            face: 2,
            uv: Some(pos),
        },
    );
    let m2 = view_mesh(d.model()).unwrap();
    let first = verts.iter().min().copied().unwrap();
    let got: Vec<f32> = m2.uvs[2 * first..2 * first + preview.len()].to_vec();
    assert_eq!(got, preview);
    // Twice the size (24 in), turned a quarter: the image's width now runs across the 6 in
    // width of the board, a quarter of a repeat.
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for &i in &verts {
        lo = lo.min(m2.uvs[2 * i]);
        hi = hi.max(m2.uvs[2 * i]);
    }
    assert!(((hi - lo) - 0.25).abs() < 1e-4, "{lo}..{hi}");
    // Scale out of range, NaN: refused.
    let msg = refused(
        &mut d,
        Op::SetFaceUv {
            object: b,
            face: 2,
            uv: Some(UvXform {
                scale: [0.0, 1.0],
                ..Default::default()
            }),
        },
    );
    assert!(msg.contains("scale"), "{msg}");
    refused(
        &mut d,
        Op::SetFaceUv {
            object: b,
            face: 99,
            uv: None,
        },
    );
    // Back to the material's own placement.
    ok(
        &mut d,
        Op::SetFaceUv {
            object: b,
            face: 2,
            uv: None,
        },
    );
    assert!(d.model().object(b).unwrap().face_paint().is_empty());
    // A texture named in a material but not held: no coordinates, the colour shows.
    ok(
        &mut d,
        Op::DefineMaterial {
            name: "Old".into(),
            color: [10, 20, 30, 255],
            texture: Some("walnut.jpg".into()),
            size: None,
            rotation: 0.0,
            offset: [0.0, 0.0],
            stock: None,
        },
    );
    ok(&mut d, paint_items(&[Item::Object(b)], Some("Old")));
    assert!(face_uvs(d.model(), b, 2, &UvXform::default()).is_none());
}

#[test]
fn texture_refusals() {
    let mut d = Document::with_ground_sketch();
    for (path, mime, data) in [
        ("oak.png", "image/png", tiny_png(2, 2, [1, 2, 3])),
        ("assets/../oak.png", "image/png", tiny_png(2, 2, [1, 2, 3])),
        ("assets/oak.png", "image/jpeg", tiny_png(2, 2, [1, 2, 3])),
        ("assets/oak.png", "image/png", b"not an image".to_vec()),
        ("assets/oak.png", "image/png", vec![]),
    ] {
        refused(
            &mut d,
            Op::AddTexture {
                path: path.into(),
                mime: mime.into(),
                data,
            },
        );
    }
    refused(&mut d, paint_items(&[Item::Object(1)], Some("Nope")));
    refused(
        &mut d,
        Op::DefineMaterial {
            name: "Huge".into(),
            color: [0; 4],
            texture: None,
            size: Some([f64::NAN, 1.0]),
            rotation: 0.0,
            offset: [0.0, 0.0],
            stock: None,
        },
    );
    refused(
        &mut d,
        Op::SetShadows {
            shadows: Some(Shadows {
                month: 13,
                ..Default::default()
            }),
        },
    );
    // Adding the same image twice under one path changes nothing the second time.
    ok(&mut d, add_texture("assets/oak.png"));
    let h = d.hash();
    ok(&mut d, add_texture("assets/oak.png"));
    assert_eq!(d.hash(), h);
}

#[test]
fn renaming_and_deleting_a_material_reach_everything() {
    let mut d = Document::with_ground_sketch();
    ok(&mut d, define("Oak", [200, 170, 120], None));
    ok(&mut d, define("Blue", [40, 60, 160], None));
    let b = board(&mut d, 0.0, 0.0, 0.0, 10.0, 4.0, 1.0);
    ok(&mut d, paint_faces(b, &[2], Some("Oak")));
    let OpOutcome::ComponentCreated { instance, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(b)],
            name: "Box".into(),
        },
    ) else {
        panic!()
    };
    ok(&mut d, paint_items(&[Item::Node(instance)], Some("Oak")));
    ok(
        &mut d,
        Op::RenameMaterial {
            name: "Oak".into(),
            to: "White oak".into(),
        },
    );
    assert_eq!(shows(&d, b, 2).as_deref(), Some("White oak"));
    assert_eq!(shows(&d, b, 3).as_deref(), Some("White oak"));
    let msg = refused(
        &mut d,
        Op::RenameMaterial {
            name: "White oak".into(),
            to: "Blue".into(),
        },
    );
    assert!(msg.contains("already exists"), "{msg}");
    ok(
        &mut d,
        Op::DeleteMaterial {
            name: "White oak".into(),
        },
    );
    assert_eq!(shows(&d, b, 2), None);
    assert_eq!(shows(&d, b, 3), None);
    assert!(d.model().object(b).unwrap().face_paint().is_empty());
}

#[test]
fn renaming_a_material_carries_its_nesting_stock() {
    let mut d = Document::with_ground_sketch();
    ok(&mut d, define("Ply", [220, 200, 160], None));
    ok(
        &mut d,
        define(
            "Maple",
            [230, 210, 170],
            Some((StockKind::Lumber, "Hard maple")),
        ),
    );
    let set = |material: &str| Op::SetMaterialStock {
        material: material.into(),
        stock: Some("4 x 8 sheet, 1/2".into()),
    };
    ok(&mut d, set("Ply"));
    ok(&mut d, set("Hard maple"));
    let rename = |name: &str, to: &str| Op::RenameMaterial {
        name: name.into(),
        to: to.into(),
    };
    // Read by its own name: the default moves with it.
    ok(&mut d, rename("Ply", "Birch ply"));
    let lib = d.model().stock_library().into_owned();
    assert_eq!(lib.default_for("Birch ply"), Some("4 x 8 sheet, 1/2"));
    assert_eq!(lib.default_for("Ply"), None);
    // Read by its species: the default stays with the species.
    ok(&mut d, rename("Maple", "Maple top"));
    let lib = d.model().stock_library().into_owned();
    assert_eq!(lib.default_for("Hard maple"), Some("4 x 8 sheet, 1/2"));
    assert_eq!(lib.default_for("Maple top"), None);
    // One undo puts the old name and its default back.
    d.undo().unwrap();
    d.undo().unwrap();
    let lib = d.model().stock_library().into_owned();
    assert_eq!(lib.default_for("Ply"), Some("4 x 8 sheet, 1/2"));
    assert_eq!(lib.default_for("Birch ply"), None);
}

#[test]
fn locked_things_are_not_painted() {
    let mut d = Document::with_ground_sketch();
    ok(&mut d, define("Oak", [200, 170, 120], None));
    let b = board(&mut d, 0.0, 0.0, 0.0, 10.0, 4.0, 1.0);
    ok(
        &mut d,
        Op::SetLocked {
            items: vec![Item::Object(b)],
            locked: true,
        },
    );
    let msg = refused(&mut d, paint_faces(b, &[2], Some("Oak")));
    assert!(msg.contains("locked"), "{msg}");
    refused(&mut d, paint_items(&[Item::Object(b)], Some("Oak")));
}

fn workshop() -> Style {
    Style {
        preset: Some("Workshop".into()),
        face: FaceStyle::HiddenLine,
        edges: EdgeStyle {
            edges: true,
            profiles: true,
            extensions: true,
            endpoints: false,
        },
        background: Some([250, 248, 240]),
        sky: Some([200, 220, 240]),
        ground: None,
        ..Style::default()
    }
}

#[test]
fn styles_and_shadows_live_in_the_document_and_in_scenes() {
    let mut d = Document::with_ground_sketch();
    let h0 = d.hash();
    // The default style is no style: nothing changes the hash.
    ok(
        &mut d,
        Op::SetStyle {
            style: Some(Style::default()),
        },
    );
    assert_eq!(d.hash(), h0);
    ok(
        &mut d,
        Op::SetStyle {
            style: Some(workshop()),
        },
    );
    let sun = Shadows {
        on: true,
        minutes: 15 * 60,
        ..Default::default()
    };
    ok(&mut d, Op::SetShadows { shadows: Some(sun) });
    assert_eq!(d.model().style(), Some(&workshop()));
    ok(
        &mut d,
        Op::AddScene {
            scene: SceneDef {
                name: "Afternoon".into(),
                eye: [3.0, -3.0, 2.0],
                target: [0.0; 3],
                up: [0.0, 0.0, 1.0],
                fov: 35.0,
                parallel: false,
                hidden_tags: vec![],
                context: None,
                style: Some(workshop()),
                shadows: Some(sun),
                sections: None,
            },
        },
    );
    ok(&mut d, Op::SetStyle { style: None });
    ok(&mut d, Op::SetShadows { shadows: None });
    assert_eq!(d.model().style(), None);
    ok(
        &mut d,
        Op::ActivateScene {
            name: "Afternoon".into(),
        },
    );
    assert_eq!(d.model().style(), Some(&workshop()));
    assert_eq!(d.model().shadows(), Some(&sun));
    let back = sawhorse_kernel::Model::from_snapshot(&d.model().snapshot()).unwrap();
    assert_eq!(back.hash(), d.model().hash());
    // The sun at 3 pm is in the west, above the horizon.
    let s = sun.sun_direction();
    assert!(s[0] < 0.0 && s[2] > 0.0, "{s:?}");
}

#[test]
fn hashes_only_change_when_looks_are_used() {
    // A model with plain colour materials hashes as before lane 10 (the goldens check the
    // rest); anything new changes the hash.
    let mut d = Document::with_ground_sketch();
    let b = board(&mut d, 0.0, 0.0, 0.0, 10.0, 4.0, 1.0);
    ok(&mut d, define("Oak", [200, 170, 120], None));
    ok(
        &mut d,
        Op::SetMaterial {
            object: b,
            material: Some("Oak".into()),
        },
    );
    let plain = d.hash();
    // Painting the solid through the new op is the same state as SetMaterial.
    ok(&mut d, paint_items(&[Item::Object(b)], Some("Oak")));
    assert_eq!(d.hash(), plain);
    ok(&mut d, paint_faces(b, &[2], Some("Oak")));
    assert_ne!(d.hash(), plain, "face paint is hashed");
}

/// The ops of `tests/golden/painted.json`: a two-part cabinet as components, library-style
/// materials with a tiny texture and stock links, face paint inside an open component, an
/// instance painted on its own, a texture positioned, a split and an imprint on painted
/// faces, a push, a rename, a style and shadows saved in a scene, undo and redo.
fn painted_ops() -> Vec<Op> {
    let mut d = Document::with_ground_sketch();
    let side = board(&mut d, 0.0, 0.0, 0.0, 0.75, 11.25, 30.0);
    let OpOutcome::ComponentCreated { instance: s1, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(side)],
            name: "Side".into(),
        },
    ) else {
        panic!()
    };
    let OpOutcome::Copied { nodes, .. } = ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![s1],
            xform: Xform::translation(DVec3::new(23.25 * IN, 0.0, 0.0)),
        },
    ) else {
        panic!()
    };
    let s2 = nodes[0];
    let shelf = board(&mut d, 0.75, 0.0, 10.0, 22.5, 11.0, 0.75);
    let back = board(&mut d, 0.75, 11.0, 0.0, 22.5, 0.25, 30.0);
    ok(&mut d, add_texture("assets/oak.png"));
    ok(
        &mut d,
        textured("White oak", "assets/oak.png", [24.0, 12.0]),
    );
    ok(
        &mut d,
        define(
            "Birch ply",
            [232, 208, 170],
            Some((StockKind::Sheet, "1/4 Baltic birch ply")),
        ),
    );
    ok(&mut d, define("Barn red", [124, 38, 32], None));
    // Paint every face of the side inside its component: both sides follow.
    ok(&mut d, Op::SetContext { node: Some(s1) });
    let faces = d.model().object(side).unwrap().brep().persistent_face_ids();
    ok(&mut d, paint_faces(side, &faces, Some("White oak")));
    ok(&mut d, Op::LeaveContext);
    // The second side painted red on its own (only faces without paint would show it).
    ok(&mut d, paint_items(&[Item::Node(s2)], Some("Barn red")));
    ok(
        &mut d,
        paint_items(&[Item::Object(shelf)], Some("White oak")),
    );
    ok(
        &mut d,
        paint_items(&[Item::Object(back)], Some("Birch ply")),
    );
    ok(
        &mut d,
        Op::SetFaceUv {
            object: shelf,
            face: 2,
            uv: Some(UvXform {
                scale: [1.0, 1.0],
                rotation: 15.0,
                offset: [1.0 * IN, 0.5 * IN],
            }),
        },
    );
    ok(&mut d, paint_faces(shelf, &[2], Some("Barn red")));
    let z = 10.75 * IN;
    ok(
        &mut d,
        Op::Draw {
            target: DrawTarget::Face {
                object: shelf,
                face: 2,
            },
            points: vec![[12.0 * IN, 0.0, z], [12.0 * IN, 11.0 * IN, z]],
            closed: false,
        },
    );
    let pts = [[3.0, 3.0], [6.0, 3.0], [6.0, 6.0], [3.0, 6.0]].map(|[x, y]| [x * IN, y * IN, z]);
    let OpOutcome::Drawn {
        inside: Some(pocket),
        ..
    } = ok(
        &mut d,
        Op::Draw {
            target: DrawTarget::Face {
                object: shelf,
                face: 2,
            },
            points: pts.to_vec(),
            closed: true,
        },
    )
    else {
        panic!()
    };
    ok(
        &mut d,
        Op::PushPull {
            object: shelf,
            face: pocket,
            distance: -0.25 * IN,
        },
    );
    ok(
        &mut d,
        Op::RenameMaterial {
            name: "Barn red".into(),
            to: "Red".into(),
        },
    );
    ok(
        &mut d,
        Op::SetStyle {
            style: Some(workshop()),
        },
    );
    ok(
        &mut d,
        Op::SetShadows {
            shadows: Some(Shadows {
                on: true,
                minutes: 15 * 60,
                ..Default::default()
            }),
        },
    );
    let (style, shadows) = (d.model().style().cloned(), d.model().shadows().copied());
    ok(
        &mut d,
        Op::AddScene {
            scene: SceneDef {
                name: "3 pm".into(),
                eye: [1.5, -1.5, 1.2],
                target: [0.3, 0.15, 0.4],
                up: [0.0, 0.0, 1.0],
                fov: 35.0,
                parallel: false,
                hidden_tags: vec![],
                context: None,
                style,
                shadows,
                sections: None,
            },
        },
    );
    ok(&mut d, paint_faces(back, &[2], Some("Red")));
    d.undo().unwrap();
    d.redo().unwrap();
    d.undo().unwrap();
    d.log().to_vec()
}

#[test]
fn the_painted_golden_is_these_ops() {
    let ops = painted_ops();
    let d = Document::replay(&ops).unwrap();
    // What it shows: the sides are oak on every face (face paint beats the red instance),
    // the back sheet goods by its link, the shelf oak with a red top cut in three.
    let l = list(&d);
    let names: Vec<(&str, usize, Option<&str>, bool)> = l
        .rows
        .iter()
        .map(|r| (r.name.as_str(), r.qty, r.material.as_deref(), r.sheet))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Side", 2, Some("White oak"), false),
            ("Solid 3", 1, Some("White oak"), false),
            ("Solid 4", 1, Some("1/4 Baltic birch ply"), true),
        ]
    );
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("painted.json");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let golden: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
    let want = serde_json::to_value(&ops).unwrap();
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") || golden.get("ops") != Some(&want) {
        assert!(
            std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1"),
            "tests/golden/painted.json is not these ops: run with UPDATE_GOLDEN=1 (then the golden test fills in the hashes)"
        );
        let g = serde_json::json!({
            "name": "painted",
            "description": "Lane 10: a cabinet of components painted with materials (a tiny texture, stock links), face paint inside an open component reaching both copies, an instance painted on its own, a texture positioned, a split and a pocket on a painted face, a material renamed, a style and shadows saved in a scene, undo and redo. Replayed through Document::replay and in WASM.",
            "ops": want,
            "hash": golden.get("hash").cloned().unwrap_or(serde_json::json!("")),
            "stats": golden.get("stats").cloned().unwrap_or(serde_json::json!({"objects":0,"faces":0,"edges":0,"vertices":0,"genus":0,"watertight":true})),
        });
        std::fs::write(&path, serde_json::to_string_pretty(&g).unwrap() + "\n").unwrap();
    }
}
