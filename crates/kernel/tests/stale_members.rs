//! What a component's copies do when its definition loses a member (#148, #149). Each copy
//! is re-derived from the definition; what a copy holds that the definition no longer has
//! is removed there, a group or nested instance with everything in it.
//!
//! #148: the kernel panicked when the definition lost a group that held another group, and
//! another copy had a line drawn in the inner group. Those items are taken in the order of
//! the slots they are stored in, which is not always the order they were made in (a slot
//! freed by a delete is used again). When the outer group came first, it took the inner
//! group and its strokes with it, and the inner group's turn then read a stroke that was
//! gone.
//!
//! #149: what a person drew or placed in one copy (a stroke or a section plane: no
//! definition holds them) inside a group or nested instance the definition loses stays in
//! that copy, at the same place, in the nearest node around it that is still there: the
//! copy itself, or a group or nested instance of it the definition still has. That holds
//! at every depth. Only its parent changes: strokes and planes are stored in world
//! coordinates, so they stay where they were, with their ids, and are not joined to what
//! is already in their new parent. Planes are moved in the order the loop meets them (each
//! vanished item's own, then those inside a vanished nested instance): when the new parent
//! has no active plane, the first active one met stays active and the others are switched
//! off; when it has one, that one stays active and every plane moved in is switched off.
//! Make Component in another copy turns the group there into an instance: this copy's
//! group is replaced by a new instance, and what was drawn in the group goes to the
//! group's parent, not into that instance. Tidying is as it was: a copy left with nothing
//! but a kept line stays, and one left with nothing but a kept plane is removed with it,
//! as a group or copy holding only a plane of its own is (tidying does not count planes).
//!
//! Solids and open meshes the definition loses go from every copy too (each its own test).
//!
//! The recorded sequences that found the panic are in `tests/repro/` (replayed by
//! `tests/repro.rs`). Below: the lists from both issues, the other order, the planes, every
//! shape of what holds the kept items, rotated and mirrored copies, a kept stroke beside
//! another, emptied copies, and seeded models of components holding groups inside groups,
//! with strokes and section planes drawn in one copy's groups and the definition then
//! losing members through Delete, Make Component, Make Group, Explode, Make Unique, Undo
//! and Redo, in contexts opened several levels deep (Move Into is tried as well, and
//! refused in these models).

use std::collections::{BTreeMap, BTreeSet};

use sawhorse_kernel::testing::{check_model, check_saved, check_sequence};
use sawhorse_kernel::{Document, Op, OpOutcome};
use serde_json::{Value, json};

fn op(v: &Value) -> Op {
    serde_json::from_value(v.clone()).unwrap()
}

fn apply_all(ops: &[Value]) -> Document {
    let mut d = Document::new();
    for (i, v) in ops.iter().enumerate() {
        d.apply(&op(v))
            .unwrap_or_else(|e| panic!("op {i} {v}: {e}"));
    }
    d
}

fn plane(z: f64) -> Value {
    json!({"on": "plane", "origin": [0, 0, z], "normal": [0, 0, 1]})
}

fn line(x: f64, y: f64) -> Value {
    json!({"op": "draw", "target": plane(0.0), "points": [[x, y, 0], [x + 1.0, y, 0]], "closed": false})
}

fn section(z: f64) -> Value {
    json!({"op": "add_section", "origin": [0.5, 3.5, z], "normal": [1, 0, 0], "active": true})
}

const I3: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A 1 x 1 x 0.5 solid and a copy of it 2 along red.
fn two_solids() -> Vec<Value> {
    vec![
        json!({"op": "draw", "target": plane(0.0), "points": [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]], "closed": true}),
        json!({"op": "extrude", "sketch": 1, "region": 1, "distance": 0.5}),
        json!({"op": "copy", "objects": [1], "xform": {"linear": I3, "translation": [2, 0, 0]}}),
    ]
}

/// `n` solids: solid 1 and copies of it 2 apart along red (solids 2 to `n`).
fn solids(n: usize) -> Vec<Value> {
    let mut v = two_solids();
    for k in 2..n {
        v.push(json!({"op": "copy", "objects": [1], "xform": {"linear": I3, "translation": [2.0 * k as f64, 0, 0]}}));
    }
    v
}

fn copy_up(node: u64) -> Value {
    copy_by(node, I3, [0.0, 3.0, 0.0])
}

fn copy_by(node: u64, linear: [[f64; 3]; 3], translation: [f64; 3]) -> Value {
    json!({"op": "copy", "nodes": [node], "xform": {"linear": linear, "translation": translation}})
}

/// The list in #148: solid 1 in group 1, in which it is grouped again (group 2), a
/// component of group 1 and solid 2 (instance 3), a copy of it (instance 4, holding groups
/// 5 and 6, its counterparts of 1 and 2), a line drawn in group 6, then instance 3 opened.
/// Group 1 has the lower key, so the outer group comes first.
fn outer_first(line_in_inner: bool, planes: bool) -> Vec<Value> {
    let mut v = two_solids();
    v.extend([
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "set_context", "node": 1}),
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "leave_context"}),
        json!({"op": "make_component", "items": [{"node": 1}, {"object": 2}], "name": "C"}),
        copy_up(3),
    ]);
    if planes {
        v.push(json!({"op": "set_context", "node": 5}));
        v.push(section(0.2));
    }
    v.push(json!({"op": "set_context", "node": 6}));
    if line_in_inner {
        v.push(line(5.0, 5.0));
    }
    if planes {
        v.push(section(0.1));
    }
    v.push(json!({"op": "set_context", "node": 3}));
    v
}

/// The list in #149, the copy placed by `linear` and `translation`: solid 1 in group 1, a
/// component of it and solid 2 (instance 2), its copy (instance 3, holding group 4), a line
/// drawn in group 4 (sketch 2), then instance 2 opened and group 1 deleted.
fn issue_149(linear: [[f64; 3]; 3], translation: [f64; 3]) -> Vec<Value> {
    let mut v = two_solids();
    v.extend([
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "make_component", "items": [{"node": 1}, {"object": 2}], "name": "C"}),
        copy_by(2, linear, translation),
        json!({"op": "set_context", "node": 4}),
        line(5.0, 5.0),
        json!({"op": "set_context", "node": 2}),
        json!({"op": "delete_node", "node": 1}),
    ]);
    v
}

/// Public ids of the groups and instances, with their parents.
fn nodes(d: &Document) -> Vec<(u64, &'static str, Option<u64>)> {
    d.model()
        .graph()
        .nodes
        .iter()
        .map(|n| (n.id, n.kind, n.parent))
        .collect()
}

/// The nodes directly in `parent` of `kind`, in id order.
fn kids(d: &Document, parent: Option<u64>, kind: &str) -> Vec<u64> {
    nodes(d)
        .into_iter()
        .filter(|&(_, k, p)| k == kind && p == parent)
        .map(|(id, _, _)| id)
        .collect()
}

/// `(id, parent, active)` of every section plane.
fn planes(d: &Document) -> Vec<(u64, Option<u64>, bool)> {
    d.model()
        .graph()
        .sections
        .iter()
        .map(|s| (s.id, s.parent, s.active))
        .collect()
}

fn sketches(d: &Document) -> Vec<(u64, Option<u64>)> {
    d.model().graph().sketches
}

