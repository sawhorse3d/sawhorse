//! `check_clearance` and `measure` (#14), over the protocol.

mod common;

use common::{Session, folder};
use serde_json::{Value, json};

/// A board `t` x `w` x `l` mm at `at` (mm), lying flat with its length along x. Its item.
fn board(s: &mut Session, name: &str, size: [f64; 3], at: [f64; 3]) -> Value {
    let r = s.ok(
        "add_board",
        json!({"name": name, "thickness": size[0], "width": size[1], "length": size[2], "position": at}),
    );
    json!({"object": r["board"]["object"]})
}

fn clearance(s: &mut Session, a: &Value, b: &Value) -> Value {
    s.ok("check_clearance", json!({"a": a, "b": b}))
}

fn close(got: &Value, want: f64, tol: f64) {
    let g = got
        .as_f64()
        .unwrap_or_else(|| panic!("not a number: {got}"));
    assert!((g - want).abs() <= tol, "{g} is not {want} (within {tol})");
}

/// A 19 x 300 x 600 board, and a second the same above it with its underside at `z` mm.
fn stacked(s: &mut Session, z: f64) -> (Value, Value) {
    s.ok("new_model", json!({}));
    let a = board(s, "Lower", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
    let b = board(s, "Upper", [19.0, 300.0, 600.0], [0.0, 0.0, z]);
    (a, b)
}

#[test]
fn a_gap_touching_and_an_overlap() {
    let mut s = Session::new(&folder("clearance"));
    // 5 mm apart.
    let (a, b) = stacked(&mut s, 24.0);
    let r = clearance(&mut s, &a, &b);
    close(&r["gap_mm"], 5.0, 1e-6);
    assert_eq!(r["overlap_mm3"], 0.0);
    assert_eq!(r["overlap_size_mm"], Value::Null);
    assert_eq!(r["seated"], false);
    assert_eq!(r["merge_mm"], 0.01);
    assert_eq!(r["a"]["name"], "Lower");
    assert_eq!(r["b"]["name"], "Upper");
    assert_eq!(r["a"]["hidden"], false);
    close(&r["closest_mm"][0][2], 19.0, 1e-6);
    close(&r["closest_mm"][1][2], 24.0, 1e-6);
    // Touching.
    let (a, b) = stacked(&mut s, 19.0);
    let r = clearance(&mut s, &a, &b);
    assert_eq!(r["gap_mm"], 0.0);
    assert_eq!(r["overlap_mm3"], 0.0);
    assert_eq!(r["seated"], true);
    // 3 mm into it: 3 x 300 x 600 mm.
    let (a, b) = stacked(&mut s, 16.0);
    let r = clearance(&mut s, &a, &b);
    assert_eq!(r["gap_mm"], 0.0);
    close(&r["overlap_mm3"], 540_000.0, 1e-3);
    close(&r["overlap_size_mm"][0], 600.0, 1e-6);
    close(&r["overlap_size_mm"][1], 300.0, 1e-6);
    close(&r["overlap_size_mm"][2], 3.0, 1e-6);
    assert_eq!(r["seated"], false);
}

/// At the merge distance (0.01 mm) and a step either side, apart and into each other.
#[test]
fn seated_at_the_merge_distance_and_not_past_it() {
    let mut s = Session::new(&folder("seated"));
    for (z, seated) in [
        (19.009, true),
        (19.01, true),
        (19.0101, false),
        (18.991, true),
        (18.99, true),
        (18.9899, false),
    ] {
        let (a, b) = stacked(&mut s, z);
        let r = clearance(&mut s, &a, &b);
        if z < 19.0 && seated {
            // Thinner than the merge distance: the kernel's intersection is empty.
            assert_eq!(r["overlap_mm3"], 0.0, "{r}");
        }
        assert_eq!(r["seated"], seated, "underside at {z} mm: {r}");
        if z > 19.0 {
            close(&r["gap_mm"], z - 19.0, 1e-6);
        } else {
            assert_eq!(r["gap_mm"], 0.0, "{r}");
        }
    }
}

/// A thin rail right through a block: no gap, the overlap is the rail's middle.
#[test]
fn a_part_right_through_another() {
    let mut s = Session::new(&folder("through"));
    let block = board(&mut s, "Block", [100.0, 100.0, 100.0], [0.0, 0.0, 0.0]);
    let rail = board(&mut s, "Rail", [10.0, 20.0, 300.0], [-100.0, 40.0, 45.0]);
    let r = clearance(&mut s, &block, &rail);
    assert_eq!(r["gap_mm"], 0.0);
    close(&r["overlap_mm3"], 100.0 * 20.0 * 10.0, 1e-3);
    // A part wholly inside another: no surfaces meet, still no gap.
    let core = board(&mut s, "Core", [10.0, 10.0, 10.0], [45.0, 10.0, 10.0]);
    let r = clearance(&mut s, &block, &core);
    assert_eq!(r["gap_mm"], 0.0, "{r}");
    close(&r["overlap_mm3"], 1000.0, 1e-3);
}

/// Rotate `v` by `angle` about the unit `axis` (Rodrigues).
fn rotate(v: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    let (c, s) = (angle.cos(), angle.sin());
    let k = axis;
    let dot = k[0] * v[0] + k[1] * v[1] + k[2] * v[2];
    let cross = [
        k[1] * v[2] - k[2] * v[1],
        k[2] * v[0] - k[0] * v[2],
        k[0] * v[1] - k[1] * v[0],
    ];
    [0, 1, 2].map(|i| v[i] * c + cross[i] * s + k[i] * dot * (1.0 - c))
}

/// Turn the whole model by `angle` about `axis` and move it by `by` metres.
fn turn_all(s: &mut Session, objects: &[u64], axis: [f64; 3], angle: f64, by: [f64; 3]) {
    let cols = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|e| rotate(e, axis, angle));
    let linear: Vec<Vec<f64>> = (0..3)
        .map(|r| (0..3).map(|c| cols[c][r]).collect())
        .collect();
    s.ok(
        "apply_ops",
        json!({"name": "Turn", "ops": [{"op": "transform", "objects": objects, "nodes": [],
            "xform": {"linear": linear, "translation": by}}]}),
    );
}

