//! Lane 15: soft, smooth and hidden edges. A 24-sided cylinder softened by angle reads as a
//! round surface: its 24 side edges are soft and smooth, its rims stay hard; the flags
//! follow their edges through moves and pushes, save and load, hash only when present, come
//! in from an import, reach every copy of a component, and are one undo step.

use sawhorse_kernel::Document;
use sawhorse_kernel::edges::{EdgeAt, HIDDEN, SMOOTH, SOFT};
use sawhorse_kernel::hash::model_hash;
use sawhorse_kernel::mesh::view_mesh;
use sawhorse_kernel::meshbuild::ImportItem;
use sawhorse_kernel::model::{Model, Op, OpOutcome};

const IN: f64 = 0.0254;
const SIDES: u32 = 24;

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

/// A 24-sided prism, radius 2 in, 1 in tall, as an import item: bottom (index 0..24) and
/// top (24..48) rings, the two caps and 24 side quads. `open` leaves the top cap off (a
/// mesh, not a solid). `flags`: (index, index, flags) to bring in with it.
fn prism(name: &str, open: bool, flags: Vec<[u32; 3]>) -> ImportItem {
    let mut positions = Vec::new();
    for z in [0.0, 1.0] {
        for k in 0..SIDES {
            let a = std::f64::consts::TAU * f64::from(k) / f64::from(SIDES);
            positions.push([2.0 * IN * a.cos(), 2.0 * IN * a.sin(), z * IN]);
        }
    }
    let n = SIDES;
    let mut faces: Vec<Vec<Vec<u32>>> = vec![vec![(0..n).rev().collect()]];
    if !open {
        faces.push(vec![(n..2 * n).collect()]);
    }
    for k in 0..n {
        let j = (k + 1) % n;
        faces.push(vec![vec![k, j, n + j, n + k]]);
    }
    ImportItem {
        name: name.into(),
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
        edge_flags: flags,
        curves: vec![],
    }
}

fn import(items: Vec<ImportItem>) -> Op {
    Op::Import {
        items,
        strokes: vec![],
        materials: vec![],
        scenes: vec![],
        hidden_tags: vec![],
        definitions: vec![],
        nodes: vec![],
    }
}

fn cylinder(d: &mut Document) -> u64 {
    match ok(d, import(vec![prism("Cylinder", false, vec![])])) {
        OpOutcome::Imported { objects, .. } => objects[0],
        other => panic!("{other:?}"),
    }
}

/// (soft, smooth, hidden) edge counts of the whole view.
fn counts(m: &Model) -> (usize, usize, usize) {
    let v = view_mesh(m).unwrap();
    let c = |b: u8| v.edge_flags.iter().filter(|f| **f & b != 0).count();
    (c(SOFT), c(SMOOTH), c(HIDDEN))
}

fn soften(objects: Vec<u64>, meshes: Vec<u64>, angle: f64) -> Op {
    Op::SoftenEdges {
        objects,
        meshes,
        angle,
        soft: true,
        smooth: true,
    }
}

#[test]
fn softening_a_cylinder_by_angle_leaves_the_rims_hard() {
    let mut d = Document::with_ground_sketch();
    let c = cylinder(&mut d);
    let plain = model_hash(d.model());
    let v = view_mesh(d.model()).unwrap();
    assert_eq!(v.edge_flags.len(), 72, "24 bottom, 24 top, 24 side edges");
    assert!(
        v.normals.is_empty(),
        "no smooth edge: flat shading, no normals sent"
    );
    assert_eq!(counts(d.model()), (0, 0, 0));
    // Adjacent sides turn by 15 degrees; the rims by 90.
    match ok(&mut d, soften(vec![c], vec![], 20.0)) {
        OpOutcome::EdgesSet { changed } => assert_eq!(changed, 24),
        other => panic!("{other:?}"),
    }
    assert_eq!(counts(d.model()), (24, 24, 0));
    assert_ne!(model_hash(d.model()), plain, "flags are part of the model");
    // Smooth shading: every side vertex's normal is horizontal and points straight out
    // (the average of its two side faces), not a face's own normal.
    let v = view_mesh(d.model()).unwrap();
    assert_eq!(v.normals.len(), v.positions.len());
    let mut radial = 0;
    for k in 0..v.positions.len() / 3 {
        let p = [
            v.positions[3 * k],
            v.positions[3 * k + 1],
            v.positions[3 * k + 2],
        ];
        let n = [v.normals[3 * k], v.normals[3 * k + 1], v.normals[3 * k + 2]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-5, "unit normals");
        if n[2].abs() < 1e-6 {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            let along = (p[0] * n[0] + p[1] * n[1]) / r;
            assert!(
                along > 1.0 - 1e-5,
                "a side vertex's normal points out from the axis: {along}"
            );
            radial += 1;
        }
    }
    assert_eq!(radial, 24 * 4, "every side quad's four corners");
    // A tighter angle leaves the sides hard again; it is one undo step back to soft.
    ok(&mut d, soften(vec![c], vec![], 10.0));
    assert_eq!(counts(d.model()), (0, 0, 0));
    assert_eq!(
        model_hash(d.model()),
        plain,
        "no flags left: the hash of the plain cylinder"
    );
    d.undo().unwrap();
    assert_eq!(counts(d.model()), (24, 24, 0));
    // Soften only (no smoothing): soft, flat shaded.
    ok(
        &mut d,
        Op::SoftenEdges {
            objects: vec![c],
            meshes: vec![],
            angle: 20.0,
            soft: true,
            smooth: false,
        },
    );
    assert_eq!(counts(d.model()), (24, 0, 0));
    assert!(view_mesh(d.model()).unwrap().normals.is_empty());
}

