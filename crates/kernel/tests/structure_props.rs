//! Property tests for groups and components (lane 5):
//!
//! - an edit inside one instance reaches every instance of the definition, each its exact
//!   image under its own placement (same face ids, same volume);
//! - Make Unique forks: editing the unique copy leaves every other copy bit for bit;
//! - Make Component then Explode leaves the model exactly as it was (hash identity);
//! - Make Group then Explode (nested too) is the identity on hashes.

use glam::DVec3;
use proptest::prelude::*;
use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome};
use sawhorse_kernel::nodes::{Item, NodeKind};
use sawhorse_kernel::transform::Xform;
use sawhorse_kernel::{Document, hash::object_hash};

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

fn board(doc: &mut Document, x: f64, y: f64, w: f64, d: f64, h: f64) -> u64 {
    let out = ok(
        doc,
        Op::Draw {
            target: DrawTarget::Plane {
                origin: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            },
            points: vec![
                [x, y, 0.0],
                [x + w, y, 0.0],
                [x + w, y + d, 0.0],
                [x, y + d, 0.0],
            ],
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

fn solid_of(m: &Model, node: u64) -> u64 {
    m.graph()
        .objects
        .iter()
        .find(|o| o.parent == Some(node))
        .map(|o| o.id)
        .expect("a solid in the instance")
}

fn face_ids(m: &Model, o: u64) -> Vec<u64> {
    let ob = m.object(o).unwrap();
    (0..ob.brep().face_count())
        .map(|i| ob.face_id_at(i).unwrap())
        .collect()
}

/// A placement: a move, optionally a quarter-ish turn about Z, optionally a mirror.
fn placement() -> impl Strategy<Value = Xform> {
    (
        (-6i32..6, -6i32..6, 0i32..3),
        prop_oneof![3 => Just(0.0), 1 => -3.0..3.0f64],
        any::<bool>(),
    )
        .prop_map(|((x, y, z), turn, mirror)| {
            let mut x = Xform::translation(DVec3::new(
                f64::from(x) * 1.5,
                f64::from(y) * 1.5,
                f64::from(z) * 0.75,
            ));
            if turn != 0.0 {
                x = x.after(&Xform::rotation(DVec3::ZERO, DVec3::Z, turn));
            }
            if mirror {
                x = x.after(&Xform::scaling(DVec3::ZERO, DVec3::new(-1.0, 1.0, 1.0)));
            }
            x
        })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(48),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn an_instance_edit_reaches_every_instance(
        size in (1u8..5, 1u8..5, 1u8..4),
        places in prop::collection::vec(placement(), 1..4),
        which in any::<u8>(),
        face in any::<u8>(),
        dist in prop_oneof![(1i32..4).prop_map(|k| f64::from(k) * 0.125), (-3i32..0).prop_map(|k| f64::from(k) * 0.0625)],
    ) {
        let mut d = Document::with_ground_sketch();
        let (w, dd, h) = (f64::from(size.0) * 0.5, f64::from(size.1) * 0.5, f64::from(size.2) * 0.25);
        let o = board(&mut d, 0.0, 0.0, w, dd, h);
        let OpOutcome::ComponentCreated { instance: first, .. } = ok(&mut d, Op::MakeComponent { items: vec![Item::Object(o)], name: "Part".into() }) else { panic!() };
        let mut all = vec![first];
        for x in &places {
            let OpOutcome::Copied { nodes, .. } = ok(&mut d, Op::Copy { objects: vec![], nodes: vec![first], xform: *x }) else { panic!() };
            all.extend(nodes);
        }
        let pick = all[which as usize % all.len()];
        ok(&mut d, Op::SetContext { node: Some(pick) });
        let s = solid_of(d.model(), pick);
        let ids = face_ids(d.model(), s);
        let f = ids[face as usize % ids.len()];
        let r = d.apply(&Op::PushPull { object: s, face: f, distance: dist });
        prop_assume!(r.is_ok());
        let m = d.model();
        let edited = solid_of(m, pick);
        let (want_ids, want_volume) = (face_ids(m, edited), m.object(edited).unwrap().volume());
        let NodeKind::Instance(def) = m.node(pick).unwrap().kind else { panic!() };
        let template = m.definitions().into_iter().find(|(id, _)| *id == m.definition_id(def)).unwrap().1.clone();
        let (_, member) = template.members.iter().next().unwrap();
        let sawhorse_kernel::nodes::MemberKind::Solid(t) = &member.kind else { panic!() };
        for &i in &all {
            let o = solid_of(m, i);
            let ob = m.object(o).unwrap();
            prop_assert_eq!(face_ids(m, o), want_ids.clone(), "same face ids in every copy");
            prop_assert!((ob.volume() - want_volume).abs() < 1e-9 * want_volume.max(1.0));
            // Exactly the definition's solid under this instance's placement.
            let x = m.node(i).unwrap().xform;
            prop_assert_eq!(object_hash(ob), object_hash(&t.mapped(&x).unwrap()));
        }
    }

    #[test]
    fn make_unique_forks_the_definition(
        places in prop::collection::vec(placement(), 1..3),
        dist in (1i32..4).prop_map(|k| f64::from(k) * 0.125),
    ) {
        let mut d = Document::with_ground_sketch();
        let o = board(&mut d, 0.0, 0.0, 1.0, 0.5, 0.25);
        let OpOutcome::ComponentCreated { instance: first, .. } = ok(&mut d, Op::MakeComponent { items: vec![Item::Object(o)], name: "Part".into() }) else { panic!() };
        let mut others = vec![];
        for x in &places {
            let OpOutcome::Copied { nodes, .. } = ok(&mut d, Op::Copy { objects: vec![], nodes: vec![first], xform: *x }) else { panic!() };
            others.extend(nodes);
        }
        let before: Vec<u64> = others.iter().map(|&i| object_hash(d.model().object(solid_of(d.model(), i)).unwrap())).collect();
        ok(&mut d, Op::MakeUnique { node: first });
        ok(&mut d, Op::SetContext { node: Some(first) });
        let s = solid_of(d.model(), first);
        ok(&mut d, Op::PushPull { object: s, face: 2, distance: dist });
        let after: Vec<u64> = others.iter().map(|&i| object_hash(d.model().object(solid_of(d.model(), i)).unwrap())).collect();
        prop_assert_eq!(before, after, "the other copies did not change");
        prop_assert!((d.model().object(s).unwrap().volume() - 0.5 * (0.25 + dist)).abs() < 1e-9);
    }

    #[test]
    fn make_component_then_explode_is_the_identity(
        n in 1usize..4,
        pick in prop::collection::vec(any::<bool>(), 4),
        moved in prop::option::of(placement()),
    ) {
        let mut d = Document::with_ground_sketch();
        let objs: Vec<u64> = (0..4).map(|k| board(&mut d, 2.0 * k as f64, 0.0, 1.0, 1.0, 0.5)).collect();
        let mut items: Vec<Item> = objs.iter().zip(&pick).filter(|(_, p)| **p).map(|(&o, _)| Item::Object(o)).collect();
        if items.is_empty() { items.push(Item::Object(objs[n % 4])); }
        let before = d.hash();
        let OpOutcome::ComponentCreated { instance, .. } = ok(&mut d, Op::MakeComponent { items: items.clone(), name: "Parts".into() }) else { panic!() };
        ok(&mut d, Op::Explode { node: instance });
        prop_assert_eq!(d.hash(), before);
        // Moved and back again is not exact in general (rounding), so only check the
        // round trip when the copy stays put; a moved instance must still explode cleanly.
        if let Some(x) = moved {
            let OpOutcome::ComponentCreated { instance, .. } = ok(&mut d, Op::MakeComponent { items, name: "Parts".into() }) else { panic!() };
            ok(&mut d, Op::Transform { objects: vec![], nodes: vec![instance], xform: x });
            ok(&mut d, Op::Explode { node: instance });
            prop_assert!(d.model().definitions().is_empty());
            prop_assert!(d.model().nodes().is_empty());
        }
    }

    #[test]
    fn group_then_explode_is_the_identity(
        pick in prop::collection::vec(any::<bool>(), 4),
        nest in any::<bool>(),
    ) {
        let mut d = Document::with_ground_sketch();
        let objs: Vec<u64> = (0..4).map(|k| board(&mut d, 2.0 * k as f64, 0.0, 1.0, 1.0, 0.5)).collect();
        let mut pick = pick;
        if !pick.contains(&true) { pick[0] = true; }
        let items: Vec<Item> = objs.iter().zip(&pick).filter(|(_, p)| **p).map(|(&o, _)| Item::Object(o)).collect();
        let before = d.hash();
        let OpOutcome::GroupCreated { group } = ok(&mut d, Op::MakeGroup { items, name: None }) else { panic!() };
        let mut groups = vec![group];
        if nest {
            let rest: Vec<Item> = objs.iter().zip(&pick).filter(|(_, p)| !**p).map(|(&o, _)| Item::Object(o)).chain([Item::Node(group)]).collect();
            let OpOutcome::GroupCreated { group: outer } = ok(&mut d, Op::MakeGroup { items: rest, name: Some("Outer".into()) }) else { panic!() };
            groups.insert(0, outer);
        }
        prop_assert_ne!(d.hash(), before);
        for g in groups {
            ok(&mut d, Op::Explode { node: g });
        }
        prop_assert_eq!(d.hash(), before);
    }
}
