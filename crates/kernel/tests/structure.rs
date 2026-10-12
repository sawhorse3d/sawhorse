//! Groups, components, the editing context, tags and scenes (lane 5): the behaviour the
//! UI builds on, op by op. The invariants under random sequences are in
//! `structure_props.rs` and the fuzzer (`sequences.rs`).

use glam::DVec3;
use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome, SceneDef};
use sawhorse_kernel::nodes::{Item, MemberKind, NodeKind};
use sawhorse_kernel::transform::Xform;
use sawhorse_kernel::{Document, KernelError, StructureError};

const IN: f64 = 0.0254;

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

fn refused(d: &mut Document, op: Op) -> KernelError {
    let (h, log, depth) = (d.hash(), d.log().len(), d.depth());
    let e = d.apply(&op).expect_err("should be refused");
    assert!(!e.is_bug(), "{op:?}: {e}");
    assert_eq!(
        (d.hash(), d.log().len(), d.depth()),
        (h, log, depth),
        "a refusal changes nothing"
    );
    e
}

/// A board `w` x `d` (on the ground, from (x, y)) and `h` high, drawn and pulled up in the
/// open context. Returns its object id.
fn board(doc: &mut Document, x: f64, y: f64, z: f64, w: f64, d: f64, h: f64) -> u64 {
    let out = ok(
        doc,
        Op::Draw {
            target: DrawTarget::Plane {
                origin: [0.0, 0.0, z],
                normal: [0.0, 0.0, 1.0],
            },
            points: vec![[x, y, z], [x + w, y, z], [x + w, y + d, z], [x, y + d, z]],
            closed: true,
        },
    );
    let OpOutcome::Drawn {
        sketch: Some(sketch),
        inside: Some(region),
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    match ok(
        doc,
        Op::Extrude {
            sketch,
            region,
            distance: h,
        },
    ) {
        OpOutcome::ObjectCreated { object } => object,
        o => panic!("{o:?}"),
    }
}

/// The persistent id of the face of `object` facing `dir`.
fn face_towards(m: &Model, object: u64, dir: [f64; 3]) -> u64 {
    let o = m.object(object).unwrap();
    let p = o.brep().to_polys();
    let d = DVec3::from_array(dir);
    let fi = (0..p.faces.len())
        .find(|&i| p.plane(i).is_some_and(|pl| pl.normal.dot(d) > 0.999))
        .expect("a face that way");
    o.face_id_at(fi).unwrap()
}

fn size(m: &Model, object: u64) -> DVec3 {
    let (lo, hi) = m.object(object).unwrap().bounds();
    hi - lo
}

fn min(m: &Model, object: u64) -> DVec3 {
    m.object(object).unwrap().bounds().0
}

fn near(a: DVec3, b: DVec3) {
    assert!(a.distance(b) < 1e-9, "{a} vs {b}");
}

/// Solids of an instance (or group), in creation order.
fn solids_in(m: &Model, node: u64) -> Vec<u64> {
    let g = m.graph();
    g.objects
        .iter()
        .filter(|o| {
            let mut at = o.parent;
            while let Some(p) = at {
                if p == node {
                    return true;
                }
                at = g.nodes.iter().find(|n| n.id == p).and_then(|n| n.parent);
            }
            false
        })
        .map(|o| o.id)
        .collect()
}

fn instances(m: &Model) -> Vec<u64> {
    m.nodes()
        .into_iter()
        .filter(|(_, n)| matches!(n.kind, NodeKind::Instance(_)))
        .map(|(id, _)| id)
        .collect()
}

fn move_by(x: f64, y: f64, z: f64) -> Xform {
    Xform::translation(DVec3::new(x, y, z))
}

#[test]
fn a_bookshelf_from_components_edits_every_copy_at_once() {
    let mut d = Document::with_ground_sketch();
    // Side: 3/4 x 11 1/4 x 72 in, made a component, copied 34 1/4 in along red.
    let side = board(&mut d, 0.0, 0.0, 0.0, 0.75 * IN, 11.25 * IN, 72.0 * IN);
    let OpOutcome::ComponentCreated {
        instance: side_a,
        definition: side_def,
    } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(side)],
            name: "Side".into(),
        },
    )
    else {
        panic!()
    };
    assert_eq!(
        d.model().definition(side_def).unwrap().name,
        "Side",
        "the definition is named"
    );
    let OpOutcome::Copied { objects, nodes } = ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![side_a],
            xform: move_by(34.25 * IN, 0.0, 0.0),
        },
    ) else {
        panic!()
    };
    assert!(objects.is_empty(), "copying an instance makes an instance");
    let side_b = nodes[0];
    assert_eq!(
        d.model().node(side_b).unwrap().kind,
        d.model().node(side_a).unwrap().kind,
        "same definition"
    );
    // Shelf: 33 1/2 x 11 1/4 x 3/4, one component, then a Move array of 3 more.
    let shelf = board(
        &mut d,
        0.75 * IN,
        0.0,
        0.0,
        33.5 * IN,
        11.25 * IN,
        0.75 * IN,
    );
    let OpOutcome::ComponentCreated {
        instance: shelf_a, ..
    } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(shelf)],
            name: "Shelf".into(),
        },
    )
    else {
        panic!()
    };
    let OpOutcome::Copied { nodes: more, .. } = ok(
        &mut d,
        Op::Array {
            objects: vec![],
            nodes: vec![shelf_a],
            xform: move_by(0.0, 0.0, 23.75 * IN),
            count: 3,
        },
    ) else {
        panic!()
    };
    assert_eq!(more.len(), 3);
    assert_eq!(instances(d.model()).len(), 6);
    let graph = d.model().graph();
    let shelf_def = graph
        .definitions
        .iter()
        .find(|x| x.name == "Shelf")
        .unwrap();
    assert_eq!(shelf_def.instances, 4);
    assert!((shelf_def.max[2] - shelf_def.min[2] - 0.75 * IN).abs() < 1e-12);

    // A solid of a closed component cannot be edited from outside.
    let top_shelf = solids_in(d.model(), more[2])[0];
    let up = face_towards(d.model(), top_shelf, [0.0, 0.0, 1.0]);
    let e = refused(
        &mut d,
        Op::PushPull {
            object: top_shelf,
            face: up,
            distance: 0.25 * IN,
        },
    );
    assert_eq!(e, KernelError::Structure(StructureError::OutsideContext));

    // Open one shelf and make it 1 in thick: all four follow.
    let depth = d.depth();
    ok(
        &mut d,
        Op::SetContext {
            node: Some(more[1]),
        },
    );
    assert_eq!(d.depth(), depth, "opening a component is not an undo step");
    let s = solids_in(d.model(), more[1])[0];
    let up = face_towards(d.model(), s, [0.0, 0.0, 1.0]);
    ok(
        &mut d,
        Op::PushPull {
            object: s,
            face: up,
            distance: 0.25 * IN,
        },
    );
    for &i in std::iter::once(&shelf_a).chain(&more) {
        let o = solids_in(d.model(), i)[0];
        assert!(
            (size(d.model(), o).z - IN).abs() < 1e-9,
            "every shelf is 1 in thick"
        );
    }
    near(
        min(d.model(), solids_in(d.model(), more[2])[0]),
        DVec3::new(0.75 * IN, 0.0, 3.0 * 23.75 * IN),
    );
    // The sides did not change.
    assert!((size(d.model(), solids_in(d.model(), side_b)[0]).z - 72.0 * IN).abs() < 1e-9);
    ok(&mut d, Op::LeaveContext);

    // Make Unique on one shelf, change it back to 3/4: only that one changes.
    let OpOutcome::MadeUnique { definition } = ok(&mut d, Op::MakeUnique { node: more[0] }) else {
        panic!()
    };
    assert_eq!(d.model().definition(definition).unwrap().name, "Shelf (2)");
    ok(
        &mut d,
        Op::SetContext {
            node: Some(more[0]),
        },
    );
    let s = solids_in(d.model(), more[0])[0];
    let up = face_towards(d.model(), s, [0.0, 0.0, 1.0]);
    ok(
        &mut d,
        Op::PushPull {
            object: s,
            face: up,
            distance: -0.25 * IN,
        },
    );
    ok(&mut d, Op::LeaveContext);
    let thick = |d: &Document, i: u64| size(d.model(), solids_in(d.model(), i)[0]).z;
    assert!((thick(&d, more[0]) - 0.75 * IN).abs() < 1e-9);
    for &i in [shelf_a, more[1], more[2]].iter() {
        assert!((thick(&d, i) - IN).abs() < 1e-9);
    }
    // Undo walks back through the edits, each into the context it was made in.
    d.undo().unwrap();
    assert_eq!(d.model().context(), Some(more[0]));
    assert!((thick(&d, more[0]) - IN).abs() < 1e-9);
    let h = d.hash();
    d.redo().unwrap();
    d.undo().unwrap();
    assert_eq!(d.hash(), h);
    // The whole history replays to the same state.
    let r = Document::replay(d.log()).unwrap();
    assert_eq!(r.hash(), d.hash());
}

