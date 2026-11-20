//! Booleans of solids that touch face to face where they meet (lane 81, #124): a shelf in a
//! dado, a shelf end on a dado's floor, two boards lying on each other. The boolean engine
//! can keep both touching faces over the contact area, facing opposite ways (a sheet of no
//! thickness hinged to the true result), and the validator refused that result as faces
//! overlapping or an edge through a face. The kernel now cancels such sheets
//! (`boolean::combine`), so these answer, the same whichever solid is first, square to the
//! axes, turned and far from the origin. Volumes are worked out by hand from the profiles.
//!
//! What still refuses is pinned by the ignored tests at the end, with what they should
//! answer.

use proptest::prelude::*;
use sawhorse_kernel::boolean::BoolOp;
use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::object::Object;

/// A prism from a profile in x and z (mm) on the plane y = 0, extruded `depth` mm along +y.
fn prism(m: &mut Model, profile: &[(f64, f64)], depth: f64) -> u64 {
    let points: Vec<[f64; 3]> = profile
        .iter()
        .map(|&(x, z)| [x / 1000.0, 0.0, z / 1000.0])
        .collect();
    solid(m, [0.0, 1.0, 0.0], [0.0; 3], points, depth / 1000.0, 1)
}

/// A board drawn as the MCP server's `add_board` draws one: a rectangle on the plane
/// z = `z` (mm) from (`x`, `y`), `length` along x and `width` along y, extruded `thickness`
/// upwards.
fn board(m: &mut Model, [x, y, z]: [f64; 3], length: f64, width: f64, thickness: f64) -> u64 {
    let [x, y, z, l, w] = [x, y, z, length, width].map(|v| v / 1000.0);
    let points = vec![[x, y, z], [x + l, y, z], [x + l, y + w, z], [x, y + w, z]];
    solid(
        m,
        [0.0, 0.0, 1.0],
        [0.0, 0.0, z],
        points,
        thickness / 1000.0,
        2,
    )
}

/// Draw `points` on the plane through `origin` with `normal` and extrude the region `d`
/// to the positive side of axis `axis` (the extrude goes the way the sketch faces, which
/// is tried on a copy first, as `add_board` does).
fn solid(
    m: &mut Model,
    normal: [f64; 3],
    origin: [f64; 3],
    points: Vec<[f64; 3]>,
    d: f64,
    axis: usize,
) -> u64 {
    let draw: Op = serde_json::from_value(serde_json::json!({"op": "draw",
        "target": {"on": "plane", "origin": origin, "normal": normal},
        "points": points, "closed": true}))
    .unwrap();
    let out = serde_json::to_value(m.apply(&draw).unwrap()).unwrap();
    let (sketch, region) = (
        out["sketch"].as_u64().unwrap(),
        out["inside"].as_u64().unwrap(),
    );
    for distance in [d, -d] {
        let mut t = m.clone();
        let op = Op::Extrude {
            sketch,
            region,
            distance,
        };
        if let OpOutcome::ObjectCreated { object } = t.apply(&op).unwrap()
            && t.object(object).unwrap().bounds().0[axis] >= origin[axis] - d / 2.0
        {
            *m = t;
            return object;
        }
    }
    panic!("no solid")
}

/// The rotation by `angle` about the unit axis along `axis`, rows of a 3 x 3 matrix.
fn rotation(axis: [f64; 3], angle: f64) -> [[f64; 3]; 3] {
    let l = axis.iter().map(|c| c * c).sum::<f64>().sqrt();
    let k = axis.map(|c| c / l);
    let (c, s) = (angle.cos(), angle.sin());
    let turn = |v: [f64; 3]| {
        let d = k[0] * v[0] + k[1] * v[1] + k[2] * v[2];
        let x = [
            k[1] * v[2] - k[2] * v[1],
            k[2] * v[0] - k[0] * v[2],
            k[0] * v[1] - k[1] * v[0],
        ];
        [0, 1, 2].map(|i| v[i] * c + x[i] * s + k[i] * d * (1.0 - c))
    };
    let cols = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(turn);
    [0, 1, 2].map(|r| [0, 1, 2].map(|c| cols[c][r]))
}

/// The issue's turn: 0.83 rad about the axis along (0.3, -0.7, 0.648), as it gives it.
const ISSUE_TURN: [[f64; 3]; 3] = [
    [0.704139751007983, -0.5464851282683473, -0.4533665356330833],
    [
        0.40991983723033976,
        0.8342019329489425,
        -0.36888055263092934,
    ],
    [0.5797869764796959, 0.0738995239888332, 0.8114098361856088],
];

const SKEW: [f64; 3] = [0.3, -0.7, 0.648];

