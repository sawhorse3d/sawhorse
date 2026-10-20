//! Section planes (lane 12): the entity and its ops, where planes live in the tree, scenes,
//! the exact cut (caps with holes and the outline) on the component bookshelf, Create Group
//! from Slice, the `.shz` round trip, and property tests on random boxes:
//!
//! - the cap's area equals the plane's cross-section of the boxes, worked out independently
//!   (the convex hull of where the box edges cross the plane);
//! - flipping a plane gives the same caps the other way round, and the two kept halves add
//!   up to the whole;
//! - the kept volume is the integral of the cap area along the normal.
//!
//! The last test writes the `section.json` golden's ops under `UPDATE_GOLDEN=1`.

use glam::{DVec2, DVec3};
use proptest::prelude::*;
use sawhorse_kernel::cutlist::M_PER_IN;
use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome, SceneDef};
use sawhorse_kernel::nodes::Item;
use sawhorse_kernel::section::SceneSections;
use sawhorse_kernel::transform::Xform;
use sawhorse_kernel::{Document, KernelError};

const IN: f64 = M_PER_IN;

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

fn refused(d: &mut Document, op: Op) -> KernelError {
    let before = d.hash();
    let e = d.apply(&op).expect_err("refused");
    assert_eq!(d.hash(), before, "a refusal changes nothing");
    e
}

/// A board from (x, y, z), `w` along red, `d` along green, `h` up (inches).
fn board(doc: &mut Document, x: f64, y: f64, z: f64, w: f64, d: f64, h: f64) -> u64 {
    let (x, y, z, w, d, h) = (x * IN, y * IN, z * IN, w * IN, d * IN, h * IN);
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

fn part(doc: &mut Document, name: &str, b: [f64; 6]) -> u64 {
    let o = board(doc, b[0], b[1], b[2], b[3], b[4], b[5]);
    match ok(
        doc,
        Op::MakeComponent {
            items: vec![Item::Object(o)],
            name: name.into(),
        },
    ) {
        OpOutcome::ComponentCreated { instance, .. } => instance,
        o => panic!("{o:?}"),
    }
}

fn by(x: f64, y: f64, z: f64) -> Xform {
    Xform::translation(DVec3::new(x * IN, y * IN, z * IN))
}

/// The user guide's bookshelf as components (the cut list's): two 3/4 x 11 1/4 x 71 1/4
/// sides, four 34 1/2 x 11 shelves, a 36 in top, a toe kick and a 1/4 in back panel.
fn bookshelf() -> Document {
    let mut d = Document::with_ground_sketch();
    let side = part(&mut d, "Side", [0.0, 0.0, 0.0, 0.75, 11.25, 71.25]);
    ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![side],
            xform: by(35.25, 0.0, 0.0),
        },
    );
    let shelf = part(&mut d, "Shelf", [0.75, 0.0, 3.5, 34.5, 11.0, 0.75]);
    ok(
        &mut d,
        Op::Array {
            objects: vec![],
            nodes: vec![shelf],
            xform: by(0.0, 0.0, 18.0),
            count: 3,
        },
    );
    part(&mut d, "Top", [0.0, 0.0, 71.25, 36.0, 11.25, 0.75]);
    part(&mut d, "Toe kick", [0.75, 2.0, 0.0, 34.5, 0.75, 3.5]);
    part(&mut d, "Back panel", [0.75, 11.0, 0.0, 34.5, 0.25, 71.25]);
    d
}

fn add(d: &mut Document, origin: [f64; 3], normal: [f64; 3]) -> u64 {
    match ok(
        d,
        Op::AddSection {
            origin,
            normal,
            name: None,
            active: true,
            hidden: false,
        },
    ) {
        OpOutcome::SectionAdded { section } => section,
        o => panic!("{o:?}"),
    }
}

fn sq_in(a: f64) -> f64 {
    a / (IN * IN)
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * (1.0 + a.abs().max(b.abs()))
}

