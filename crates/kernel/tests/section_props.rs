//! Section cuts of folded solids with plane snapping off (lane 12c).
//!
//! The lane-12 re-verifier's long fuzz with `FUZZ_PLANE_SNAP=0` found three solids whose
//! kept halves did not add up to the whole (seeds 20261803, 20261904, 20261907; the shrunk
//! sequences are `tests/repro/2026-10-12-section-*-pinch*.json`). Each was a box with part
//! of it moved and folded (Move with autofold), cut by a plane through a vertex where the
//! surface crosses the plane twice: the cut's pieces meet four at a time there, and the cap
//! chain could run one backwards. These properties cut such solids, at planes through
//! their own vertices and along their own faces as well as at random, and check:
//!
//! - the two halves a plane and its flip keep make the whole (the fuzzer's invariant,
//!   `testing::check_sections`, which also checks that each half is a closed surface);
//! - every cap ring is simple: no node twice, outer rings counter-clockwise, holes clockwise;
//! - Create Group from Slice draws closed loops: the sketch's faces cover exactly the caps'
//!   outer rings.

use glam::{DVec2, DVec3};
use proptest::prelude::*;
use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::testing::{Repro, check_sections};
use sawhorse_kernel::transform::Xform;
use sawhorse_kernel::{Document, snap};

/// Plane snapping off for the closure, put back afterwards (it is per thread).
fn snapping_off<T>(f: impl FnOnce() -> T) -> T {
    let was = snap::plane_snapping();
    snap::set_plane_snapping(false);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    snap::set_plane_snapping(was);
    r.unwrap_or_else(|p| std::panic::resume_unwind(p))
}

#[derive(Debug, Clone)]
struct Fold {
    corners: Vec<bool>,
    axis: [f64; 3],
    angle: f64,
    shift: [f64; 3],
}

#[derive(Debug, Clone)]
enum At {
    /// Through one of the solid's own vertices (picked modulo the count).
    Vertex(usize),
    /// At fractions of the solid's bounding box.
    Inside([f64; 3]),
}

fn quarters() -> impl Strategy<Value = f64> {
    prop_oneof![3 => (1i32..8).prop_map(|k| f64::from(k) * 0.125), 1 => 0.05..1.0f64]
}

/// Which corners move: any set, or (as in the three finds) a face's four, an edge's two or
/// a single corner. Corner `k` is at `(k & 1, k & 2, k & 4)` times the size.
fn corners() -> impl Strategy<Value = Vec<bool>> {
    let set = |ks: Vec<usize>| (0..8).map(|k| ks.contains(&k)).collect::<Vec<bool>>();
    let faces = vec![
        vec![0, 1, 2, 3],
        vec![4, 5, 6, 7],
        vec![0, 1, 4, 5],
        vec![2, 3, 6, 7],
        vec![0, 2, 4, 6],
        vec![1, 3, 5, 7],
    ];
    let edges: Vec<Vec<usize>> = (0..8)
        .flat_map(|a| [1, 2, 4].into_iter().map(move |b| (a, a ^ b)))
        .filter(|(a, b)| a < b)
        .map(|(a, b)| vec![a, b])
        .collect();
    prop_oneof![
        2 => prop::sample::select(faces).prop_map(set),
        2 => prop::sample::select(edges).prop_map(set),
        1 => (0usize..8).prop_map(move |k| set(vec![k])),
        1 => prop::collection::vec(any::<bool>(), 8),
    ]
}

fn fold() -> impl Strategy<Value = Fold> {
    (
        corners(),
        prop::array::uniform3(-1.0..1.0f64),
        prop_oneof![1 => Just(std::f64::consts::FRAC_PI_2), 3 => -1.2..1.2f64, 1 => -2.5..2.5f64],
        prop::array::uniform3(
            prop_oneof![2 => (-4i32..4).prop_map(|k| f64::from(k) * 0.125), 1 => -0.6..0.6f64],
        ),
    )
        .prop_map(|(corners, axis, angle, shift)| Fold {
            corners,
            axis,
            angle,
            shift,
        })
}