#[test]
fn groups_select_as_one_and_open_for_editing() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 2.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let before = d.hash();
    let ids_before: Vec<Vec<u64>> = [a, b]
        .iter()
        .map(|&o| {
            let ob = d.model().object(o).unwrap();
            (0..ob.brep().face_count())
                .map(|i| ob.face_id_at(i).unwrap())
                .collect()
        })
        .collect();
    let OpOutcome::GroupCreated { group } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(a), Item::Object(b)],
            name: Some("Pair".into()),
        },
    ) else {
        panic!()
    };
    // Moving the group moves both; face ids do not change; the group's axes follow.
    ok(
        &mut d,
        Op::Transform {
            objects: vec![],
            nodes: vec![group],
            xform: move_by(0.0, 3.0, 0.0),
        },
    );
    near(min(d.model(), b), DVec3::new(2.0, 3.0, 0.0));
    assert_eq!(
        d.model().node(group).unwrap().xform.translation,
        [0.0, 3.0, 0.0]
    );
    for (k, &o) in [a, b].iter().enumerate() {
        let ob = d.model().object(o).unwrap();
        let ids: Vec<u64> = (0..ob.brep().face_count())
            .map(|i| ob.face_id_at(i).unwrap())
            .collect();
        assert_eq!(ids, ids_before[k]);
    }
    // Members are edited inside; the group's own solids are what the context offers.
    let up = face_towards(d.model(), a, [0.0, 0.0, 1.0]);
    refused(
        &mut d,
        Op::PushPull {
            object: a,
            face: up,
            distance: 0.1,
        },
    );
    ok(&mut d, Op::SetContext { node: Some(group) });
    assert_eq!(d.model().context_objects(), vec![a, b]);
    ok(
        &mut d,
        Op::PushPull {
            object: a,
            face: up,
            distance: 0.1,
        },
    );
    ok(&mut d, Op::LeaveContext);
    assert_eq!(
        refused(&mut d, Op::LeaveContext),
        KernelError::Structure(StructureError::NothingToLeave)
    );
    // Undo back to before the group: the hash is exactly the starting one.
    while d.hash() != before && d.can_undo() {
        d.undo().unwrap();
    }
    assert_eq!(d.hash(), before);
}

