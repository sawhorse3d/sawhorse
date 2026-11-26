//! #148: the kernel panicked when a component's definition lost a group that held another
//! group, and another copy of the component had a line drawn in the inner group. Each copy
//! is re-derived from the definition; what a copy holds that the definition no longer has
//! is removed there, a group with everything in it. Those items are taken in the order of
//! the slots they are stored in, which is not always the order they were made in (a slot
//! freed by a delete is used again). When the outer group came first, it took the inner
//! group and its strokes with it, and the inner group's turn then read a stroke that was
//! gone.
//!
//! The result is what the kernel gives when the inner group comes first, which never
//! panicked: the strokes of every vanished group are removed with it, and its section
//! planes go to the copy. When the copy has no active plane of its own, the first active
//! one met stays active there and the others are switched off; when it has one, that one
//! stays active and every plane moved in is switched off. Whether a line drawn in a group
//! that vanishes this way should stay is an open question, #149; the tests marked with it
//! hold today's result, so a decision there changes them on purpose.
//!
//! Solids and open meshes the definition loses go from every copy too (each its own test).
//!
//! The recorded sequences that found the panic are in `tests/repro/` (replayed by
//! `tests/repro.rs`). Below: the two short lists from the issue, the result of the other
//! order and of a single group, the planes, and seeded models of components holding
//! groups inside groups, with strokes and section planes drawn in one copy's groups and
//! the definition then losing members through Delete, Make Component, Make Group, Explode,
//! Make Unique, Undo and Redo, in contexts opened several levels deep (Move Into is tried
//! as well, and refused in these models).

use std::collections::{BTreeMap, BTreeSet};

use sawhorse_kernel::testing::{check_model, check_sequence};
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