#[test]
fn the_bookshelf_cut_down_the_middle() {
    let mut d = bookshelf();
    // A plane through the middle facing left: the left half stays.
    let s = add(&mut d, [18.0 * IN, 5.0 * IN, 30.0 * IN], [-1.0, 0.0, 0.0]);
    let cut = d.model().section_cut(s).unwrap();
    // Shelves 4 x (11 x 3/4), the top 11 1/4 x 3/4, the toe kick 3/4 x 3 1/2, the back
    // 1/4 x 71 1/4: seven caps, 61 7/8 square inches. The sides are not reached.
    assert_eq!(cut.caps.len(), 7);
    assert!(close(sq_in(cut.area), 61.875, 1e-12), "{}", sq_in(cut.area));
    let mut areas: Vec<f64> = cut.caps.iter().map(|c| sq_in(c.area)).collect();
    areas.sort_by(f64::total_cmp);
    let want = [2.625, 8.25, 8.25, 8.25, 8.25, 8.4375, 17.8125];
    for (a, w) in areas.iter().zip(want) {
        assert!(close(*a, w, 1e-12), "{areas:?}");
    }
    // Every cap of a box is a rectangle with four outline edges.
    assert_eq!(cut.outline.len(), 28);
    for c in &cut.caps {
        assert_eq!(c.outer.len(), 4);
        assert!(c.holes.is_empty());
    }
    // Deterministic: the same model, the same cut and hash.
    assert_eq!(d.model().section_cut(s).unwrap(), cut);
    // Turned to face front to back: the sides, shelves and top (106 7/8 + 103 1/2 + 27).
    ok(
        &mut d,
        Op::PlaceSection {
            section: s,
            origin: [0.0, 5.5 * IN, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
    );
    let cut = d.model().section_cut(s).unwrap();
    assert_eq!(cut.caps.len(), 7);
    assert!(
        close(sq_in(cut.area), 237.375, 1e-12),
        "{}",
        sq_in(cut.area)
    );
}

#[test]
fn flipping_keeps_the_other_half() {
    let mut d = bookshelf();
    let s = add(&mut d, [18.0 * IN, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    let a = d.model().section_cut(s).unwrap();
    let total: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
    let left = d.model().kept_volume(s).unwrap();
    ok(&mut d, Op::FlipSection { section: s });
    let b = d.model().section_cut(s).unwrap();
    let right = d.model().kept_volume(s).unwrap();
    assert!(close(a.area, b.area, 1e-12));
    // Mirrored: the same caps, each ring the other way round in the flipped frame.
    assert_eq!(a.caps.len(), b.caps.len());
    // The two halves make the whole (the sides sit wholly on one side each).
    assert!(
        close(left + right, total, 1e-9),
        "{left} + {right} vs {total}"
    );
    assert!(left > 0.0 && right > 0.0);
    // The bookshelf is symmetric about x = 18 in.
    assert!(close(left, right, 1e-9), "{left} vs {right}");
    // Flipping twice is the identity.
    let h = d.hash();
    ok(&mut d, Op::FlipSection { section: s });
    ok(&mut d, Op::FlipSection { section: s });
    assert_eq!(d.hash(), h);
}

#[test]
fn one_active_plane_per_context() {
    let mut d = bookshelf();
    let a = add(&mut d, [18.0 * IN, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    let b = add(&mut d, [0.0, 5.0 * IN, 0.0], [0.0, 1.0, 0.0]);
    let m = d.model();
    assert!(!m.section(a).unwrap().active && m.section(b).unwrap().active);
    ok(
        &mut d,
        Op::SetSectionActive {
            section: a,
            active: true,
        },
    );
    let m = d.model();
    assert!(m.section(a).unwrap().active && !m.section(b).unwrap().active);
    // A new inactive plane leaves the active one alone.
    let c = match ok(
        &mut d,
        Op::AddSection {
            origin: [0.0, 0.0, 30.0 * IN],
            normal: [0.0, 0.0, -1.0],
            name: Some("Plan cut".into()),
            active: false,
            hidden: false,
        },
    ) {
        OpOutcome::SectionAdded { section } => section,
        o => panic!("{o:?}"),
    };
    let m = d.model();
    assert!(m.section(a).unwrap().active && !m.section(c).unwrap().active);
    assert_eq!(m.section(c).unwrap().name, "Plan cut");
    assert_eq!(m.section(a).unwrap().name, "Section 1");
    assert_eq!(m.section(b).unwrap().name, "Section 2");
    // Every solid is cut by the model's active plane.
    for (id, _) in m.objects() {
        assert_eq!(m.sections_cutting(id), vec![a]);
    }
    ok(
        &mut d,
        Op::SetSectionActive {
            section: a,
            active: false,
        },
    );
    assert!(d.model().sections().iter().all(|(_, s)| !s.active));
    // Rename, hide, show; bad names and planes are refused.
    ok(
        &mut d,
        Op::RenameSection {
            section: b,
            name: "  Side section ".into(),
        },
    );
    assert_eq!(d.model().section(b).unwrap().name, "Side section");
    refused(
        &mut d,
        Op::RenameSection {
            section: b,
            name: " ".into(),
        },
    );
    ok(
        &mut d,
        Op::SetSectionHidden {
            section: b,
            hidden: true,
        },
    );
    assert!(!d.model().section_shown(b));
    refused(
        &mut d,
        Op::AddSection {
            origin: [0.0; 3],
            normal: [0.0; 3],
            name: None,
            active: true,
            hidden: false,
        },
    );
    refused(
        &mut d,
        Op::AddSection {
            origin: [f64::NAN, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            name: None,
            active: true,
            hidden: false,
        },
    );
    refused(&mut d, Op::FlipSection { section: 999 });
    // Undo walks it all back.
    while d.can_undo() {
        d.undo().unwrap();
        if d.model().sections().is_empty() {
            break;
        }
    }
    assert!(d.model().sections().is_empty());
}

#[test]
fn planes_live_in_groups_and_move_with_them() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 10.0, 10.0, 10.0);
    let b = board(&mut d, 20.0, 0.0, 0.0, 10.0, 10.0, 10.0);
    let g = match ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(a)],
            name: Some("Box".into()),
        },
    ) {
        OpOutcome::GroupCreated { group } => group,
        o => panic!("{o:?}"),
    };
    ok(&mut d, Op::SetContext { node: Some(g) });
    let s = add(&mut d, [5.0 * IN, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    ok(&mut d, Op::LeaveContext);
    let m = d.model();
    // It cuts its own group only.
    assert_eq!(m.sections_cutting(a), vec![s]);
    assert!(m.sections_cutting(b).is_empty());
    assert_eq!(m.section_cut(s).unwrap().caps.len(), 1);
    // Outside its context it cannot be moved, flipped or deleted, but it can be activated.
    refused(&mut d, Op::FlipSection { section: s });
    refused(&mut d, Op::DeleteSection { section: s });
    refused(
        &mut d,
        Op::MoveSections {
            sections: vec![s],
            xform: by(1.0, 0.0, 0.0),
        },
    );
    ok(
        &mut d,
        Op::SetSectionActive {
            section: s,
            active: false,
        },
    );
    ok(
        &mut d,
        Op::SetSectionActive {
            section: s,
            active: true,
        },
    );
    // A model-level plane does not deactivate the group's: one per context.
    let top = add(&mut d, [0.0, 0.0, 5.0 * IN], [0.0, 0.0, -1.0]);
    let m = d.model();
    assert!(m.section(s).unwrap().active && m.section(top).unwrap().active);
    assert_eq!(m.sections_cutting(a), vec![s, top]);
    // The group's cap is clipped by the model's plane: only the lower half stays.
    let cut = m.section_cut(s).unwrap();
    assert!(close(sq_in(cut.area), 50.0, 1e-12), "{}", sq_in(cut.area));
    // Moving the group moves its plane.
    ok(
        &mut d,
        Op::Transform {
            objects: vec![],
            xform: by(0.0, 3.0, 0.0),
            nodes: vec![g],
        },
    );
    let p = d.model().section(s).unwrap().origin;
    assert!(close(p.y, 3.0 * IN, 1e-12), "{p}");
    // A copy of the group copies its plane, active in the copy's own context.
    let copy = match ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            xform: by(0.0, 40.0, 0.0),
            nodes: vec![g],
        },
    ) {
        OpOutcome::Copied { nodes, .. } => nodes[0],
        o => panic!("{o:?}"),
    };
    let m = d.model();
    let copied: Vec<_> = m
        .sections()
        .into_iter()
        .filter(|(_, x)| x.parent.map(sawhorse_kernel::ids::to_public) == Some(copy))
        .collect();
    assert_eq!(copied.len(), 1);
    assert!(copied[0].1.active);
    // Exploding the group moves its plane up; the model already has an active plane, so it
    // arrives switched off.
    ok(&mut d, Op::Explode { node: g });
    let m = d.model();
    let x = m.section(s).unwrap();
    assert!(x.parent.is_none() && !x.active);
    // Deleting the copy deletes its plane.
    ok(&mut d, Op::DeleteNode { node: copy });
    assert_eq!(d.model().sections().len(), 2);
}

#[test]
fn moving_and_copying_planes() {
    let mut d = bookshelf();
    let s = add(&mut d, [18.0 * IN, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    ok(
        &mut d,
        Op::MoveSections {
            sections: vec![s],
            xform: by(-6.0, 0.0, 0.0),
        },
    );
    assert!(close(
        d.model().section(s).unwrap().origin.x,
        12.0 * IN,
        1e-12
    ));
    // A quarter turn about a vertical axis turns the normal too.
    ok(
        &mut d,
        Op::MoveSections {
            sections: vec![s],
            xform: Xform::rotation(
                DVec3::new(12.0 * IN, 0.0, 0.0),
                DVec3::Z,
                std::f64::consts::FRAC_PI_2,
            ),
        },
    );
    let n = d.model().section(s).unwrap().normal;
    assert!(n.distance(DVec3::new(0.0, -1.0, 0.0)) < 1e-12, "{n}");
    let made = match ok(
        &mut d,
        Op::CopySections {
            sections: vec![s],
            xform: by(0.0, 2.0, 0.0),
            count: 3,
        },
    ) {
        OpOutcome::SectionsCopied { sections } => sections,
        o => panic!("{o:?}"),
    };
    assert_eq!(made.len(), 3);
    let m = d.model();
    assert!(made.iter().all(|&k| !m.section(k).unwrap().active));
    assert!(m.section(s).unwrap().active);
    refused(
        &mut d,
        Op::CopySections {
            sections: vec![s],
            xform: by(0.0, 2.0, 0.0),
            count: 0,
        },
    );
    refused(
        &mut d,
        Op::MoveSections {
            sections: vec![],
            xform: by(0.0, 2.0, 0.0),
        },
    );
    // Delete one; undo brings it back with the same id.
    ok(&mut d, Op::DeleteSection { section: made[1] });
    assert!(d.model().section(made[1]).is_none());
    d.undo().unwrap();
    assert!(d.model().section(made[1]).is_some());
}

fn scene(name: &str, sections: Option<SceneSections>) -> SceneDef {
    let sections = sections.map(Box::new);
    SceneDef {
        name: name.into(),
        eye: [3.0, -3.0, 2.0],
        target: [0.0; 3],
        up: [0.0, 0.0, 1.0],
        fov: 35.0,
        parallel: true,
        hidden_tags: vec![],
        context: None,
        style: None,
        shadows: None,
        sections,
    }
}

#[test]
fn scenes_bring_their_sections_back() {
    let mut d = bookshelf();
    let a = add(&mut d, [18.0 * IN, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    let b = add(&mut d, [0.0, 5.5 * IN, 0.0], [0.0, 1.0, 0.0]);
    ok(
        &mut d,
        Op::SetSectionHidden {
            section: a,
            hidden: true,
        },
    );
    let saved = d.model().scene_sections_now();
    assert_eq!(saved.active, vec![b]);
    assert_eq!(saved.hidden, vec![a]);
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Side cut", Some(saved.clone())),
        },
    );
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Leaves sections alone", None),
        },
    );
    ok(
        &mut d,
        Op::SetSectionActive {
            section: a,
            active: true,
        },
    );
    ok(
        &mut d,
        Op::SetSectionHidden {
            section: a,
            hidden: false,
        },
    );
    ok(
        &mut d,
        Op::ActivateScene {
            name: "Leaves sections alone".into(),
        },
    );
    assert!(d.model().section(a).unwrap().active);
    ok(
        &mut d,
        Op::ActivateScene {
            name: "Side cut".into(),
        },
    );
    let m = d.model();
    assert!(m.section(b).unwrap().active && !m.section(a).unwrap().active);
    assert!(m.section(a).unwrap().hidden);
    // A scene naming a plane that does not exist is refused.
    refused(
        &mut d,
        Op::AddScene {
            scene: scene(
                "Bad",
                Some(SceneSections {
                    active: vec![77],
                    hidden: vec![],
                }),
            ),
        },
    );
    // Deleting a plane: scenes forget it.
    ok(&mut d, Op::DeleteSection { section: b });
    let sc = d.model().scenes()[0].sections.clone().unwrap();
    assert!(sc.active.is_empty());
    assert_eq!(sc.hidden, vec![a]);
}

#[test]
fn a_tube_cut_across_has_a_hole() {
    let mut d = Document::with_ground_sketch();
    // A 4 x 4 in square with a 2 x 2 in hole, 10 in tall.
    let out = ok(
        &mut d,
        Op::Draw {
            target: DrawTarget::Plane {
                origin: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            },
            points: vec![
                [0.0, 0.0, 0.0],
                [4.0 * IN, 0.0, 0.0],
                [4.0 * IN, 4.0 * IN, 0.0],
                [0.0, 4.0 * IN, 0.0],
            ],
            closed: true,
            curve: None,
        },
    );
    let OpOutcome::Drawn {
        sketch: Some(sketch),
        inside: Some(outer),
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    ok(
        &mut d,
        Op::Draw {
            target: DrawTarget::Sketch { sketch },
            points: vec![
                [1.0 * IN, 1.0 * IN, 0.0],
                [3.0 * IN, 1.0 * IN, 0.0],
                [3.0 * IN, 3.0 * IN, 0.0],
                [1.0 * IN, 3.0 * IN, 0.0],
            ],
            closed: true,
            curve: None,
        },
    );
    ok(
        &mut d,
        Op::Extrude {
            sketch,
            region: outer,
            distance: 10.0 * IN,
        },
    );
    let s = add(&mut d, [0.0, 0.0, 5.0 * IN], [0.0, 0.0, -1.0]);
    let cut = d.model().section_cut(s).unwrap();
    assert_eq!(cut.caps.len(), 1);
    assert_eq!(cut.caps[0].holes.len(), 1);
    assert!(close(sq_in(cut.area), 12.0, 1e-12), "{}", sq_in(cut.area));
    assert_eq!(cut.outline.len(), 8);
    // The world triangles cover the cap: their area is the cap's.
    let (pos, tris) = cut.triangles();
    let area: f64 = tris
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| pos[i as usize]);
            (b - a).cross(c - a).length() / 2.0
        })
        .sum();
    assert!(close(area, cut.area, 1e-9));
    // They face away from what is kept (the cap's outward side), towards -normal... here +Z.
    let [a, b, c] = tris[0].map(|i| pos[i as usize]);
    assert!((b - a).cross(c - a).z > 0.0);
}