#[test]
fn group_then_explode_is_the_identity_on_hashes() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 2.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let c = board(&mut d, 4.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let before = d.hash();
    let OpOutcome::GroupCreated { group: inner } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(a), Item::Object(b)],
            name: None,
        },
    ) else {
        panic!()
    };
    let OpOutcome::GroupCreated { group: outer } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Node(inner), Item::Object(c)],
            name: Some("Outer".into()),
        },
    ) else {
        panic!()
    };
    assert_ne!(d.hash(), before);
    ok(&mut d, Op::Explode { node: outer });
    ok(&mut d, Op::Explode { node: inner });
    assert_eq!(
        d.hash(),
        before,
        "group then explode leaves the model as it was"
    );
    // A component made and exploded leaves the solids exactly as they were too.
    let OpOutcome::ComponentCreated { instance, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(a), Item::Object(c)],
            name: "Pair".into(),
        },
    ) else {
        panic!()
    };
    let OpOutcome::Exploded { objects, .. } = ok(&mut d, Op::Explode { node: instance }) else {
        panic!()
    };
    assert_eq!(objects, vec![a, c], "the same solids, the same ids");
    assert_eq!(d.hash(), before);
    assert!(
        d.model().definitions().is_empty(),
        "the unused definition is purged"
    );
}