fn normal() -> impl Strategy<Value = [f64; 3]> {
    prop_oneof![
        1 => Just([1.0, 0.0, 0.0]),
        1 => Just([-1.0, 0.0, 0.0]),
        1 => Just([0.0, 1.0, 0.0]),
        1 => Just([0.0, -1.0, 0.0]),
        1 => Just([0.0, 0.0, 1.0]),
        1 => Just([0.0, 0.0, -1.0]),
        3 => prop::array::uniform3(-1.0..1.0f64),
    ]
}

fn at() -> impl Strategy<Value = At> {
    prop_oneof![
        2 => (0usize..64).prop_map(At::Vertex),
        1 => prop::array::uniform3(0.0..1.0f64).prop_map(At::Inside),
    ]
}

/// A box (`w`, `d`, `h`) with a set of its corners moved and folded (Move with autofold);
/// a fold the kernel refuses leaves the plain box, which is tested all the same.
fn folded_box(size: [f64; 3], fold: &Fold) -> (Document, u64) {
    let mut d = Document::new();
    d.apply(&Op::NewSketch {
        origin: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
    })
    .unwrap();
    d.apply(&Op::AddRectangle {
        sketch: 1,
        x: 0.0,
        y: 0.0,
        w: size[0],
        h: size[1],
    })
    .unwrap();
    let object = match d
        .apply(&Op::Extrude {
            sketch: 1,
            region: 1,
            distance: size[2],
        })
        .unwrap()
    {
        OpOutcome::ObjectCreated { object } => object,
        o => panic!("{o:?}"),
    };
    let corners: Vec<[f64; 3]> = (0..8)
        .filter(|&k| fold.corners[k])
        .map(|k| {
            [
                if k & 1 == 0 { 0.0 } else { size[0] },
                if k & 2 == 0 { 0.0 } else { size[1] },
                if k & 4 == 0 { 0.0 } else { size[2] },
            ]
        })
        .collect();
    if !corners.is_empty() && corners.len() < 8 {
        let axis = DVec3::from_array(fold.axis);
        let axis = if axis.length() < 1e-3 { DVec3::Z } else { axis };
        let centre = DVec3::from_array(size) * 0.5;
        let x = Xform::translation(DVec3::from_array(fold.shift))
            .after(&Xform::rotation(centre, axis, fold.angle));
        // Refused folds (a self-intersecting result) leave the box as it was.
        let _ = d.apply(&Op::TransformGeometry {
            object,
            points: corners,
            xform: x,
            autofold: true,
        });
    }
    (d, object)
}

fn plane_origin(m: &Model, object: u64, at: &At) -> [f64; 3] {
    let b = m.object(object).unwrap().brep();
    let vs: Vec<DVec3> = b.vertices().map(|(_, v)| v.pos).collect();
    match at {
        At::Vertex(k) => vs[k % vs.len()].to_array(),
        At::Inside(f) => {
            let lo = vs
                .iter()
                .fold(DVec3::splat(f64::INFINITY), |a, v| a.min(*v));
            let hi = vs
                .iter()
                .fold(DVec3::splat(f64::NEG_INFINITY), |a, v| a.max(*v));
            (lo + (hi - lo) * DVec3::from_array(*f)).to_array()
        }
    }
}

/// Every cap ring is simple (no point twice), outer rings counter-clockwise, holes clockwise.
fn caps_are_simple(m: &Model, section: u64) -> Result<(), String> {
    let cut = m.section_cut(section).map_err(|e| e.to_string())?;
    for c in &cut.caps {
        for (k, r) in std::iter::once(&c.outer).chain(&c.holes).enumerate() {
            for i in 0..r.len() {
                for j in i + 1..r.len() {
                    if r[i] == r[j] {
                        return Err(format!("a cap ring passes {:?} twice: {r:?}", r[i]));
                    }
                }
            }
            let ring: Vec<DVec2> = r.iter().map(|p| DVec2::from_array(*p)).collect();
            let a = sawhorse_tessellate::signed_area(&ring);
            if (k == 0) != (a > 0.0) {
                return Err(format!("cap ring {k} has area {a}"));
            }
        }
    }
    Ok(())
}