#[test]
fn create_group_from_slice() {
    let mut d = bookshelf();
    let s = add(&mut d, [18.0 * IN, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    let (group, sketch) = match ok(
        &mut d,
        Op::SliceToGroup {
            section: s,
            name: None,
        },
    ) {
        OpOutcome::Sliced { group, sketch } => (group, sketch),
        o => panic!("{o:?}"),
    };
    let m = d.model();
    assert_eq!(
        m.node(group).unwrap().name.as_deref(),
        Some("Section 1 slice")
    );
    let sk = m.sketch(sketch).unwrap();
    assert_eq!(m.sketch_parent(sketch), Some(group));
    // One face per cap (the shelves', top's, toe kick's and back's profiles).
    assert_eq!(sk.region_count(), 7);
    // A plane cutting nothing has nothing to slice.
    let far = add(&mut d, [500.0 * IN, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    let e = refused(
        &mut d,
        Op::SliceToGroup {
            section: far,
            name: None,
        },
    );
    assert!(e.to_string().contains("cuts nothing"), "{e}");
    // Inside a component: refused (it would not reach the other copies).
    let inst = d
        .model()
        .graph()
        .nodes
        .iter()
        .find(|n| n.kind == "instance")
        .map(|n| n.id)
        .unwrap();
    ok(&mut d, Op::SetContext { node: Some(inst) });
    let e = refused(
        &mut d,
        Op::SliceToGroup {
            section: s,
            name: None,
        },
    );
    assert!(e.to_string().contains("inside a component"), "{e}");
}

#[test]
fn sections_survive_the_snapshot() {
    let mut d = bookshelf();
    let a = add(&mut d, [18.0 * IN, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    add(&mut d, [0.0, 5.5 * IN, 0.0], [0.0, 1.0, 0.0]);
    ok(
        &mut d,
        Op::SetSectionHidden {
            section: a,
            hidden: true,
        },
    );
    let now = d.model().scene_sections_now();
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Cut", Some(now)),
        },
    );
    let snap = d.model().snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: sawhorse_kernel::snapshot::Snapshot = serde_json::from_str(&json).unwrap();
    let m = Model::from_snapshot(&back).unwrap();
    assert_eq!(m.hash(), d.model().hash());
    assert_eq!(
        m.section_cut(a).unwrap().hash,
        d.model().section_cut(a).unwrap().hash
    );
    // A plane in a missing group, or a scene naming a missing plane, is refused.
    let mut bad = snap.clone();
    bad.sections[0].parent = Some(4242);
    assert!(Model::from_snapshot(&bad).is_err());
    let mut bad = snap.clone();
    bad.scenes[0].sections = Some(Box::new(SceneSections {
        active: vec![99],
        hidden: vec![],
    }));
    assert!(Model::from_snapshot(&bad).is_err());
}

#[test]
fn a_model_without_sections_hashes_as_before() {
    let d = bookshelf();
    let h = d.hash();
    let mut e = d.clone();
    let s = add(&mut e, [18.0 * IN, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    assert_ne!(e.hash(), h);
    ok(&mut e, Op::DeleteSection { section: s });
    assert_eq!(e.model().hash(), d.model().hash());
}

#[test]
fn imported_planes_land_in_their_groups_and_components() {
    let cube = |x: f64| {
        serde_json::json!({
            "name": "Cube",
            "positions": [[x,0,0],[x+1.0,0,0],[x+1.0,1,0],[x,1,0],[x,0,1],[x+1.0,0,1],[x+1.0,1,1],[x,1,1]],
            "faces": [[[0,3,2,1]],[[4,5,6,7]],[[0,1,5,4]],[[1,2,6,5]],[[2,3,7,6]],[[3,0,4,7]]]
        })
    };
    let op: Op = serde_json::from_value(serde_json::json!({
        "op": "import",
        "definitions": [{
            "name": "Box",
            "items": [cube(0.0)],
            "sections": [{"origin": [0.5, 0, 0], "normal": [-1, 0, 0], "name": "Box cut"}]
        }],
        "nodes": [
            {"kind": "group", "name": "G", "items": [cube(5.0)],
             "sections": [{"origin": [5.5, 0, 0], "normal": [-1, 0, 0], "name": "Group cut", "hidden": true}]},
            {"kind": "instance", "definition": 0, "xform": {"linear": [[1,0,0],[0,1,0],[0,0,1]], "translation": [10, 0, 0]}},
            {"kind": "instance", "definition": 0, "xform": {"linear": [[1,0,0],[0,1,0],[0,0,1]], "translation": [20, 0, 0]}}
        ]
    }))
    .unwrap();
    let mut d = Document::with_ground_sketch();
    ok(&mut d, op);
    let m = d.model();
    let planes = m.sections();
    assert_eq!(planes.len(), 3);
    assert!(planes.iter().all(|(_, s)| !s.active && s.parent.is_some()));
    let g = planes.iter().find(|(_, s)| s.name == "Group cut").unwrap();
    assert!(g.1.hidden);
    let mut xs: Vec<f64> = planes
        .iter()
        .filter(|(_, s)| s.name == "Box cut")
        .map(|(_, s)| s.origin.x)
        .collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(xs, vec![10.5, 20.5]);
    // Activated, the group's plane cuts its cube in half.
    let (gid, _) = *g;
    ok(
        &mut d,
        Op::SetSectionActive {
            section: gid,
            active: true,
        },
    );
    assert!(close(d.model().section_cut(gid).unwrap().area, 1.0, 1e-12));
}

// ---- property tests on random boxes ---------------------------------------------------

/// A box `size` (metres) at `at`, turned by `turn` (radians about a random axis).
#[derive(Debug, Clone)]
struct BoxSpec {
    at: [f64; 3],
    size: [f64; 3],
    axis: [f64; 3],
    turn: f64,
}

fn box_spec() -> impl Strategy<Value = BoxSpec> {
    (
        prop::array::uniform3(-0.5..0.5f64),
        prop::array::uniform3(0.02..0.6f64),
        prop::array::uniform3(-1.0..1.0f64),
        -3.1..3.1f64,
    )
        .prop_map(|(at, size, axis, turn)| BoxSpec {
            at,
            size,
            axis,
            turn,
        })
}

/// Boxes far enough apart never to touch (one per metre-and-a-half along x).
fn build(boxes: &[BoxSpec]) -> Document {
    let mut d = Document::with_ground_sketch();
    for (k, b) in boxes.iter().enumerate() {
        let o = board(
            &mut d,
            0.0,
            0.0,
            0.0,
            b.size[0] / IN,
            b.size[1] / IN,
            b.size[2] / IN,
        );
        let axis = DVec3::from_array(b.axis);
        let axis = if axis.length() < 1e-3 { DVec3::Z } else { axis };
        let x = Xform::rotation(DVec3::ZERO, axis, b.turn).after(&Xform::IDENTITY);
        let shift = Xform::translation(DVec3::new(b.at[0] + 1.5 * k as f64, b.at[1], b.at[2]));
        ok(
            &mut d,
            Op::Transform {
                objects: vec![o],
                xform: shift.after(&x),
                nodes: vec![],
            },
        );
    }
    d
}

fn hull_area(pts: &[DVec2]) -> f64 {
    let mut p: Vec<DVec2> = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| a.distance(*b) < 1e-15);
    if p.len() < 3 {
        return 0.0;
    }
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<DVec2> = Vec::new();
    for &q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 0.0 {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Vec<DVec2> = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 0.0 {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    sawhorse_tessellate::signed_area(&lower)
}

/// The plane's cross-section of every (convex) box, independently of the kernel's cut:
/// the convex hull of where the box's edges cross the plane.
fn cross_section(m: &Model, origin: DVec3, normal: DVec3) -> f64 {
    let n = normal.normalize();
    let plane = sawhorse_kernel::plane::Plane::new(origin, n).unwrap();
    let mut total = 0.0;
    for (_, o) in m.objects() {
        let b = o.brep();
        let mut pts = Vec::new();
        for (e, _) in b.edges() {
            let (p, q) = b.edge_points(e);
            let (dp, dq) = ((p - origin).dot(n), (q - origin).dot(n));
            if (dp >= 0.0) != (dq >= 0.0) {
                pts.push(plane.to_local(p + (q - p) * (dp / (dp - dq))));
            }
        }
        total += hull_area(&pts);
    }
    total
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn caps_are_the_cross_section(
        boxes in prop::collection::vec(box_spec(), 1..4),
        origin in prop::array::uniform3(-0.4..0.4f64),
        normal in prop::array::uniform3(-1.0..1.0f64),
    ) {
        let n = DVec3::from_array(normal);
        prop_assume!(n.length() > 0.05);
        let mut d = build(&boxes);
        // The plane through `origin` shifted to cross the boxes' row.
        let o = DVec3::from_array(origin) + DVec3::new(0.75 * (boxes.len() - 1) as f64, 0.0, 0.0);
        let s = add(&mut d, o.to_array(), n.to_array());
        let cut = d.model().section_cut(s).unwrap();
        let want = cross_section(d.model(), o, n);
        prop_assert!(close(cut.area, want, 1e-9), "cap {} vs cross-section {}", cut.area, want);
        // Every cap is a convex box section: one ring, no holes, counter-clockwise.
        for c in &cut.caps {
            prop_assert!(c.holes.is_empty());
            let r: Vec<DVec2> = c.outer.iter().map(|p| DVec2::from_array(*p)).collect();
            prop_assert!(sawhorse_tessellate::signed_area(&r) > 0.0);
        }
        // The outline is the caps' edges.
        let ring_edges: usize = cut.caps.iter().map(|c| c.outer.len()).sum();
        prop_assert_eq!(cut.outline.len(), ring_edges);

        // Flipped: the same area, every cap the mirror image, and the two kept halves make
        // the whole.
        let total: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
        let kept = d.model().kept_volume(s).unwrap();
        ok(&mut d, Op::FlipSection { section: s });
        let flipped = d.model().section_cut(s).unwrap();
        prop_assert!(close(flipped.area, cut.area, 1e-9));
        prop_assert_eq!(flipped.caps.len(), cut.caps.len());
        let other = d.model().kept_volume(s).unwrap();
        prop_assert!(close(kept + other, total, 1e-9), "{} + {} vs {}", kept, other, total);
        prop_assert!(kept >= -1e-12 && other >= -1e-12);
        // Mirror: a cap's points seen in the flipped frame are the original's with one
        // coordinate negated (the frame turns over), so the areas match ring by ring.
        let mut a: Vec<f64> = cut.caps.iter().map(|c| c.area).collect();
        let mut b: Vec<f64> = flipped.caps.iter().map(|c| c.area).collect();
        a.sort_by(f64::total_cmp);
        b.sort_by(f64::total_cmp);
        for (x, y) in a.iter().zip(&b) {
            prop_assert!(close(*x, *y, 1e-9));
        }
    }

    #[test]
    fn kept_volume_is_the_integral_of_the_cap(
        b in box_spec(),
        normal in prop::array::uniform3(-1.0..1.0f64),
        t in 0.05..0.95f64,
    ) {
        let n = DVec3::from_array(normal);
        prop_assume!(n.length() > 0.05);
        let n = n.normalize();
        let mut d = build(&[b]);
        let m = d.model().clone();
        // The box's extent along the normal.
        let ds: Vec<f64> = m.objects()[0].1.brep().vertices().map(|(_, v)| v.pos.dot(n)).collect();
        let (lo, hi) = ds.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), &x| (a.min(x), z.max(x)));
        let at = lo + (hi - lo) * t;
        let s = add(&mut d, (n * at).to_array(), n.to_array());
        let kept = d.model().kept_volume(s).unwrap();
        // Simpson over the kept side, from the plane to the far end.
        let steps = 200;
        let h = (hi - at) / steps as f64;
        let mut sum = 0.0;
        for k in 0..=steps {
            let x = at + h * k as f64;
            let w = if k == 0 || k == steps { 1.0 } else if k % 2 == 1 { 4.0 } else { 2.0 };
            sum += w * cross_section(&m, n * x, n);
        }
        let integral = sum * h / 3.0;
        prop_assert!(close(kept, integral, 2e-3), "kept {} vs integral {}", kept, integral);
    }
}

/// The golden `section.json` replays exactly this: the component bookshelf cut down the
/// middle, the plane flipped and moved, a second plane in a group, scenes saving both,
/// Create Group from Slice, undo and redo. Written from here under `UPDATE_GOLDEN=1` (then
/// `--test golden` fills in the hashes) and must match it otherwise.
#[test]
fn the_section_golden_is_this_bookshelf() {
    let mut d = bookshelf();
    let s = add(&mut d, [18.0 * IN, 5.0 * IN, 30.0 * IN], [-1.0, 0.0, 0.0]);
    ok(&mut d, Op::FlipSection { section: s });
    ok(
        &mut d,
        Op::MoveSections {
            sections: vec![s],
            xform: by(-6.0, 0.0, 0.0),
        },
    );
    let now = d.model().scene_sections_now();
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Section A", Some(now)),
        },
    );
    let plan = add(&mut d, [0.0, 0.0, 30.0 * IN], [0.0, 0.0, -1.0]);
    ok(
        &mut d,
        Op::RenameSection {
            section: plan,
            name: "Plan at 30 in".into(),
        },
    );
    ok(
        &mut d,
        Op::SliceToGroup {
            section: plan,
            name: None,
        },
    );
    ok(
        &mut d,
        Op::ActivateScene {
            name: "Section A".into(),
        },
    );
    d.undo().unwrap();
    d.redo().unwrap();
    let m = d.model();
    assert!(m.section(s).unwrap().active && !m.section(plan).unwrap().active);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/section.json");
    let ops = serde_json::to_value(d.log()).unwrap();
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        let mut g: serde_json::Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| {
                serde_json::json!({
                    "name": "section",
                    "hash": "",
                    "stats": {"objects": 0, "faces": 0, "edges": 0, "vertices": 0, "genus": 0, "watertight": true}
                })
            });
        g["description"] = "Lane 12: the component bookshelf cut down the middle by a section plane, flipped and moved, saved in a scene; a plan plane at 30 in, renamed, sliced into a group; the scene brought back; undo and redo. Its sections hashes prove WASM cuts the same caps as native.".into();
        g["ops"] = ops;
        std::fs::write(&path, serde_json::to_string_pretty(&g).unwrap() + "\n").unwrap();
    } else {
        let g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            g["ops"], ops,
            "golden/section.json is not this bookshelf (UPDATE_GOLDEN=1)"
        );
    }
}