#[test]
fn strokes_stick_only_within_their_editing_context() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let OpOutcome::GroupCreated { group } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(a)],
            name: None,
        },
    ) else {
        panic!()
    };
    // A rectangle on the ground at the top level.
    ok(
        &mut d,
        Op::AddRectangle {
            sketch: 1,
            x: 2.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
    );
    let root_regions = d.model().sketch(1).unwrap().region_count();
    // Inside the group: the same ground plane, an overlapping rectangle: a new sketch of the
    // group's own, so nothing splits the top-level rectangle.
    ok(&mut d, Op::SetContext { node: Some(group) });
    let out = ok(
        &mut d,
        Op::Draw {
            target: DrawTarget::Sketch { sketch: 1 },
            points: vec![
                [2.5, 0.5, 0.0],
                [3.5, 0.5, 0.0],
                [3.5, 1.5, 0.0],
                [2.5, 1.5, 0.0],
            ],
            closed: true,
        },
    );
    let OpOutcome::Drawn {
        sketch: Some(s), ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_ne!(s, 1, "the group's own sketch");
    assert_eq!(d.model().sketch_parent(s), Some(group));
    assert_eq!(d.model().sketch(1).unwrap().region_count(), root_regions);
    assert_eq!(d.model().sketch(s).unwrap().region_count(), 1);
    // The top-level sketch cannot be edited from inside the group.
    refused(
        &mut d,
        Op::AddRectangle {
            sketch: 1,
            x: 5.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
    );
    // Extruding the group's region makes a solid in the group.
    let region = d
        .model()
        .sketch(s)
        .unwrap()
        .regions()
        .next()
        .map(|(k, _)| sawhorse_kernel::ids::to_public(k))
        .unwrap();
    let OpOutcome::ObjectCreated { object } = ok(
        &mut d,
        Op::Extrude {
            sketch: s,
            region,
            distance: 0.2,
        },
    ) else {
        panic!()
    };
    assert_eq!(
        d.model().object(object).unwrap().membership().parent(),
        Some(group)
    );
}

#[test]
fn a_cut_that_severs_a_shared_component_makes_that_copy_unique() {
    let mut d = Document::with_ground_sketch();
    let o = board(&mut d, 0.0, 0.0, 0.0, 4.0, 1.0, 0.5);
    let OpOutcome::ComponentCreated { instance: a, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(o)],
            name: "Rail".into(),
        },
    ) else {
        panic!()
    };
    let OpOutcome::Copied { nodes, .. } = ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![a],
            xform: move_by(0.0, 2.0, 0.0),
        },
    ) else {
        panic!()
    };
    let b = nodes[0];
    let b_solid = solids_in(d.model(), b)[0];
    // Inside `a`: a strip across the top (drawn in both copies: it is the definition)...
    ok(&mut d, Op::SetContext { node: Some(a) });
    let s = solids_in(d.model(), a)[0];
    let top = face_towards(d.model(), s, [0.0, 0.0, 1.0]);
    let out = ok(
        &mut d,
        Op::Draw {
            target: DrawTarget::Face {
                object: s,
                face: top,
            },
            points: vec![
                [1.9, 0.0, 0.5],
                [2.1, 0.0, 0.5],
                [2.1, 1.0, 0.5],
                [1.9, 1.0, 0.5],
            ],
            closed: true,
        },
    );
    let OpOutcome::Drawn {
        inside: Some(strip),
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(
        d.model().object(b_solid).unwrap().brep().face_count(),
        d.model().object(s).unwrap().brep().face_count(),
        "the strip is in the other copy too"
    );
    let b_before = sawhorse_kernel::hash::object_hash(d.model().object(b_solid).unwrap());
    let drawn = d.hash();
    // ...pushed down through the board: it comes apart, and only this copy changes.
    let out = ok(
        &mut d,
        Op::PushPull {
            object: s,
            face: strip,
            distance: -0.5,
        },
    );
    assert!(
        matches!(out, OpOutcome::Severed { ref objects } if objects.len() == 2),
        "{out:?}"
    );
    // `a` now has its own definition with both pieces; `b` is untouched.
    let ka = d.model().node(a).unwrap().kind;
    let kb = d.model().node(b).unwrap().kind;
    assert_ne!(ka, kb, "the severed copy was made unique");
    assert_eq!(solids_in(d.model(), a).len(), 2);
    assert_eq!(solids_in(d.model(), b), vec![b_solid]);
    assert_eq!(
        sawhorse_kernel::hash::object_hash(d.model().object(b_solid).unwrap()),
        b_before
    );
    d.undo().unwrap();
    assert_eq!(d.hash(), drawn, "undo joins them again");
}