/// A name, the linear part and the translation (m) of a placement.
type Pose = (&'static str, [[f64; 3]; 3], [f64; 3]);

/// Square to the axes, turned about x, the issue's skew turn, that turn moved about 50 m,
/// and a turn of 2.1 rad about the same axis moved about 1 km.
fn poses() -> Vec<Pose> {
    vec![
        ("square", rotation([1.0, 0.0, 0.0], 0.0), [0.0; 3]),
        ("0.3 rad about x", rotation([1.0, 0.0, 0.0], 0.3), [0.0; 3]),
        ("0.83 rad about the skew axis", ISSUE_TURN, [0.0; 3]),
        (
            "the same, moved (40, -30, 20) m",
            ISSUE_TURN,
            [40.0, -30.0, 20.0],
        ),
        (
            "2.1 rad, moved (1000, 300, -50) m",
            rotation(SKEW, 2.1),
            [1000.0, 300.0, -50.0],
        ),
    ]
}

fn place(m: &mut Model, objects: &[u64], linear: [[f64; 3]; 3], translation: [f64; 3]) {
    if linear == rotation([1.0, 0.0, 0.0], 0.0) && translation == [0.0; 3] {
        return;
    }
    let op: Op = serde_json::from_value(serde_json::json!({"op": "transform", "objects": objects,
        "nodes": [], "xform": {"linear": linear, "translation": translation}}))
    .unwrap();
    m.apply(&op).unwrap();
}

/// `a op b`: its volume in mm3, `None` when empty; panics with the refusal.
fn volume(a: &Object, b: &Object, op: BoolOp, what: &str) -> Option<f64> {
    match a.combine(b, op) {
        Ok(Some(o)) => Some(o.volume() * 1e9),
        Ok(None) => None,
        Err(e) => panic!("{what}: {op:?} refused: {e}"),
    }
}

fn close(got: Option<f64>, want: f64, what: &str) {
    near(got, want, want * 1e-5, what);
}

/// `got` within `tol` mm3 of `want`.
fn near(got: Option<f64>, want: f64, tol: f64, what: &str) {
    let got = got.unwrap_or_else(|| panic!("{what}: empty, expected {want} mm3"));
    assert!(
        (got - want).abs() <= tol,
        "{what}: {got} mm3, expected {want} within {tol}"
    );
}

/// The side with a dado of steps 1 to 5: 19 x 40 mm, the dado x 13 to 19, z 10 to 29.
const SIDE: [(f64, f64); 8] = [
    (0.0, 0.0),
    (19.0, 0.0),
    (19.0, 10.0),
    (13.0, 10.0),
    (13.0, 29.0),
    (19.0, 29.0),
    (19.0, 40.0),
    (0.0, 40.0),
];
/// The side's volume: (19 x 40 - 6 x 19) x 10 mm3.
const SIDE_MM3: f64 = 6460.0;

/// Steps 2 to 5 of #124: a shelf 20 mm thick in the 19 mm dado, its end on the dado's floor
/// (x = 13: 6 x 1 x 10 = 60 mm3 in common) or 1 mm off it (x = 14: 50 mm3). Every order and
/// every operation, in every pose.
#[test]
fn a_shelf_too_thick_for_its_dado() {
    for (end, common) in [(13.0, 60.0), (14.0, 50.0)] {
        let shelf_mm3 = (40.0 - end) * 20.0 * 10.0;
        for (pose, linear, at) in poses() {
            let mut m = Model::new();
            let side = prism(&mut m, &SIDE, 10.0);
            let shelf = prism(
                &mut m,
                &[(end, 10.0), (40.0, 10.0), (40.0, 30.0), (end, 30.0)],
                10.0,
            );
            place(&mut m, &[side, shelf], linear, at);
            let (a, b) = (m.object(side).unwrap(), m.object(shelf).unwrap());
            let what = format!("end at x = {end}, {pose}");
            close(volume(a, b, BoolOp::Intersection, &what), common, &what);
            close(volume(b, a, BoolOp::Intersection, &what), common, &what);
            close(
                volume(a, b, BoolOp::Difference, &what),
                SIDE_MM3 - common,
                &what,
            );
            close(
                volume(b, a, BoolOp::Difference, &what),
                shelf_mm3 - common,
                &what,
            );
            close(
                volume(a, b, BoolOp::Union, &what),
                SIDE_MM3 + shelf_mm3 - common,
                &what,
            );
        }
    }
}

/// Step 7 of #124: two 19 x 300 x 600 mm boards, one lying on the other. Touching faces
/// have no volume in common: empty both ways in every pose, and turned 0.1 and 0.83 rad
/// about the skew axis about 5 km out.
#[test]
fn boards_that_only_touch_have_nothing_in_common() {
    let mut poses = poses();
    poses.push((
        "0.1 rad, moved (5000, 300, -50) m",
        rotation(SKEW, 0.1),
        [5000.0, 300.0, -50.0],
    ));
    poses.push((
        "0.83 rad, moved (5000, 300, -50) m",
        rotation(SKEW, 0.83),
        [5000.0, 300.0, -50.0],
    ));
    for (pose, linear, at) in poses {
        let mut m = Model::new();
        let lower = board(&mut m, [0.0; 3], 600.0, 300.0, 19.0);
        let upper = board(&mut m, [0.0, 0.0, 19.0], 600.0, 300.0, 19.0);
        place(&mut m, &[lower, upper], linear, at);
        let (a, b) = (m.object(lower).unwrap(), m.object(upper).unwrap());
        assert_eq!(volume(a, b, BoolOp::Intersection, pose), None, "{pose}");
        assert_eq!(volume(b, a, BoolOp::Intersection, pose), None, "{pose}");
        close(volume(a, b, BoolOp::Difference, pose), 3_420_000.0, pose);
        close(volume(a, b, BoolOp::Union, pose), 6_840_000.0, pose);
    }
}

/// Boards 0.005 mm into each other, less than the merge distance: their overlap is empty
/// both ways, also turned 2.1 rad about 1 km out and 0.1 rad about 5 km out, where the two
/// welded faces used to be refused as faces overlapping with no volume.
#[test]
fn an_overlap_thinner_than_the_merge_distance_is_empty() {
    let mut poses = poses();
    poses.push((
        "0.1 rad, moved (5000, 300, -50) m",
        rotation(SKEW, 0.1),
        [5000.0, 300.0, -50.0],
    ));
    for (pose, linear, at) in poses {
        let mut m = Model::new();
        let lower = board(&mut m, [0.0; 3], 600.0, 300.0, 19.0);
        let upper = board(&mut m, [0.0, 0.0, 18.995], 600.0, 300.0, 19.0);
        place(&mut m, &[lower, upper], linear, at);
        let (a, b) = (m.object(lower).unwrap(), m.object(upper).unwrap());
        assert_eq!(volume(a, b, BoolOp::Intersection, pose), None, "{pose}");
        assert_eq!(volume(b, a, BoolOp::Intersection, pose), None, "{pose}");
    }
}

/// The side of step 8: a 19 mm board 700 long and 300 deep with a dado 19 wide and 6 deep
/// at z 300 to 319.
const SIDE8: [(f64, f64); 8] = [
    (0.0, 0.0),
    (19.0, 0.0),
    (19.0, 300.0),
    (13.0, 300.0),
    (13.0, 319.0),
    (19.0, 319.0),
    (19.0, 700.0),
    (0.0, 700.0),
];
/// (19 x 700 - 6 x 19) x 300 mm3.
const SIDE8_MM3: f64 = 3_955_800.0;

/// Step 8 of #124: a 19 x 300 x 462 mm shelf cut to the dado's size, its end 1 mm into the
/// dado's floor (x = 12: 1 x 19 x 300 = 5,700 mm3) or 0.5 mm (2,850 mm3). The overlap's
/// end faces are 19 x 300 mm, and each may sit up to the planarity tolerance (1 um) off:
/// 5.7 mm3 (turned 0.3 rad about x it measures 1.1 mm3 short, a face 0.2 um off).
#[test]
fn a_shelf_pushed_into_its_dados_floor() {
    let tol = 19.0 * 300.0 * 1e-3;
    let mut poses = poses();
    poses.push(("0.83 rad, 40 m along x", ISSUE_TURN, [40.0, 0.0, 0.0]));
    poses.push(("0.83 rad, 40 m along z", ISSUE_TURN, [0.0, 0.0, 40.0]));
    for (x, common) in [(12.0, 5700.0), (12.5, 2850.0)] {
        for &(pose, linear, at) in &poses {
            let mut m = Model::new();
            let side = prism(&mut m, &SIDE8, 300.0);
            let shelf = board(&mut m, [x, 0.0, 300.0], 462.0, 300.0, 19.0);
            place(&mut m, &[side, shelf], linear, at);
            let (a, b) = (m.object(side).unwrap(), m.object(shelf).unwrap());
            let what = format!("end at x = {x}, {pose}");
            near(
                volume(a, b, BoolOp::Intersection, &what),
                common,
                tol,
                &what,
            );
            near(
                volume(b, a, BoolOp::Intersection, &what),
                common,
                tol,
                &what,
            );
            let d = volume(a, b, BoolOp::Difference, &what);
            near(d, SIDE8_MM3 - common, tol, &what);
            let d = volume(b, a, BoolOp::Difference, &what);
            near(d, 462.0 * 19.0 * 300.0 - common, tol, &what);
        }
    }
}

/// The notes of #124: the step 8 side with a shelf from 0.011 to 1 mm too thick for the
/// dado, its end on the floor (x = 13): the overlap is the dado's 6 mm depth by 300 by the
/// excess. Before this lane, turned 0.83 rad about the skew axis, it was refused both ways
/// at every excess, and square to the axes one way.
#[test]
fn a_shelf_a_hair_too_thick_for_its_dado() {
    for excess in [0.011, 0.02, 0.1, 1.0] {
        for (pose, linear, at) in poses() {
            let mut m = Model::new();
            let side = prism(&mut m, &SIDE8, 300.0);
            let shelf = board(&mut m, [13.0, 0.0, 300.0], 462.0, 300.0, 19.0 + excess);
            place(&mut m, &[side, shelf], linear, at);
            let (a, b) = (m.object(side).unwrap(), m.object(shelf).unwrap());
            let what = format!("{excess} mm too thick, {pose}");
            let common = 6.0 * 300.0 * excess;
            close(volume(a, b, BoolOp::Intersection, &what), common, &what);
            close(volume(b, a, BoolOp::Intersection, &what), common, &what);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(64),
        ..ProptestConfig::default()
    })]

    /// Two boards, one lying on the other and slid by a random amount (often none, so side
    /// faces line up too), both turned about a random axis and moved up to 1 km: nothing in
    /// common, whichever is first. (Their difference is not held here: turned, it can still
    /// be refused, a weld leaving a zero-length edge.)
    #[test]
    fn two_boards_touching_turned_and_moved(
        dx in prop_oneof![Just(0.0), -200.0f64..200.0],
        dy in prop_oneof![Just(0.0), -100.0f64..100.0],
        ax in -1.0f64..1.0, ay in -1.0f64..1.0, az in -1.0f64..1.0,
        angle in 0.0f64..std::f64::consts::PI,
        far in prop_oneof![Just(0.0), 1.0f64..1000.0],
        dir in 0usize..3,
    ) {
        prop_assume!(ax * ax + ay * ay + az * az > 0.01);
        let mut m = Model::new();
        let lower = board(&mut m, [0.0; 3], 600.0, 300.0, 19.0);
        let upper = board(&mut m, [dx, dy, 19.0], 600.0, 300.0, 19.0);
        let mut at = [0.0; 3];
        at[dir] = far;
        place(&mut m, &[lower, upper], rotation([ax, ay, az], angle), at);
        let (a, b) = (m.object(lower).unwrap(), m.object(upper).unwrap());
        for (first, second) in [(a, b), (b, a)] {
            match first.combine(second, BoolOp::Intersection) {
                Ok(None) => {}
                other => prop_assert!(false, "expected empty: {:?}", other.map(|o| o.map(|o| o.volume()))),
            }
        }
    }
}