/// Where each sketch and plane is in the world, to the bit: a sketch's plane and frame,
/// its vertices and its edges; a plane's origin, normal and frame.
fn world(d: &Document) -> (BTreeMap<u64, String>, BTreeMap<u64, String>) {
    let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    let s = d
        .model()
        .sketches()
        .into_iter()
        .map(|(k, s)| {
            let p = s.plane();
            let frame = [p.origin, p.normal, p.u, p.v].map(|x| bits(&x.to_array()));
            let verts: Vec<_> = s.vertices().map(|(v, q)| (v, bits(&[q.x, q.y]))).collect();
            let edges: Vec<_> = s.edges().collect();
            (k, format!("{frame:?} {verts:?} {edges:?}"))
        })
        .collect();
    let p = d
        .model()
        .graph()
        .sections
        .iter()
        .map(|x| {
            (
                x.id,
                format!("{:?}", [x.origin, x.normal, x.u, x.v].map(|a| bits(&a))),
            )
        })
        .collect();
    (s, p)
}

/// The op is applied, and Undo gives back the model before it, Redo the model after it;
/// every stroke and plane is where it was in the world, with its id; the model saves and
/// opens as itself.
fn last_op_round_trips(ops: &[Value]) -> Document {
    let (head, last) = ops.split_at(ops.len() - 1);
    let mut d = apply_all(head);
    let before = d.hash();
    let (ws, wp) = world(&d);
    d.apply(&op(&last[0])).unwrap_or_else(|e| panic!("{e}"));
    check_model(d.model()).unwrap();
    check_saved(&d).unwrap();
    let after = d.hash();
    let (ws2, wp2) = world(&d);
    for (k, w) in &ws2 {
        if let Some(was) = ws.get(k) {
            assert_eq!(w, was, "sketch {k} stays where it was");
        }
    }
    for (k, w) in &wp2 {
        if let Some(was) = wp.get(k) {
            assert_eq!(w, was, "plane {k} stays where it was");
        }
    }
    d.apply(&op(&json!({"op": "undo"}))).unwrap();
    assert_eq!(d.hash(), before, "Undo gives back the model before the op");
    d.apply(&op(&json!({"op": "redo"}))).unwrap();
    assert_eq!(d.hash(), after, "Redo gives back the model after the op");
    check_sequence(&ops.iter().map(op).collect::<Vec<_>>()).unwrap();
    d
}

/// #149's list: the line drawn in the copy's group stays in the copy, the group goes.
#[test]
fn the_line_drawn_in_the_copys_group_stays_in_the_copy_149() {
    let v = issue_149(I3, [0.0, 3.0, 0.0]);
    let d = apply_all(&v[..v.len() - 1]);
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(4))]);
    let d = last_op_round_trips(&v);
    assert_eq!(
        nodes(&d),
        vec![(2, "instance", None), (3, "instance", None)]
    );
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(3))]);
    // Saved and opened, it is still there.
    let back = Document::open(d.model().snapshot()).unwrap();
    assert_eq!(sketches(&back), vec![(1, None), (2, Some(3))]);
    assert_eq!(back.hash(), d.hash());
}

/// A copy turned a quarter round, and one mirrored: the kept line is where it was, to the
/// bit (the world places are compared in `last_op_round_trips`).
#[test]
fn in_a_rotated_or_mirrored_copy_the_kept_line_stays_where_it_was_149() {
    for linear in [
        [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [[0.6, -0.8, 0.0], [0.8, 0.6, 0.0], [0.0, 0.0, 1.0]],
    ] {
        let v = issue_149(linear, [0.5, 3.25, 0.0]);
        let d = last_op_round_trips(&v);
        assert_eq!(sketches(&d), vec![(1, None), (2, Some(3))], "{linear:?}");
    }
}

#[test]
fn deleting_a_group_holding_a_group_with_a_line_in_another_copy() {
    let mut ops = outer_first(true, false);
    ops.push(json!({"op": "delete_node", "node": 1}));
    let d = last_op_round_trips(&ops);
    // Both copies lose both groups; the line in group 6 stays in copy 4 (#149).
    assert_eq!(
        nodes(&d),
        vec![(3, "instance", None), (4, "instance", None)]
    );
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(4))]);
}

#[test]
fn making_a_component_of_a_group_holding_a_group_with_a_line_in_another_copy() {
    let mut ops = outer_first(true, false);
    ops.push(json!({"op": "make_component", "items": [{"node": 1}], "name": "D"}));
    let d = last_op_round_trips(&ops);
    // Group 1 becomes the instance of D (it keeps its id); the copy gets its own instance
    // of D in place of groups 5 and 6. The line drawn in group 6 goes to the groups'
    // parent, copy 4, not into the new instance (#149).
    let n = nodes(&d);
    assert!(n.contains(&(1, "instance", Some(3))), "{n:?}");
    assert!(!n.iter().any(|&(id, _, _)| id == 5 || id == 6), "{n:?}");
    let new = kids(&d, Some(4), "instance");
    assert_eq!(new.len(), 1, "{n:?}");
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(4))]);
}

/// The planes of both vanished groups go to the copy, whichever order the groups come in,
/// and the one met first stays active: here the outer group's (plane 1, drawn in group 5),
/// as the kernel gives for the same model without the line. The line stays in the copy.
#[test]
fn the_planes_of_both_groups_go_to_the_copy_and_the_first_stays_active() {
    let mut with = outer_first(true, true);
    with.push(json!({"op": "delete_node", "node": 1}));
    let mut without = outer_first(false, true);
    without.push(json!({"op": "delete_node", "node": 1}));
    let (a, b) = (last_op_round_trips(&with), last_op_round_trips(&without));
    assert_eq!(planes(&a), vec![(1, Some(4), true), (2, Some(4), false)]);
    assert_eq!(planes(&a), planes(&b));
    assert_eq!(nodes(&a), nodes(&b));
    assert_eq!(sketches(&a), vec![(1, None), (2, Some(4))]);
    assert_eq!(sketches(&b), vec![(1, None)]);
}

/// The other order (the inner group made first, so it has the lower key), a line and a
/// plane in each of the copy's groups. The planes go to the copy, the inner group's met
/// first and active; both lines stay in the copy (#149).
#[test]
fn inner_group_first_its_lines_stay_in_the_copy_149() {
    let mut v = two_solids();
    v.extend([
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "make_group", "items": [{"node": 1}]}),
        json!({"op": "make_component", "items": [{"node": 2}, {"object": 2}], "name": "C"}),
        copy_up(3),
        json!({"op": "set_context", "node": 5}),
        line(5.0, 5.0),
        section(0.1),
        json!({"op": "set_context", "node": 6}),
        line(7.0, 5.0),
        section(0.2),
        json!({"op": "set_context", "node": 3}),
    ]);
    let d = apply_all(&v);
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(5)), (3, Some(6))]);
    v.push(json!({"op": "delete_node", "node": 2}));
    let d = last_op_round_trips(&v);
    assert_eq!(
        nodes(&d),
        vec![(3, "instance", None), (4, "instance", None)]
    );
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(4)), (3, Some(4))]);
    assert_eq!(planes(&d), vec![(1, Some(4), true), (2, Some(4), false)]);
}

/// When the copy already has an active plane of its own, that one stays active and both
/// planes moved in from the vanished groups are switched off.
#[test]
fn the_copys_own_active_plane_stays_active_and_both_moved_in_are_switched_off() {
    let mut v = outer_first(true, true);
    let last = v.pop().unwrap();
    v.extend([json!({"op": "set_context", "node": 4}), section(0.3), last]);
    let d = apply_all(&v);
    assert_eq!(
        planes(&d),
        vec![(1, Some(5), true), (2, Some(6), true), (3, Some(4), true)]
    );
    v.push(json!({"op": "delete_node", "node": 1}));
    let d = last_op_round_trips(&v);
    assert_eq!(
        planes(&d),
        vec![(1, Some(4), false), (2, Some(4), false), (3, Some(4), true)]
    );
}

