//! Timing of a cursor's inference and pick on a large model (lane 23): the hot path a
//! pointer move runs. Ignored by default; run it with
//! `scripts/cargo.sh test --release -p sawhorse-inference --test perf -- --ignored --nocapture`.
//!
//! Two models of about 9,600 faces each: 1,600 boxes (9,600 faces), and 368 cylinders of 24
//! segments (9,568 faces) drawn as circles (curves since lane 23). Each is looked at
//! straight down, the whole array about 1,000 pixels across, and 2,500 rays on a 50 x 50
//! grid are inferred and picked in turn; the median and 95th percentile per query are
//! printed, best of eleven passes (the scene is built once, as the app caches it between
//! edits).

use std::time::Instant;

use glam::DVec3;
use sawhorse_inference::geom::PixelModel;
use sawhorse_inference::infer::{InferQuery, infer};
use sawhorse_inference::pick::{PickQuery, pick};
use sawhorse_inference::scene::Scene;
use sawhorse_kernel::model::{Model, Op};
use sawhorse_kernel::transform::Xform;

fn apply(m: &mut Model, op: Op) -> serde_json::Value {
    let out = m.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    serde_json::to_value(out).unwrap()
}

/// `nx` x `ny` copies of the model's one solid, `step` metres apart.
fn array(m: &mut Model, nx: u32, ny: u32, step: f64) {
    let first = m.object_ids();
    apply(
        m,
        Op::Array {
            objects: first,
            xform: Xform::translation(DVec3::new(step, 0.0, 0.0)),
            count: nx - 1,
            nodes: vec![],
        },
    );
    let row = m.object_ids();
    apply(
        m,
        Op::Array {
            objects: row,
            xform: Xform::translation(DVec3::new(0.0, step, 0.0)),
            count: ny - 1,
            nodes: vec![],
        },
    );
}

fn boxes() -> Model {
    let mut m = Model::with_ground_sketch();
    apply(
        &mut m,
        Op::AddRectangle {
            sketch: 1,
            x: 0.0,
            y: 0.0,
            w: 0.3,
            h: 0.3,
        },
    );
    apply(
        &mut m,
        Op::Extrude {
            sketch: 1,
            region: 1,
            distance: 0.3,
        },
    );
    array(&mut m, 40, 40, 0.5);
    m
}

fn cylinders() -> Model {
    let mut m = Model::with_ground_sketch();
    let pts: Vec<[f64; 3]> = (0..24)
        .map(|k| {
            let a = std::f64::consts::TAU * f64::from(k) / 24.0;
            [0.15 + 0.15 * a.cos(), 0.15 + 0.15 * a.sin(), 0.0]
        })
        .collect();
    let drawn = apply(&mut m, perf_circle(pts));
    let region = drawn["regions"][0].as_u64().expect("a region");
    apply(
        &mut m,
        Op::Extrude {
            sketch: 1,
            region,
            distance: 0.3,
        },
    );
    array(&mut m, 16, 23, 0.5);
    // The walls soft and smooth on both kernels (lane 23 pulls a circle into a cylinder
    // with soft walls already; before it they were hard): the same model either way.
    let all = m.object_ids();
    let _ = m.apply(&Op::SoftenEdges {
        objects: all,
        meshes: vec![],
        angle: 20.0,
        soft: true,
        smooth: true,
    });
    m
}

/// The Circle tool's op: a closed stroke into the ground sketch (as a circle when the
/// kernel has curves).
fn perf_circle(points: Vec<[f64; 3]>) -> Op {
    serde_json::from_value(serde_json::json!({
        "op": "draw",
        "target": { "on": "sketch", "sketch": 1 },
        "points": points,
        "closed": true,
        "curve": { "kind": "circle", "centre": [0.15, 0.15, 0.0] },
    }))
    .or_else(|_| {
        serde_json::from_value(serde_json::json!({
            "op": "draw",
            "target": { "on": "sketch", "sketch": 1 },
            "points": points,
            "closed": true,
        }))
    })
    .expect("a draw op")
}

fn time(name: &str, m: &Model) {
    let scene = Scene::build(m);
    let faces: usize = m
        .objects()
        .iter()
        .map(|(_, o)| o.brep().faces().count())
        .sum();
    let (lo, hi) = m.objects().iter().map(|(_, o)| o.bounds()).fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(a, b), (c, d)| (a.min(c), b.max(d)),
    );
    let span = (hi - lo).max_element();
    let mpp = span / 1000.0;
    let mut best_infer: Option<(f64, f64)> = None;
    let mut best_pick: Option<(f64, f64)> = None;
    for _ in 0..11 {
        let mut ti = Vec::with_capacity(2500);
        let mut tp = Vec::with_capacity(2500);
        for i in 0..50 {
            for j in 0..50 {
                let x = lo.x + span * (f64::from(i) + 0.37) / 50.0;
                let y = lo.y + span * (f64::from(j) + 0.61) / 50.0;
                let px = PixelModel {
                    ortho: true,
                    scale: mpp,
                    eye: [x, y, 10.0],
                    forward: [0.0, 0.0, -1.0],
                };
                let q = InferQuery {
                    origin: [x, y, 10.0],
                    dir: [0.0, 0.0, -1.0],
                    px,
                    start: None,
                    plane: None,
                    lock: None,
                    refs: vec![],
                    anchors: vec![],
                    grid: true,
                    grid_step: Some([0.0254, 0.3048]),
                    point_px: 10.0,
                    line_px: 7.0,
                    soft: false,
                };
                let t = Instant::now();
                std::hint::black_box(infer(&scene, &q));
                ti.push(t.elapsed().as_secs_f64() * 1e6);
                let p = PickQuery {
                    origin: q.origin,
                    dir: q.dir,
                    px,
                    point_px: 8.0,
                    edge_px: 5.0,
                    soft: false,
                };
                let t = Instant::now();
                std::hint::black_box(pick(&scene, &p));
                tp.push(t.elapsed().as_secs_f64() * 1e6);
            }
        }
        let stats = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            (v[v.len() / 2], v[v.len() * 95 / 100])
        };
        let (a, b) = (stats(&mut ti), stats(&mut tp));
        if best_infer.is_none_or(|x| a.0 < x.0) {
            best_infer = Some(a);
        }
        if best_pick.is_none_or(|x| b.0 < x.0) {
            best_pick = Some(b);
        }
    }
    let (im, ip) = best_infer.unwrap();
    let (pm, pp) = best_pick.unwrap();
    println!(
        "[perf] {name}: {faces} faces, {} segs, {} arcs; infer median {im:.2} us, p95 {ip:.2} us; pick median {pm:.3} us, p95 {pp:.3} us",
        scene.segs.len(),
        scene.arcs.len()
    );
}

#[test]
#[ignore]
fn inference_hot_path_on_9600_faces() {
    time("1,600 boxes", &boxes());
    time("368 cylinders", &cylinders());
}