#[test]
fn the_eraser_flags_one_edge_and_the_flags_follow_it() {
    let mut d = Document::with_ground_sketch();
    let c = cylinder(&mut d);
    let side = |k: u32| {
        let a = std::f64::consts::TAU * f64::from(k) / f64::from(SIDES);
        let (x, y) = (2.0 * IN * a.cos(), 2.0 * IN * a.sin());
        ([x, y, 0.0], [x, y, IN])
    };
    let (a, b) = side(3);
    let at = |a: [f64; 3], b: [f64; 3]| EdgeAt {
        object: Some(c),
        mesh: None,
        a,
        b,
    };
    // Ctrl: soften and smooth. Ends given either way round.
    ok(
        &mut d,
        Op::SetEdgeFlags {
            edges: vec![at(b, a)],
            set: SOFT | SMOOTH,
            clear: 0,
        },
    );
    assert_eq!(counts(d.model()), (1, 1, 0));
    // Shift: hide another; one op, two edges.
    let (p, q) = side(7);
    let (r, s) = side(8);
    ok(
        &mut d,
        Op::SetEdgeFlags {
            edges: vec![at(p, q), at(r, s)],
            set: HIDDEN,
            clear: 0,
        },
    );
    assert_eq!(counts(d.model()), (1, 1, 2));
    // Moving the solid keeps them (the faces keep their ids).
    ok(
        &mut d,
        Op::Transform {
            objects: vec![c],
            nodes: vec![],
            xform: sawhorse_kernel::transform::Xform::translation(glam::DVec3::new(
                10.0 * IN,
                0.0,
                0.0,
            )),
        },
    );
    assert_eq!(counts(d.model()), (1, 1, 2));
    // Ctrl+Shift: unsoften (clear soft and smooth; hidden stays as it is).
    let moved = |p: [f64; 3]| [p[0] + 10.0 * IN, p[1], p[2]];
    ok(
        &mut d,
        Op::SetEdgeFlags {
            edges: vec![at(moved(a), moved(b))],
            set: 0,
            clear: SOFT | SMOOTH,
        },
    );
    assert_eq!(counts(d.model()), (0, 0, 2));
    // Refusals: no such edge; nothing to do; a flag that is not one.
    for bad in [
        Op::SetEdgeFlags {
            edges: vec![at([9.0, 9.0, 9.0], [9.0, 9.0, 8.0])],
            set: SOFT,
            clear: 0,
        },
        Op::SetEdgeFlags {
            edges: vec![],
            set: SOFT,
            clear: 0,
        },
        Op::SetEdgeFlags {
            edges: vec![at(a, b)],
            set: 8,
            clear: 0,
        },
        Op::SetEdgeFlags {
            edges: vec![at(a, b)],
            set: SOFT,
            clear: SOFT,
        },
        soften(vec![c], vec![], 181.0),
        soften(vec![], vec![], 20.0),
    ] {
        let before = model_hash(d.model());
        assert!(d.apply(&bad).is_err(), "{bad:?}");
        assert_eq!(model_hash(d.model()), before);
    }
}