/// Two planes in one vanished group, the second made active (which switches the first
/// off): both go to the copy as they were, the active one still active.
#[test]
fn two_planes_of_one_vanished_group_go_to_the_copy_as_they_were() {
    let mut v = outer_first(false, false);
    let last = v.pop().unwrap();
    v.extend([
        json!({"op": "set_context", "node": 5}),
        section(0.1),
        section(0.2),
        last,
    ]);
    let d = apply_all(&v);
    assert_eq!(planes(&d), vec![(1, Some(5), false), (2, Some(5), true)]);
    v.push(json!({"op": "delete_node", "node": 1}));
    let d = last_op_round_trips(&v);
    assert_eq!(planes(&d), vec![(1, Some(4), false), (2, Some(4), true)]);
}

/// Opens `path`, from the outermost node in, one node at a time.
fn open_path(v: &mut Vec<Value>, path: &[u64]) {
    for &n in path {
        v.push(json!({"op": "set_context", "node": n}));
    }
}

/// Solid 1 in group G, inside group K with solid 3, a component of K and solid 2, and a
/// copy of it: `(ops, first, K, G, copy, K', G')`.
fn group_in_group() -> (Vec<Value>, [u64; 6]) {
    let mut v = solids(3);
    v.extend([
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "make_group", "items": [{"node": 1}, {"object": 3}]}),
        json!({"op": "make_component", "items": [{"node": 2}, {"object": 2}], "name": "C"}),
    ]);
    let d = apply_all(&v);
    let first = kids(&d, None, "instance")[0];
    let k = kids(&d, Some(first), "group")[0];
    let g = kids(&d, Some(k), "group")[0];
    v.push(copy_up(first));
    let d = apply_all(&v);
    let copy = *kids(&d, None, "instance").last().unwrap();
    let k2 = kids(&d, Some(copy), "group")[0];
    let g2 = kids(&d, Some(k2), "group")[0];
    (v, [first, k, g, copy, k2, g2])
}

/// A group inside a group the definition keeps: what was drawn and placed in the copy's
/// counterpart of it goes to the kept group around it, not to the copy.
#[test]
fn a_vanished_group_inside_a_kept_group_hands_its_line_and_plane_to_the_kept_group() {
    let (mut v, [first, k, g, copy, k2, g2]) = group_in_group();
    open_path(&mut v, &[copy, k2, g2]);
    v.extend([line(5.0, 5.0), section(0.1)]);
    open_path(&mut v, &[first, k]);
    v.push(json!({"op": "delete_node", "node": g}));
    let d = last_op_round_trips(&v);
    assert!(!nodes(&d).iter().any(|&(id, _, _)| id == g2));
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(k2))]);
    assert_eq!(planes(&d), vec![(1, Some(k2), true)]);
}

/// The same, where the kept group has an active plane of its own: it stays active and the
/// one moved in is switched off.
#[test]
fn a_kept_groups_own_active_plane_stays_active() {
    let (mut v, [first, k, g, copy, k2, g2]) = group_in_group();
    open_path(&mut v, &[copy, k2]);
    v.push(section(0.3));
    open_path(&mut v, &[copy, k2, g2]);
    v.push(section(0.1));
    open_path(&mut v, &[first, k]);
    v.push(json!({"op": "delete_node", "node": g}));
    let d = last_op_round_trips(&v);
    assert_eq!(planes(&d), vec![(1, Some(k2), true), (2, Some(k2), false)]);
}

/// Component O holds component N (group G with solid 1, and solid 3) and solid 2; O is
/// copied: `(ops, [O, N, G, O', N', G'])`.
fn nested() -> (Vec<Value>, [u64; 6]) {
    let mut v = solids(3);
    v.push(json!({"op": "make_group", "items": [{"object": 1}]}));
    v.push(json!({"op": "make_component", "items": [{"node": 1}, {"object": 3}], "name": "N"}));
    let d = apply_all(&v);
    let n = kids(&d, None, "instance")[0];
    v.push(json!({"op": "make_component", "items": [{"node": n}, {"object": 2}], "name": "O"}));
    let d = apply_all(&v);
    let o = kids(&d, None, "instance")[0];
    let n = kids(&d, Some(o), "instance")[0];
    let g = kids(&d, Some(n), "group")[0];
    v.push(copy_up(o));
    let d = apply_all(&v);
    let o2 = *kids(&d, None, "instance").last().unwrap();
    let n2 = kids(&d, Some(o2), "instance")[0];
    let g2 = kids(&d, Some(n2), "group")[0];
    (v, [o, n, g, o2, n2, g2])
}

/// The comment of #149: a line and a plane in N's group, a line and a plane directly in N,
/// in the copy; N deleted in the first O. All four stay in the copy of O, where they were.
/// N's own plane is met first and stays active; the one from its group is switched off.
#[test]
fn a_vanished_nested_component_hands_everything_drawn_in_it_to_the_copy_149() {
    let (mut v, [o, n, _, o2, n2, g2]) = nested();
    open_path(&mut v, &[o2, n2, g2]);
    v.extend([line(5.0, 5.0), section(0.1)]);
    open_path(&mut v, &[o2, n2]);
    v.extend([line(5.0, 7.0), section(0.2)]);
    open_path(&mut v, &[o]);
    v.push(json!({"op": "delete_node", "node": n}));
    let d = apply_all(&v[..v.len() - 1]);
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(g2)), (3, Some(n2))]);
    let d = last_op_round_trips(&v);
    assert!(!nodes(&d).iter().any(|&(id, _, _)| id == n2 || id == g2));
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(o2)), (3, Some(o2))]);
    assert_eq!(planes(&d), vec![(1, Some(o2), false), (2, Some(o2), true)]);
}

/// A group inside a nested component the definition keeps: the copy's N keeps what was
/// drawn in its group.
#[test]
fn a_vanished_group_inside_a_kept_nested_component_hands_its_line_to_it() {
    let (mut v, [o, n, g, o2, n2, g2]) = nested();
    open_path(&mut v, &[o2, n2, g2]);
    v.extend([line(5.0, 5.0), section(0.1)]);
    open_path(&mut v, &[o, n]);
    v.push(json!({"op": "delete_node", "node": g}));
    let d = last_op_round_trips(&v);
    assert!(!nodes(&d).iter().any(|&(id, _, _)| id == g2));
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(n2))]);
    assert_eq!(planes(&d), vec![(1, Some(n2), true)]);
}

/// A nested component inside a group the definition keeps: the copy's group keeps what was
/// drawn in the copy's N and in N's group.
#[test]
fn a_vanished_nested_component_inside_a_kept_group_hands_its_line_to_the_group() {
    let mut v = solids(4);
    v.push(json!({"op": "make_group", "items": [{"object": 1}]}));
    v.push(json!({"op": "make_component", "items": [{"node": 1}, {"object": 3}], "name": "N"}));
    let d = apply_all(&v);
    let n = kids(&d, None, "instance")[0];
    v.push(json!({"op": "make_group", "items": [{"node": n}, {"object": 4}]}));
    let d = apply_all(&v);
    let k = kids(&d, None, "group")[0];
    v.push(json!({"op": "make_component", "items": [{"node": k}, {"object": 2}], "name": "O"}));
    let d = apply_all(&v);
    let o = kids(&d, None, "instance")[0];
    let k = kids(&d, Some(o), "group")[0];
    let n = kids(&d, Some(k), "instance")[0];
    v.push(copy_up(o));
    let d = apply_all(&v);
    let o2 = *kids(&d, None, "instance").last().unwrap();
    let k2 = kids(&d, Some(o2), "group")[0];
    let n2 = kids(&d, Some(k2), "instance")[0];
    let g2 = kids(&d, Some(n2), "group")[0];
    open_path(&mut v, &[o2, k2, n2]);
    v.extend([line(5.0, 5.0), section(0.1)]);
    open_path(&mut v, &[o2, k2, n2, g2]);
    v.extend([line(5.0, 7.0), section(0.2)]);
    open_path(&mut v, &[o, k]);
    v.push(json!({"op": "delete_node", "node": n}));
    let d = last_op_round_trips(&v);
    assert!(!nodes(&d).iter().any(|&(id, _, _)| id == n2 || id == g2));
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(k2)), (3, Some(k2))]);
    assert_eq!(planes(&d), vec![(1, Some(k2), true), (2, Some(k2), false)]);
}