// ---- what still refuses (#124), with what it should answer -------------------------------

/// Step 6 of #124: boards exactly the merge distance (0.01 mm) into each other. Square to
/// the axes the overlap is empty (its faces are the same point); turned, the engine's
/// result is welded where its corners round to the merge distance and not where they round
/// past it, and the result is refused ("left a triangle off every neighbouring source
/// face", a vertex on a face, zero-length edges). It should be empty, as square, or the
/// 0.01 mm slab (1,800 mm3), the same both ways. A fix needs a decision on what an overlap
/// of exactly the merge distance is, made once for the whole pair rather than vertex by
/// vertex.
#[test]
#[ignore = "#124: an overlap of exactly the merge distance, turned, is refused"]
fn boards_exactly_the_merge_distance_into_each_other_turned() {
    for (pose, linear) in [
        ("0.3 rad about x", rotation([1.0, 0.0, 0.0], 0.3)),
        (
            "45 degrees about x",
            rotation([1.0, 0.0, 0.0], std::f64::consts::FRAC_PI_4),
        ),
        ("0.83 rad about the skew axis", ISSUE_TURN),
    ] {
        let mut m = Model::new();
        let lower = board(&mut m, [0.0; 3], 600.0, 300.0, 19.0);
        let upper = board(&mut m, [0.0, 0.0, 18.99], 600.0, 300.0, 19.0);
        place(&mut m, &[lower, upper], linear, [0.0; 3]);
        let (a, b) = (m.object(lower).unwrap(), m.object(upper).unwrap());
        let ab = volume(a, b, BoolOp::Intersection, pose);
        let ba = volume(b, a, BoolOp::Intersection, pose);
        for v in [ab, ba] {
            assert!(
                v.is_none_or(|v| (v - 1800.0).abs() <= 1800.0 * 1e-5),
                "{pose}: {v:?}"
            );
        }
        assert_eq!(ab.is_some(), ba.is_some(), "{pose}");
    }
}