/// A box on the plane z = `z0` over (x0, y0)..(x1, y1), `h` tall (metres).
fn block(doc: &mut Document, x0: f64, y0: f64, x1: f64, y1: f64, z0: f64, h: f64) -> u64 {
    let out = ok(
        doc,
        Op::Draw {
            target: DrawTarget::Plane {
                origin: [0.0, 0.0, z0],
                normal: [0.0, 0.0, 1.0],
            },
            points: vec![[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]],
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

#[test]
fn a_slice_far_along_a_tilted_plane_fits_its_sketch() {
    // Fuzz find (fix pass of lane 23, seed 20269807, plane snapping off;
    // `tests/repro/2026-10-19-slice-of-a-wall-offset-14km-out-along-a-tilted-plane.json`): a
    // plane through the world's origin, tilted, cuts a tall slab 9.9 km out. Every point of
    // the slab is within the model's bounds, but along the plane the cap lies 10.6 km from the
    // section's point, past what a sketch holds from its origin, and Create Group from Slice
    // was refused by the sketch although the cap is representable. The slice's sketch now
    // puts its origin where the caps fit.
    let mut d = Document::new();
    block(&mut d, -9900.0, 7000.0, -9700.0, 7200.0, -5000.0, 10_000.0);
    let n = [-0.5399464803667886, -0.617294133162572, -0.8492521248611404];
    let s = add(&mut d, [0.0; 3], n);
    let cut = d.model().section_cut(s).unwrap();
    assert!(
        cut.caps
            .iter()
            .flat_map(|c| &c.outer)
            .any(|p| p[0].abs() > 1e4 || p[1].abs() > 1e4),
        "the cap lies beyond a sketch's reach from the section's point"
    );
    let sketch = match ok(
        &mut d,
        Op::SliceToGroup {
            section: s,
            name: None,
        },
    ) {
        OpOutcome::Sliced { sketch, .. } => sketch,
        o => panic!("{o:?}"),
    };
    let m = d.model();
    let sk = m.sketch(sketch).unwrap();
    assert_eq!(sk.region_count(), 1);
    // Drawn where the cap is: its region's area is the cap's, its plane the section's.
    let plane = sk.plane();
    assert!(plane.normal.dot(DVec3::from_array(n).normalize()) > 1.0 - 1e-12);
    assert!(plane.signed_distance(DVec3::ZERO).abs() < 1e-6);
    let area: f64 = cut.caps.iter().map(|c| c.area).sum();
    let got = sk.region_area(sk.regions().next().unwrap().1).abs();
    assert!((got - area).abs() <= 1e-6 * area, "{got} vs {area}");
    sawhorse_kernel::testing::check_model(m).unwrap();
}

#[test]
fn a_slice_wider_than_a_sketch_is_refused() {
    // Two tall slabs at opposite corners of the model, cut by one vertical plane along the
    // diagonal: the caps lie 27 km apart along the plane, more than one sketch can hold from
    // any origin. Refused in its own words, nothing changed (fix pass of lane 23).
    let mut d = Document::new();
    block(&mut d, -9900.0, -9900.0, -9800.0, -9800.0, 0.0, 1.0);
    block(&mut d, 9800.0, 9800.0, 9900.0, 9900.0, 0.0, 1.0);
    let s = add(&mut d, [0.0, 0.0, 0.0], [1.0, -1.0, 0.0]);
    assert_eq!(d.model().section_cut(s).unwrap().caps.len(), 2);
    let e = refused(
        &mut d,
        Op::SliceToGroup {
            section: s,
            name: None,
        },
    );
    assert!(e.to_string().contains("20 km across"), "{e}");
    sawhorse_kernel::testing::check_model(d.model()).unwrap();
}

#[test]
fn a_section_across_a_hair_thin_fin_keeps_two_halves_above_zero() {
    // Lane 23b (fuzz seed 20271003, snapping on, sequence 10,317): a 0.1 mm circle pulled
    // 3 m down, two neighbouring points of its rim moved 2.2 m up (autofold): a fin a few
    // hundredths of a millimetre wide. A plane at 45 degrees across the fin kept halves of
    // 1.43e-7 and -4.5e-8 m3 of a 9.7e-8 m3 solid: each kept face was measured through its
    // stored plane, whose normal a face that narrow does not pin down (its corners sit within
    // tolerance of it, but tilt across its width), about the world origin 1.5 m away. Now
    // the kept volume is the cone over the solid's own triangles, each clipped to the kept
    // side, from the solid's centre (lane 23b's second fix pass; 23b itself took the fan
    // over each kept face ring's corners).
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/repro/2026-10-20-section-across-a-fin-pulled-up-from-a-tiny-cylinder.json"
    ))
    .unwrap();
    let repro: sawhorse_kernel::testing::Repro = serde_json::from_str(&text).unwrap();
    sawhorse_kernel::snap::set_plane_snapping(true);
    let mut d = Document::replay(&repro.ops).unwrap();
    let s = d.model().sections()[0].0;
    let whole: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
    let kept = d.model().kept_volume(s).unwrap();
    ok(&mut d, Op::FlipSection { section: s });
    let other = d.model().kept_volume(s).unwrap();
    assert!(kept > 0.0 && other > 0.0, "{kept} + {other} of {whole}");
    // The whole is the tessellated volume (the faces' triangles) and the halves are those
    // triangles clipped, so they make it to rounding (1.5e-14 of it; lane 23b's ring fans
    // were 3e-5 off, the fin's faces bending within tolerance).
    assert!(
        (kept + other - whole).abs() <= 1e-11 * whole,
        "{kept} + {other} vs {whole}"
    );
    // The fin above the plane is a sliver of the whole.
    assert!(other < 0.05 * whole, "{other} of {whole}");
    sawhorse_kernel::testing::check_model(d.model()).unwrap();
}

/// Replays a named fuzzer find (`tests/repro/<name>`) with its plane snapping, as the fuzzer
/// played it: an op the kernel refuses changes nothing and the sequence goes on.
fn replay_find(name: &str) -> Document {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/repro")
            .join(name),
    )
    .unwrap();
    let repro: sawhorse_kernel::testing::Repro = serde_json::from_str(&text).unwrap();
    sawhorse_kernel::snap::set_plane_snapping(repro.plane_snap.unwrap_or(true));
    let mut d = Document::new();
    for op in &repro.ops {
        let _ = d.apply(op);
    }
    sawhorse_kernel::snap::set_plane_snapping(true);
    d
}

