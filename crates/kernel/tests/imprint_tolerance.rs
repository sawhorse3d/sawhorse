//! Imprints whose loops sit at or near the merge tolerance, from a face's edges or from each
//! other (2026-10-11, after the CI fuzzer's `imprint-near-tolerance-non-positive-volume`).
//!
//! The rule: an imprint either succeeds and leaves a valid solid, or is refused with a typed
//! face-edit error and changes nothing. It is never a validator rejection (a kernel bug).
//! A loop that touches the face's boundary within tolerance is refused (`NotInside` or
//! `CrossesBoundary`), not snapped: cutting a face from edge to edge is `split_face`'s job,
//! and the tools already route a stroke that reaches an edge there (`draw`).

use sawhorse_kernel::Op;
use sawhorse_kernel::testing::check_sequence;
use sawhorse_kernel::tolerance::POINT_MERGE;

/// A 1 x 1 x 0.5 m box from the ground sketch; its top face is z = 0.5.
fn boxed() -> Vec<Op> {
    serde_json::from_str(
        r#"[
          {"op":"new_sketch","origin":[0,0,0],"normal":[0,0,1]},
          {"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":1,"h":1},
          {"op":"extrude","sketch":1,"region":1,"distance":0.5}
        ]"#,
    )
    .unwrap()
}

/// The top face's persistent id (the face whose normal is +z).
fn top_face() -> u64 {
    let mut d = sawhorse_kernel::Document::new();
    for op in boxed() {
        d.apply(&op).unwrap();
    }
    let o = d.model().object(1).unwrap();
    let p = o.brep().to_polys();
    (0..p.faces.len())
        .find(|&f| p.plane(f).is_some_and(|pl| pl.normal.z > 0.9))
        .and_then(|f| o.face_id_at(f))
        .expect("a top face")
}

fn imprint(face: u64, pts: &[[f64; 2]]) -> Op {
    Op::Imprint {
        object: 1,
        face,
        points: pts.iter().map(|p| [p[0], p[1], 0.5]).collect(),
    }
}

/// Run the box, then `op`, through every fuzzer invariant; return whether `op` was accepted.
fn run(op: Op) -> bool {
    let mut ops = boxed();
    ops.push(op.clone());
    let s = check_sequence(&ops).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    s.refused == 0
}

#[test]
fn loops_near_an_edge_are_imprinted_or_refused_never_a_bug() {
    let face = top_face();
    let mut accepted = Vec::new();
    for k in [0.0, 0.25, 0.5, 0.9, 1.0, 1.1, 1.5, 2.0, 3.0, 10.0, 100.0] {
        let d = k * POINT_MERGE;
        // One side of the loop d from the face's left edge (x = 0).
        let op = imprint(face, &[[d, 0.25], [0.5, 0.25], [0.5, 0.75], [d, 0.75]]);
        accepted.push((k, run(op)));
        // One corner of the loop d from the face's corner (0, 0).
        let op = imprint(face, &[[d, d], [0.5, 0.25], [0.5, 0.5], [0.25, 0.5]]);
        run(op);
    }
    // Inside tolerance is refused; comfortably clear of it is imprinted. (Exactly at
    // POINT_MERGE either is fine: the rounding of the clearance decides.)
    for (k, ok) in &accepted {
        if *k < 1.0 {
            assert!(!ok, "a loop {k} x POINT_MERGE from the edge was imprinted");
        }
        if *k >= 10.0 {
            assert!(ok, "a loop {k} x POINT_MERGE from the edge was refused");
        }
    }
}

#[test]
fn loops_whose_points_nearly_coincide_are_imprinted_or_refused_never_a_bug() {
    let face = top_face();
    for k in [0.0, 0.1, 0.5, 0.9, 1.0, 1.1, 2.0, 10.0] {
        let e = k * POINT_MERGE;
        // Two neighbouring points e apart.
        run(imprint(
            face,
            &[
                [0.25, 0.25],
                [0.5, 0.25],
                [0.5 + e, 0.25],
                [0.75, 0.75],
                [0.25, 0.75],
            ],
        ));
        // A sliver: the loop's width collapses to e.
        run(imprint(
            face,
            &[[0.25, 0.5], [0.75, 0.5], [0.75, 0.5 + e], [0.25, 0.5 + e]],
        ));
        // A spike: one point doubles back to within e of an earlier one.
        run(imprint(
            face,
            &[
                [0.25, 0.25],
                [0.75, 0.25],
                [0.75, 0.75],
                [0.25 + e, 0.25 + e],
                [0.25, 0.75],
            ],
        ));
    }
}