#[test]
fn nested_groups_and_instances_propagate_through_definitions() {
    let mut d = Document::with_ground_sketch();
    // A drawer: a box and a knob; the knob a component of its own, then the drawer.
    let body = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.25);
    let knob = board(&mut d, 0.45, -0.1, 0.1, 0.1, 0.1, 0.05);
    let OpOutcome::ComponentCreated { instance: k, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(knob)],
            name: "Knob".into(),
        },
    ) else {
        panic!()
    };
    let OpOutcome::GroupCreated { group: g } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(body)],
            name: Some("Box".into()),
        },
    ) else {
        panic!()
    };
    let OpOutcome::ComponentCreated {
        instance: drawer,
        definition,
    } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Node(g), Item::Node(k)],
            name: "Drawer".into(),
        },
    )
    else {
        panic!()
    };
    let def = d.model().definition(definition).unwrap();
    assert_eq!(def.members.len(), 3, "group, its solid, the knob instance");
    assert!(
        def.members
            .values()
            .any(|m| matches!(m.kind, MemberKind::Instance(..)))
    );
    // Stack two more drawers above.
    ok(
        &mut d,
        Op::Array {
            objects: vec![],
            nodes: vec![drawer],
            xform: move_by(0.0, 0.0, 0.3),
            count: 2,
        },
    );
    assert_eq!(
        d.model()
            .graph()
            .nodes
            .iter()
            .filter(|n| n.label == "Knob")
            .count(),
        3
    );
    // Open the top drawer's knob and make it longer: every knob follows.
    let knobs: Vec<u64> = d
        .model()
        .graph()
        .nodes
        .iter()
        .filter(|n| n.label == "Knob")
        .map(|n| n.id)
        .collect();
    ok(
        &mut d,
        Op::SetContext {
            node: Some(knobs[2]),
        },
    );
    let s = solids_in(d.model(), knobs[2])[0];
    let front = face_towards(d.model(), s, [0.0, -1.0, 0.0]);
    ok(
        &mut d,
        Op::PushPull {
            object: s,
            face: front,
            distance: 0.05,
        },
    );
    for &n in &knobs {
        let o = solids_in(d.model(), n)[0];
        assert!((size(d.model(), o).y - 0.15).abs() < 1e-9);
    }
    // Into the drawer's group (inside the drawer): its box edit spreads to all drawers.
    ok(&mut d, Op::SetContext { node: None });
    let boxes: Vec<u64> = d
        .model()
        .graph()
        .nodes
        .iter()
        .filter(|n| n.label == "Box")
        .map(|n| n.id)
        .collect();
    assert_eq!(boxes.len(), 3);
    ok(
        &mut d,
        Op::SetContext {
            node: Some(boxes[1]),
        },
    );
    let s = solids_in(d.model(), boxes[1])[0];
    let up = face_towards(d.model(), s, [0.0, 0.0, 1.0]);
    ok(
        &mut d,
        Op::PushPull {
            object: s,
            face: up,
            distance: -0.05,
        },
    );
    for &b in &boxes {
        let o = solids_in(d.model(), b)[0];
        assert!((size(d.model(), o).z - 0.2).abs() < 1e-9);
    }
}