/// A 1 x 1 x 0.5 solid and a copy of it 2 along red.
fn two_solids() -> Vec<Value> {
    vec![
        json!({"op": "draw", "target": plane(0.0), "points": [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]], "closed": true}),
        json!({"op": "extrude", "sketch": 1, "region": 1, "distance": 0.5}),
        json!({"op": "copy", "objects": [1], "xform": {"linear": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": [2, 0, 0]}}),
    ]
}

fn copy_up(node: u64) -> Value {
    json!({"op": "copy", "nodes": [node], "xform": {"linear": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": [0, 3, 0]}})
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

/// Public ids of the groups and instances, with their parents.
fn nodes(d: &Document) -> Vec<(u64, &'static str, Option<u64>)> {
    d.model()
        .graph()
        .nodes
        .iter()
        .map(|n| (n.id, n.kind, n.parent))
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

/// The op is applied, and Undo gives back the model before it, Redo the model after it.
fn last_op_round_trips(ops: &[Value]) -> Document {
    let (head, last) = ops.split_at(ops.len() - 1);
    let mut d = apply_all(head);
    let before = d.hash();
    d.apply(&op(&last[0])).unwrap_or_else(|e| panic!("{e}"));
    check_model(d.model()).unwrap();
    let after = d.hash();
    d.apply(&op(&json!({"op": "undo"}))).unwrap();
    assert_eq!(d.hash(), before, "Undo gives back the model before the op");
    d.apply(&op(&json!({"op": "redo"}))).unwrap();
    assert_eq!(d.hash(), after, "Redo gives back the model after the op");
    check_sequence(&ops.iter().map(op).collect::<Vec<_>>()).unwrap();
    d
}

#[test]
fn deleting_a_group_holding_a_group_with_a_line_in_another_copy() {
    let mut ops = outer_first(true, false);
    ops.push(json!({"op": "delete_node", "node": 1}));
    let d = last_op_round_trips(&ops);
    // Both copies lose both groups; the line in group 6 goes with them (#149).
    assert_eq!(
        nodes(&d),
        vec![(3, "instance", None), (4, "instance", None)]
    );
    assert_eq!(sketches(&d), vec![(1, None)]);
}

#[test]
fn making_a_component_of_a_group_holding_a_group_with_a_line_in_another_copy() {
    let mut ops = outer_first(true, false);
    ops.push(json!({"op": "make_component", "items": [{"node": 1}], "name": "D"}));
    let d = last_op_round_trips(&ops);
    // Group 1 becomes the instance of D (it keeps its id); the copy gets its own instance
    // of D in place of groups 5 and 6, and the line in group 6 goes with them (#149).
    let n = nodes(&d);
    assert!(n.contains(&(1, "instance", Some(3))), "{n:?}");
    assert!(!n.iter().any(|&(id, _, _)| id == 5 || id == 6), "{n:?}");
    assert_eq!(sketches(&d), vec![(1, None)]);
}

/// The planes of both vanished groups go to the copy, whichever order the groups come in,
/// and the one met first stays active: here the outer group's (plane 1, drawn in group 5),
/// as the kernel gives for the same model without the line, where it never panicked.
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
    assert_eq!(sketches(&a), sketches(&b));
}

/// #149, today's result: the other order (the inner group made first, so it has the lower
/// key), a line and a plane in each of the copy's groups. The planes go to the copy, the
/// inner group's met first and active; both lines are removed with the groups.
#[test]
fn inner_group_first_its_lines_go_with_the_groups_149() {
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
    assert_eq!(sketches(&d), vec![(1, None)]);
    assert_eq!(planes(&d), vec![(1, Some(4), true), (2, Some(4), false)]);
}

/// #149, today's result: one vanished group, nothing inside it but a solid; the line
/// drawn in the copy's group is removed with it.
#[test]
fn a_single_group_its_line_goes_with_it_149() {
    let mut v = two_solids();
    v.extend([
        json!({"op": "make_group", "items": [{"object": 1}]}),
        json!({"op": "make_component", "items": [{"node": 1}, {"object": 2}], "name": "C"}),
        copy_up(2),
        json!({"op": "set_context", "node": 4}),
        line(5.0, 5.0),
        json!({"op": "set_context", "node": 2}),
        json!({"op": "delete_node", "node": 1}),
    ]);
    let d = last_op_round_trips(&v);
    assert_eq!(
        nodes(&d),
        vec![(2, "instance", None), (3, "instance", None)]
    );
    assert_eq!(sketches(&d), vec![(1, None)]);
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

/// The checks of one accepted edit `e` from `before` to `after` (see the module notes):
/// a stroke whose group vanished is gone (#149, today's result), unless the group was
/// exploded, when it is in the group's parent; a plane whose node vanished is either in a
/// node that held that node, at the same place, or gone, and gone only when its node was
/// deleted itself, sat inside an instance that vanished, lost every node around it (a copy
/// emptied and tidied away), or the edit was an Explode; every other stroke and plane is
/// where it was, as it was.
fn check_edit(e: &Value, before: &View, after: &View) -> Result<(), String> {
    let gone = |n: Option<u64>| n.is_some_and(|k| !after.parent.contains_key(&k));
    let exploded = (e["op"] == "explode").then(|| e["node"].as_u64().unwrap());
    let deleted: BTreeSet<u64> = if e["op"] == "delete_node" {
        before
            .below(e["node"].as_u64().unwrap())
            .into_iter()
            .collect()
    } else {
        BTreeSet::new()
    };
    for (s, (p, pts)) in &before.strokes {
        let now = after.strokes.get(s);
        if gone(*p) {
            if exploded.is_some() && *p == exploded {
                let up = before.parent[&p.unwrap()];
                if now != Some(&(up, pts.clone())) {
                    return Err(format!("stroke {s} of the exploded group: {now:?}"));
                }
            } else if now.is_some() {
                return Err(format!("stroke {s} of vanished group {p:?} is still there"));
            }
        } else if now != Some(&(*p, pts.clone())) {
            return Err(format!("stroke {s} in {p:?} moved: {now:?}"));
        }
    }
    for (s, (p, active, at)) in &before.planes {
        let now = after.planes.get(s);
        if gone(*p) {
            if let Some((q, _, at2)) = now {
                let held = before.path(*p);
                if at2 != at || !q.is_none_or(|q| held.contains(&q)) {
                    return Err(format!(
                        "plane {s} of vanished {p:?} is in {q:?}, not a node around it"
                    ));
                }
            } else if !p.is_some_and(|k| deleted.contains(&k)) && exploded.is_none() {
                // Removed only where its node was deleted itself, inside an instance that
                // vanished whole, or with every node around it (an emptied copy tidied).
                let path = before.path(*p);
                let in_instance = path
                    .iter()
                    .any(|k| before.kind[k] == "instance" && gone(Some(*k)));
                let around_left = path.iter().any(|k| after.parent.contains_key(k));
                if !in_instance && around_left {
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
fn seeded_models_lose_groups_without_a_panic_and_as_before() {
    let mut accepted = BTreeMap::<String, usize>::new();
    let mut vanished_strokes = 0usize;
    let mut moved_planes = 0usize;
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
                check_edit(&e, &before, &after).unwrap_or_else(|x| panic!("seed {seed}: {e}: {x}"));
                vanished_strokes += before
                    .strokes
                    .iter()
                    .filter(|(_, (p, _))| p.is_some_and(|k| !after.parent.contains_key(&k)))
                    .count();
                moved_planes += before
                    .planes
                    .iter()
                    .filter(|(k, (p, ..))| {
                        p.is_some_and(|n| !after.parent.contains_key(&n))
                            && after.planes.contains_key(k)
                    })
                    .count();
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
        "accepted {accepted:?}; strokes of vanished groups {vanished_strokes}; planes moved {moved_planes}"
    );
    // The family reaches what it is for: strokes and planes in groups that vanish, through
    // the ops it names (the numbers are the same on every run). Move Into is tried too and
    // always refused in these models: inside an open component it is, and at the top there
    // is no group to move into.
    assert!(vanished_strokes > 0 && moved_planes > 0);
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