#[test]
fn a_section_of_a_bent_ring_wall_keeps_halves_that_make_its_tessellated_volume() {
    // Lane 23b's second fix pass (fuzz seed 20281103, snapping on, sequence 17,214): a 700 m
    // 14-gon offset 0.25 m inward and the ring pulled 0.25 m down, then the four corners of
    // one outer wall moved up a micrometre, bending the ring's top and bottom faces within
    // tolerance; a plane through the centre across x. Lane 23b measured each half as fans
    // over its kept face rings and held them to the fan over each whole ring: a ring round
    // a hole fans across the hole (1.5e6 m2 for a 1,063 m2 face), so the whole read 266.19 m3
    // against 265.72 tessellated, while the halves (whose rings the plane had split) read
    // 265.72. Both are now cones over the solid's own triangles, clipped to each side.
    let mut d = replay_find(
        "2026-10-20-section-of-a-700m-ring-wall-with-two-corners-nudged-a-micrometre.json",
    );
    let s = d.model().sections()[0].0;
    let whole: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
    assert!((whole - 265.721_959_175_5).abs() < 1e-9, "{whole}");
    let kept = d.model().kept_volume(s).unwrap();
    ok(&mut d, Op::FlipSection { section: s });
    let other = d.model().kept_volume(s).unwrap();
    // Measured: 9e-14 of the whole apart.
    assert!(
        (kept + other - whole).abs() <= 1e-11 * whole,
        "{kept} + {other} vs {whole}"
    );
    // The plane through the centre cuts the ring in two nearly equal halves.
    assert!((kept - other).abs() < 0.1, "{kept} {other}");
    sawhorse_kernel::testing::check_model(d.model()).unwrap();
}

