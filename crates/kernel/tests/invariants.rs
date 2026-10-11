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
    fn extrude_then_push_back_restores_the_sketch(o in origin(), n in normal(), w in size(), h in size(), d in size()) {
        let ops = box_ops(o, n, 0.0, 0.0, w, h, d);
        let sketch_only = Model::replay(&ops[..2]).unwrap();
        let mut m = Model::replay(&ops).unwrap();
        // Face 1 is the cap, d away from the sketch plane; pushing it back by -|d| along its
        // own outward normal flattens the solid.
        let out = m.apply(&Op::PushPull { object: 1, face: 1, distance: -d.abs() }).map_err(|e| TestCaseError::fail(e.to_string()))?;
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
