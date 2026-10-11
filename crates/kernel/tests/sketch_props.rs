//! Property tests for the sketch tier's insertion rules (ADR-0001, lane 2).
//!
//! - **Order independence**: the same set of segments, inserted in any order and any
//!   direction, yields the same graph, the same regions and the same canonical hash.
//! - **Simple boundaries**: every region ring is a simple closed polygon (no repeated
//!   vertex, no two non-adjacent edges touching), outers counter-clockwise, holes clockwise
//!   and inside their outer.
//! - **Split then merge is the identity**: inserting a stroke that splits edges and regions,
//!   then erasing it, gives back the sketch's canonical hash.
//!
//! Coordinates come from a 0.25 m grid in [-1.5, 1.5]. Crossing points of such segments are
//! rationals at least ~7e-4 m apart, far above the 0.01 mm merge distance, so no snapping
//! decision is ambiguous and order independence is exact (up to the 1 nm hash quantum).

use std::collections::BTreeSet;

use proptest::prelude::*;
use sawhorse_kernel::DVec2;
use sawhorse_kernel::hash::sketch_hash;
use sawhorse_kernel::plane::Plane;
use sawhorse_kernel::sketch::Sketch;

type Seg = (DVec2, DVec2);

fn coord() -> impl Strategy<Value = f64> {
    (-6i32..=6).prop_map(|k| f64::from(k) * 0.25)
}

fn seg() -> impl Strategy<Value = Seg> {
    (coord(), coord(), coord(), coord())
        .prop_filter("non-degenerate", |(a, b, c, d)| a != c || b != d)
        .prop_map(|(a, b, c, d)| (DVec2::new(a, b), DVec2::new(c, d)))
}

fn build(segs: &[Seg]) -> Sketch {
    let mut s = Sketch::new(Plane::GROUND);
    for &(a, b) in segs {
        s.insert_segment(a, b)
            .unwrap_or_else(|e| panic!("grid segment {a} -> {b} refused: {e}"));
    }
    s
}

/// Deterministic shuffle + random reversal from a seed (xorshift64*).
fn permute(segs: &[Seg], seed: u64) -> Vec<Seg> {
    let mut x = seed | 1;
    let mut next = || {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    let mut out = segs.to_vec();
    for i in (1..out.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        out.swap(i, j);
    }
    for s in &mut out {
        if next() & 1 == 1 {
            *s = (s.1, s.0);
        }
    }
    out
}

fn q(p: DVec2) -> (i64, i64) {
    ((p.x * 1e9).round() as i64, (p.y * 1e9).round() as i64)
}

/// A region as quantised position rings: outer, sorted holes, pinched.
type PosRegion = (Vec<(i64, i64)>, Vec<Vec<(i64, i64)>>, bool);

/// Regions as position rings (rotated to the smallest point), holes sorted.
fn region_set(s: &Sketch) -> BTreeSet<PosRegion> {
    let canon = |ring: &[DVec2]| {
        let pts: Vec<(i64, i64)> = ring.iter().map(|&p| q(p)).collect();
        let start = (0..pts.len()).min_by_key(|&i| pts[i]).unwrap_or(0);
        pts[start..]
            .iter()
            .chain(pts[..start].iter())
            .copied()
            .collect::<Vec<_>>()
    };
    s.all_regions()
        .map(|(_, r)| {
            let (o, h) = s.region_rings(r);
            let mut holes: Vec<Vec<(i64, i64)>> = h.iter().map(|h| canon(h)).collect();
            holes.sort();
            (canon(&o), holes, r.pinched)
        })
        .collect()
}

fn orient(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    robust::orient2d(
        robust::Coord { x: a.x, y: a.y },
        robust::Coord { x: b.x, y: b.y },
        robust::Coord { x: c.x, y: c.y },
    )
}

fn on_seg(a: DVec2, b: DVec2, p: DVec2) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Closed segments touch, cross or overlap (exact predicates).
fn touch(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    (o1 == 0.0 && on_seg(a, b, c))
        || (o2 == 0.0 && on_seg(a, b, d))
        || (o3 == 0.0 && on_seg(c, d, a))
        || (o4 == 0.0 && on_seg(c, d, b))
}

fn area(r: &[DVec2]) -> f64 {
    (0..r.len())
        .map(|i| r[i].perp_dot(r[(i + 1) % r.len()]))
        .sum::<f64>()
        * 0.5
}

fn inside(p: DVec2, ring: &[DVec2]) -> bool {
    let mut c = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a.y > p.y) != (b.y > p.y) && (b.y > a.y) == (orient(a, b, p) > 0.0) {
            c = !c;
        }
    }
    c
}