/// Turned 45 degrees about two axes and moved 40 m out, the same gap and overlap.
#[test]
fn turned_and_far_from_the_origin() {
    let mut s = Session::new(&folder("turned"));
    let axis = {
        let l = 3f64.sqrt();
        [1.0 / l, 1.0 / l, 1.0 / l]
    };
    for (z, gap, overlap) in [(24.0, 5.0, 0.0), (16.0, 0.0, 540_000.0)] {
        let (a, b) = stacked(&mut s, z);
        turn_all(
            &mut s,
            &[1, 2],
            axis,
            std::f64::consts::FRAC_PI_4,
            [40.0, -30.0, 20.0],
        );
        let r = clearance(&mut s, &a, &b);
        close(&r["gap_mm"], gap, 1e-6);
        close(&r["overlap_mm3"], overlap, 1e-2);
        let m = s.ok("measure", json!({"a": a, "b": b}));
        if z > 19.0 {
            close(&m["facing"][0]["distance_mm"], 5.0, 1e-6);
        }
    }
}

/// Random boards, turned and moved together: the gap is the boxes' own distance, worked
/// out here from where they sat before the turn.
#[test]
fn the_gap_matches_the_boxes_distance_on_random_boards() {
    let mut s = Session::new(&folder("random"));
    let mut seed: u64 = 0x5eed_0014;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for case in 0..40 {
        s.ok("new_model", json!({}));
        let mut boxes = Vec::new();
        for name in ["A", "B"] {
            let size = [
                5.0 + next() * 60.0,
                20.0 + next() * 200.0,
                50.0 + next() * 500.0,
            ];
            let at = [
                next() * 400.0 - 200.0,
                next() * 400.0 - 200.0,
                next() * 400.0 - 200.0,
            ];
            board(&mut s, name, size, at);
            // Length along x, width y, thickness z.
            boxes.push((at, [at[0] + size[2], at[1] + size[1], at[2] + size[0]]));
        }
        let ((la, ha), (lb, hb)) = (boxes[0], boxes[1]);
        let sep: f64 = (0..3)
            .map(|i| (la[i] - hb[i]).max(lb[i] - ha[i]).max(0.0).powi(2))
            .sum::<f64>()
            .sqrt();
        let axis = {
            let v = [next() - 0.5, next() - 0.5, next() - 0.5];
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            v.map(|x| x / l)
        };
        turn_all(
            &mut s,
            &[1, 2],
            axis,
            next() * 6.0,
            [next() * 80.0 - 40.0, next() * 80.0 - 40.0, 0.0],
        );
        let r = clearance(&mut s, &json!({"object": 1}), &json!({"object": 2}));
        close(&r["gap_mm"], sep, 1e-5);
        if sep > 0.0 {
            assert_eq!(r["overlap_mm3"], 0.0, "case {case}: {r}");
        }
    }
}

