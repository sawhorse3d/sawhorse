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