/// A ring is simple: distinct vertices, non-adjacent edges never touch, no fold-backs.
fn simple(ring: &[DVec2]) -> Result<(), String> {
    let n = ring.len();
    if n < 3 {
        return Err(format!("ring of {n}"));
    }
    let pts: BTreeSet<(i64, i64)> = ring.iter().map(|&p| q(p)).collect();
    if pts.len() != n {
        return Err("repeated vertex".into());
    }
    for i in 0..n {
        for j in i + 1..n {
            let adjacent = j == i + 1 || (i == 0 && j == n - 1);
            let (a, b, c, d) = (ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n]);
            if adjacent {
                let (p, s, r) = if j == i + 1 { (a, b, d) } else { (b, a, c) };
                if orient(p, s, r) == 0.0 && (p - s).dot(r - s) > 0.0 {
                    return Err("fold-back".into());
                }
            } else if touch(a, b, c, d) {
                return Err(format!("edges {i} and {j} touch"));
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn insertion_order_does_not_matter(segs in prop::collection::vec(seg(), 1..12), seed in any::<u64>()) {
        let a = build(&segs);
        let b = build(&permute(&segs, seed));
        prop_assert_eq!(a.vertex_count(), b.vertex_count());
        prop_assert_eq!(a.edge_count(), b.edge_count());
        prop_assert_eq!(region_set(&a), region_set(&b));
        prop_assert_eq!(sketch_hash(&a), sketch_hash(&b));
    }

    #[test]
    fn every_region_boundary_is_a_simple_closed_polygon(segs in prop::collection::vec(seg(), 1..16)) {
        let s = build(&segs);
        s.check().map_err(TestCaseError::fail)?;
        for (_, r) in s.all_regions() {
            let (o, holes) = s.region_rings(r);
            simple(&o).map_err(|e| TestCaseError::fail(format!("outer: {e}")))?;
            prop_assert!(area(&o) > 0.0, "outer must be counter-clockwise");
            for h in &holes {
                simple(h).map_err(|e| TestCaseError::fail(format!("hole: {e}")))?;
                prop_assert!(area(h) < 0.0, "holes must be clockwise");
                // Inside the outer: some vertex strictly inside, none outside.
                let strictly = h.iter().any(|&p| inside(p, &o) && !o.contains(&p));
                prop_assert!(strictly || r.pinched, "hole outside its outer");
            }
            prop_assert!(inside(r.anchor, &o) && !holes.iter().any(|h| inside(r.anchor, h)));
        }
    }

    #[test]
    fn split_then_erase_is_identity(rects in prop::collection::vec((coord(), coord(), 1i32..5, 1i32..5), 1..5), x in seg()) {
        let mut s = Sketch::new(Plane::GROUND);
        for (cx, cy, w, h) in rects {
            s.add_rectangle(DVec2::new(cx, cy), DVec2::new(f64::from(w) * 0.25, f64::from(h) * 0.25)).unwrap();
        }
        // Erasing removes everything along the stroke, so the stroke must not run along an
        // existing edge, and must not pass through a vertex the erase would then heal.
        let (a, b) = x;
        let near = |p: DVec2| {
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            (a + ab * t).distance(p) <= 1e-5
        };
        let edges: Vec<[_; 2]> = s.edges().map(|(_, e)| e).collect();
        // Collinear overlap: both ends of an edge on the stroke's line, spans overlapping.
        let ab = b - a;
        let line_dist = |p: DVec2| (p - a).perp_dot(ab).abs() / ab.length();
        let param = |p: DVec2| (p - a).dot(ab) / ab.length_squared();
        let overlaps = |[u, v]: &[_; 2]| {
            let (pu, pv) = (s.vertex(*u).unwrap(), s.vertex(*v).unwrap());
            let (t0, t1) = (param(pu).min(param(pv)), param(pu).max(param(pv)));
            line_dist(pu) <= 1e-5 && line_dist(pv) <= 1e-5 && t1.min(1.0) - t0.max(0.0) > 1e-9
        };
        prop_assume!(!edges.iter().any(overlaps));
        let straight_through = |v| {
            let nbrs: Vec<DVec2> = edges
                .iter()
                .filter(|e| e.contains(&v))
                .map(|e| s.vertex(if e[0] == v { e[1] } else { e[0] }).unwrap())
                .collect();
            nbrs.len() == 2 && orient(nbrs[0], s.vertex(v).unwrap(), nbrs[1]) == 0.0
        };
        prop_assume!(!s.vertices().any(|(v, p)| near(p) && straight_through(v)));
        let before = sketch_hash(&s);
        let regions_before = region_set(&s);
        s.insert_segment(a, b).unwrap();
        s.erase_segment(a, b).unwrap();
        prop_assert_eq!(region_set(&s), regions_before);
        prop_assert_eq!(sketch_hash(&s), before);
    }
}

#[test]
fn a_classic_case_in_every_order() {
    // Tic-tac-toe board plus a frame: 9 cells, all orders agree.
    let mut segs: Vec<Seg> = Vec::new();
    for k in [0.0, 1.0, 2.0, 3.0] {
        segs.push((DVec2::new(k, 0.0), DVec2::new(k, 3.0)));
        segs.push((DVec2::new(0.0, k), DVec2::new(3.0, k)));
    }
    let base = build(&segs);
    assert_eq!(base.region_count(), 9);
    for seed in 1..50 {
        assert_eq!(
            sketch_hash(&build(&permute(&segs, seed))),
            sketch_hash(&base)
        );
    }
}

#[test]
fn golden_sketch_crossings_extrudes_the_overlap() {
    #[derive(serde::Deserialize)]
    struct Golden {
        ops: Vec<sawhorse_kernel::Op>,
    }
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/sketch_crossings.json");
    let g: Golden = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let m = sawhorse_kernel::Model::replay(&g.ops).unwrap();
    let st = m.stats();
    assert!(
        (st.volume_m3 - 0.5).abs() < 1e-12,
        "the 1 x 1 overlap, 0.5 high: {}",
        st.volume_m3
    );
    let s = m.sketch(1).unwrap();
    assert_eq!(s.all_regions().filter(|(_, r)| r.consumed).count(), 1);
    assert_eq!(
        s.region_count(),
        3,
        "two L-shapes (one holding the triangle as a hole) and the triangle"
    );
}
