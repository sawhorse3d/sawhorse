//! `move_into` (#117): solids, groups and component instances moved into a group, or out to
//! the model, as one step. Only where each item sits in the tree changes.

use sawhorse_kernel::cutlist::{CutOptions, Grain, PartRef};
use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome, SceneDef};
use sawhorse_kernel::nodes::Item;
use sawhorse_kernel::transform::Xform;
use sawhorse_kernel::{Document, KernelError, StructureError};

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

fn structure(e: KernelError) -> StructureError {
    match e {
        KernelError::Structure(s) => s,
        e => panic!("not a structure refusal: {e}"),
    }
}

/// A box `w` x `d` x `h` at (x, y, z), in the open context. Its object id.
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
            curve: None,
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

fn group(d: &mut Document, items: Vec<Item>, name: &str) -> u64 {
    match ok(
        d,
        Op::MakeGroup {
            items,
            name: Some(name.into()),
        },
    ) {
        OpOutcome::GroupCreated { group } => group,
        o => panic!("{o:?}"),
    }
}

fn component(d: &mut Document, items: Vec<Item>, name: &str) -> (u64, u64) {
    match ok(
        d,
        Op::MakeComponent {
            items,
            name: name.into(),
        },
    ) {
        OpOutcome::ComponentCreated {
            instance,
            definition,
        } => (instance, definition),
        o => panic!("{o:?}"),
    }
}

fn move_into(items: Vec<Item>, into: Option<u64>) -> Op {
    Op::MoveInto { items, into }
}

fn object_parent(m: &Model, id: u64) -> Option<u64> {
    m.object(id).unwrap().membership().parent()
}

fn node_parent(m: &Model, id: u64) -> Option<u64> {
    m.node(id).unwrap().member.parent()
}

#[test]
fn moves_solids_groups_and_instances_in_and_out_and_nothing_moves() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 2.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let c = board(&mut d, 4.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let e = board(&mut d, 6.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let g = group(&mut d, vec![Item::Object(a)], "Box");
    let h = group(&mut d, vec![Item::Object(c)], "Inner");
    let (i, def) = component(&mut d, vec![Item::Object(e)], "Leg");
    let bounds = |m: &Model| -> Vec<_> {
        let mut v: Vec<_> = m
            .objects()
            .iter()
            .map(|(id, o)| (*id, o.bounds()))
            .collect();
        v.sort_by_key(|x| x.0);
        v
    };
    let before = bounds(d.model());
    let out = ok(
        &mut d,
        move_into(vec![Item::Object(b), Item::Node(h), Item::Node(i)], Some(g)),
    );
    assert_eq!(
        out,
        OpOutcome::MovedInto {
            into: Some(g),
            objects: vec![b],
            nodes: vec![h, i],
        }
    );
    let m = d.model();
    assert_eq!(object_parent(m, b), Some(g));
    assert_eq!(node_parent(m, h), Some(g));
    assert_eq!(node_parent(m, i), Some(g));
    // The solid inside the moved group came along.
    assert_eq!(object_parent(m, c), Some(h));
    // Nothing moved in the world, and the instance is still of its definition.
    assert_eq!(bounds(m), before);
    assert_eq!(
        m.node(i).unwrap().kind,
        sawhorse_kernel::nodes::NodeKind::Instance(sawhorse_kernel::ids::from_public(def))
    );
    // One undo step; Redo gives the same hash.
    let after = d.hash();
    d.undo().unwrap();
    assert_eq!(object_parent(d.model(), b), None);
    d.redo().unwrap();
    assert_eq!(d.hash(), after);
    // Out again: open the group, move to the model.
    ok(&mut d, Op::SetContext { node: Some(g) });
    let out = ok(&mut d, move_into(vec![Item::Object(b)], None));
    assert_eq!(
        out,
        OpOutcome::MovedInto {
            into: None,
            objects: vec![b],
            nodes: vec![],
        }
    );
    assert_eq!(object_parent(d.model(), b), None);
    assert_eq!(bounds(d.model()), before);
}

#[test]
fn the_item_keeps_what_is_its_own() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 2.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let g = group(&mut d, vec![Item::Object(a)], "Box");
    ok(
        &mut d,
        Op::RenameObject {
            object: b,
            name: Some("Shelf".into()),
        },
    );
    ok(
        &mut d,
        Op::TagItems {
            items: vec![Item::Object(b)],
            tag: Some("Shelves".into()),
        },
    );
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Object(b),
            grain: Grain::Width,
        },
    );
    ok(
        &mut d,
        Op::SetHidden {
            items: vec![Item::Object(b)],
            hidden: true,
        },
    );
    ok(&mut d, move_into(vec![Item::Object(b)], Some(g)));
    let o = d.model().object(b).unwrap();
    assert_eq!(o.name(), Some("Shelf"));
    assert_eq!(o.tag(), Some("Shelves"));
    assert_eq!(o.part().grain, Some(Grain::Width));
    assert!(o.membership().hidden);
    assert!(!o.membership().locked);
}

