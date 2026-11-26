//! #142: moving or copying a group got sharply slower with the number of groups inside it.
//! A Move, a Copy and an Array walk a group's tree, and each group they met built the
//! tree's child lists again (`Model::kids`: one pass over every node, solid, mesh and
//! sketch of the model), and each component instance twice when moved and once when
//! copied. With 11,111 groups that was 11,111 passes over the model for one Move. The
//! lists are now built once per op and handed down.
//!
//! These tests count the builds (`testing::kids_builds`, the same number on every run and
//! machine) rather than time the op: a model of nested groups, the part a plain solid in a
//! group or a component instance, at 21 and at 341 groups, where an op must build the
//! lists as many times at both sizes, and no more than a few times. The rest hold what a
//! Move and a Copy of a group do, item by item and field by field, over seeded models, the
//! refusal a Move gives when two things in a group would refuse, and a Move inside an open
//! component reaching its other copies.

use sawhorse_kernel::testing::kids_builds;
use sawhorse_kernel::{Document, Op, OpOutcome};
use serde_json::{Value, json};

fn op(v: Value) -> Op {
    serde_json::from_value(v).unwrap()
}

fn ok(d: &mut Document, v: Value) -> OpOutcome {
    d.apply(&op(v)).unwrap_or_else(|e| panic!("{e}"))
}