#[test]
fn tags_hide_and_show_without_moving_anything() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 2.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let OpOutcome::GroupCreated { group } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(a)],
            name: Some("Carcass".into()),
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::CreateTag {
            name: "Carcass".into(),
            color: Some([196, 138, 42]),
        },
    );
    assert_eq!(
        refused(
            &mut d,
            Op::CreateTag {
                name: "Carcass".into(),
                color: None
            }
        ),
        KernelError::Structure(StructureError::TagExists("Carcass".into()))
    );
    refused(
        &mut d,
        Op::CreateTag {
            name: "Untagged".into(),
            color: None,
        },
    );
    ok(
        &mut d,
        Op::TagItems {
            items: vec![Item::Node(group)],
            tag: Some("Carcass".into()),
        },
    );
    let at = min(d.model(), a);
    ok(
        &mut d,
        Op::SetTagVisible {
            tag: "Carcass".into(),
            visible: false,
        },
    );
    assert!(
        !d.model().is_visible(a),
        "a hidden tag hides what is inside"
    );
    assert!(d.model().is_visible(b));
    near(min(d.model(), a), at);
    // Rename follows everywhere, colour too; scenes keep the reference.
    ok(
        &mut d,
        Op::AddScene {
            scene: SceneDef {
                name: "Front".into(),
                eye: [0.0, -5.0, 1.0],
                target: [0.0, 0.0, 0.5],
                up: [0.0, 0.0, 1.0],
                fov: 35.0,
                parallel: true,
                hidden_tags: vec!["Carcass".into()],
                context: None,
            },
        },
    );
    ok(
        &mut d,
        Op::RenameTag {
            tag: "Carcass".into(),
            name: "Case".into(),
        },
    );
    assert_eq!(d.model().node(group).unwrap().tag.as_deref(), Some("Case"));
    assert_eq!(d.model().tag_colors()["Case"], [196, 138, 42]);
    assert_eq!(d.model().scenes()[0].hidden_tags, vec!["Case".to_string()]);
    // Deleting the tag leaves the group untagged, shown, where it was.
    ok(&mut d, Op::DeleteTag { tag: "Case".into() });
    assert_eq!(d.model().node(group).unwrap().tag, None);
    assert!(d.model().is_visible(a));
    assert!(d.model().scenes()[0].hidden_tags.is_empty());
}

#[test]
fn scenes_switch_tag_visibility_and_context() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 2.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    ok(
        &mut d,
        Op::TagItems {
            items: vec![Item::Object(a)],
            tag: Some("Carcass".into()),
        },
    );
    ok(
        &mut d,
        Op::TagItems {
            items: vec![Item::Object(b)],
            tag: Some("Shelves".into()),
        },
    );
    let OpOutcome::GroupCreated { group } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(b)],
            name: None,
        },
    ) else {
        panic!()
    };
    let scene = |name: &str, hidden: &[&str], context: Option<u64>| SceneDef {
        name: name.into(),
        eye: [3.0, -3.0, 3.0],
        target: [0.0; 3],
        up: [0.0, 0.0, 1.0],
        fov: 35.0,
        parallel: false,
        hidden_tags: hidden.iter().map(|s| s.to_string()).collect(),
        context,
    };
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Front", &["Shelves"], None),
        },
    );
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Iso", &[], Some(group)),
        },
    );
    refused(
        &mut d,
        Op::AddScene {
            scene: scene("Iso", &[], None),
        },
    );
    ok(
        &mut d,
        Op::ActivateScene {
            name: "Front".into(),
        },
    );
    assert!(!d.model().is_visible(b));
    assert!(d.model().is_visible(a));
    assert_eq!(d.model().context(), None);
    ok(&mut d, Op::ActivateScene { name: "Iso".into() });
    assert!(d.model().is_visible(b));
    assert_eq!(d.model().context(), Some(group));
    // Update: Iso now hides the carcass too.
    ok(
        &mut d,
        Op::UpdateScene {
            name: "Iso".into(),
            scene: scene("Iso", &["Carcass"], None),
        },
    );
    ok(&mut d, Op::ActivateScene { name: "Iso".into() });
    assert!(!d.model().is_visible(a));
    ok(
        &mut d,
        Op::DeleteScene {
            name: "Front".into(),
        },
    );
    assert_eq!(d.model().scenes().len(), 1);
    refused(
        &mut d,
        Op::ActivateScene {
            name: "Front".into(),
        },
    );
}