#[test]
fn flags_survive_a_push_save_and_load() {
    let mut d = Document::with_ground_sketch();
    let c = cylinder(&mut d);
    ok(&mut d, soften(vec![c], vec![], 20.0));
    // Pull the top up an inch: the sides grow, keep their ids and so their flags.
    let o = d.model().object(c).unwrap();
    let top = o
        .brep()
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.99)
        .map(|(_, f)| f.id)
        .unwrap();
    ok(
        &mut d,
        Op::PushPull {
            object: c,
            face: top,
            distance: IN,
        },
    );
    assert_eq!(counts(d.model()), (24, 24, 0));
    // Save and load: the same model, hash for hash.
    let snap = d.model().snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    assert!(json.contains("\"edge_flags\""));
    let back = Model::from_snapshot(&serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(model_hash(&back), model_hash(d.model()));
    assert_eq!(counts(&back), (24, 24, 0));
    // A snapshot naming faces that do not meet is refused, not silently changed.
    let mut broken = snap.clone();
    broken.objects[0].brep.as_mut().unwrap().edge_flags = vec![[1, 2, 1]];
    assert!(Model::from_snapshot(&broken).is_err());
    // A plain model's snapshot has no edge_flags at all.
    let plain = Document::with_ground_sketch();
    assert!(
        !serde_json::to_string(&plain.model().snapshot())
            .unwrap()
            .contains("edge_flags")
    );
}

#[test]
fn an_import_brings_soft_edges_onto_solids_and_meshes() {
    let n = SIDES;
    // Every side edge (k, n + k) soft and smooth, as a SketchUp file stores a circle pulled up.
    let flags: Vec<[u32; 3]> = (0..n)
        .map(|k| [k, n + k, u32::from(SOFT | SMOOTH)])
        .collect();
    let mut d = Document::with_ground_sketch();
    let out = ok(
        &mut d,
        import(vec![prism("Slab", false, flags.clone()), {
            let mut m = prism("Tray", true, flags);
            for p in &mut m.positions {
                p[0] += 10.0 * IN;
            }
            m
        }]),
    );
    let OpOutcome::Imported {
        objects, meshes, ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(
        (objects.len(), meshes.len()),
        (1, 1),
        "a solid and an open tray"
    );
    let m = d.model();
    assert_eq!(m.object(objects[0]).unwrap().edge_flags().len(), 24);
    let (_, tray) = m.meshes().into_iter().next().unwrap();
    assert_eq!(tray.edge_flags.len(), 24);
    assert_eq!(counts(m), (48, 48, 0));
    // Smooth shading on the open tray too: its normals come with the view.
    assert!(!view_mesh(m).unwrap().normals.is_empty());
    // Out-of-range flags are a bad import.
    let mut bad = prism("Bad", false, vec![[0, 999, 1]]);
    bad.name = "Bad".into();
    assert!(d.apply(&import(vec![bad])).is_err());
    let bad = prism("Bad", false, vec![[0, 24, 9]]);
    assert!(d.apply(&import(vec![bad])).is_err());
}

#[test]
fn a_softened_component_is_soft_in_every_copy() {
    let mut d = Document::with_ground_sketch();
    let c = cylinder(&mut d);
    let node = match ok(
        &mut d,
        Op::MakeComponent {
            items: vec![sawhorse_kernel::nodes::Item::Object(c)],
            name: "Post".into(),
        },
    ) {
        OpOutcome::ComponentCreated { instance, .. } => instance,
        o => panic!("{o:?}"),
    };
    ok(
        &mut d,
        Op::Array {
            objects: vec![],
            nodes: vec![node],
            xform: sawhorse_kernel::transform::Xform::translation(glam::DVec3::new(
                6.0 * IN,
                0.0,
                0.0,
            )),
            count: 2,
        },
    );
    assert_eq!(counts(d.model()), (0, 0, 0));
    // Open the first copy, soften its solid, close it: all three are soft.
    ok(&mut d, Op::SetContext { node: Some(node) });
    let inside: Vec<u64> = d
        .model()
        .graph()
        .objects
        .iter()
        .filter(|o| o.parent == Some(node))
        .map(|o| o.id)
        .collect();
    assert_eq!(inside.len(), 1);
    ok(&mut d, soften(inside, vec![], 20.0));
    ok(&mut d, Op::LeaveContext);
    assert_eq!(counts(d.model()), (72, 72, 0));
    // Saved and loaded, the copies are still derived from the softened definition.
    let back = Model::from_snapshot(&d.model().snapshot()).unwrap();
    assert_eq!(model_hash(&back), model_hash(d.model()));
    d.undo().unwrap();
    d.undo().unwrap();
    assert_eq!(
        counts(d.model()),
        (0, 0, 0),
        "one undo step for the softening"
    );
}

#[test]
fn obj_and_glb_carry_the_smoothing() {
    let mut d = Document::with_ground_sketch();
    let c = cylinder(&mut d);
    let o = d.model().object(c).unwrap().clone();
    assert!(o.smoothing_groups().is_none());
    ok(&mut d, soften(vec![c], vec![], 20.0));
    let o = d.model().object(c).unwrap();
    let g = o.smoothing_groups().unwrap();
    assert_eq!(
        g.iter().filter(|&&x| x == 1).count(),
        24,
        "the sides: one group"
    );
    assert_eq!(g.iter().filter(|&&x| x == 0).count(), 2, "the caps: off");
}