/// Three groups inside each other, all three lost, made from the outside in (so the
/// innermost has the highest key and comes last in the loop): a line and a plane in the
/// innermost of the copy's go past both vanished groups around it to the copy.
#[test]
fn three_vanished_groups_inside_each_other_hand_their_line_to_the_copy() {
    let mut v = two_solids();
    v.extend([
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "set_context", "node": 1}),
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "set_context", "node": 2}),
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "leave_context"}),
        json!({"op": "leave_context"}),
        json!({"op": "make_component", "items": [{"node": 1}, {"object": 2}], "name": "C"}),
    ]);
    let d = apply_all(&v);
    let first = kids(&d, None, "instance")[0];
    let outer = kids(&d, Some(first), "group")[0];
    v.push(copy_up(first));
    let d = apply_all(&v);
    let copy = *kids(&d, None, "instance").last().unwrap();
    let a = kids(&d, Some(copy), "group")[0];
    let b = kids(&d, Some(a), "group")[0];
    let c = kids(&d, Some(b), "group")[0];
    assert!(
        a < b && b < c,
        "the innermost comes last in the loop: {a} {b} {c}"
    );
    open_path(&mut v, &[copy, a, b, c]);
    v.extend([line(5.0, 5.0), section(0.1)]);
    open_path(&mut v, &[first]);
    v.push(json!({"op": "delete_node", "node": outer}));
    let d = last_op_round_trips(&v);
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(copy))]);
    assert_eq!(planes(&d), vec![(1, Some(copy), true)]);
}

/// A nested component inside a group, both lost, the group taken first (it was made
/// first, so its counterpart in the copy has the lower key and goes first, with the
/// nested instance inside it): the plane placed in the nested component's group still
/// goes to the copy.
#[test]
fn a_nested_component_inside_a_vanished_group_taken_first_hands_its_plane_to_the_copy() {
    let mut v = solids(3);
    v.extend([
        json!({"op": "make_group", "items": [{"object": 1}, {"object": 3}]}),
        json!({"op": "set_context", "node": 1}),
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "make_component", "items": [{"node": 2}, {"object": 3}], "name": "N"}),
        json!({"op": "leave_context"}),
        json!({"op": "make_component", "items": [{"node": 1}, {"object": 2}], "name": "O"}),
    ]);
    let d = apply_all(&v);
    let o = kids(&d, None, "instance")[0];
    let k = kids(&d, Some(o), "group")[0];
    v.push(copy_up(o));
    let d = apply_all(&v);
    let o2 = *kids(&d, None, "instance").last().unwrap();
    let k2 = kids(&d, Some(o2), "group")[0];
    let n2 = kids(&d, Some(k2), "instance")[0];
    let g2 = kids(&d, Some(n2), "group")[0];
    assert!(k2 < n2, "the group comes first in the loop: {k2} {n2}");
    open_path(&mut v, &[o2, k2, n2, g2]);
    v.extend([line(5.0, 5.0), section(0.1)]);
    open_path(&mut v, &[o]);
    v.push(json!({"op": "delete_node", "node": k}));
    let d = last_op_round_trips(&v);
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(o2))]);
    assert_eq!(planes(&d), vec![(1, Some(o2), true)]);
}

/// A kept line arriving beside a line drawn in the copy itself, on the same plane: the two
/// stay two sketches, each with its own id, vertices and edges, nothing joined or split.
#[test]
fn a_kept_line_arrives_beside_the_copys_own_line_untouched() {
    let mut v = issue_149(I3, [0.0, 3.0, 0.0]);
    let last = v.pop().unwrap();
    let ctx = v.pop().unwrap();
    v.extend([
        json!({"op": "set_context", "node": 3}),
        // Across the kept line's place, and touching its end.
        json!({"op": "draw", "target": plane(0.0), "points": [[5.5, 4.5, 0], [5.5, 5.5, 0]], "closed": false}),
        json!({"op": "draw", "target": plane(0.0), "points": [[6, 5, 0], [7, 5, 0]], "closed": false}),
        ctx,
        last,
    ]);
    let d = apply_all(&v[..v.len() - 1]);
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(4)), (3, Some(3))]);
    let (before, _) = world(&d);
    let d = last_op_round_trips(&v);
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(3)), (3, Some(3))]);
    assert_eq!(world(&d).0, before);
}

/// The definition emptied in one copy (its group and solid deleted there, the copy open):
/// the other copy, left with only the line kept from its group, stays with the line; left
/// with only a kept plane, it is removed with the plane, as a copy holding only a plane of
/// its own is (tidying does not count planes, ADR-0005).
#[test]
fn an_emptied_copy_stays_with_a_kept_line_and_goes_with_a_kept_plane() {
    let run = |what: Value, own: bool| {
        let mut v = two_solids();
        v.extend([
            json!({"op": "make_group", "items": [{"object": 1}]}),
            json!({"op": "make_component", "items": [{"node": 1}, {"object": 2}], "name": "C"}),
            copy_up(2),
            json!({"op": "set_context", "node": 3}),
        ]);
        if !own {
            v.push(json!({"op": "set_context", "node": 4}));
        }
        v.extend([
            what,
            json!({"op": "leave_context"}),
            json!({"op": "set_context", "node": 2}),
            json!({"op": "delete_node", "node": 1}),
            json!({"op": "delete_object", "object": 2}),
        ]);
        last_op_round_trips(&v)
    };
    let d = run(line(5.0, 5.0), false);
    assert_eq!(
        nodes(&d),
        vec![(2, "instance", None), (3, "instance", None)]
    );
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(3))]);
    let d = run(section(0.1), false);
    assert_eq!(nodes(&d), vec![(2, "instance", None)]);
    assert!(planes(&d).is_empty());
    // A plane of the copy's own, not kept from anywhere: the same.
    let d = run(section(0.1), true);
    assert_eq!(nodes(&d), vec![(2, "instance", None)]);
    assert!(planes(&d).is_empty());
}

/// Explode of a group in one copy: the other copy's group goes, and what was drawn in it
/// goes to the group's parent, where Explode puts the first copy's.
#[test]
fn explode_in_one_copy_hands_the_other_copys_line_to_the_groups_parent() {
    let (mut v, [first, k, g, copy, k2, g2]) = group_in_group();
    open_path(&mut v, &[copy, k2, g2]);
    v.push(line(5.0, 5.0));
    open_path(&mut v, &[first, k, g]);
    v.push(line(5.0, 9.0));
    open_path(&mut v, &[first, k]);
    v.push(json!({"op": "explode", "node": g}));
    let d = last_op_round_trips(&v);
    assert_eq!(sketches(&d), vec![(1, None), (2, Some(k2)), (3, Some(k))]);
}

/// The ops that make a component of `items` (after `head`), copy its instance up and open
/// the copy; and the copy's id.
fn component_and_open_copy(mut head: Vec<Value>, items: Value) -> (Vec<Value>, u64) {
    head.push(json!({"op": "make_component", "items": items, "name": "C"}));
    let d = apply_all(&head);
    let first = nodes(&d)[0].0;
    head.push(copy_up(first));
    let d = apply_all(&head);
    let copy = nodes(&d)
        .into_iter()
        .find(|&(id, kind, parent)| kind == "instance" && parent.is_none() && id != first)
        .unwrap()
        .0;
    head.push(json!({"op": "set_context", "node": copy}));
    (head, copy)
}