#[test]
fn hidden_and_locked_items() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let OpOutcome::GroupCreated { group } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(a)],
            name: None,
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::SetLocked {
            items: vec![Item::Node(group)],
            locked: true,
        },
    );
    for op in [
        Op::SetContext { node: Some(group) },
        Op::Explode { node: group },
        Op::DeleteNode { node: group },
        Op::Transform {
            objects: vec![],
            nodes: vec![group],
            xform: move_by(1.0, 0.0, 0.0),
        },
    ] {
        assert_eq!(
            refused(&mut d, op),
            KernelError::Structure(StructureError::Locked)
        );
    }
    ok(
        &mut d,
        Op::SetLocked {
            items: vec![Item::Node(group)],
            locked: false,
        },
    );
    ok(
        &mut d,
        Op::SetHidden {
            items: vec![Item::Node(group)],
            hidden: true,
        },
    );
    assert!(!d.model().is_visible(a));
    let mesh = sawhorse_kernel::mesh::view_mesh(d.model()).unwrap();
    assert_eq!(mesh.triangle_count(), 0, "a hidden group draws nothing");
    // Locking the open group is refused.
    ok(&mut d, Op::SetContext { node: Some(group) });
    assert_eq!(
        refused(
            &mut d,
            Op::SetLocked {
                items: vec![Item::Node(group)],
                locked: true
            }
        ),
        KernelError::Structure(StructureError::OnContextPath)
    );
}

#[test]
fn rotated_and_mirrored_instances_follow_their_definition() {
    let mut d = Document::with_ground_sketch();
    let o = board(&mut d, 0.0, 0.0, 0.0, 2.0, 1.0, 0.5);
    let OpOutcome::ComponentCreated { instance: a, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(o)],
            name: "Block".into(),
        },
    ) else {
        panic!()
    };
    let turn = Xform::rotation(DVec3::new(5.0, 0.0, 0.0), DVec3::Z, 0.7);
    let mirror = Xform::scaling(DVec3::new(-3.0, 0.0, 0.0), DVec3::new(-1.0, 1.0, 1.0));
    for x in [turn, mirror] {
        ok(
            &mut d,
            Op::Copy {
                objects: vec![],
                nodes: vec![a],
                xform: x,
            },
        );
    }
    ok(&mut d, Op::SetContext { node: Some(a) });
    let s = solids_in(d.model(), a)[0];
    let east = face_towards(d.model(), s, [1.0, 0.0, 0.0]);
    ok(
        &mut d,
        Op::PushPull {
            object: s,
            face: east,
            distance: 0.5,
        },
    );
    for i in instances(d.model()) {
        let o = d.model().object(solids_in(d.model(), i)[0]).unwrap();
        assert!(
            (o.volume() - 1.25).abs() < 1e-9,
            "every copy is 2.5 x 1 x 0.5"
        );
        o.validate().unwrap();
    }
}

#[test]
fn shz_snapshot_stores_definitions_once_and_round_trips() {
    let mut d = Document::with_ground_sketch();
    let o = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let OpOutcome::ComponentCreated { instance: a, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(o)],
            name: "Block".into(),
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::Array {
            objects: vec![],
            nodes: vec![a],
            xform: Xform::rotation(DVec3::new(3.0, 0.0, 0.0), DVec3::Z, 0.5),
            count: 3,
        },
    );
    let g = board(&mut d, 0.0, 4.0, 0.0, 1.0, 1.0, 0.5);
    ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(g)],
            name: Some("Loose".into()),
        },
    );
    ok(
        &mut d,
        Op::CreateTag {
            name: "Jigs".into(),
            color: Some([10, 20, 30]),
        },
    );
    let snap = d.model().snapshot();
    assert_eq!(snap.definitions.len(), 1);
    assert_eq!(
        snap.objects.iter().filter(|o| o.brep.is_some()).count(),
        1,
        "instance solids are not stored, only the definition's"
    );
    let json = serde_json::to_string(&snap).unwrap();
    let back: sawhorse_kernel::snapshot::Snapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back, snap);
    let m = Model::from_snapshot(&back).unwrap();
    assert_eq!(
        m.snapshot(),
        snap,
        "saving what was opened writes the same data"
    );
    // Tampering is refused, never repaired.
    let mut bad = snap.clone();
    bad.objects
        .iter_mut()
        .find(|o| o.slot.is_some())
        .unwrap()
        .slot = Some(99);
    assert!(Model::from_snapshot(&bad).is_err());
    let mut bad = snap;
    bad.nodes[0].definition = Some(5);
    assert!(Model::from_snapshot(&bad).is_err());
}