/// Step 8 of #124 with the shelf 0.5 mm into the floor, turned 0.83 rad about the skew
/// axis and moved by (40, -25, 12) m (moved by (40, -30, 20) m it answers): the engine's
/// result is right (2,850 mm3, closed), but the weld that closes slivers
/// (`snap::weld_output`, every vertex pair within the merge distance) joins a corner of the
/// dado's floor with a point the engine made about 2.6 um from it on the same edge and keeps
/// the made point, so a face comes out 1.4 um off its plane (refused as not planar one way,
/// an edge through a face the other). It should be 2,850 mm3 both ways.
/// A fix needs the weld to keep the corner of a cluster that lies on its source planes,
/// which changes which point a weld keeps for every boolean, so it is left for a lane that
/// can show what that does to every result that welds today.
#[test]
#[ignore = "#124: the output weld moves a corner of the dado's floor"]
fn a_half_millimetre_into_the_floor_turned_and_40_m_out() {
    let mut m = Model::new();
    let side = prism(&mut m, &SIDE8, 300.0);
    let shelf = board(&mut m, [12.5, 0.0, 300.0], 462.0, 300.0, 19.0);
    place(&mut m, &[side, shelf], ISSUE_TURN, [40.0, -25.0, 12.0]);
    let (a, b) = (m.object(side).unwrap(), m.object(shelf).unwrap());
    let tol = 19.0 * 300.0 * 1e-3;
    near(
        volume(a, b, BoolOp::Intersection, "side first"),
        2850.0,
        tol,
        "side first",
    );
    near(
        volume(b, a, BoolOp::Intersection, "shelf first"),
        2850.0,
        tol,
        "shelf first",
    );
}