fn shift(x: f64, y: f64) -> Value {
    json!({"linear": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": [x, y, 0]})
}

/// What is directly in the model: (nodes, solids).
fn tops(d: &Document) -> (Vec<u64>, Vec<u64>) {
    let m = d.model();
    let nodes = m
        .nodes()
        .into_iter()
        .filter(|(_, n)| n.member.parent.is_none())
        .map(|(k, _)| k)
        .collect();
    let objects = m
        .objects()
        .into_iter()
        .filter(|(_, o)| o.membership().parent.is_none())
        .map(|(k, _)| k)
        .collect();
    (nodes, objects)
}

/// A 100 x 60 x 30 mm board in a group (or as the one instance of a component), then
/// `levels` times: an array of three copies (four in all) and, but for the last level, a
/// group of the four. The model ends with four groups in it, holding 4^levels parts.
fn nested(levels: u32, component: bool) -> Document {
    let mut d = Document::new();
    ok(
        &mut d,
        json!({"op": "new_sketch", "origin": [0, 0, 0], "normal": [0, 0, 1]}),
    );
    ok(
        &mut d,
        json!({"op": "add_rectangle", "sketch": 1, "x": 0, "y": 0, "w": 0.1, "h": 0.06}),
    );
    ok(
        &mut d,
        json!({"op": "extrude", "sketch": 1, "region": 1, "distance": 0.03}),
    );
    let (_, objects) = tops(&d);
    let items: Vec<Value> = objects.iter().map(|o| json!({"object": o})).collect();
    if component {
        ok(
            &mut d,
            json!({"op": "make_component", "items": items, "name": "Part"}),
        );
    } else {
        ok(
            &mut d,
            json!({"op": "make_group", "items": items, "name": "Part"}),
        );
    }
    // Each level's step clears the level below it (its extent is under 3.5 steps).
    let mut step = 0.2;
    for level in 1..=levels {
        let (nodes, _) = tops(&d);
        ok(
            &mut d,
            json!({"op": "array", "objects": [], "nodes": nodes, "xform": shift(step, 0.0), "count": 3}),
        );
        if level < levels {
            group_all(&mut d);
        }
        step *= 4.0;
    }
    d
}

fn group_all(d: &mut Document) {
    let (nodes, _) = tops(d);
    let items: Vec<Value> = nodes.iter().map(|n| json!({"node": n})).collect();
    ok(
        d,
        json!({"op": "make_group", "items": items, "name": "All"}),
    );
}

/// The groups in `d`, and its groups and instances.
fn groups(d: &Document) -> (usize, usize) {
    let nodes = d.model().nodes();
    let groups = nodes
        .iter()
        .filter(|(_, n)| n.kind == sawhorse_kernel::nodes::NodeKind::Group)
        .count();
    (groups, nodes.len())
}

/// The builds of one op on `d`, which is then undone.
fn builds(d: &mut Document, v: Value) -> u64 {
    let before = d.hash();
    let (out, n) = kids_builds(|| d.apply(&op(v)));
    out.unwrap_or_else(|e| panic!("{e}"));
    ok(d, json!({"op": "undo"}));
    assert_eq!(d.hash(), before);
    n
}

/// Each op's builds on the model `nested(levels, component)`: a Move, a Copy and an Array
/// of 2 of the four groups in it together, then, after a group of those four, of the one
/// group holding everything.
fn counts(levels: u32, component: bool) -> ((usize, usize), Vec<(&'static str, u64)>) {
    let mut d = nested(levels, component);
    let mut out = Vec::new();
    for top in [false, true] {
        if top {
            group_all(&mut d);
        }
        let (nodes, _) = tops(&d);
        let far = 4.0f64.powi(levels as i32 + 1);
        let moved = builds(
            &mut d,
            json!({"op": "transform", "objects": [], "nodes": nodes, "xform": shift(0.0, 0.01)}),
        );
        let copied = builds(
            &mut d,
            json!({"op": "copy", "objects": [], "nodes": nodes, "xform": shift(0.0, far)}),
        );
        let arrayed = builds(
            &mut d,
            json!({"op": "array", "objects": [], "nodes": nodes, "xform": shift(0.0, far), "count": 2}),
        );
        let [m, c, a] = if top {
            ["move one", "copy one", "array one"]
        } else {
            ["move four", "copy four", "array four"]
        };
        out.extend([(m, moved), (c, copied), (a, arrayed)]);
    }
    (groups(&d), out)
}

/// What one op may build: the op's own lists once, and the fixed number the steps after
/// every op build whatever it was (removing empty groups, the validator). Three now; one
/// more is room, not a target. Four nodes selected with the lists built once per node
/// make six.
const MAX_BUILDS: u64 = 4;

fn check(component: bool) {
    let ((small_groups, small_nodes), small) = counts(2, component);
    let ((big_groups, big_nodes), big) = counts(4, component);
    println!("component {component}: {small_groups} groups, {small_nodes} nodes {small:?}");
    println!("component {component}: {big_groups} groups, {big_nodes} nodes {big:?}");
    // Two levels: 16 parts; four levels: 256 parts, as groups (341 groups in all) or as
    // instances in 85 groups.
    assert_eq!(small_nodes, 21);
    assert_eq!(big_nodes, 341);
    assert_eq!(big_groups, if component { 85 } else { 341 });
    for ((what, s), (_, b)) in small.iter().zip(&big) {
        assert!(
            *b <= MAX_BUILDS,
            "{what}: {b} builds of the child lists with {big_nodes} groups and instances \
             (at most {MAX_BUILDS})"
        );
        assert_eq!(
            s, b,
            "{what}: {s} builds with {small_nodes} groups and instances, {b} with {big_nodes}"
        );
    }
}

#[test]
fn moving_and_copying_nested_groups_builds_the_child_lists_a_fixed_number_of_times() {
    check(false);
}

#[test]
fn moving_and_copying_groups_of_components_builds_the_child_lists_a_fixed_number_of_times() {
    check(true);
}

// ---- what a Move and a Copy of a group do, item by item ----------------------------------
//
// Seeded models of nested groups (and, in a second family, component instances among them,
// opened and drawn in), with solids, strokes, section planes and open meshes at every level
// and hidden and locked members (meshes hidden and locked as a file can have them), made
// without deleting anything (so each new item takes the next slot and the model's lists
// are in creation order). The deepest nesting each family reaches is measured and held
// below (`DEEPEST_GROUPS`, `DEEPEST_COMPONENTS`). A Move of a top-level group must move
// everything in it and nothing else, and leave every id where it was; a Copy must make the
// group again item for item, each group's groups first and then its solids and meshes, in
// the order the model lists them, which is the order the copies are numbered in, and each
// copy must be, field for field in the saved snapshot, its original moved the same way.

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
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A random model of nested groups (with `components`, some of them made components
/// instead), every context left at the end and everything at the top that is not locked
/// grouped as "Top". `slot` places each new box on its own spot of a grid, so no two
/// solids meet.
fn random_model(seed: u64, components: bool) -> Document {
    let mut r = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut d = Document::new();
    let mut slot = 0usize;
    let try_op = |d: &mut Document, v: Value| d.apply(&op(v)).ok();
    let add_box = |d: &mut Document, r: &mut Rng, slot: &mut usize| {
        let (x, y) = ((*slot % 8) as f64 * 0.5, (*slot / 8) as f64 * 0.5);
        *slot += 1;
        let (w, h, z) = (
            0.05 + 0.2 * r.unit(),
            0.04 + 0.2 * r.unit(),
            0.1 * r.below(3) as f64,
        );
        let drawn = try_op(
            d,
            json!({"op": "draw", "target": {"on": "plane", "origin": [0, 0, z], "normal": [0, 0, 1]},
                   "points": [[x, y, z], [x + w, y, z], [x + w, y + h, z], [x, y + h, z]], "closed": true}),
        );
        if let Some(OpOutcome::Drawn {
            sketch: Some(sketch),
            inside: Some(region),
            ..
        }) = drawn
        {
            try_op(
                d,
                json!({"op": "extrude", "sketch": sketch, "region": region, "distance": 0.02 + 0.03 * r.unit()}),
            );
        }
    };
    for _ in 0..3 + r.below(3) {
        add_box(&mut d, &mut r, &mut slot);
    }
    for step in 0..30 {
        let g = d.model().graph();
        let ctx = g.context;
        let nodes: Vec<u64> = g
            .nodes
            .iter()
            .filter(|n| n.parent == ctx)
            .map(|n| n.id)
            .collect();
        let objects: Vec<u64> = g
            .objects
            .iter()
            .filter(|o| o.parent == ctx)
            .map(|o| o.id)
            .collect();
        let depth = g.path.len();
        match r.below(10) {
            0..=2 if nodes.len() + objects.len() >= 2 => {
                let mut items: Vec<Value> = Vec::new();
                for &n in &nodes {
                    if r.below(2) == 0 {
                        items.push(json!({"node": n}));
                    }
                }
                for &o in &objects {
                    if r.below(2) == 0 {
                        items.push(json!({"object": o}));
                    }
                }
                if !items.is_empty() {
                    if components && r.below(3) == 0 {
                        try_op(
                            &mut d,
                            json!({"op": "make_component", "items": items, "name": format!("C{step}")}),
                        );
                    } else {
                        try_op(
                            &mut d,
                            json!({"op": "make_group", "items": items, "name": format!("G{step}")}),
                        );
                    }
                }
            }
            3..=4 if !nodes.is_empty() && depth < 4 => {
                let n = nodes[r.below(nodes.len())];
                try_op(&mut d, json!({"op": "set_context", "node": n}));
            }
            5 if ctx.is_some() => {
                try_op(&mut d, json!({"op": "leave_context"}));
            }
            6 => add_box(&mut d, &mut r, &mut slot),
            7 => {
                let (x, y, z) = (
                    4.0 * r.unit(),
                    4.0 * r.unit(),
                    0.5 + 0.1 * r.below(4) as f64,
                );
                if r.below(2) == 0 {
                    try_op(
                        &mut d,
                        json!({"op": "draw", "target": {"on": "plane", "origin": [0, 0, z], "normal": [0, 0, 1]},
                               "points": [[x, y, z], [x + 0.1, y, z], [x, y + 0.07, z]], "closed": true}),
                    );
                } else {
                    try_op(
                        &mut d,
                        json!({"op": "add_section", "origin": [x, y, 0.01], "normal": [1, 0, 0], "active": r.below(2) == 0}),
                    );
                }
            }
            8 => {
                let (x, y, z) = (4.0 * r.unit(), 4.0 * r.unit(), 1.0 + r.unit());
                try_op(
                    &mut d,
                    json!({"op": "import", "items": [{"name": format!("M{step}"),
                           "positions": [[x, y, z], [x + 0.1, y, z], [x, y + 0.1, z]], "faces": [[[0, 1, 2]]]}]}),
                );
            }
            _ => {
                // Hide or lock something anywhere but on the path to the open group.
                let all: Vec<Value> = g
                    .nodes
                    .iter()
                    .filter(|n| !g.path.contains(&n.id))
                    .map(|n| json!({"node": n.id}))
                    .chain(g.objects.iter().map(|o| json!({"object": o.id})))
                    .collect();
                if !all.is_empty() {
                    let item = all[r.below(all.len())].clone();
                    let (flag, key) = if r.below(2) == 0 {
                        ("set_hidden", "hidden")
                    } else {
                        ("set_locked", "locked")
                    };
                    let mut v = json!({"op": flag, "items": [item]});
                    v[key] = json!(r.below(3) != 0);
                    try_op(&mut d, v);
                }
            }
        }
    }
    while d.model().context().is_some() {
        ok(&mut d, json!({"op": "leave_context"}));
    }
    // No op hides or locks a mesh by itself (an item is a solid or a node), a file can:
    // some of the meshes in groups hidden or locked, through the model's own snapshot
    // (not those of a component, which its definition decides).
    let mut s = serde_json::to_value(d.model().snapshot()).unwrap();
    let mut changed = false;
    for m in s["meshes"].as_array_mut().unwrap() {
        let in_group = m.get("parent").is_some_and(|p| !p.is_null());
        let in_component = m.get("slot").is_some_and(|p| !p.is_null());
        if in_group && !in_component && r.below(2) == 0 {
            let key = if r.below(2) == 0 { "hidden" } else { "locked" };
            m[key] = json!(true);
            changed = true;
        }
    }
    if changed {
        ok(&mut d, json!({"op": "restore", "snapshot": s}));
    }
    // A group of everything at the top that is not locked, so there is one to move.
    let g = d.model().graph();
    let items: Vec<Value> = g
        .nodes
        .iter()
        .filter(|n| n.parent.is_none() && !n.locked)
        .map(|n| json!({"node": n.id}))
        .chain(
            g.objects
                .iter()
                .filter(|o| o.parent.is_none() && !o.locked)
                .map(|o| json!({"object": o.id})),
        )
        .collect();
    if !items.is_empty() {
        try_op(
            &mut d,
            json!({"op": "make_group", "items": items, "name": "Top"}),
        );
    }
    d
}

/// A quarter turn about the blue axis through (1, 2, 0), then 0.3 along red and 0.1 up: the
/// map the tests apply, exact on axis-aligned boxes.
fn turn() -> sawhorse_kernel::transform::Xform {
    serde_json::from_value(
        json!({"linear": [[0, -1, 0], [1, 0, 0], [0, 0, 1]], "translation": [3.3, 1.0, 0.1]}),
    )
    .unwrap()
}

fn near(a: glam::DVec3, b: glam::DVec3) -> bool {
    a.distance(b) < 1e-9
}

fn near_xform(
    a: &sawhorse_kernel::transform::Xform,
    b: &sawhorse_kernel::transform::Xform,
) -> bool {
    let (p, q) = (
        a.linear.iter().flatten().chain(&a.translation),
        b.linear.iter().flatten().chain(&b.translation),
    );
    p.zip(q).all(|(u, v)| (u - v).abs() < 1e-9)
}

/// `(min, max)` of the box `b` mapped by `x`.
fn mapped_box(
    x: &sawhorse_kernel::transform::Xform,
    b: (glam::DVec3, glam::DVec3),
) -> (glam::DVec3, glam::DVec3) {
    let (mut lo, mut hi) = (glam::DVec3::splat(f64::MAX), glam::DVec3::splat(f64::MIN));
    for i in 0..8 {
        let p = glam::DVec3::new(
            if i & 1 == 0 { b.0.x } else { b.1.x },
            if i & 2 == 0 { b.0.y } else { b.1.y },
            if i & 4 == 0 { b.0.z } else { b.1.z },
        );
        let q = x.apply(p);
        lo = lo.min(q);
        hi = hi.max(q);
    }
    (lo, hi)
}

/// The groups and instances under `n` in pre-order (`n` first), each one's in the model's
/// order.
fn pre_order(g: &sawhorse_kernel::nodes::Graph, n: u64) -> Vec<u64> {
    let mut out = vec![n];
    for c in g.nodes.iter().filter(|c| c.parent == Some(n)) {
        out.extend(pre_order(g, c.id));
    }
    out
}

/// How deep `n` is: 1 for a node at the top of the model.
fn depth_of(g: &sawhorse_kernel::nodes::Graph, n: u64) -> usize {
    let mut at = Some(n);
    let mut depth = 0;
    while let Some(k) = at {
        depth += 1;
        at = g.nodes.iter().find(|x| x.id == k).and_then(|x| x.parent);
    }
    depth
}

/// The solids (or meshes) under `n` in the order a copy makes them: each group's groups
/// first, then its own, in the model's order.
fn leaves_in_copy_order(
    g: &sawhorse_kernel::nodes::Graph,
    leaves: &[sawhorse_kernel::nodes::GraphLeaf],
    n: u64,
) -> Vec<u64> {
    let mut out = Vec::new();
    for c in g.nodes.iter().filter(|c| c.parent == Some(n)) {
        out.extend(leaves_in_copy_order(g, leaves, c.id));
    }
    out.extend(leaves.iter().filter(|l| l.parent == Some(n)).map(|l| l.id));
    out
}

/// A sketch's points in the world.
fn sketch_points(s: &sawhorse_kernel::sketch::Sketch) -> Vec<glam::DVec3> {
    let p = s.plane();
    s.vertices()
        .map(|(_, q)| p.origin + p.u * q.x + p.v * q.y)
        .collect()
}

fn top_group(d: &Document) -> Option<u64> {
    let g = d.model().graph();
    g.nodes
        .iter()
        .find(|n| n.parent.is_none() && n.name.as_deref() == Some("Top"))
        .map(|n| n.id)
}

/// Every public id of the model, list by list, in the model's order.
fn ids(m: &sawhorse_kernel::Model) -> Vec<Vec<u64>> {
    let g = m.graph();
    vec![
        g.nodes.iter().map(|n| n.id).collect(),
        m.object_ids(),
        g.meshes.iter().map(|l| l.id).collect(),
        g.sketches.iter().map(|&(s, _)| s).collect(),
        g.sections.iter().map(|s| s.id).collect(),
        g.definitions.iter().map(|d| d.id).collect(),
    ]
}

const MODELS: u64 = 200;
/// The deepest nesting the 200 models of each family reach, a node at the top counting 1
/// (measured by the tests below, which fail if it changes).
const DEEPEST_GROUPS: usize = 9;
const DEEPEST_COMPONENTS: usize = 9;

/// A Move of the "Top" group of each model of a family; what moved, by kind.
fn check_moves(components: bool) -> [usize; 7] {
    let x = turn();
    // Solids, meshes, strokes, section planes, groups and instances moved, strokes in a
    // group inside an instance moved, the deepest nesting.
    let mut moved = [0usize; 7];
    for seed in 1..=MODELS {
        let mut d = random_model(seed, components);
        let Some(top) = top_group(&d) else { continue };
        let before = d.model().clone();
        let g = before.graph();
        let inside = pre_order(&g, top);
        for n in &g.nodes {
            moved[6] = moved[6].max(depth_of(&g, n.id));
        }
        ok(
            &mut d,
            json!({"op": "transform", "objects": [], "nodes": [top], "xform": x}),
        );
        let m = d.model();
        // Nothing is made or removed, and every id stays where it was.
        assert_eq!(ids(m), ids(&before), "seed {seed}: the ids");
        let h = m.graph();
        for n in &g.nodes {
            let now = h.nodes.iter().find(|k| k.id == n.id).unwrap();
            let want = if inside.contains(&n.id) {
                x.after(&n.xform)
            } else {
                n.xform
            };
            assert!(near_xform(&now.xform, &want), "seed {seed}: node {}", n.id);
            assert_eq!(now.parent, n.parent, "seed {seed}: node {}", n.id);
        }
        for o in &g.objects {
            let b = before.object(o.id).unwrap().bounds();
            let a = m.object(o.id).unwrap().bounds();
            let in_it = o.parent.is_some_and(|p| inside.contains(&p));
            let want = if in_it { mapped_box(&x, b) } else { b };
            assert!(
                near(a.0, want.0) && near(a.1, want.1),
                "seed {seed}: solid {}",
                o.id
            );
            moved[0] += in_it as usize;
        }
        for l in &g.meshes {
            let b = &before.mesh(l.id).unwrap().positions;
            let a = &m.mesh(l.id).unwrap().positions;
            let in_it = l.parent.is_some_and(|p| inside.contains(&p));
            for (p, q) in b.iter().zip(a) {
                assert!(
                    near(*q, if in_it { x.apply(*p) } else { *p }),
                    "seed {seed}: mesh {}",
                    l.id
                );
            }
            moved[1] += in_it as usize;
        }
        // Strokes and section planes go with the groups and instances they are drawn in:
        // a moved instance takes those in it and in its groups, down to the next instance,
        // whose own are left where they are (as before #142).
        let follows = |p: Option<u64>| {
            let Some(p) = p.filter(|p| inside.contains(p)) else {
                return false;
            };
            let mut at = Some(p);
            let mut instances = 0;
            while let Some(k) = at {
                let n = g.nodes.iter().find(|n| n.id == k).unwrap();
                instances += (n.kind == "instance") as usize;
                if k == top {
                    break;
                }
                at = n.parent;
            }
            instances <= 1
        };
        for &(s, parent) in &g.sketches {
            let b = sketch_points(before.sketch(s).unwrap());
            let a = sketch_points(m.sketch(s).unwrap());
            let in_it = follows(parent);
            assert_eq!(a.len(), b.len());
            for (p, q) in b.iter().zip(&a) {
                assert!(
                    near(*q, if in_it { x.apply(*p) } else { *p }),
                    "seed {seed}: sketch {s}"
                );
            }
            moved[2] += in_it as usize;
            let in_instance_group = parent.is_some_and(|p| {
                let n = g.nodes.iter().find(|n| n.id == p).unwrap();
                n.kind == "group" && n.in_component
            });
            moved[5] += (in_it && in_instance_group) as usize;
        }
        for s in &g.sections {
            let b = before.section(s.id).unwrap().origin;
            let a = m.section(s.id).unwrap().origin;
            let in_it = follows(s.parent);
            assert!(
                near(a, if in_it { x.apply(b) } else { b }),
                "seed {seed}: section {}",
                s.id
            );
            moved[3] += in_it as usize;
        }
        moved[4] += inside.len();
    }
    println!(
        "components {components}: moved {} solids, {} meshes, {} strokes ({} in groups inside \
         instances), {} section planes, {} groups and instances; deepest nesting {}",
        moved[0], moved[1], moved[2], moved[5], moved[3], moved[4], moved[6]
    );
    moved
}

#[test]
fn a_moved_group_moves_everything_in_it_and_nothing_else() {
    let moved = check_moves(false);
    // The models hold every kind of thing inside the groups moved.
    assert!(moved[..5].iter().all(|&k| k >= 100), "{moved:?}");
    assert_eq!(moved[6], DEEPEST_GROUPS, "the deepest nesting");
}

#[test]
fn a_moved_group_of_components_moves_everything_in_it_and_nothing_else() {
    let moved = check_moves(true);
    assert!(moved[..5].iter().all(|&k| k >= 100), "{moved:?}");
    // Strokes drawn in a group inside a component instance, which the instance takes.
    assert!(moved[5] >= 20, "{moved:?}");
    assert_eq!(moved[6], DEEPEST_COMPONENTS, "the deepest nesting");
}

/// `v` without the fields named, for comparing the rest.
fn without(v: &Value, fields: &[&str]) -> Value {
    let mut v = v.clone();
    for f in fields {
        v.as_object_mut().unwrap().remove(*f);
    }
    v
}

/// The entry with public id `id` in list `list` of a snapshot.
fn entry<'a>(s: &'a Value, list: &str, id: u64) -> &'a Value {
    s[list]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == json!(id))
        .unwrap_or_else(|| panic!("{list} {id} not in the snapshot"))
}

#[test]
fn a_copied_group_is_its_source_item_for_item_in_the_order_it_is_walked() {
    let x = turn();
    let mut made = [0usize; 6];
    for seed in 1..=MODELS {
        let mut d = random_model(seed, false);
        let Some(top) = top_group(&d) else { continue };
        let before = d.model().clone();
        let g = before.graph();
        // The same group moved by the same map: each copy must be its original moved.
        let mut moved = before.clone();
        moved
            .apply(&op(
                json!({"op": "transform", "objects": [], "nodes": [top], "xform": x}),
            ))
            .unwrap();
        let out = ok(
            &mut d,
            json!({"op": "copy", "objects": [], "nodes": [top], "xform": x}),
        );
        let OpOutcome::Copied { objects, nodes } = out else {
            panic!("{out:?}")
        };
        assert!(objects.is_empty());
        let m = d.model();
        let h = m.graph();
        let (was, now) = (
            serde_json::to_value(moved.snapshot()).unwrap(),
            serde_json::to_value(m.snapshot()).unwrap(),
        );
        // The groups: the copies are made in the source's pre-order, each a new slot.
        let src_nodes = pre_order(&g, top);
        let old: std::collections::BTreeSet<u64> = g.nodes.iter().map(|n| n.id).collect();
        let new_nodes: Vec<&sawhorse_kernel::nodes::GraphNode> =
            h.nodes.iter().filter(|n| !old.contains(&n.id)).collect();
        assert_eq!(new_nodes.len(), src_nodes.len(), "seed {seed}");
        assert_eq!(nodes, vec![new_nodes[0].id], "seed {seed}");
        let copy_of: std::collections::BTreeMap<u64, u64> = src_nodes
            .iter()
            .zip(&new_nodes)
            .map(|(&s, c)| (s, c.id))
            .collect();
        // Every field of a copy is its original's, moved, but its id, its parent (the copy
        // of the original's) and its lock (a copy is never locked).
        let same = |list: &str, s: u64, c: u64, extra: &[&str]| {
            let (a, b) = (entry(&was, list, s), entry(&now, list, c));
            let mut skip = vec!["id", "parent", "locked"];
            skip.extend_from_slice(extra);
            assert_eq!(
                without(a, &skip),
                without(b, &skip),
                "seed {seed}: {list} {s} and its copy {c}"
            );
            assert!(
                b.get("locked").is_none_or(|l| l == &json!(false)),
                "seed {seed}: {list} {c} locked"
            );
            let parent = a.get("parent").and_then(Value::as_u64);
            let want = if list == "nodes" && s == top {
                None
            } else {
                parent.map(|p| copy_of[&p])
            };
            assert_eq!(
                b.get("parent").and_then(Value::as_u64),
                want,
                "seed {seed}: {list} {c} parent"
            );
        };
        for (&s, c) in src_nodes.iter().zip(&new_nodes) {
            same("nodes", s, c.id, &[]);
            made[3] += 1;
        }
        // Solids and meshes: made each group's groups first, then its own. A solid's
        // display number is new, and it no longer names the sketch it was extruded from.
        for (kind, list, src_leaves, new_leaves) in [
            ("solid", "objects", &g.objects, &h.objects),
            ("mesh", "meshes", &g.meshes, &h.meshes),
        ] {
            let old: std::collections::BTreeSet<u64> = src_leaves.iter().map(|l| l.id).collect();
            let fresh: Vec<_> = new_leaves.iter().filter(|l| !old.contains(&l.id)).collect();
            let want = leaves_in_copy_order(&g, src_leaves, top);
            assert_eq!(fresh.len(), want.len(), "seed {seed}: {kind}s");
            for (c, &s) in fresh.iter().zip(&want) {
                if kind == "solid" {
                    same(list, s, c.id, &["number", "source_sketch"]);
                    let copy = entry(&now, list, c.id);
                    assert!(
                        copy.get("source_sketch").is_none(),
                        "seed {seed}: solid {}",
                        c.id
                    );
                    assert!(
                        m.object(c.id).unwrap().source().is_none(),
                        "seed {seed}: solid {}",
                        c.id
                    );
                    made[5] += before.object(s).unwrap().source().is_some() as usize;
                    made[0] += 1;
                } else {
                    same(list, s, c.id, &[]);
                    made[1] += 1;
                }
            }
        }
        // Section planes: each group's, in id order, as its copy is made; strokes are not
        // copied.
        let old: std::collections::BTreeSet<u64> = g.sections.iter().map(|s| s.id).collect();
        let fresh: Vec<_> = h.sections.iter().filter(|s| !old.contains(&s.id)).collect();
        let want: Vec<&sawhorse_kernel::nodes::GraphSection> = src_nodes
            .iter()
            .flat_map(|&n| g.sections.iter().filter(move |s| s.parent == Some(n)))
            .collect();
        assert_eq!(fresh.len(), want.len(), "seed {seed}: section planes");
        for (c, s) in fresh.iter().zip(&want) {
            same("sections", s.id, c.id, &[]);
            made[2] += 1;
        }
        let strokes = serde_json::to_value(before.snapshot()).unwrap();
        assert_eq!(
            now["sketches"], strokes["sketches"],
            "seed {seed}: strokes are not copied"
        );
        made[4] += g
            .sketches
            .iter()
            .filter(|(_, p)| p.is_some_and(|p| src_nodes.contains(&p)))
            .count();
    }
    println!(
        "copied: {} solids ({} from a sketch), {} meshes, {} section planes, {} groups \
         ({} strokes left)",
        made[0], made[5], made[1], made[2], made[3], made[4]
    );
    assert!(made.iter().all(|&k| k >= 100), "{made:?}");
}

// ---- the refusal a Move gives, and what it tells the open component --------------------

/// A box drawn and extruded at `x` (metres along red): its solid.
fn board_at(d: &mut Document, x: f64, y: f64) -> u64 {
    let drawn = ok(
        d,
        json!({"op": "draw", "target": {"on": "plane", "origin": [0, 0, 0], "normal": [0, 0, 1]},
               "points": [[x, y, 0], [x + 0.1, y, 0], [x + 0.1, y + 0.1, 0], [x, y + 0.1, 0]], "closed": true}),
    );
    let OpOutcome::Drawn {
        sketch: Some(sketch),
        inside: Some(region),
        ..
    } = drawn
    else {
        panic!("{drawn:?}")
    };
    let OpOutcome::ObjectCreated { object } = ok(
        d,
        json!({"op": "extrude", "sketch": sketch, "region": region, "distance": 0.05}),
    ) else {
        panic!()
    };
    object
}

/// The refusal of a 500 m Move of a group holding a group moved out to 9.6 km (`inner`)
/// and a solid at 9.8 km (`solid`): each alone, and both.
fn far_move_refusal(inner: bool, solid: bool) -> String {
    let mut d = Document::new();
    let mut items = Vec::new();
    if inner {
        let b = board_at(&mut d, 0.0, 0.0);
        let OpOutcome::GroupCreated { group } = ok(
            &mut d,
            json!({"op": "make_group", "items": [{"object": b}], "name": "Inner"}),
        ) else {
            panic!()
        };
        ok(
            &mut d,
            json!({"op": "transform", "objects": [], "nodes": [group], "xform": shift(9600.0, 0.0)}),
        );
        items.push(json!({"node": group}));
    }
    if solid {
        items.push(json!({"object": board_at(&mut d, 9800.0, 1.0)}));
    }
    let OpOutcome::GroupCreated { group } = ok(
        &mut d,
        json!({"op": "make_group", "items": items, "name": "Outer"}),
    ) else {
        panic!()
    };
    let before = d.hash();
    let e = d
        .apply(&op(
            json!({"op": "transform", "objects": [], "nodes": [group], "xform": shift(500.0, 0.0)}),
        ))
        .expect_err("past 10 km");
    assert_eq!(d.hash(), before);
    e.to_string()
}

#[test]
fn a_refused_move_names_what_the_group_meets_first() {
    // A group's groups are moved before its solids, so the inner group's refusal is the
    // one given when both would refuse.
    let (inner, solid, both) = (
        far_move_refusal(true, false),
        far_move_refusal(false, true),
        far_move_refusal(true, true),
    );
    println!("inner group: {inner}\nsolid: {solid}\nboth: {both}");
    assert_ne!(
        inner, solid,
        "the two refusals must differ for this test to mean anything"
    );
    assert_eq!(both, inner);
}

#[test]
fn a_move_inside_an_open_component_reaches_its_other_copies() {
    // A component "Frame" holding a board and an instance of "Leg"; a second Frame 2 m
    // along red. With the first Frame open, its Leg moved 0.3 m along green: the Leg of
    // the other Frame moves with it, and nothing else does.
    let mut d = Document::new();
    let leg = board_at(&mut d, 0.0, 0.0);
    let OpOutcome::ComponentCreated { instance: leg, .. } = ok(
        &mut d,
        json!({"op": "make_component", "items": [{"object": leg}], "name": "Leg"}),
    ) else {
        panic!()
    };
    let rail = board_at(&mut d, 0.5, 0.0);
    let OpOutcome::ComponentCreated {
        instance: frame, ..
    } = ok(
        &mut d,
        json!({"op": "make_component", "items": [{"node": leg}, {"object": rail}], "name": "Frame"}),
    )
    else {
        panic!()
    };
    ok(
        &mut d,
        json!({"op": "copy", "objects": [], "nodes": [frame], "xform": shift(2.0, 0.0)}),
    );
    let boxes = |d: &Document| {
        let mut b: Vec<[i64; 6]> = d
            .model()
            .objects()
            .into_iter()
            .map(|(_, o)| {
                let (lo, hi) = o.bounds();
                let r = |v: f64| (v * 1e6).round() as i64;
                [r(lo.x), r(lo.y), r(lo.z), r(hi.x), r(hi.y), r(hi.z)]
            })
            .collect();
        b.sort();
        b
    };
    let before = boxes(&d);
    ok(&mut d, json!({"op": "set_context", "node": frame}));
    let g = d.model().graph();
    let inner = g
        .nodes
        .iter()
        .find(|n| n.parent == Some(frame))
        .map(|n| n.id)
        .unwrap();
    ok(
        &mut d,
        json!({"op": "transform", "objects": [], "nodes": [inner], "xform": shift(0.0, 0.3)}),
    );
    let after = boxes(&d);
    // The two Legs (at x 0 and 2) are 0.3 m along green; the two rails stay.
    let mut want: Vec<[i64; 6]> = before
        .iter()
        .map(|b| {
            if b[0] == 0 || b[0] == 2_000_000 {
                [b[0], b[1] + 300_000, b[2], b[3], b[4] + 300_000, b[5]]
            } else {
                *b
            }
        })
        .collect();
    want.sort();
    assert_eq!(before.len(), 4);
    assert_eq!(after, want);
}