#[test]
fn make_group_inside_a_component_reaches_every_copy() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let OpOutcome::ComponentCreated { instance: i, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Object(a), Item::Object(b)],
            name: "Pair".into(),
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![i],
            xform: move_by(0.0, 3.0, 0.0),
        },
    );
    ok(&mut d, Op::SetContext { node: Some(i) });
    ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(a)],
            name: Some("Left".into()),
        },
    );
    ok(&mut d, Op::LeaveContext);
    let lefts = d
        .model()
        .graph()
        .nodes
        .iter()
        .filter(|n| n.label == "Left")
        .count();
    assert_eq!(lefts, 2, "the new group is in both copies");
    // Delete one solid inside: gone from both.
    ok(&mut d, Op::SetContext { node: Some(i) });
    ok(&mut d, Op::DeleteObject { object: b });
    ok(&mut d, Op::LeaveContext);
    assert_eq!(d.model().objects().len(), 2);
}

#[test]
fn make_component_on_a_group_makes_the_group_itself_the_instance() {
    // Model › Side › Solid, never Model › Side › Group › Solid.
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 0.75 * IN, 11.25 * IN, 72.0 * IN);
    let b = board(&mut d, 5.0 * IN, 0.0, 0.0, 0.75 * IN, 11.25 * IN, 72.0 * IN);
    let OpOutcome::GroupCreated { group } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(a), Item::Object(b)],
            name: Some("Sides".into()),
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::TagItems {
            items: vec![Item::Node(group)],
            tag: Some("Carcass".into()),
        },
    );
    // Move the group so its axes are not the world's: the definition is in the group's axes.
    ok(
        &mut d,
        Op::Transform {
            objects: vec![],
            nodes: vec![group],
            xform: move_by(10.0 * IN, 0.0, 0.0),
        },
    );
    let before: Vec<DVec3> = [a, b].iter().map(|&o| min(d.model(), o)).collect();
    let OpOutcome::ComponentCreated {
        instance,
        definition,
    } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Node(group)],
            name: "Side pair".into(),
        },
    )
    else {
        panic!()
    };
    assert_eq!(instance, group, "the group itself became the instance");
    let m = d.model();
    assert_eq!(m.nodes().len(), 1, "no wrapper node: {:?}", m.nodes());
    let n = m.node(instance).unwrap();
    assert!(matches!(n.kind, NodeKind::Instance(_)));
    assert_eq!(n.name, None, "it shows its definition's name");
    assert_eq!(n.tag.as_deref(), Some("Carcass"), "it keeps its tag");
    assert_eq!(m.definition(definition).unwrap().name, "Side pair");
    let g = m.graph();
    for o in [a, b] {
        let leaf = g.objects.iter().find(|x| x.id == o).unwrap();
        assert_eq!(
            leaf.parent,
            Some(instance),
            "solids sit directly in the instance"
        );
        assert!(leaf.in_component);
    }
    for (o, was) in [a, b].iter().zip(&before) {
        near(min(m, *o), *was);
    }
    // The definition is in the group's axes (moved 10 in along red with it): in its own
    // axes the left solid starts at 0, not at 10 in.
    let def = g.definitions.iter().find(|x| x.id == definition).unwrap();
    assert!(def.min[0].abs() < 1e-12, "{:?}", def.min);
    assert!((n.xform.translation[0] - 10.0 * IN).abs() < 1e-12);
    // A copy of it is another instance of the same definition.
    let OpOutcome::Copied { nodes, .. } = ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![instance],
            xform: move_by(0.0, 20.0 * IN, 0.0),
        },
    ) else {
        panic!()
    };
    assert_eq!(
        d.model().node(nodes[0]).unwrap().kind,
        d.model().node(instance).unwrap().kind
    );
    // Several items (or a mix) still go into a new instance around them.
    let c = board(&mut d, 40.0 * IN, 0.0, 0.0, IN, IN, IN);
    let OpOutcome::ComponentCreated { instance: wrap, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Node(instance), Item::Object(c)],
            name: "Assembly".into(),
        },
    ) else {
        panic!()
    };
    assert_ne!(wrap, instance);
    assert_eq!(
        d.model().node(instance).unwrap().member.parent(),
        Some(wrap)
    );
}