#[test]
fn refusals_say_why_and_change_nothing() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 2.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let c = board(&mut d, 4.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let e = board(&mut d, 6.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let f = board(&mut d, 8.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let inner = group(&mut d, vec![Item::Object(a)], "Inner");
    let outer = group(&mut d, vec![Item::Node(inner)], "Outer");
    let (inst, _) = component(&mut d, vec![Item::Object(e)], "Leg");
    let s = |e: KernelError| structure(e);

    // Nothing named, and things that are not there.
    assert_eq!(
        s(refused(&mut d, move_into(vec![], Some(outer)))),
        StructureError::NothingSelected
    );
    assert!(matches!(
        refused(&mut d, move_into(vec![Item::Object(999)], Some(outer))),
        KernelError::Document(_)
    ));
    assert_eq!(
        s(refused(&mut d, move_into(vec![Item::Object(b)], Some(999)))),
        StructureError::NoSuchNode(999)
    );
    // Not directly in the open context.
    assert_eq!(
        s(refused(&mut d, move_into(vec![Item::Object(a)], None))),
        StructureError::OutsideContext
    );
    // Into itself, or into something inside it.
    assert_eq!(
        s(refused(
            &mut d,
            move_into(vec![Item::Node(outer)], Some(outer))
        )),
        StructureError::IntoItself
    );
    assert_eq!(
        s(refused(
            &mut d,
            move_into(vec![Item::Node(outer)], Some(inner))
        )),
        StructureError::IntoItself
    );
    // A component, or a group inside one.
    assert_eq!(
        s(refused(
            &mut d,
            move_into(vec![Item::Object(b)], Some(inst))
        )),
        StructureError::IntoComponent
    );
    ok(&mut d, Op::SetContext { node: Some(inst) });
    let g_in = group(&mut d, vec![Item::Object(e)], "Foot");
    // An item inside a component's definition (the open context is inside an instance).
    assert_eq!(
        s(refused(&mut d, move_into(vec![Item::Node(g_in)], None))),
        StructureError::InComponent
    );
    ok(&mut d, Op::SetContext { node: None });
    assert_eq!(
        s(refused(
            &mut d,
            move_into(vec![Item::Object(b)], Some(g_in))
        )),
        StructureError::IntoComponent
    );
    // Already there: the whole op is refused, not the item skipped.
    assert_eq!(
        s(refused(&mut d, move_into(vec![Item::Object(b)], None))),
        StructureError::AlreadyThere
    );
    let gb = group(&mut d, vec![Item::Object(c)], "Box");
    ok(&mut d, Op::SetContext { node: Some(gb) });
    let c2 = board(&mut d, 10.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    assert_eq!(
        s(refused(
            &mut d,
            move_into(vec![Item::Object(c2), Item::Object(c)], Some(gb))
        )),
        StructureError::AlreadyThere
    );
    ok(&mut d, Op::SetContext { node: None });
    // Locked: the item, the target, or a group around the target.
    ok(
        &mut d,
        Op::SetLocked {
            items: vec![Item::Object(f)],
            locked: true,
        },
    );
    assert_eq!(
        s(refused(
            &mut d,
            move_into(vec![Item::Object(f)], Some(outer))
        )),
        StructureError::Locked
    );
    ok(
        &mut d,
        Op::SetLocked {
            items: vec![Item::Node(outer)],
            locked: true,
        },
    );
    assert_eq!(
        s(refused(
            &mut d,
            move_into(vec![Item::Object(b)], Some(outer))
        )),
        StructureError::Locked
    );
    assert_eq!(
        s(refused(
            &mut d,
            move_into(vec![Item::Object(b)], Some(inner))
        )),
        StructureError::Locked
    );
    // The words.
    assert_eq!(
        StructureError::IntoItself.to_string(),
        "a group cannot go into itself or into something inside it"
    );
    assert_eq!(
        StructureError::IntoComponent.to_string(),
        "that is a component: a part put into it would be in every copy"
    );
}

#[test]
fn a_repeated_item_is_moved_once() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 2.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let g = group(&mut d, vec![Item::Object(a)], "Box");
    let out = ok(
        &mut d,
        move_into(vec![Item::Object(b), Item::Object(b)], Some(g)),
    );
    assert_eq!(
        out,
        OpOutcome::MovedInto {
            into: Some(g),
            objects: vec![b],
            nodes: vec![],
        }
    );
}

fn rows(m: &Model) -> Vec<(String, usize, Vec<Item>)> {
    m.cut_list(&CutOptions::default())
        .rows
        .iter()
        .map(|r| {
            (
                r.name.clone(),
                r.qty,
                r.occurrences.iter().map(|o| o.item).collect(),
            )
        })
        .collect()
}

/// A group of only solids is one part. A solid moved in joins it; a group moved in ends
/// it; the last group moved out of a group leaves it empty, and it goes.
#[test]
fn the_cut_list_follows_the_tree() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let c = board(&mut d, 5.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let g = group(&mut d, vec![Item::Object(a)], "Panel");
    let before = rows(d.model());
    assert!(
        before.iter().any(|r| r.2 == vec![Item::Node(g)]),
        "{before:?}"
    );
    // A solid into a solids-only group: one part, the group, wider now.
    ok(&mut d, move_into(vec![Item::Object(b)], Some(g)));
    let m = d.model();
    let list = m.cut_list(&CutOptions::default());
    let panel = list
        .rows
        .iter()
        .find(|r| r.occurrences.iter().any(|o| o.item == Item::Node(g)))
        .expect("the group is a part");
    assert!(
        !list
            .rows
            .iter()
            .any(|r| r.occurrences.iter().any(|o| o.item == Item::Object(b))),
        "the solid is no row of its own"
    );
    assert!((panel.size[2] - 2.0).abs() < 1e-9, "{:?}", panel.size);
    // A group into a solids-only group: the target is no longer one part.
    let h = group(&mut d, vec![Item::Object(c)], "Rail");
    ok(&mut d, move_into(vec![Item::Node(h)], Some(g)));
    let r = rows(d.model());
    assert!(!r.iter().any(|x| x.2 == vec![Item::Node(g)]), "{r:?}");
    assert!(r.iter().any(|x| x.2 == vec![Item::Node(h)]), "{r:?}");
    // That group out again: the target holds only solids, so it is one part again.
    ok(&mut d, Op::SetContext { node: Some(g) });
    ok(&mut d, move_into(vec![Item::Node(h)], None));
    ok(&mut d, Op::SetContext { node: None });
    let r = rows(d.model());
    assert!(r.iter().any(|x| x.2 == vec![Item::Node(g)]), "{r:?}");
    assert!(r.iter().any(|x| x.2 == vec![Item::Node(h)]), "{r:?}");
    ok(&mut d, move_into(vec![Item::Node(h)], Some(g)));
    // The last group out of one: the one it leaves, emptied, goes once it is closed.
    let outer = group(&mut d, vec![Item::Node(g)], "Outer");
    ok(&mut d, Op::SetContext { node: Some(outer) });
    ok(&mut d, move_into(vec![Item::Node(g)], None));
    // Still there while it is the open context.
    assert!(d.model().node(outer).is_some());
    ok(&mut d, Op::LeaveContext);
    assert!(d.model().node(outer).is_none());
    d.undo().unwrap();
    assert!(d.model().node(outer).is_some());
}

/// A solid in a group is cut by the section planes of the groups around it: moved into a
/// group with a plane, that plane cuts it.
#[test]
fn the_landing_places_section_planes_cut_it() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    let b = board(&mut d, 3.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    let g = group(&mut d, vec![Item::Object(a)], "Box");
    ok(&mut d, Op::SetContext { node: Some(g) });
    let s = match ok(
        &mut d,
        Op::AddSection {
            origin: [0.0, 0.0, 0.5],
            normal: [0.0, 0.0, -1.0],
            name: None,
            active: true,
            hidden: false,
        },
    ) {
        OpOutcome::SectionAdded { section } => section,
        o => panic!("{o:?}"),
    };
    ok(&mut d, Op::LeaveContext);
    assert!(d.model().sections_cutting(b).is_empty());
    ok(&mut d, move_into(vec![Item::Object(b)], Some(g)));
    assert_eq!(d.model().sections_cutting(b), vec![s]);
}

/// A group left with only a section plane goes, and the plane with it, as tidying does
/// after any step. Undo brings both back.
#[test]
fn a_group_left_with_only_a_plane_goes_with_it() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    let g = group(&mut d, vec![Item::Object(a)], "Box");
    ok(&mut d, Op::SetContext { node: Some(g) });
    ok(
        &mut d,
        Op::AddSection {
            origin: [0.0, 0.0, 0.5],
            normal: [0.0, 0.0, -1.0],
            name: None,
            active: true,
            hidden: false,
        },
    );
    ok(&mut d, move_into(vec![Item::Object(a)], None));
    ok(&mut d, Op::LeaveContext);
    assert!(d.model().node(g).is_none());
    assert!(d.model().sections().is_empty());
    d.undo().unwrap();
    assert!(d.model().node(g).is_some());
    assert_eq!(d.model().sections().len(), 1);
}

/// A scene whose open group is tidied away falls back to the model, as after Explode.
#[test]
fn a_scene_whose_group_goes_falls_back_to_the_model() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    let g = group(&mut d, vec![Item::Object(a)], "Box");
    ok(
        &mut d,
        Op::AddScene {
            scene: SceneDef {
                name: "Inside".into(),
                eye: [3.0, -3.0, 3.0],
                target: [0.0; 3],
                up: [0.0, 0.0, 1.0],
                fov: 35.0,
                parallel: false,
                hidden_tags: vec![],
                context: Some(g),
                style: None,
                shadows: None,
                sections: None,
                grid: None,
            },
        },
    );
    ok(&mut d, Op::SetContext { node: Some(g) });
    ok(&mut d, move_into(vec![Item::Object(a)], None));
    ok(&mut d, Op::LeaveContext);
    assert!(d.model().node(g).is_none());
    assert_eq!(d.model().scenes()[0].context, None);
}