/// A solid deleted in one copy leaves the definition, and so every copy.
#[test]
fn a_solid_the_definition_loses_goes_from_every_copy() {
    let (mut v, copy) =
        component_and_open_copy(two_solids(), json!([{"object": 1}, {"object": 2}]));
    let d = apply_all(&v);
    let g = d.model().graph();
    let mine: Vec<u64> = g
        .objects
        .iter()
        .filter(|o| o.parent == Some(copy))
        .map(|o| o.id)
        .collect();
    assert_eq!((g.objects.len(), mine.len()), (4, 2));
    v.push(json!({"op": "delete_object", "object": mine[0]}));
    let d = last_op_round_trips(&v);
    let g = d.model().graph();
    let per: BTreeMap<Option<u64>, usize> = g.objects.iter().fold(BTreeMap::new(), |mut m, o| {
        *m.entry(o.parent).or_default() += 1;
        m
    });
    assert_eq!(per.len(), 2, "{per:?}");
    assert!(per.values().all(|&n| n == 1), "{per:?}");
}

/// An open mesh deleted in one copy leaves the definition, and so every copy.
#[test]
fn an_open_mesh_the_definition_loses_goes_from_every_copy() {
    let mut head = two_solids()[..2].to_vec();
    head.extend([
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "set_context", "node": 1}),
        json!({"op": "import", "items": [{"name": "M", "positions": [[0, 0, 1], [0.5, 0, 1], [0, 0.5, 1]], "faces": [[[0, 1, 2]]]}]}),
        json!({"op": "leave_context"}),
    ]);
    let (mut v, copy) = component_and_open_copy(head, json!([{"node": 1}]));
    let d = apply_all(&v);
    let g = d.model().graph();
    assert_eq!(g.meshes.len(), 2);
    let mine = g.meshes.iter().find(|m| m.parent == Some(copy)).unwrap().id;
    v.push(json!({"op": "delete_mesh", "mesh": mine}));
    let d = last_op_round_trips(&v);
    assert!(d.model().graph().meshes.is_empty());
    assert_eq!(d.model().graph().objects.len(), 2);
}

/// A copy scaled evenly and one scaled unevenly: the kept line is where it was, to the bit.
#[test]
fn in_a_scaled_copy_the_kept_line_stays_where_it_was_149() {
    for linear in [
        [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]],
        [[1.5, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 0.0, 1.25]],
    ] {
        let v = issue_149(linear, [0.5, 3.25, 0.0]);
        let d = last_op_round_trips(&v);
        assert_eq!(sketches(&d), vec![(1, None), (2, Some(3))], "{linear:?}");
    }
}

// ---- unsaved work from a build before #149: the guarded replay ------------------------

/// The log the build before #149 (d1dbe4e) wrote for the #149 list with `mid` done in the
/// copy's group, then two rectangles (at z 2 and z 4) and the second one pushed, then Undo
/// and Redo when `history`; ids as that build gave them. `push` is the sketch id it gave
/// the second rectangle.
fn base_log(mid: &[Value], push: u64, history: bool) -> Vec<Op> {
    let rect = |z: f64, x: f64| json!({"op": "draw", "target": plane(z), "points": [[x, 0, z], [x + 1.0, 0, z], [x + 1.0, 1, z], [x, 1, z]], "closed": true});
    let mut v = vec![json!({"op": "new_sketch", "origin": [0, 0, 0], "normal": [0, 0, 1]})];
    v.extend(two_solids());
    v.extend([
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "make_component", "items": [{"node": 1}, {"object": 2}], "name": "C"}),
        copy_up(2),
        json!({"op": "set_context", "node": 4}),
    ]);
    v.extend(mid.iter().cloned());
    v.extend([
        json!({"op": "set_context", "node": 2}),
        json!({"op": "delete_node", "node": 1}),
        json!({"op": "leave_context"}),
        rect(2.0, 10.0),
        rect(4.0, 12.0),
        json!({"op": "extrude", "sketch": push, "region": 1, "distance": 0.25}),
    ]);
    if history {
        v.extend([json!({"op": "undo"}), json!({"op": "redo"})]);
    }
    v.iter().map(op).collect()
}

/// The pushed solid's box: `(min, max)`.
fn last_solid(d: &Document) -> ([f64; 3], [f64; 3]) {
    let m = d.model();
    let (_, o) = m.objects().into_iter().last().unwrap();
    let (lo, hi) = o.bounds();
    (lo.to_array(), hi.to_array())
}

/// The log of the second check: on the build before #149 the line was removed, so the
/// rectangles took sketch ids 4294967298 and 3 and the push named 3, the rectangle at z 4.
/// Replayed plainly here the line keeps id 2, the rectangles take 3 and 4, and the push
/// lands on the rectangle at z 2: accepted, and wrong. The guarded replay refuses it at the
/// Delete, the first step whose result differs; with Undo and Redo after the case too.
#[test]
fn the_guarded_replay_refuses_a_log_whose_step_differs_in_this_build() {
    let log = base_log(&[line(5.0, 5.0)], 3, false);
    let plain = Document::replay(&log).unwrap();
    assert_eq!(last_solid(&plain), ([10.0, 0.0, 2.0], [11.0, 1.0, 2.25]));
    let at = log
        .iter()
        .position(|o| matches!(o, Op::DeleteNode { .. }))
        .unwrap();
    match Document::replay_guarded(&log) {
        Err((i, e)) => {
            assert_eq!(i, at);
            assert_eq!(
                e.to_string(),
                "an older version set it aside, and one of its steps gives a different result in this version"
            );
        }
        Ok(_) => panic!("the guarded replay accepted a log whose Delete differs here"),
    }
    let mut undone = log[..=at].to_vec();
    undone.extend([op(&json!({"op": "undo"})), op(&json!({"op": "redo"}))]);
    assert_eq!(
        Document::replay_guarded(&undone).err().map(|(i, _)| i),
        Some(at)
    );
    // The guard is off again after it refused: a plain replay and live edits are as before.
    assert!(Document::replay(&log).is_ok());
}

/// A log with no such case (nothing drawn in the copy), and one where only the plane move of
/// the build before happens (a plane placed directly in the copy's group goes to the copy in
/// both builds), each with Undo and Redo at the end: accepted, and exactly the model and log
/// that build gave (its hashes, taken from its own WASM kernel).
#[test]
fn the_guarded_replay_accepts_what_both_builds_do_alike() {
    for (mid, model, log_hash) in [
        (vec![], "2bd3d91742bf2749", "aa7718f3312c4625"),
        (vec![section(0.2)], "829a7b3da6a70402", "25c4f8a9572b091e"),
    ] {
        let log = base_log(&mid, 3, true);
        let g = Document::replay_guarded(&log).unwrap_or_else(|(i, e)| panic!("op {i}: {e}"));
        let p = Document::replay(&log).unwrap();
        assert_eq!(g.hash_hex(), model);
        assert_eq!(g.log_hash_hex(), log_hash);
        assert_eq!((p.hash(), p.log_hash()), (g.hash(), g.log_hash()));
        assert_eq!(last_solid(&g), ([12.0, 0.0, 4.0], [13.0, 1.0, 4.25]));
    }
}