// ---- fixed pairs that tell the step's parts apart (lane 81 fix pass) ---------------------

/// A profile in x and z (mm), by its corners.
enum Profile {
    Poly(&'static [(f64, f64)]),
}

impl Profile {
    fn corners(&self) -> Vec<(f64, f64)> {
        let Profile::Poly(p) = self;
        p.to_vec()
    }
}
use Profile::Poly;

/// A prism of `profile` from y = `y[0]` to `y[1]` (mm).
fn prism_at(m: &mut Model, profile: &Profile, y: [f64; 2]) -> u64 {
    let points: Vec<[f64; 3]> = profile
        .corners()
        .iter()
        .map(|&(x, z)| [x / 1000.0, y[0] / 1000.0, z / 1000.0])
        .collect();
    solid(
        m,
        [0.0, 1.0, 0.0],
        [0.0, y[0] / 1000.0, 0.0],
        points,
        (y[1] - y[0]) / 1000.0,
        1,
    )
}

/// One of twelve placements: square, 0.3 rad about x, 0.83 and 2.1 rad about the skew
/// axis, each in place, moved (40, -30, 20) m and moved (1000, 300, -50) m.
fn placement(i: u64) -> ([[f64; 3]; 3], [f64; 3]) {
    let turns = [
        ([1.0, 0.0, 0.0], 0.0),
        ([1.0, 0.0, 0.0], 0.3),
        (SKEW, 0.83),
        (SKEW, 2.1),
    ];
    let moves = [[0.0; 3], [40.0, -30.0, 20.0], [1000.0, 300.0, -50.0]];
    let (axis, angle) = turns[(i % 4) as usize];
    (rotation(axis, angle), moves[((i / 4) % 3) as usize])
}

/// (seed, a, a's y, b, b's y, placement, operation: I, D or U, `ab` with a first).
type Pair = (u64, Profile, [f64; 2], Profile, [f64; 2], u64, &'static str);

/// Pairs from a seeded sweep of boxes, L and U profiles and polygons (the seed is the
/// sweep's case number), each chosen because it tells one part of the cancelling of sheets
/// apart: each part, taken out or changed, changes at least one row's answer (the comment
/// above each row says which). Four rows are refused today although taking out a guard
/// makes them answer with the right volume (worked out as a less what both hold, or a plus b
/// less it): the guard costs those answers. A change that makes them answer must give that
/// volume. Same-facing faces of one plane overlapping are not cancelled: a boolean of two
/// valid solids cannot leave two sheets facing one way over the same area (the region
/// between them would be inside the result twice), and in the 24,000-pair sweep these rows
/// come from, cancelling them too changed nothing.
const PAIRS: &[Pair] = &[
    // healing (F15): healed, 16 vertices; unhealed, 17
    (
        4,
        Poly(&[
            (10.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (30.0, 20.0),
            (30.0, 40.0),
            (10.0, 40.0),
        ]),
        [20.0, 50.0],
        Poly(&[(20.0, 0.0), (50.0, 0.0), (50.0, 10.0), (20.0, 10.0)]),
        [20.0, 50.0],
        1,
        "D_ab",
    ),
    // corners put back (F14): refused today; without them 16,000 mm3, the right volume
    (
        57,
        Poly(&[
            (10.0, 10.0),
            (50.0, 10.0),
            (50.0, 40.0),
            (40.0, 40.0),
            (40.0, 20.0),
            (30.0, 20.0),
            (30.0, 40.0),
            (10.0, 40.0),
        ]),
        [10.0, 30.0],
        Poly(&[
            (0.0, 0.0),
            (50.0, 0.0),
            (50.0, 30.0),
            (30.0, 30.0),
            (30.0, 20.0),
            (20.0, 20.0),
            (20.0, 30.0),
            (0.0, 30.0),
        ]),
        [10.0, 30.0],
        3,
        "D_ba",
    ),
    // grouping tolerance (F13): empty; grouped ten times tighter, refused
    (
        191,
        Poly(&[
            (10.0, 30.0),
            (50.0, 30.0),
            (50.0, 40.0),
            (30.0, 40.0),
            (30.0, 39.98),
            (19.99, 39.98),
            (19.99, 40.0),
            (10.0, 40.0),
        ]),
        [10.005, 20.01],
        Poly(&[(9.98, 10.0), (40.0, 10.0), (40.0, 50.0), (9.98, 50.0)]),
        [20.0, 30.0],
        2,
        "I_ba",
    ),
    // after the snapping (F10), once (F11), the grouping tolerance (F13), the plane a piece keeps (F17): the result before it is built
    (
        301,
        Poly(&[
            (0.0, 20.0),
            (40.0, 20.0),
            (40.0, 50.0),
            (30.0, 50.0),
            (30.0, 40.0),
            (19.98, 40.0),
            (19.98, 50.0),
            (0.0, 50.0),
        ]),
        [9.99, 40.0],
        Poly(&[
            (0.01, 10.0),
            (49.9, 10.0),
            (49.9, 40.0),
            (10.0, 40.0),
            (10.0, 50.0),
            (0.01, 50.0),
        ]),
        [10.0, 20.0],
        8,
        "D_ab",
    ),
    // corners put back (F14) and healing (F15): refused today; without the corners 44,000 mm3, the right volume
    (
        698,
        Poly(&[(0.0, 20.0), (20.0, 20.0), (20.0, 30.0), (0.0, 30.0)]),
        [30.0, 40.0],
        Poly(&[
            (0.0, 10.0),
            (40.0, 10.0),
            (40.0, 50.0),
            (20.0, 50.0),
            (20.0, 30.0),
            (10.0, 30.0),
            (10.0, 50.0),
            (0.0, 50.0),
        ]),
        [0.0, 30.0],
        2,
        "U_ab",
    ),
    // the area floor (F16): 75,000 mm3; with slivers kept, refused; and healing (F15)
    (
        1989,
        Poly(&[
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 20.0),
            (20.0, 20.0),
            (20.0, 10.0),
            (10.0, 10.0),
            (10.0, 20.0),
            (0.0, 20.0),
        ]),
        [0.0, 40.0],
        Poly(&[
            (0.0, 20.0),
            (50.0, 20.0),
            (50.0, 40.0),
            (10.0, 40.0),
            (10.0, 50.0),
            (0.0, 50.0),
        ]),
        [0.0, 50.0],
        2,
        "U_ab",
    ),
    // after the snapping (F10), once (F11), the grouping tolerance (F13), the plane a piece keeps (F17): the result before it is built
    (
        8002,
        Poly(&[
            (0.0, 30.005),
            (50.0, 30.005),
            (50.0, 40.0),
            (30.0, 40.0),
            (30.0, 30.1),
            (10.0, 30.1),
            (10.0, 40.0),
            (0.0, 40.0),
        ]),
        [0.005, 20.1],
        Poly(&[
            (30.011, 30.0),
            (40.0, 30.0),
            (40.0, 50.011),
            (30.011, 50.011),
        ]),
        [0.0, 20.0],
        2,
        "D_ba",
    ),
    // the corner check (F04): refused today; without it 18,000 mm3, the right volume
    (
        8620,
        Poly(&[
            (0.0, 10.0),
            (50.0, 10.0),
            (50.0, 50.0),
            (40.0, 50.0),
            (40.0, 20.0),
            (30.0, 20.0),
            (30.0, 50.0),
            (0.0, 50.0),
        ]),
        [10.0, 40.0],
        Poly(&[
            (0.0, 0.0),
            (50.0, 0.0),
            (50.0, 30.0),
            (30.0, 30.0),
            (30.0, 50.0),
            (0.0, 50.0),
        ]),
        [10.0, 40.0],
        6,
        "D_ba",
    ),
    // after the snapping (F10): 19,000 mm3; run before it, refused
    (
        9314,
        Poly(&[
            (10.0, 20.0),
            (40.0, 20.0),
            (40.0, 30.0),
            (20.0, 30.0),
            (20.0, 40.0),
            (10.0, 40.0),
        ]),
        [20.0, 30.0],
        Poly(&[
            (20.0, 30.0),
            (40.0, 30.0),
            (40.0, 40.0),
            (30.0, 40.0),
            (30.0, 50.0),
            (20.0, 50.0),
        ]),
        [0.0, 50.0],
        3,
        "U_ab",
    ),
    // the corner check (F04): refused today; without it 19,000 mm3, the right volume
    (
        9314,
        Poly(&[
            (10.0, 20.0),
            (40.0, 20.0),
            (40.0, 30.0),
            (20.0, 30.0),
            (20.0, 40.0),
            (10.0, 40.0),
        ]),
        [20.0, 30.0],
        Poly(&[
            (20.0, 30.0),
            (40.0, 30.0),
            (40.0, 40.0),
            (30.0, 40.0),
            (30.0, 50.0),
            (20.0, 50.0),
        ]),
        [0.0, 50.0],
        3,
        "U_ba",
    ),
    // after the snapping (F10): 7,069.189456 mm3; run before it, 7,069.164131
    (
        19777,
        Poly(&[
            (0.0, 9.9),
            (50.0, 9.9),
            (50.0, 50.01),
            (49.98, 50.01),
            (49.98, 49.9),
            (40.0, 49.9),
            (40.0, 50.01),
            (0.0, 50.01),
        ]),
        [10.1, 49.995],
        Poly(&[
            (0.0, 10.1),
            (20.011, 10.1),
            (20.011, 40.0),
            (10.01, 40.0),
            (10.01, 50.011),
            (0.0, 50.011),
        ]),
        [-0.02, 20.0],
        2,
        "D_ba",
    ),
];

/// What a pair gives: the solid (volume in mm3, faces, vertices), empty or refused; and the
/// boolean's own result before it is built (faces, points, faces without a stored plane, and
/// whether sheets were cancelled from it).
fn answer(p: &Pair) -> String {
    let (_, a, ya, b, yb, at, op) = p;
    let mut m = Model::new();
    let (ia, ib) = (prism_at(&mut m, a, *ya), prism_at(&mut m, b, *yb));
    let (linear, by) = placement(*at);
    place(&mut m, &[ia, ib], linear, by);
    let (oa, ob) = (m.object(ia).unwrap(), m.object(ib).unwrap());
    let (x, y) = if op.ends_with("ab") {
        (oa, ob)
    } else {
        (ob, oa)
    };
    let bop = match &op[..1] {
        "I" => BoolOp::Intersection,
        "D" => BoolOp::Difference,
        _ => BoolOp::Union,
    };
    let solid = match x.combine(y, bop) {
        Ok(Some(o)) => format!(
            "{:.6} mm3 {}f {}v",
            o.volume() * 1e9,
            o.brep().face_count(),
            o.brep().vertex_count()
        ),
        Ok(None) => "empty".into(),
        Err(_) => "refused".into(),
    };
    let engine = sawhorse_kernel::boolean::default_engine().unwrap();
    let parts = sawhorse_kernel::boolean::combine_parts(
        &x.brep().to_polys(),
        &y.brep().to_polys(),
        bop,
        engine.as_ref(),
    );
    let raw = match parts {
        Ok(c) => match &c.result {
            Some(r) => format!(
                "{}f {}p {}n{}",
                r.faces.len(),
                r.points.len(),
                r.faces.iter().filter(|f| f.plane.is_none()).count(),
                if c.before_fins.is_some() {
                    " cancelled"
                } else {
                    ""
                }
            ),
            None => "empty".into(),
        },
        Err(_) => "refused".into(),
    };
    format!("{solid} | {raw}")
}

#[test]
fn pairs_that_tell_the_steps_parts_apart() {
    let got: Vec<String> = PAIRS.iter().map(answer).collect();
    if std::env::var("L81_PRINT").is_ok() {
        for g in &got {
            println!("PIN \"{g}\",");
        }
        return;
    }
    assert_eq!(got.len(), PINNED.len());
    let mut wrong = Vec::new();
    for (k, (p, (g, w))) in PAIRS.iter().zip(got.iter().zip(PINNED)).enumerate() {
        if g != w {
            wrong.push(format!("row {k} (seed {} {}): {g}, pinned {w}", p.0, p.6));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// What each row of `PAIRS` gives.
const PINNED: &[&str] = &[
    "24000.000000 mm3 10f 16v | 10f 16p 0n cancelled",
    "refused | 14f 22p 0n cancelled",
    "empty | empty",
    "refused | 18f 32p 5n cancelled",
    "refused | 14f 23p 0n cancelled",
    "75000.000000 mm3 15f 27v | 15f 27p 0n cancelled",
    "refused | 9f 16p 3n cancelled",
    "refused | 16f 34p 0n",
    "19000.000000 mm3 15f 24v | 15f 24p 0n cancelled",
    "refused | 17f 28p 0n",
    "7069.189456 mm3 8f 12v | 8f 12p 0n cancelled",
];

/// A boolean the cancelling of sheets cannot save is refused with the words it had before the
/// step (lane 81 fix pass): the step's own result often fails a later, structural check
/// where the result without it failed a geometric one. Square to the axes, a U profile x
/// 10.011 to 50 mm (its notch x 19.9 to 30), y -0.01 to 40, less a 10.1 x 9.98 mm box at
/// x 40 to 50.1, z 40 to 49.98, y 0 to 10: before this fix the refusal read a face 6.5 um
/// off its plane, and without the step it reads a vertex on a face. And an L profile less a
/// box, moved (40, -30, 20) m: the step's result is not watertight, and without the step a
/// vertex on a face.
#[test]
fn a_refusal_keeps_the_words_it_had_before_the_step() {
    const REFUSED: &[Pair] = &[
        (
            2680,
            Poly(&[(40.0, 40.0), (50.1, 40.0), (50.1, 49.98), (40.0, 49.98)]),
            [0.0, 10.0],
            Poly(&[
                (10.011, 0.0),
                (50.0, 0.0),
                (50.0, 50.0),
                (30.0, 50.0),
                (30.0, 20.0),
                (19.9, 20.0),
                (19.9, 50.0),
                (10.011, 50.0),
            ]),
            [-0.01, 40.0],
            0,
            "D_ba",
        ),
        (
            656,
            Poly(&[
                (20.0, 0.0),
                (50.005, 0.0),
                (50.005, 40.011),
                (30.005, 40.011),
                (30.005, 50.0),
                (20.0, 50.0),
            ]),
            [10.1, 19.9],
            Poly(&[(20.0, 10.0), (50.0, 10.0), (50.0, 20.0), (20.0, 20.0)]),
            [10.0, 40.0],
            4,
            "D_ab",
        ),
    ];
    for p in REFUSED {
        let (_, a, ya, b, yb, at, op) = p;
        let mut m = Model::new();
        let (ia, ib) = (prism_at(&mut m, a, *ya), prism_at(&mut m, b, *yb));
        let (linear, by) = placement(*at);
        place(&mut m, &[ia, ib], linear, by);
        let (oa, ob) = (m.object(ia).unwrap(), m.object(ib).unwrap());
        let (x, y) = if op.ends_with("ab") {
            (oa, ob)
        } else {
            (ob, oa)
        };
        let with = x
            .combine(y, BoolOp::Difference)
            .map(|o| o.map(|o| o.volume()));
        sawhorse_kernel::boolean::set_fin_cancelling(false);
        let without = x
            .combine(y, BoolOp::Difference)
            .map(|o| o.map(|o| o.volume()));
        sawhorse_kernel::boolean::set_fin_cancelling(true);
        // Sheets are cancelled here (the step runs), and what it gives is refused.
        let engine = sawhorse_kernel::boolean::default_engine().unwrap();
        let parts = sawhorse_kernel::boolean::combine_parts(
            &x.brep().to_polys(),
            &y.brep().to_polys(),
            BoolOp::Difference,
            engine.as_ref(),
        )
        .unwrap();
        assert!(parts.before_fins.is_some(), "seed {}: the step runs", p.0);
        let e = with.as_ref().expect_err("refused");
        assert!(e.to_string().contains("VertexOnFace"), "seed {}: {e}", p.0);
        assert_eq!(with, without, "seed {}", p.0);
    }
}