#[test]
fn a_sliver_of_a_700m_circle_measures_its_planes_about_itself() {
    // Lane 23b's second fix pass (fuzz seed 20281207, snapping off, sequence 6,000; on
    // 1baa37d too): a 700 m arc drawn as 0.34 mm segments and a chord, copied turned 45
    // degrees, a 3.8 mm sliver of it pulled 0.5 m down. Its side faces lie within 1e-8 m of
    // their stored planes, whose normals tilt 2.5e-6 rad off their corners' across 3.8 mm.
    // The oracle's measure over the stored planes was taken about the world origin, 700 m
    // away, and read the 1.84e-6 m3 sliver as 3.17e-6; about the solid's own centre (as the
    // kept volume and `TriSolid::volume` are taken) it reads it to 1e-6 of itself.
    let d = replay_find("2026-10-20-section-of-a-sliver-of-a-700m-circle-snap-off.json");
    let whole: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
    assert!((whole - 1.842_746_758e-6).abs() < 1e-14, "{whole}");
    sawhorse_kernel::testing::check_model(d.model()).unwrap();
    // The section at z = 0 runs along the sliver's top face: one half keeps it all.
    let s = d.model().sections()[0].0;
    let kept = d.model().kept_volume(s).unwrap();
    let mut flipped = d.clone();
    flipped.apply(&Op::FlipSection { section: s }).unwrap();
    let other = flipped.model().kept_volume(s).unwrap();
    assert!(
        (kept + other - whole).abs() <= 1e-11 * whole,
        "{kept} + {other} vs {whole}"
    );
}