/// A log the build before #149 (d1dbe4e) wrote, steps and ids as it gave them (its WASM
/// kernel accepts it and replays it to itself). O holds N, a component of a group G and a
/// solid, and a solid; O is copied; a section plane, and nothing else, is placed in the
/// copy's G; N is deleted in the first O; then a plane is placed at the top and a rectangle
/// pushed. That build removed the copy's plane with N (it was inside the vanished nested
/// instance) and gave its freed id 4294967297 to the plane placed after; this build keeps it
/// in the copy, so the plane after is 2.
const PLANE_IN_A_VANISHED_NESTED_INSTANCE: &str = r#"[{"op":"new_sketch","origin":[0.0,0.0,0.0],"normal":[0.0,0.0,1.0]},{"op":"draw","target":{"on":"plane","origin":[0.0,0.0,0.0],"normal":[0.0,0.0,1.0]},"points":[[0.0,0.0,0.0],[1.0,0.0,0.0],[1.0,1.0,0.0],[0.0,1.0,0.0]],"closed":true},{"op":"extrude","sketch":1,"region":1,"distance":0.5},{"op":"copy","objects":[1],"xform":{"linear":[[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]],"translation":[2.0,0.0,0.0]}},{"op":"copy","objects":[1],"xform":{"linear":[[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]],"translation":[4.0,0.0,0.0]}},{"op":"make_group","items":[{"object":1}]},{"op":"make_component","items":[{"node":1},{"object":3}],"name":"N"},{"op":"make_component","items":[{"node":2},{"object":2}],"name":"O"},{"op":"copy","objects":[],"xform":{"linear":[[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]],"translation":[0.0,3.0,0.0]},"nodes":[3]},{"op":"set_context","node":4},{"op":"set_context","node":5},{"op":"set_context","node":6},{"op":"add_section","origin":[0.5,3.5,0.1],"normal":[1.0,0.0,0.0]},{"op":"set_context","node":3},{"op":"delete_node","node":2},{"op":"leave_context"},{"op":"add_section","origin":[9.0,0.5,0.1],"normal":[1.0,0.0,0.0]},{"op":"draw","target":{"on":"plane","origin":[0.0,0.0,2.0],"normal":[0.0,0.0,1.0]},"points":[[10.0,0.0,2.0],[11.0,0.0,2.0],[11.0,1.0,2.0],[10.0,1.0,2.0]],"closed":true},{"op":"extrude","sketch":2,"region":1,"distance":0.25}]"#;

/// The same, for a group G inside a group K the definition keeps: C holds K (G with a solid,
/// and a solid) and a solid; C is copied; a section plane, and nothing else, is placed in the
/// copy's G; G is deleted in the first copy's K. That build moved the plane to the copy (the
/// instance); this build moves it to the copy's K, the nearest node still there.
const PLANE_OF_A_GROUP_IN_A_KEPT_GROUP: &str = r#"[{"op":"new_sketch","origin":[0.0,0.0,0.0],"normal":[0.0,0.0,1.0]},{"op":"draw","target":{"on":"plane","origin":[0.0,0.0,0.0],"normal":[0.0,0.0,1.0]},"points":[[0.0,0.0,0.0],[1.0,0.0,0.0],[1.0,1.0,0.0],[0.0,1.0,0.0]],"closed":true},{"op":"extrude","sketch":1,"region":1,"distance":0.5},{"op":"copy","objects":[1],"xform":{"linear":[[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]],"translation":[2.0,0.0,0.0]}},{"op":"copy","objects":[1],"xform":{"linear":[[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]],"translation":[4.0,0.0,0.0]}},{"op":"make_group","items":[{"object":1}]},{"op":"make_group","items":[{"node":1},{"object":3}]},{"op":"make_component","items":[{"node":2},{"object":2}],"name":"C"},{"op":"copy","objects":[],"xform":{"linear":[[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]],"translation":[0.0,3.0,0.0]},"nodes":[3]},{"op":"set_context","node":4},{"op":"set_context","node":6},{"op":"set_context","node":5},{"op":"add_section","origin":[0.5,3.5,0.1],"normal":[1.0,0.0,0.0]},{"op":"set_context","node":3},{"op":"set_context","node":2},{"op":"delete_node","node":1},{"op":"leave_context"},{"op":"add_section","origin":[9.0,0.5,0.1],"normal":[1.0,0.0,0.0]},{"op":"draw","target":{"on":"plane","origin":[0.0,0.0,2.0],"normal":[0.0,0.0,1.0]},"points":[[10.0,0.0,2.0],[11.0,0.0,2.0],[11.0,1.0,2.0],[10.0,1.0,2.0]],"closed":true},{"op":"extrude","sketch":2,"region":1,"distance":0.25}]"#;

/// Planes alone, no stroke in any node that goes: the guarded replay refuses `log` at its
/// Delete, the first step whose result differs, with the reason sentence.
fn refused_at_the_delete(log: &[Op]) {
    let at = log
        .iter()
        .position(|o| matches!(o, Op::DeleteNode { .. }))
        .unwrap();
    match Document::replay_guarded(log) {
        Err((i, e)) => {
            assert_eq!(i, at, "refused at the Delete");
            assert_eq!(
                e.to_string(),
                "an older version set it aside, and one of its steps gives a different result in this version"
            );
        }
        Ok(_) => panic!("the guarded replay accepted a log whose Delete differs here"),
    }
}

/// `(id, parent)` of every section plane.
fn plane_parents(d: &Document) -> Vec<(u64, Option<u64>)> {
    d.model()
        .graph()
        .sections
        .iter()
        .map(|s| (s.id, s.parent))
        .collect()
}

/// A plane inside a vanished nested instance, and no stroke: refused guarded; replayed
/// plainly it is accepted, the plane kept in the copy (instance 4), where the build before
/// removed it.
#[test]
fn the_guarded_replay_refuses_a_plane_inside_a_vanished_nested_instance() {
    let log: Vec<Op> = serde_json::from_str(PLANE_IN_A_VANISHED_NESTED_INSTANCE).unwrap();
    refused_at_the_delete(&log);
    let plain = Document::replay(&log).unwrap();
    assert_eq!(plane_parents(&plain), vec![(1, Some(4)), (2, None)]);
}

/// A vanished group's own plane that goes to a kept group, and no stroke: refused guarded;
/// replayed plainly it is accepted, the plane in the kept group (6), where the build before
/// moved it to the copy (instance 4).
#[test]
fn the_guarded_replay_refuses_a_plane_going_to_a_kept_group() {
    let log: Vec<Op> = serde_json::from_str(PLANE_OF_A_GROUP_IN_A_KEPT_GROUP).unwrap();
    refused_at_the_delete(&log);
    let plain = Document::replay(&log).unwrap();
    assert_eq!(plane_parents(&plain), vec![(1, Some(6)), (2, Some(3))]);
}

/// Work made in this build: its log replays plainly to the model it was, the line kept, and
/// a guarded replay of it is refused (it is never asked for: such work carries the mark).
#[test]
fn this_builds_own_log_replays_plainly_as_it_was() {
    let v = issue_149(I3, [0.0, 3.0, 0.0]);
    let mut d = apply_all(&v);
    d.apply(&op(&json!({"op": "undo"}))).unwrap();
    d.apply(&op(&json!({"op": "redo"}))).unwrap();
    let r = Document::replay(d.log()).unwrap();
    assert_eq!((r.hash(), r.log_hash()), (d.hash(), d.log_hash()));
    assert_eq!(sketches(&r), vec![(1, None), (2, Some(3))]);
    assert!(Document::replay_guarded(d.log()).is_err());
}

// ---- seeded models of the family -----------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> Option<T> {
        (!v.is_empty()).then(|| v[self.below(v.len())])
    }
}

/// What the tests read of a model: each node's parent and kind, each stroke's parent and
/// points, each plane's parent, flag and origin.
struct View {
    parent: BTreeMap<u64, Option<u64>>,
    kind: BTreeMap<u64, &'static str>,
    strokes: BTreeMap<u64, (Option<u64>, String)>,
    planes: BTreeMap<u64, (Option<u64>, bool, [u64; 3])>,
}