/// Two sides of a shelf 481 mm apart outside: 462 mm inside. Their outer faces look away
/// and are not listed.
#[test]
fn measure_gives_the_inside_width_between_two_sides() {
    let mut s = Session::new(&folder("measure"));
    // Sides standing on end: 19 thick along x.
    let side = |s: &mut Session, name: &str, x: f64| {
        let r = s.ok(
            "add_board",
            json!({"name": name, "thickness": 19, "width": 300, "length": 700,
                "position": [x, 0, 0], "length_axis": "z", "thickness_axis": "x"}),
        );
        json!({"object": r["board"]["object"]})
    };
    let left = side(&mut s, "Left", 0.0);
    let right = side(&mut s, "Right", 481.0);
    let m = s.ok("measure", json!({"a": left, "b": right}));
    let facing = m["facing"].as_array().unwrap();
    assert_eq!(facing.len(), 1, "{m}");
    close(&facing[0]["distance_mm"], 462.0, 1e-6);
    assert_eq!(facing[0]["normal"], json!([1.0, 0.0, 0.0]));
    assert_eq!(facing[0]["a_face"]["object"], left["object"]);
    assert_eq!(facing[0]["b_face"]["object"], right["object"]);
    assert!(facing[0]["a_face"]["face"].as_u64().unwrap() > 0);
    assert_eq!(m["more"], 0);
    // A shelf between them touches both: 0 to each side.
    let shelf = board(&mut s, "Shelf", [19.0, 300.0, 462.0], [19.0, 0.0, 300.0]);
    let m = s.ok("measure", json!({"a": left, "b": shelf}));
    close(&m["facing"][0]["distance_mm"], 0.0, 1e-9);
    // Faces that are not parallel give nothing: a board turned 10 degrees.
    s.ok("new_model", json!({}));
    let a = board(&mut s, "A", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
    let b = board(&mut s, "B", [19.0, 300.0, 600.0], [0.0, 0.0, 50.0]);
    turn_all(
        &mut s,
        &[2],
        [1.0, 0.0, 0.0],
        10f64.to_radians(),
        [0.0, 0.0, 0.0],
    );
    let m = s.ok("measure", json!({"a": a, "b": b}));
    assert_eq!(m["facing"], json!([]), "{m}");
}

/// Neither tool changes the model: the same summary after, and Undo still undoes the last
/// real change.
#[test]
fn neither_tool_changes_the_model() {
    let mut s = Session::new(&folder("readonly"));
    let (a, b) = stacked(&mut s, 16.0);
    let before = s.ok("summary", json!({}));
    clearance(&mut s, &a, &b);
    s.ok("measure", json!({"a": a, "b": b}));
    assert_eq!(s.ok("summary", json!({})), before);
    s.ok("undo", json!({}));
    let parts = s.ok("summary", json!({}))["parts"].clone();
    assert_eq!(
        parts.as_array().unwrap().len(),
        1,
        "undo took the second board: {parts}"
    );
    assert_eq!(parts[0]["name"], "Lower");

    // Calls refused, and a call where the kernel's intersection fails (0.01 mm into each
    // other, turned 0.3 rad about x), leave the summary, the hash and the saved file as they
    // were, and make no file.
    let dir = folder("readonly-file");
    let mut s = Session::new(&dir);
    let (a, b) = stacked(&mut s, 18.99);
    turn_all(&mut s, &[1, 2], [1.0, 0.0, 0.0], 0.3, [0.0, 0.0, 0.0]);
    s.ok("save_model", json!({"path": "ro.shz"}));
    let bytes = std::fs::read(dir.join("ro.shz")).unwrap();
    let before = s.ok("summary", json!({}));
    assert!(clearance(&mut s, &a, &b)["overlap_unknown"].is_string());
    for tool in ["check_clearance", "measure"] {
        s.ok(tool, json!({"a": a, "b": b}));
        s.refused(tool, json!({"a": a, "b": a}));
        s.refused(tool, json!({"a": a, "b": {"object": 99}}));
    }
    assert_eq!(s.ok("summary", json!({})), before);
    assert_eq!(std::fs::read(dir.join("ro.shz")).unwrap(), bytes);
    assert_eq!(common::names(&dir), ["ro.shz"]);
}

/// Hidden parts are measured, and the answer says which.
#[test]
fn a_hidden_part_is_measured_and_flagged() {
    let mut s = Session::new(&folder("hidden"));
    let (a, b) = stacked(&mut s, 24.0);
    s.ok(
        "apply_ops",
        json!({"name": "Hide", "ops": [{"op": "set_hidden", "items": [b], "hidden": true}]}),
    );
    let r = clearance(&mut s, &a, &b);
    close(&r["gap_mm"], 5.0, 1e-6);
    assert_eq!(r["a"]["hidden"], false);
    assert_eq!(r["b"]["hidden"], true);
    let m = s.ok("measure", json!({"a": a, "b": b}));
    assert_eq!(m["b"]["hidden"], true);
}

/// A group of several solids is all of them; a component instance is measured where it
/// sits; the same part twice, a part inside the other and an item that is not there are
/// refused plainly.
#[test]
fn groups_instances_and_refusals() {
    let mut s = Session::new(&folder("groups"));
    let a = board(&mut s, "A", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
    let b = board(&mut s, "B", [19.0, 300.0, 600.0], [0.0, 0.0, 30.0]);
    let c = board(&mut s, "C", [19.0, 300.0, 600.0], [0.0, 0.0, 100.0]);
    let r = s.ok(
        "apply_ops",
        json!({"name": "Group", "ops": [{"op": "make_group", "items": [a, b], "name": "Pair"}]}),
    );
    let group = json!({"node": r["outcome"]["outcomes"][0]["group"]});
    let r = clearance(&mut s, &group, &c);
    assert_eq!(r["a"]["solids"], 2);
    assert_eq!(r["a"]["name"], "Pair");
    // The nearer solid of the group: B's top at 49, C's underside at 100.
    close(&r["gap_mm"], 51.0, 1e-6);
    // Refusals.
    let e = s.refused("check_clearance", json!({"a": c, "b": c}));
    assert!(e.contains("same part"), "{e}");
    let e = s.refused("measure", json!({"a": group, "b": a}));
    assert!(e.contains("inside the other"), "{e}");
    let e = s.refused("check_clearance", json!({"a": {"object": 99}, "b": c}));
    assert!(e.starts_with("a: no solid"), "{e}");
    let e = s.refused("measure", json!({"a": c, "b": {"node": 99}}));
    assert!(e.starts_with("b: no group or component"), "{e}");
    let e = s.refused("measure", json!({"a": c, "b": {"edge": 1}}));
    assert!(!e.is_empty());
    let e = s.refused("measure", json!({"a": c}));
    assert!(!e.is_empty());

    // A component and a copy of its instance, 200 mm along y.
    s.ok("new_model", json!({}));
    let leg = board(&mut s, "Leg", [40.0, 40.0, 700.0], [0.0, 0.0, 0.0]);
    let r = s.ok(
        "apply_ops",
        json!({"name": "Component", "ops": [{"op": "make_component", "items": [leg], "name": "Leg"}]}),
    );
    let first = r["outcome"]["outcomes"][0]["instance"].as_u64().unwrap();
    let r = s.ok(
        "apply_ops",
        json!({"name": "Copy", "ops": [{"op": "copy", "objects": [], "nodes": [first],
            "xform": {"linear": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": [0, 0.2, 0]}}]}),
    );
    let second = r["outcome"]["outcomes"][0]["nodes"][0].as_u64().unwrap();
    let r = clearance(&mut s, &json!({"node": first}), &json!({"node": second}));
    close(&r["gap_mm"], 160.0, 1e-6);
    assert_eq!(r["a"]["name"], "Leg");
    let m = s.ok(
        "measure",
        json!({"a": {"node": first}, "b": {"node": second}}),
    );
    close(&m["facing"][0]["distance_mm"], 160.0, 1e-6);
}

/// Two many-sided discs: more triangle pairs than a call may compare, refused before any
/// work, and the model as it was.
#[test]
fn parts_too_big_to_compare_are_refused_up_front() {
    let mut s = Session::new(&folder("big"));
    let disc = |s: &mut Session, z: f64| {
        let n = 3000;
        let points: Vec<[f64; 3]> = (0..n)
            .map(|i| {
                let a = i as f64 / n as f64 * std::f64::consts::TAU;
                [0.1 * a.cos(), 0.1 * a.sin(), z]
            })
            .collect();
        let r = s.ok(
            "apply_ops",
            json!({"name": "Disc", "ops": [
                {"op": "draw", "target": {"on": "plane", "origin": [0, 0, z], "normal": [0, 0, 1]},
                 "points": points, "closed": true},
                {"op": "extrude", "sketch": {"$ref": 0, "path": "sketch"},
                 "region": {"$ref": 0, "path": "inside"}, "distance": 0.02}]}),
        );
        json!({"object": r["outcome"]["outcomes"][1]["object"]})
    };
    let a = disc(&mut s, 0.0);
    let b = disc(&mut s, 0.1);
    let hash = s.hash();
    for tool in ["check_clearance", "measure"] {
        let e = s.refused(tool, json!({"a": a, "b": b}));
        assert!(
            e.contains("pairs to compare") && e.contains("minutes"),
            "{e}"
        );
    }
    assert_eq!(s.hash(), hash);
}

/// The verifier's finds on the first version.
#[test]
fn verifier_finds_stay_fixed() {
    let mut s = Session::new(&folder("finds"));
    // A thin overlap turned to the axes: the kernel's intersection gives up on it. The gap
    // still answers, and the overlap and seated say they are not known, and why.
    for angle in [0.3, std::f64::consts::FRAC_PI_4] {
        let (a, b) = stacked(&mut s, 18.99);
        turn_all(&mut s, &[1, 2], [1.0, 0.0, 0.0], angle, [0.0, 0.0, 0.0]);
        let r = clearance(&mut s, &a, &b);
        assert_eq!(r["gap_mm"], 0.0, "{r}");
        if r["overlap_unknown"].is_string() {
            assert_eq!(r["overlap_mm3"], Value::Null, "{r}");
            assert_eq!(r["overlap_size_mm"], Value::Null, "{r}");
            assert_eq!(r["seated"], Value::Null, "{r}");
        } else {
            assert_eq!(r["seated"], true, "{r}");
        }
    }

    // A solid inside a group, named by its own item, answers with its own name.
    s.ok("new_model", json!({}));
    let a = board(&mut s, "A", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
    let b = board(&mut s, "B", [19.0, 300.0, 600.0], [0.0, 0.0, 50.0]);
    s.ok(
        "apply_ops",
        json!({"name": "Group", "ops": [{"op": "make_group", "items": [a], "name": "Box"}]}),
    );
    let r = clearance(&mut s, &a, &b);
    assert_eq!(r["a"]["name"], "A", "{r}");

    // Hidden by a tag, and by the group around it.
    s.ok(
        "apply_ops",
        json!({"name": "Tag", "ops": [
            {"op": "create_tag", "name": "Back"},
            {"op": "tag_items", "items": [b], "tag": "Back"},
            {"op": "set_tag_visible", "tag": "Back", "visible": false}]}),
    );
    let r = clearance(&mut s, &a, &b);
    assert_eq!(r["a"]["hidden"], false, "{r}");
    assert_eq!(r["b"]["hidden"], true, "{r}");
    let group = json!({"node": s.ok("summary", json!({}))["parts"].as_array().unwrap()
        .iter().find(|p| p["items"][0]["node"].is_u64()).unwrap()["items"][0]["node"]});
    s.ok(
        "apply_ops",
        json!({"name": "Hide", "ops": [{"op": "set_hidden", "items": [group], "hidden": true}]}),
    );
    let r = clearance(&mut s, &a, &b);
    assert_eq!(r["a"]["hidden"], true, "the group around it is hidden: {r}");

    // Parallel faces looking at each other with no area in common: nothing.
    s.ok("new_model", json!({}));
    let a = board(&mut s, "A", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
    let b = board(&mut s, "B", [19.0, 300.0, 600.0], [700.0, 0.0, 50.0]);
    let m = s.ok("measure", json!({"a": a, "b": b}));
    assert_eq!(m["facing"], json!([]), "{m}");

    // A small block either side of the diagonal that splits a board's top into two
    // triangles (corner to corner, (0, 0) to (600, 300)), so whichever triangle is compared
    // first, one block is over the other.
    for at in [[10.0, 200.0, 30.0], [540.0, 10.0, 30.0]] {
        s.ok("new_model", json!({}));
        let a = board(&mut s, "A", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
        let b = board(&mut s, "Block", [10.0, 50.0, 50.0], at);
        let m = s.ok("measure", json!({"a": a, "b": b}));
        let facing = m["facing"].as_array().unwrap();
        assert_eq!(facing.len(), 1, "block at {at:?}: {m}");
        close(&facing[0]["distance_mm"], 11.0, 1e-6);
    }
}

/// A prism from a profile in the x-z plane (mm), drawn on y = 0 and extruded `depth` mm
/// along +y. Its item.
fn prism(s: &mut Session, profile: &[[f64; 2]], depth: f64) -> Value {
    let points: Vec<[f64; 3]> = profile
        .iter()
        .map(|p| [p[0] / 1000.0, 0.0, p[1] / 1000.0])
        .collect();
    let r = s.ok(
        "apply_ops",
        json!({"name": "Prism", "ops": [
            {"op": "draw", "target": {"on": "plane", "origin": [0, 0, 0], "normal": [0, 1, 0]},
             "points": points, "closed": true},
            {"op": "extrude", "sketch": {"$ref": 0, "path": "sketch"},
             "region": {"$ref": 0, "path": "inside"}, "distance": depth / 1000.0}]}),
    );
    json!({"object": r["outcome"]["outcomes"][1]["object"]})
}

/// A disc of `n` sides, radius 100 mm, 20 mm thick, its base at `z` metres: 4n - 4 triangles.
fn disc(s: &mut Session, n: usize, z: f64) -> Value {
    let points: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            [0.1 * a.cos(), 0.1 * a.sin(), z]
        })
        .collect();
    let r = s.ok(
        "apply_ops",
        json!({"name": "Disc", "ops": [
            {"op": "draw", "target": {"on": "plane", "origin": [0, 0, z], "normal": [0, 0, 1]},
             "points": points, "closed": true},
            {"op": "extrude", "sketch": {"$ref": 0, "path": "sketch"},
             "region": {"$ref": 0, "path": "inside"}, "distance": 0.02}]}),
    );
    json!({"object": r["outcome"]["outcomes"][1]["object"]})
}

/// `count` copies of the solid `first` in a row, each moved `step` metres from the last;
/// then all of them, the first included, made one group. The group's item.
fn row_group(s: &mut Session, first: &Value, count: u32, step: [f64; 3], name: &str) -> Value {
    let id = first["object"].as_u64().unwrap();
    s.ok(
        "apply_ops",
        json!({"name": "Row", "ops": [{"op": "array", "objects": [id], "nodes": [], "count": count,
            "xform": {"linear": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": step}}]}),
    );
    let items: Vec<Value> = (id..=id + u64::from(count))
        .map(|i| json!({"object": i}))
        .collect();
    let r = s.ok(
        "apply_ops",
        json!({"name": "Group", "ops": [{"op": "make_group", "items": items, "name": name}]}),
    );
    json!({"node": r["outcome"]["outcomes"][0]["group"]})
}

fn distances(m: &Value) -> Vec<f64> {
    m["facing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["distance_mm"].as_f64().unwrap())
        .collect()
}

/// A group two deep, a group's overlap added up over its solids, and a component copy
/// turned a quarter and moved.
#[test]
fn nested_groups_a_group_s_overlap_and_a_turned_copy() {
    let mut s = Session::new(&folder("nested"));
    let a = board(&mut s, "A", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
    let b = board(&mut s, "B", [19.0, 300.0, 600.0], [0.0, 0.0, 40.0]);
    let c = board(&mut s, "C", [19.0, 300.0, 600.0], [0.0, 0.0, 100.0]);
    let r = s.ok(
        "apply_ops",
        json!({"name": "G", "ops": [{"op": "make_group", "items": [b], "name": "Inner"}]}),
    );
    let inner = r["outcome"]["outcomes"][0]["group"].clone();
    let r = s.ok(
        "apply_ops",
        json!({"name": "G", "ops": [{"op": "make_group", "items": [{"node": inner}, c], "name": "Outer"}]}),
    );
    let outer = json!({"node": r["outcome"]["outcomes"][0]["group"]});
    // B, two deep, is the nearer: its underside at 40, A's top at 19.
    let r = clearance(&mut s, &a, &outer);
    close(&r["gap_mm"], 21.0, 1e-6);
    assert_eq!(r["b"]["solids"], 2, "{r}");
    let m = s.ok("measure", json!({"a": a, "b": outer}));
    assert_eq!(distances(&m), [21.0, 81.0], "{m}");
    let e = s.refused("check_clearance", json!({"a": {"node": inner}, "b": outer}));
    assert!(e.contains("inside the other"), "{e}");

    // A group of two boards, 3 mm and 2 mm into a third: the overlaps added up, the box
    // spanning both.
    s.ok("new_model", json!({}));
    let base = board(&mut s, "Base", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
    let l = board(&mut s, "L", [19.0, 100.0, 200.0], [0.0, 0.0, 16.0]);
    let r2 = board(&mut s, "R", [19.0, 100.0, 200.0], [300.0, 0.0, 17.0]);
    let r = s.ok(
        "apply_ops",
        json!({"name": "G", "ops": [{"op": "make_group", "items": [l, r2], "name": "Pair"}]}),
    );
    let pair = json!({"node": r["outcome"]["outcomes"][0]["group"]});
    let r = clearance(&mut s, &base, &pair);
    close(&r["overlap_mm3"], 60_000.0 + 40_000.0, 1e-3);
    assert_eq!(r["overlap_size_mm"], json!([500.0, 100.0, 3.0]), "{r}");
    assert_eq!(r["seated"], false);

    // A component and a copy of it turned a quarter about z and moved 1 m along x: the
    // copy is x 960 to 1000 and y 0 to 700, so 260 mm past the first's end.
    s.ok("new_model", json!({}));
    let leg = board(&mut s, "Leg", [40.0, 40.0, 700.0], [0.0, 0.0, 0.0]);
    let r = s.ok(
        "apply_ops",
        json!({"name": "C", "ops": [{"op": "make_component", "items": [leg], "name": "Leg"}]}),
    );
    let first = r["outcome"]["outcomes"][0]["instance"].as_u64().unwrap();
    let r = s.ok(
        "apply_ops",
        json!({"name": "Copy", "ops": [{"op": "copy", "objects": [], "nodes": [first],
            "xform": {"linear": [[0, -1, 0], [1, 0, 0], [0, 0, 1]], "translation": [1.0, 0, 0]}}]}),
    );
    let second = json!({"node": r["outcome"]["outcomes"][0]["nodes"][0]});
    let r = clearance(&mut s, &json!({"node": first}), &second);
    close(&r["gap_mm"], 260.0, 1e-6);
    let near = board(&mut s, "Board", [19.0, 100.0, 100.0], [950.0, 705.0, 0.0]);
    let r = clearance(&mut s, &second, &near);
    close(&r["gap_mm"], 5.0, 1e-6);
}

/// A side with a dado 19 mm wide and 6 mm deep, and a shelf in it.
#[test]
fn a_shelf_in_a_dado() {
    let mut s = Session::new(&folder("dado"));
    let side_profile = [
        [0.0, 0.0],
        [19.0, 0.0],
        [19.0, 300.0],
        [13.0, 300.0],
        [13.0, 319.0],
        [19.0, 319.0],
        [19.0, 700.0],
        [0.0, 700.0],
    ];
    // The shelf's thickness and where its end is; the overlap, seated, measure's distances.
    for (t, x, overlap, seated, want) in [
        (19.0, 13.0, 0.0, true, vec![0.0, 0.0, 0.0]),
        (18.5, 13.0, 0.0, true, vec![0.0, 0.0, 0.5]),
        // Not pushed home: it touches both walls, so it is seated; measure says how far its
        // end is from the floor.
        (19.0, 14.0, 0.0, true, vec![0.0, 0.0, 1.0]),
        (19.02, 13.0, 6.0 * 300.0 * 0.02, false, vec![0.0, 0.0]),
    ] {
        s.ok("new_model", json!({}));
        let side = prism(&mut s, &side_profile, 300.0);
        let shelf = s.ok(
            "add_board",
            json!({"name": "Shelf", "thickness": t, "width": 300, "length": 462,
                "position": [x, 0, 300], "length_axis": "x", "thickness_axis": "z"}),
        );
        let shelf = json!({"object": shelf["board"]["object"]});
        let r = clearance(&mut s, &side, &shelf);
        assert_eq!(r["gap_mm"], 0.0, "{t} at {x}: {r}");
        close(&r["overlap_mm3"], overlap, 1e-3);
        assert_eq!(r["seated"], seated, "{t} at {x}: {r}");
        let m = s.ok("measure", json!({"a": side, "b": shelf}));
        assert_eq!(distances(&m), want, "{t} at {x}: {m}");
    }
    // A shelf drawn 1 mm too thick, its end on the floor: the kernel's intersection refuses
    // side with shelf and builds shelf with side, so the other order answers.
    s.ok("new_model", json!({}));
    let side = prism(
        &mut s,
        &[
            [0.0, 0.0],
            [19.0, 0.0],
            [19.0, 10.0],
            [13.0, 10.0],
            [13.0, 29.0],
            [19.0, 29.0],
            [19.0, 40.0],
            [0.0, 40.0],
        ],
        10.0,
    );
    let shelf = prism(
        &mut s,
        &[[13.0, 10.0], [40.0, 10.0], [40.0, 30.0], [13.0, 30.0]],
        10.0,
    );
    let r = clearance(&mut s, &side, &shelf);
    assert!(r.get("overlap_unknown").is_none(), "{r}");
    close(&r["overlap_mm3"], 6.0 * 10.0 * 1.0, 1e-3);
    assert_eq!(r["seated"], false);
}

/// Both limits sit where they say, and refuse before any work.
#[test]
fn the_limits_sit_where_they_say() {
    let mut s = Session::new(&folder("limits"));
    // Discs of 1,118 sides, 4,468 triangles each: 19,963,024 pairs, answered.
    let a = disc(&mut s, 1118, 0.0);
    let b = disc(&mut s, 1118, 0.021);
    close(&clearance(&mut s, &a, &b)["gap_mm"], 1.0, 1e-6);
    let m = s.ok("measure", json!({"a": a, "b": b}));
    close(&m["facing"][0]["distance_mm"], 1.0, 1e-6);
    // 1,120 sides, 4,476 triangles each: 20,034,576 pairs, refused.
    s.ok("new_model", json!({}));
    let a = disc(&mut s, 1120, 0.0);
    let b = disc(&mut s, 1120, 0.021);
    for tool in ["check_clearance", "measure"] {
        let e = s.refused(tool, json!({"a": a, "b": b}));
        assert!(e.contains("4476 and 4476 triangles, 20034576 pairs"), "{e}");
    }
    // A disc of 5,000 sides (19,996 triangles) and cubes 5 mm into it: the intersections
    // take in 19,996 + 12 triangles a cube. Six cubes, 120,048, answered.
    s.ok("new_model", json!({}));
    let big = disc(&mut s, 5000, 0.0);
    let cube = board(&mut s, "Cube", [10.0, 10.0, 10.0], [0.0, 0.0, 15.0]);
    let six = row_group(&mut s, &cube, 5, [0.0005, 0.0, 0.0], "Six");
    let r = clearance(&mut s, &big, &six);
    close(&r["overlap_mm3"], 6.0 * 500.0, 1e-3);
    // Fifty cubes, 1,000,400: refused, while measure, which works out no intersection,
    // answers.
    s.ok("new_model", json!({}));
    let big = disc(&mut s, 5000, 0.0);
    let cube = board(&mut s, "Cube", [10.0, 10.0, 10.0], [0.0, 0.0, 15.0]);
    let fifty = row_group(&mut s, &cube, 49, [0.0005, 0.0, 0.0], "Fifty");
    let hash = s.hash();
    let e = s.refused("check_clearance", json!({"a": big, "b": fifty}));
    assert!(
        e.contains("50 pairs of solids whose boxes meet, taking in 1000400 triangles"),
        "{e}"
    );
    s.ok("measure", json!({"a": big, "b": fifty}));
    assert_eq!(s.hash(), hash);
}

/// measure's test for parallel is the kernel's PARALLEL_EPS, and it lists at most 50 pairs,
/// nearest first, counting the rest.
#[test]
fn measure_s_parallel_tolerance_and_its_limit() {
    let mut s = Session::new(&folder("facing"));
    for (degrees, listed) in [(0.0, 1), (0.001, 0)] {
        s.ok("new_model", json!({}));
        let a = board(&mut s, "A", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
        let b = board(&mut s, "B", [19.0, 300.0, 600.0], [0.0, 0.0, 50.0]);
        turn_all(
            &mut s,
            &[2],
            [1.0, 0.0, 0.0],
            f64::to_radians(degrees),
            [0.0, 0.0, 0.0],
        );
        let m = s.ok("measure", json!({"a": a, "b": b}));
        assert_eq!(
            m["facing"].as_array().unwrap().len(),
            listed,
            "{degrees}: {m}"
        );
    }
    // 60 blocks over a board, each 20 mm further along and 0.5 mm higher than the last.
    s.ok("new_model", json!({}));
    let top = board(&mut s, "Top", [19.0, 600.0, 1200.0], [0.0, 0.0, 0.0]);
    let block = board(&mut s, "Block", [10.0, 10.0, 10.0], [5.0, 5.0, 20.0]);
    let blocks = row_group(&mut s, &block, 59, [0.02, 0.0, 0.0005], "Blocks");
    let m = s.ok("measure", json!({"a": top, "b": blocks}));
    let want: Vec<f64> = (0..50).map(|k| 1.0 + 0.5 * f64::from(k)).collect();
    assert_eq!(distances(&m), want, "{m}");
    assert_eq!(m["more"], 10);
}

/// A group that holds no solid, only an imported mesh or only a sketch, is refused plainly.
#[test]
fn a_group_with_no_solid_is_refused() {
    let mut s = Session::new(&folder("nosolid"));
    let a = board(&mut s, "A", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
    // One open triangle: a mesh, not a solid, in a group of its own.
    let r = s.ok(
        "apply_ops",
        json!({"name": "Import", "ops": [{"op": "import", "items": [{"name": "Sheet", "group": "Loose",
            "positions": [[0, 0, 0.1], [0.1, 0, 0.1], [0, 0.1, 0.1]], "faces": [[[0, 1, 2]]]}]}]}),
    );
    let loose = r["outcome"]["outcomes"][0]["groups"][0].as_u64().unwrap();
    let e = s.refused("check_clearance", json!({"a": a, "b": {"node": loose}}));
    assert_eq!(
        e,
        format!("b: {{\"node\":{loose}}} holds no solid to measure")
    );
    let e = s.refused("measure", json!({"a": {"node": loose}, "b": a}));
    assert_eq!(
        e,
        format!("a: {{\"node\":{loose}}} holds no solid to measure")
    );
    // Create Group from Slice: a group holding only a sketch.
    let r = s.ok(
        "apply_ops",
        json!({"name": "Slice", "ops": [
            {"op": "add_section", "origin": [0.3, 0.15, 0.01], "normal": [1, 0, 0]},
            {"op": "slice_to_group", "section": {"$ref": 0, "path": "section"}}]}),
    );
    let slice = r["outcome"]["outcomes"][1]["group"].as_u64().unwrap();
    let e = s.refused("check_clearance", json!({"a": a, "b": {"node": slice}}));
    assert!(e.contains("holds no solid"), "{e}");
}

/// The pair limit at exactly its number and one over, and what the intersection limit counts:
/// solids whose boxes meet or come within 0.01 mm, never solids further apart.
#[test]
fn the_limits_at_their_edges_and_what_they_count() {
    let mut s = Session::new(&folder("edges"));
    // 1,251 and 1,001 sides, 1 mm apart: 5,000 x 4,000 = 20,000,000 pairs, answered.
    let a = disc(&mut s, 1251, 0.0);
    let b = disc(&mut s, 1001, 0.021);
    close(&clearance(&mut s, &a, &b)["gap_mm"], 1.0, 1e-6);
    // 1,252 sides: 5,004 x 4,000 = 20,016,000, refused.
    s.ok("new_model", json!({}));
    let a = disc(&mut s, 1252, 0.0);
    let b = disc(&mut s, 1001, 0.021);
    let e = s.refused("check_clearance", json!({"a": a, "b": b}));
    assert!(e.contains("5004 and 4000 triangles, 20016000 pairs"), "{e}");

    // A disc of 4,998 sides (19,988 triangles, a corner at x = 100 mm, y = 0) and 51 cubes in a
    // row past it along x: 20,000 triangles a pair whose boxes meet.
    let row_past = |s: &mut Session, x: f64, count: u32| {
        s.ok("new_model", json!({}));
        let d = disc(s, 4998, 0.0);
        let cube = board(s, "Cube", [10.0, 10.0, 10.0], [x, -5.0, 5.0]);
        let g = if count > 1 {
            row_group(s, &cube, count - 1, [0.0, 0.0, 0.0001], "Cubes")
        } else {
            cube
        };
        (d, g)
    };
    // 20 mm past the disc's box: no pair counts, so 51 cubes are answered (counting every
    // pair would make 1,020,000).
    let (d, g) = row_past(&mut s, 120.0, 51);
    let r = clearance(&mut s, &d, &g);
    close(&r["gap_mm"], 20.0, 1e-6);
    assert_eq!(r["overlap_mm3"], 0.0);
    // 0.005 mm past it, within 0.01 mm: every pair counts, 1,020,000, refused.
    let (d, g) = row_past(&mut s, 100.005, 51);
    let e = s.refused("check_clearance", json!({"a": d, "b": g}));
    assert!(
        e.contains("51 pairs of solids whose boxes meet, taking in 1020000 triangles"),
        "{e}"
    );
    // One cube there: worked out, apart by 0.005 mm, seated.
    let (d, g) = row_past(&mut s, 100.005, 1);
    let r = clearance(&mut s, &d, &g);
    close(&r["gap_mm"], 0.005, 1e-6);
    assert_eq!(r["overlap_mm3"], 0.0);
    assert_eq!(r["seated"], true);
}

/// `r` is a's answer against the solids `bs`: its `overlap_unknown` is the reason the kernel
/// gives, in the first order, for the first of them it cannot intersect with a either way
/// round, worked out on the model saved and read back; none when there is no such pair.
fn reason_is_the_first_pair_s(
    s: &mut Session,
    dir: &std::path::Path,
    a: u64,
    bs: &[u64],
    r: &Value,
) {
    use sawhorse_kernel::boolean::BoolOp;
    s.ok("save_model", json!({"path": "reason.shz"}));
    let opened = sawhorse_shz::read(&std::fs::read(dir.join("reason.shz")).unwrap()).unwrap();
    let model = opened.document.model();
    let oa = model.object(a).unwrap();
    let mut want = None;
    for &j in bs {
        let ob = model.object(j).unwrap();
        let (first, second) = (
            oa.combine(ob, BoolOp::Intersection),
            ob.combine(oa, BoolOp::Intersection),
        );
        if let (Err(e), Err(f)) = (&first, &second) {
            eprintln!("solids {a} and {j}: first order {e}; second order {f}");
            want = Some(format!(
                "the overlap of solids {a} and {j} could not be worked out: {e}"
            ));
            break;
        }
    }
    match want {
        Some(w) => {
            assert_eq!(r["overlap_unknown"], w, "{r}");
            assert_eq!(r["seated"], Value::Null, "{r}");
        }
        None => assert!(r.get("overlap_unknown").is_none(), "{r}"),
    }
}

/// When the kernel cannot build an intersection either way round, `overlap_unknown` gives
/// the reason for the first pair of solids that failed (a's solids in the model's order, each
/// against b's), from the first order (a's solid with b's): checked here against the kernel
/// itself on the saved file. And `measure` lists faces up to the merge distance past each
/// other at 0, further past not at all.
#[test]
fn the_reason_overlap_unknown_gives_and_faces_a_hair_past() {
    let dir = folder("reason");
    let mut s = Session::new(&dir);
    // A board and a group of two boards each 0.01 mm into it, all turned 0.3 rad about x.
    let a = board(&mut s, "A", [19.0, 300.0, 600.0], [0.0, 0.0, 0.0]);
    let b1 = board(&mut s, "B1", [19.0, 300.0, 600.0], [0.0, 0.0, 18.99]);
    let b2 = board(&mut s, "B2", [19.0, 300.0, 300.0], [0.0, 0.0, 18.99]);
    let r = s.ok(
        "apply_ops",
        json!({"name": "G", "ops": [{"op": "make_group", "items": [b1, b2], "name": "Pair"}]}),
    );
    let group = r["outcome"]["outcomes"][0]["group"].as_u64().unwrap();
    let (c, sn) = (0.3f64.cos(), 0.3f64.sin());
    s.ok(
        "apply_ops",
        json!({"name": "Turn", "ops": [{"op": "transform", "objects": [a["object"]], "nodes": [group],
            "xform": {"linear": [[1, 0, 0], [0, c, -sn], [0, sn, c]], "translation": [0, 0, 0]}}]}),
    );
    let r = clearance(&mut s, &a, &json!({"node": group}));
    let ids = [
        b1["object"].as_u64().unwrap(),
        b2["object"].as_u64().unwrap(),
    ];
    reason_is_the_first_pair_s(&mut s, &dir, a["object"].as_u64().unwrap(), &ids, &r);

    // A shelf 1 mm too thick for its dado, its end on the floor, turned about a skew axis:
    // here the two orders give different reasons.
    s.ok("new_model", json!({}));
    let side = prism(
        &mut s,
        &[
            [0.0, 0.0],
            [19.0, 0.0],
            [19.0, 10.0],
            [13.0, 10.0],
            [13.0, 29.0],
            [19.0, 29.0],
            [19.0, 40.0],
            [0.0, 40.0],
        ],
        10.0,
    );
    let shelf = prism(
        &mut s,
        &[[13.0, 10.0], [40.0, 10.0], [40.0, 30.0], [13.0, 30.0]],
        10.0,
    );
    let (i, j) = (
        side["object"].as_u64().unwrap(),
        shelf["object"].as_u64().unwrap(),
    );
    turn_all(&mut s, &[i, j], [0.3, -0.7, 0.648], 0.83, [0.0, 0.0, 0.0]);
    let r = clearance(&mut s, &side, &shelf);
    reason_is_the_first_pair_s(&mut s, &dir, i, &[j], &r);

    for (z, want) in [(18.99, vec![0.0]), (18.9899, vec![])] {
        let (a, b) = stacked(&mut s, z);
        let m = s.ok("measure", json!({"a": a, "b": b}));
        assert_eq!(distances(&m), want, "underside at {z}: {m}");
    }
}