/// A log from before the op replays as it did, and one with the op replays to the same
/// hash.
#[test]
fn logs_replay() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let b = board(&mut d, 2.0, 0.0, 0.0, 1.0, 1.0, 0.5);
    let g = group(&mut d, vec![Item::Object(a)], "Box");
    ok(&mut d, move_into(vec![Item::Object(b)], Some(g)));
    let again = Document::replay(d.log()).unwrap();
    assert_eq!(again.hash(), d.hash());
    // The op as it is stored.
    let text = serde_json::to_string(d.log().last().unwrap()).unwrap();
    assert!(text.contains("\"op\":\"move_into\""), "{text}");
    let _ = Xform::IDENTITY;
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 64, ..Default::default() })]

    /// Moving items into a group and out again gives back the model hash, once the open
    /// context is the same as before (the hash covers it) and the group the items went into
    /// kept something of its own, so tidying left it alone.
    #[test]
    fn in_and_out_again_gives_back_the_hash(
        n in 3usize..7,
        moved in proptest::collection::vec(proptest::bool::ANY, 7),
        nest in proptest::bool::ANY,
        sizes in proptest::collection::vec((0.1f64..2.0, 0.1f64..2.0, 0.05f64..1.0), 7),
    ) {
        let mut d = Document::with_ground_sketch();
        let ids: Vec<u64> = (0..n)
            .map(|k| {
                let (w, l, h) = sizes[k];
                board(&mut d, 3.0 * k as f64, 0.0, 0.0, w, l, h)
            })
            .collect();
        // The target holds the first board; with `nest`, the second sits in a group of its
        // own, moved as a group.
        let target = group(&mut d, vec![Item::Object(ids[0])], "Target");
        let mut items: Vec<Item> = Vec::new();
        let start = if nest {
            items.push(Item::Node(group(&mut d, vec![Item::Object(ids[1])], "Inner")));
            2
        } else {
            1
        };
        for k in start..n {
            if moved[k] {
                items.push(Item::Object(ids[k]));
            }
        }
        proptest::prop_assume!(!items.is_empty());
        let before = d.hash();
        ok(&mut d, move_into(items.clone(), Some(target)));
        proptest::prop_assert_ne!(d.hash(), before);
        ok(&mut d, Op::SetContext { node: Some(target) });
        ok(&mut d, move_into(items, None));
        ok(&mut d, Op::SetContext { node: None });
        proptest::prop_assert_eq!(d.hash(), before);
    }
}