fn view(d: &Document) -> View {
    let g = d.model().graph();
    let points: BTreeMap<u64, String> = d
        .model()
        .sketches()
        .into_iter()
        .map(|(k, s)| (k, format!("{:?}", s.vertices().collect::<Vec<_>>())))
        .collect();
    View {
        parent: g.nodes.iter().map(|n| (n.id, n.parent)).collect(),
        kind: g.nodes.iter().map(|n| (n.id, n.kind)).collect(),
        strokes: g
            .sketches
            .iter()
            .map(|&(k, p)| (k, (p, points[&k].clone())))
            .collect(),
        planes: g
            .sections
            .iter()
            .map(|s| (s.id, (s.parent, s.active, s.origin.map(f64::to_bits))))
            .collect(),
    }
}

impl View {
    /// `n` and the nodes it is in, innermost first.
    fn path(&self, n: Option<u64>) -> Vec<u64> {
        let mut out = Vec::new();
        let mut at = n;
        while let Some(k) = at {
            out.push(k);
            at = self.parent.get(&k).copied().flatten();
        }
        out
    }
    fn below(&self, n: u64) -> Vec<u64> {
        self.parent
            .keys()
            .copied()
            .filter(|&k| self.path(Some(k)).contains(&n))
            .collect()
    }
}

