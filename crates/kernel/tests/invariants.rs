//! Property tests for the kernel's core invariants (ADR-0001).
//!
//! - extrude => watertight, valid, genus 0, volume = area * |distance|, on any plane;
//! - extrude then push the cap back by -distance => the solid is gone and the sketch
//!   hashes exactly as before the extrude ("restores the sketch");
//! - push/pull any face by k then by -k => the same solid (topology identical, every
//!   vertex back within tolerance);
//! - a nested rectangle extrudes to a genus-1 tube.

use proptest::prelude::*;
use sawhorse_kernel::model::{Model, Op, OpOutcome};

fn normal() -> impl Strategy<Value = [f64; 3]> {
    prop_oneof![
        3 => Just([0.0, 0.0, 1.0]),
        1 => Just([0.0, 0.0, -1.0]),
        1 => Just([1.0, 0.0, 0.0]),
        1 => Just([0.0, -1.0, 0.0]),
        4 => (-1.0..1.0f64, -1.0..1.0f64, -1.0..1.0f64)
            .prop_filter("non-degenerate normal", |(x, y, z)| x * x + y * y + z * z > 1e-3)
            .prop_map(|(x, y, z)| [x, y, z]),
    ]
}

fn origin() -> impl Strategy<Value = [f64; 3]> {
    (-50.0..50.0f64, -50.0..50.0f64, -50.0..50.0f64).prop_map(|(x, y, z)| [x, y, z])
}

/// Sizes from 1 mm to 10 m, either sign.
fn size() -> impl Strategy<Value = f64> {
    (0.001..10.0f64, any::<bool>()).prop_map(|(s, neg)| if neg { -s } else { s })
}

/// Sizes from 1 mm to 10 m, either sign, on a 1 um grid (#112). Push back redraws the
/// footprint from the solid's world points, which carries float noise of up to about 2e-14 m,
/// and the model hash rounds the sketch's plane-local positions to 1 nm: an arbitrary size can
/// sit within that noise of a half-nanometre and hash differently after the round trip, with
/// the shape right. The corners' local coordinates are 0, w and h, and a whole number of
/// micrometres sits at least 0.499998 nm from the nearest rounding boundary, so the exact
/// hash comparison holds. The origin and the normal stay arbitrary: they are hashed as the
/// plane, which the round trip does not change.
fn grid_size() -> impl Strategy<Value = f64> {
    (1_000i64..=10_000_000, any::<bool>()).prop_map(|(um, neg)| {
        let s = um as f64 / 1e6;
        if neg { -s } else { s }
    })
}

fn box_ops(origin: [f64; 3], normal: [f64; 3], x: f64, y: f64, w: f64, h: f64, d: f64) -> Vec<Op> {
    vec![
        Op::NewSketch { origin, normal },
        Op::AddRectangle {
            sketch: 1,
            x,
            y,
            w,
            h,
        },
        Op::Extrude {
            sketch: 1,
            region: 1,
            distance: d,
        },
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn extrude_is_watertight(o in origin(), n in normal(), x in -5.0..5.0f64, y in -5.0..5.0f64, w in size(), h in size(), d in size()) {
        let m = Model::replay(&box_ops(o, n, x, y, w, h, d)).map_err(|(i, e)| TestCaseError::fail(format!("op {i}: {e}")))?;
        let st = m.stats();
        prop_assert!(st.watertight);
        prop_assert_eq!((st.faces, st.edges, st.vertices), (6, 12, 8));
        prop_assert_eq!((st.euler, st.genus), (2, 0));
        let expect = (w * h * d).abs();
        prop_assert!((st.volume_m3 - expect).abs() <= 1e-9 * expect.max(1.0), "volume {} vs {}", st.volume_m3, expect);
        m.object(1).unwrap().validate().map_err(|e| TestCaseError::fail(e.to_string()))?;
    }

    #[test]
    fn extrude_then_push_back_restores_the_sketch(o in origin(), n in normal(), w in grid_size(), h in grid_size(), d in grid_size()) {
        let ops = box_ops(o, n, 0.0, 0.0, w, h, d);
        let sketch_only = Model::replay(&ops[..2]).unwrap();
        let mut m = Model::replay(&ops).unwrap();
        // Face id 2 is the cap, d away from the sketch plane; pushing it back by -|d| along its
        // own outward normal flattens the solid.
        let out = m.apply(&Op::PushPull { object: 1, face: 2, distance: -d.abs() }).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let OpOutcome::ObjectCollapsed { regions } = out else {
            return Err(TestCaseError::fail(format!("not collapsed: {out:?}")));
        };
        prop_assert_eq!(regions.len(), 1, "the footprint went back to the sketch");
        prop_assert_eq!(m.objects().len(), 0);
        prop_assert_eq!(m.hash(), sketch_only.hash());
        // ...and the returned region can be extruded again.
        let again = m.apply(&Op::Extrude { sketch: 1, region: regions[0], distance: d }).is_ok();
        prop_assert!(again, "region could not be extruded again");
    }

    #[test]
    fn push_pull_then_inverse_is_identity(n in normal(), w in size(), h in size(), d in size(), face in 0u32..6, k in 0.001..5.0f64, out_first in any::<bool>()) {
        let mut m = Model::replay(&box_ops([0.0; 3], n, 0.0, 0.0, w, h, d)).unwrap();
        let before = m.object(1).unwrap().clone();
        // Pushing inward by more than the solid's depth is refused (typed), not corrupted.
        let k = if out_first { k } else { -k };
        // A fresh box's face at ordinal k has persistent id k + 1; the id survives the push.
        let face = u64::from(face) + 1;
        match m.apply(&Op::PushPull { object: 1, face, distance: k }) {
            Ok(OpOutcome::FaceMoved) => {
                let undo = m.apply(&Op::PushPull { object: 1, face, distance: -k });
                prop_assert!(undo.is_ok(), "inverse refused: {:?}", undo);
                prop_assert!(m.object(1).unwrap().same_shape(&before, 1e-9));
            }
            Ok(OpOutcome::ObjectCollapsed { .. }) => prop_assert_eq!(m.objects().len(), 0),
            Ok(other) => prop_assert!(false, "unexpected outcome {:?}", other),
            Err(e) => prop_assert!(!e.is_bug(), "validator bug: {}", e),
        }
    }

    #[test]
    fn nested_rectangle_makes_a_tube(w in 0.01..10.0f64, h in 0.01..10.0f64, inset in 0.05..0.45f64, d in size()) {
        let (ix, iy) = (w * inset, h * inset);
        let ops = vec![
            Op::NewSketch { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] },
            Op::AddRectangle { sketch: 1, x: 0.0, y: 0.0, w, h },
            Op::AddRectangle { sketch: 1, x: ix, y: iy, w: w - 2.0 * ix, h: h - 2.0 * iy },
        ];
        let mut m = Model::replay(&ops).unwrap();
        let s = m.sketch(1).unwrap();
        let ring = s.regions().find(|(_, r)| r.holes.len() == 1).map(|(id, _)| sawhorse_kernel::ids::to_public(id)).expect("outer region has a hole");
        m.apply(&Op::Extrude { sketch: 1, region: ring, distance: d }).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let st = m.stats();
        prop_assert!(st.watertight);
        prop_assert_eq!((st.euler, st.genus, st.holes), (0, 1, 2));
        let expect = (w * h - (w - 2.0 * ix) * (h - 2.0 * iy)) * d.abs();
        prop_assert!((st.volume_m3 - expect).abs() <= 1e-9 * expect.max(1.0));
    }
}