/// Create Group from Slice on `section` (when it cuts anything) draws closed loops: its
/// sketch's faces, holes included as faces of their own, cover the caps' outer rings.
fn slice_is_closed(d: &Document, section: u64) -> Result<(), String> {
    let cut = d.model().section_cut(section).map_err(|e| e.to_string())?;
    if cut.caps.is_empty() {
        return Ok(());
    }
    let mut d = d.clone();
    // Slicing is for the model's own context; a plane inside a component is refused there.
    let sketch = match d.apply(&Op::SliceToGroup {
        section,
        name: None,
    }) {
        Ok(OpOutcome::Sliced { sketch, .. }) => sketch,
        Ok(o) => return Err(format!("slice: {o:?}")),
        Err(e) if e.to_string().contains("inside a component") => return Ok(()),
        Err(e) => return Err(format!("slice refused: {e}")),
    };
    let sk = d.model().sketch(sketch).ok_or("no sketch")?;
    let faces: f64 = sk.regions().map(|(_, r)| sk.region_area(r)).sum();
    let outers: f64 = cut
        .caps
        .iter()
        .map(|c| {
            let r: Vec<DVec2> = c.outer.iter().map(|p| DVec2::from_array(*p)).collect();
            sawhorse_tessellate::signed_area(&r)
        })
        .sum();
    // The sketch merges points within POINT_MERGE, so a sliver of a cap may close up.
    let perimeter: f64 = cut
        .outline
        .iter()
        .map(|[a, b]| DVec2::from_array(*a).distance(DVec2::from_array(*b)))
        .sum();
    let slack = 1e-9 * (1.0 + outers) + 2.0 * sawhorse_kernel::tolerance::POINT_MERGE * perimeter;
    if (faces - outers).abs() > slack {
        return Err(format!(
            "the slice's faces cover {faces} m2, the caps' outer rings {outers} m2"
        ));
    }
    Ok(())
}

proptest! {
    // PROPTEST_CASES raises it for a long run.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512),
        ..ProptestConfig::default()
    })]

    #[test]
    fn folded_solids_cut_into_halves_that_make_the_whole(
        size in prop::array::uniform3(quarters()),
        fold in fold(),
        at in at(),
        normal in normal(),
    ) {
        let n = DVec3::from_array(normal);
        prop_assume!(n.length() > 0.05);
        snapping_off(|| -> Result<(), TestCaseError> {
            let (mut d, object) = folded_box(size, &fold);
            let origin = plane_origin(d.model(), object, &at);
            let section = match d.apply(&Op::AddSection {
                origin,
                normal,
                name: None,
                active: true,
                hidden: false,
            }) {
                Ok(OpOutcome::SectionAdded { section }) => section,
                o => panic!("{o:?}"),
            };
            check_sections(d.model()).map_err(TestCaseError::fail)?;
            caps_are_simple(d.model(), section).map_err(TestCaseError::fail)?;
            slice_is_closed(&d, section).map_err(TestCaseError::fail)?;
            d.apply(&Op::FlipSection { section }).unwrap();
            caps_are_simple(d.model(), section).map_err(TestCaseError::fail)?;
            slice_is_closed(&d, section).map_err(TestCaseError::fail)?;
            Ok(())
        })?;
    }
}

/// The three counterexamples, played to their end: every plane's caps are simple, its
/// halves make the whole, and its slice closes.
#[test]
fn the_lane_12_verifier_finds_cut_cleanly() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repro");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.contains("-pinch") {
            continue;
        }
        let r: Repro = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(r.plane_snap, Some(false), "{name}");
        snapping_off(|| {
            let mut d = Document::new();
            for op in &r.ops {
                let _ = d.apply(op);
            }
            let m = d.model();
            check_sections(m).unwrap_or_else(|e| panic!("{name}: {e}"));
            let ids: Vec<u64> = m.sections().into_iter().map(|(id, _)| id).collect();
            assert!(!ids.is_empty(), "{name}");
            for id in ids {
                caps_are_simple(m, id).unwrap_or_else(|e| panic!("{name}: {e}"));
                slice_is_closed(&d, id).unwrap_or_else(|e| panic!("{name}: {e}"));
                let (kept, whole) = {
                    let p = m.kept_part(id).unwrap();
                    (p, m.objects().iter().map(|(_, o)| o.volume()).sum::<f64>())
                };
                assert!(
                    kept.volume >= -1e-12 && kept.volume <= whole,
                    "{name}: {kept:?}"
                );
            }
        });
        seen += 1;
    }
    assert_eq!(seen, 3);
}