/// A random model of the family (the ops, applied): two to four solids, groups made of
/// them inside groups (some made inside an opened group), a component of what is at the
/// top, one to three copies of it, and strokes and section planes drawn in random groups
/// of the copies, each opened from the top through the groups around it.
fn family(seed: u64) -> (Document, Vec<Value>) {
    let mut r = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut d = Document::new();
    let mut log = Vec::new();
    let go = |d: &mut Document, log: &mut Vec<Value>, v: Value| {
        let ok = d.apply(&op(&v)).is_ok();
        if ok {
            log.push(v);
        }
        ok
    };
    for k in 0..2 + r.below(3) {
        let x = 2.0 * k as f64;
        let draw = json!({"op": "draw", "target": plane(0.0), "points": [[x, 0, 0], [x + 1.0, 0, 0], [x + 1.0, 1, 0], [x, 1, 0]], "closed": true});
        if let Ok(OpOutcome::Drawn {
            sketch: Some(sketch),
            inside: Some(region),
            ..
        }) = d.apply(&op(&draw))
        {
            log.push(draw);
            go(
                &mut d,
                &mut log,
                json!({"op": "extrude", "sketch": sketch, "region": region, "distance": 0.5}),
            );
        }
    }
    for _ in 0..3 + r.below(4) {
        let g = d.model().graph();
        let ctx = g.context;
        let here: Vec<Value> = g
            .nodes
            .iter()
            .filter(|n| n.parent == ctx)
            .map(|n| json!({"node": n.id}))
            .chain(
                g.objects
                    .iter()
                    .filter(|o| o.parent == ctx)
                    .map(|o| json!({"object": o.id})),
            )
            .collect();
        let groups: Vec<u64> = g
            .nodes
            .iter()
            .filter(|n| n.parent == ctx)
            .map(|n| n.id)
            .collect();
        match r.below(4) {
            0 if !groups.is_empty() && g.path.len() < 3 => {
                let n = r.pick(&groups).unwrap();
                go(&mut d, &mut log, json!({"op": "set_context", "node": n}));
            }
            1 if ctx.is_some() => {
                go(&mut d, &mut log, json!({"op": "leave_context"}));
            }
            _ if !here.is_empty() => {
                let items: Vec<Value> = here.into_iter().filter(|_| r.below(3) != 0).collect();
                if !items.is_empty() {
                    go(
                        &mut d,
                        &mut log,
                        json!({"op": "make_group", "items": items}),
                    );
                }
            }
            _ => {}
        }
    }
    while d.model().context().is_some() {
        go(&mut d, &mut log, json!({"op": "leave_context"}));
    }
    let g = d.model().graph();
    let top: Vec<Value> = g
        .nodes
        .iter()
        .filter(|n| n.parent.is_none())
        .map(|n| json!({"node": n.id}))
        .chain(
            g.objects
                .iter()
                .filter(|o| o.parent.is_none())
                .map(|o| json!({"object": o.id})),
        )
        .collect();
    go(
        &mut d,
        &mut log,
        json!({"op": "make_component", "items": top, "name": "C"}),
    );
    let first = d
        .model()
        .graph()
        .nodes
        .iter()
        .find(|n| n.parent.is_none())
        .map(|n| n.id);
    if let Some(first) = first {
        for k in 0..1 + r.below(3) {
            go(
                &mut d,
                &mut log,
                json!({"op": "copy", "nodes": [first], "xform": {"linear": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": [0, 3.0 * (k + 1) as f64, 0]}}),
            );
        }
    }
    // Strokes and planes in random groups of random copies.
    for k in 0..2 + r.below(5) {
        let v = view(&d);
        let groups: Vec<u64> = v
            .kind
            .iter()
            .filter(|&(_, &kind)| kind == "group")
            .map(|(&n, _)| n)
            .collect();
        let Some(n) = r.pick(&groups) else { break };
        while d.model().context().is_some() {
            go(&mut d, &mut log, json!({"op": "leave_context"}));
        }
        let mut path = v.path(Some(n));
        path.reverse();
        for p in path {
            go(&mut d, &mut log, json!({"op": "set_context", "node": p}));
        }
        if r.below(3) != 0 {
            go(&mut d, &mut log, line(10.0 + k as f64, 5.0));
        }
        if r.below(2) == 0 {
            let mut s = section(0.05 + 0.04 * k as f64);
            s["active"] = json!(r.below(2) == 0);
            go(&mut d, &mut log, s);
        }
    }
    while d.model().context().is_some() {
        go(&mut d, &mut log, json!({"op": "leave_context"}));
    }
    (d, log)
}

/// One random op of the second phase: open a random group or instance (from the top,
/// through the nodes around it) or not, then an op that can make a definition lose
/// members, or Undo or Redo.
fn edit(r: &mut Rng, d: &mut Document, log: &mut Vec<Value>) -> Vec<Value> {
    let v = view(d);
    let all: Vec<u64> = v.parent.keys().copied().collect();
    let mut out = Vec::new();
    if r.below(3) != 0
        && let Some(n) = r.pick(&all)
    {
        out.push(json!({"op": "leave_context"}));
        let mut path = v.path(Some(n));
        path.reverse();
        for p in path {
            out.push(json!({"op": "set_context", "node": p}));
        }
    }
    for o in &out {
        if d.apply(&op(o)).is_ok() {
            log.push(o.clone());
        }
    }
    let g = d.model().graph();
    let ctx = g.context;
    let here: Vec<u64> = g
        .nodes
        .iter()
        .filter(|n| n.parent == ctx)
        .map(|n| n.id)
        .collect();
    let solids: Vec<u64> = g
        .objects
        .iter()
        .filter(|o| o.parent == ctx)
        .map(|o| o.id)
        .collect();
    let some = |r: &mut Rng| -> Vec<Value> {
        let mut items: Vec<Value> = Vec::new();
        for &n in &here {
            if r.below(2) == 0 {
                items.push(json!({"node": n}));
            }
        }
        for &o in &solids {
            if r.below(3) == 0 {
                items.push(json!({"object": o}));
            }
        }
        items
    };
    let e = match r.below(9) {
        0 | 1 => r
            .pick(&here)
            .map(|n| json!({"op": "delete_node", "node": n})),
        2 | 3 => Some(json!({"op": "make_component", "items": some(r), "name": "D"})),
        4 => Some(json!({"op": "make_group", "items": some(r)})),
        5 => r.pick(&here).map(|n| json!({"op": "explode", "node": n})),
        6 => {
            let into = r.pick(&here);
            let items: Vec<Value> = some(r)
                .into_iter()
                .filter(|i| i.get("node").and_then(Value::as_u64) != into)
                .collect();
            Some(json!({"op": "move_into", "items": items, "into": into}))
        }
        7 => r
            .pick(&all)
            .map(|n| json!({"op": "make_unique", "node": n})),
        _ => Some(json!({"op": if r.below(2) == 0 { "undo" } else { "redo" }})),
    };
    e.into_iter().collect()
}

/// The checks of one accepted edit `e` from `before` to `after` (see the module notes): a
/// stroke or plane whose node vanished is, unless that node was deleted by the edit itself
/// (or sat inside the node it deleted), in the innermost node around it that is still
/// there (or in a node the edit made inside that one), at the same place and with the same
/// id (#149; an Explode puts the exploded group's own there too). A plane may be gone with that node only when nothing around it
/// is left, or when the node that would hold it held nothing else and was tidied away.
/// Every other stroke and plane is where it was, as it was.
fn check_edit(e: &Value, before: &View, after: &View) -> Result<(), String> {
    let gone = |n: Option<u64>| n.is_some_and(|k| !after.parent.contains_key(&k));
    let deleted: BTreeSet<u64> = if e["op"] == "delete_node" {
        before
            .below(e["node"].as_u64().unwrap())
            .into_iter()
            .collect()
    } else {
        BTreeSet::new()
    };
    // The innermost node of `p` and the nodes around it that is still there.
    let home = |p: Option<u64>| -> Option<u64> {
        before
            .path(p)
            .into_iter()
            .find(|k| after.parent.contains_key(k))
    };
    // There, or in a node the edit made inside it (Make Component wrapping the item's
    // group, which then vanished: what was in it stays in the new instance).
    let held = |q: Option<u64>, p: Option<u64>| {
        let h = home(p);
        q == h
            || q.is_some_and(|k| {
                !before.parent.contains_key(&k)
                    && h.is_some_and(|h| after.path(Some(k)).contains(&h))
            })
    };
    for (s, (p, pts)) in &before.strokes {
        let now = after.strokes.get(s);
        if gone(*p) {
            if p.is_some_and(|k| deleted.contains(&k)) {
                if now.is_some() {
                    return Err(format!("stroke {s} of deleted {p:?} is still there"));
                }
            } else if !now.is_some_and(|(q, x)| held(*q, *p) && x == pts) {
                return Err(format!(
                    "stroke {s} of vanished {p:?} is {now:?}, not in {:?}",
                    home(*p)
                ));
            }
        } else if now != Some(&(*p, pts.clone())) {
            return Err(format!("stroke {s} in {p:?} moved: {now:?}"));
        }
    }
    for (s, (p, active, at)) in &before.planes {
        let now = after.planes.get(s);
        if gone(*p) {
            if p.is_some_and(|k| deleted.contains(&k)) {
                if now.is_some() {
                    return Err(format!("plane {s} of deleted {p:?} is still there"));
                }
            } else if let Some((q, _, at2)) = now {
                if at2 != at || !held(*q, *p) {
                    return Err(format!(
                        "plane {s} of vanished {p:?} is in {q:?}, not in {:?}",
                        home(*p)
                    ));
                }
            } else {
                // Its holder went too: nothing around it is left, or the innermost node of
                // its path that is gone but whose parent is left held no stroke and no
                // node before the edit (so, with what came in, nothing but planes).
                let path = before.path(*p);
                let left = path.iter().position(|k| after.parent.contains_key(k));
                let tidied = match left {
                    None => true,
                    Some(i) => {
                        i > 0 && {
                            let h = path[i - 1];
                            !before.strokes.values().any(|(q, _)| *q == Some(h))
                        }
                    }
                };
                if !tidied {
                    return Err(format!("plane {s} of vanished {p:?} is gone"));
                }
            }
        } else if now.is_none_or(|(q, a, at2)| q != p || a != active || at2 != at) {
            return Err(format!("plane {s} in {p:?} changed: {now:?}"));
        }
    }
    Ok(())
}

const MODELS: u64 = 300;
const EDITS: usize = 12;

#[test]
fn seeded_models_lose_groups_and_keep_what_was_drawn_in_one_copy() {
    let mut accepted = BTreeMap::<String, usize>::new();
    let mut kept_strokes = 0usize;
    let mut kept_planes = 0usize;
    let mut from_instances = 0usize;
    for seed in 1..=MODELS {
        let (mut d, mut log) = family(seed);
        let mut r = Rng(seed ^ 0x5eed_0148);
        for _ in 0..EDITS {
            for e in edit(&mut r, &mut d, &mut log) {
                let before = view(&d);
                let h = d.hash();
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut next = d.clone();
                    let out = next.apply(&op(&e));
                    (next, out)
                }));
                let (next, out) = res.unwrap_or_else(|_| {
                    panic!(
                        "seed {seed}: {e} panicked after\n{}",
                        Value::from(log.clone())
                    )
                });
                if out.is_err() {
                    continue;
                }
                d = next;
                log.push(e.clone());
                let name = e["op"].as_str().unwrap().to_string();
                *accepted.entry(name.clone()).or_default() += 1;
                check_model(d.model()).unwrap_or_else(|x| panic!("seed {seed}: {e}: {x}"));
                if name == "undo" || name == "redo" {
                    continue;
                }
                let after = view(&d);
                check_edit(&e, &before, &after).unwrap_or_else(|x| {
                    panic!(
                        "seed {seed}: {e}: {x}
before {:?} {:?}
after {:?} {:?}",
                        before.parent, before.kind, after.parent, after.kind
                    )
                });
                let vanished = |p: &Option<u64>| p.is_some_and(|k| !after.parent.contains_key(&k));
                let in_instance = |p: &Option<u64>| {
                    before
                        .path(*p)
                        .iter()
                        .any(|k| before.kind[k] == "instance" && !after.parent.contains_key(k))
                };
                for (k, (p, _)) in &before.strokes {
                    if vanished(p) && after.strokes.contains_key(k) {
                        kept_strokes += 1;
                        from_instances += usize::from(in_instance(p));
                    }
                }
                for (k, (p, ..)) in &before.planes {
                    if vanished(p) && after.planes.contains_key(k) {
                        kept_planes += 1;
                        from_instances += usize::from(in_instance(p));
                    }
                }
                let h2 = d.hash();
                let mut u = d.clone();
                u.apply(&op(&json!({"op": "undo"}))).unwrap();
                assert_eq!(u.hash(), h, "seed {seed}: Undo of {e}");
                u.apply(&op(&json!({"op": "redo"}))).unwrap();
                assert_eq!(u.hash(), h2, "seed {seed}: Redo of {e}");
            }
        }
    }
    println!(
        "accepted {accepted:?}; strokes kept {kept_strokes}; planes kept {kept_planes}; of them from a vanished instance {from_instances}"
    );
    // The family reaches what it is for: strokes and planes in groups and instances that
    // vanish, through the ops it names (the numbers are the same on every run). Move Into
    // is tried too and always refused in these models: inside an open component it is,
    // and at the top there is no group to move into.
    assert!(kept_strokes > 0 && kept_planes > 0 && from_instances > 0);
    assert!(!accepted.contains_key("move_into"));
    for k in [
        "delete_node",
        "make_component",
        "make_group",
        "explode",
        "make_unique",
        "undo",
        "redo",
    ] {
        assert!(
            accepted.get(k).is_some_and(|&n| n > 0),
            "no {k} accepted: {accepted:?}"
        );
    }
}