/// #112: the input CI's property test found, with a size off the 1 um grid. The rectangle's
/// top corners sit at v = 0.6679310874999995 m (667931087.4999995 nm) and come back from the
/// solid's world points, 49.86 m from the origin, at 0.6679310875000013 m, either side of a
/// half-nanometre, so the model hash differs while the shape is right. This holds what the
/// kernel does guarantee for it, and pins no hash.
///
/// The second input keeps the 1 pm bound meaning something the grid sizes above cannot see:
/// its sizes sit about 0.027 nm from the nearest multiple of 0.1 nm and further from a
/// half-nanometre (123456789.273 nm and -987654321.327 nm), so returned points snapped to a
/// 0.5 nm or a 0.1 nm grid fail here, where CI's input and the grid sizes let them through. It
/// is on a skew plane about 84 m from the origin, where the world coordinates are large.
#[test]
fn extrude_then_push_back_gives_the_corners_back() {
    let inputs = [
        // CI's input (#112).
        (
            [0.0, 49.86081083353802, 0.0],
            [0.0, 0.0, 1.0],
            (0.001, 0.6679310874999995, 0.001),
        ),
        // Off every 0.1 nm grid, far from the origin, on a skew plane.
        (
            [-48.7, 47.3, -49.1],
            [0.6, -0.7, 0.4],
            (0.123456789273, -0.987654321327, 0.4),
        ),
    ];
    for (origin, normal, (w, h, d)) in inputs {
        let ops = box_ops(origin, normal, 0.0, 0.0, w, h, d);
        let corners = |m: &Model| -> Vec<[f64; 2]> {
            m.sketch(1)
                .unwrap()
                .vertices()
                .map(|(_, p)| [p.x, p.y])
                .collect()
        };
        let before = corners(&Model::replay(&ops[..2]).unwrap());
        let mut m = Model::replay(&ops).unwrap();
        let out = m
            .apply(&Op::PushPull {
                object: 1,
                face: 2,
                distance: -d,
            })
            .unwrap();
        let OpOutcome::ObjectCollapsed { regions } = out else {
            panic!("not collapsed: {out:?}");
        };
        assert_eq!(regions.len(), 1, "the footprint went back to the sketch");
        assert_eq!(m.objects().len(), 0);
        let after = corners(&m);
        assert_eq!(after.len(), 4);
        // Measured: 1.8e-15 m on CI's input, and at most 2.2e-14 m over 24,000,000 random
        // cases in a harness outside the repository. 1 pm is over 40 times that and 1,000
        // times under the hash quantum. Each corner is matched with the nearest one that came
        // back, so noise either side of zero cannot pair the wrong corners.
        for a in &before {
            let gap = after
                .iter()
                .map(|b| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()))
                .fold(f64::INFINITY, f64::min);
            assert!(gap <= 1e-12, "corner {a:?} came back {gap:e} m away");
        }
        m.apply(&Op::Extrude {
            sketch: 1,
            region: regions[0],
            distance: d,
        })
        .expect("region extrudes again");
    }
}
