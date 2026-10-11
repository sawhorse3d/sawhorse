//! Topology validator (ADR-0001: robustness machinery first).
//!
//! Checks, for any [`Brep`]:
//! - referential integrity of every id (no dangling keys);
//! - half-edge `next`/`prev` consistency and loop closure; each loop has >= 3 half-edges
//!   and every half-edge in it points back at the loop;
//! - twin symmetry: `twin.twin == self`, opposite direction, same edge;
//! - edges own the half-edges that point at them; vertices' `out` leaves the vertex;
//! - no isolated vertices, no zero-length edges, no repeated directed edge;
//! - planarity of every loop vertex within [`PLANARITY`] of the stored plane, outer loop
//!   counter-clockwise and holes clockwise about the normal, hole inside its outer loop;
//! - per shell (faces connected through twinned edges): Euler-Poincare
//!   `V - E + F - H = 2 (1 - G)` for closed shells, i.e. even and <= 2, genus >= 0.
//!
//! The report carries a `watertight` flag (every half-edge twinned); the validator does not
//! reject open shells by itself. `Object` adds "must be watertight".

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use glam::DVec2;
use slotmap::Key;

use crate::brep::Brep;
use crate::ids::{EdgeId, FaceId, HalfEdgeId, LoopId, VertexId};
use crate::plane::newell;
use crate::tolerance::{LENGTH_EPS, PLANARITY, POINT_MERGE};

#[derive(Debug, Clone, PartialEq)]
pub enum Violation {
    DanglingKey(&'static str),
    NextPrevMismatch(HalfEdgeId),
    LoopNotClosed(LoopId),
    LoopTooShort(LoopId),
    HalfEdgeWrongLoop(HalfEdgeId),
    LoopWrongFace(LoopId),
    FaceWithoutOuterLoop(FaceId),
    LoopNotInFace(LoopId),
    TwinNotSymmetric(HalfEdgeId),
    TwinNotOpposite(HalfEdgeId),
    TwinOnOtherEdge(HalfEdgeId),
    EdgeHalfEdgeMismatch(EdgeId),
    HalfEdgeNotInEdge(HalfEdgeId),
    VertexOutWrong(VertexId),
    IsolatedVertex(VertexId),
    NonFiniteVertex(VertexId),
    ZeroLengthEdge(EdgeId),
    DuplicateDirectedEdge(HalfEdgeId),
    NonPlanar { face: FaceId, deviation: f64 },
    OuterLoopWinding(FaceId),
    HoleWinding { face: FaceId, hole: LoopId },
    HoleOutsideOuter { face: FaceId, hole: LoopId },
    NestedHoles { face: FaceId },
    SelfIntersectingFace(FaceId),
    SketchInvalid(String),
    EulerOdd { shell: usize, chi: i64 },
    EulerTooLarge { shell: usize, chi: i64 },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ShellReport {
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub holes: usize,
    pub closed: bool,
    /// V - E + F - H.
    pub euler: i64,
    /// Genus of a closed shell; None for open shells.
    pub genus: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct TopologyReport {
    pub vertices: usize,
    pub edges: usize,
    pub faces: usize,
    pub loops: usize,
    pub holes: usize,
    pub half_edges: usize,
    pub watertight: bool,
    pub shells: Vec<ShellReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopologyError {
    pub violations: Vec<Violation>,
    pub report: TopologyReport,
}

impl fmt::Display for TopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} topology violation(s)", self.violations.len())?;
        for v in self.violations.iter().take(5) {
            write!(f, "; {v:?}")?;
        }
        Ok(())
    }
}

impl std::error::Error for TopologyError {}

/// Signed area of a 2D ring.
fn area2(ring: &[DVec2]) -> f64 {
    sawhorse_tessellate::signed_area(ring)
}

/// Crossing-number point-in-polygon (strict interior; boundary counts as outside-ish,
/// which only matters for degenerate input the other checks already flag).
pub(crate) fn point_in_ring(p: DVec2, ring: &[DVec2]) -> bool {
    let n = ring.len();
    let mut inside = false;
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        if (a.y > p.y) != (b.y > p.y) {
            // Robust side test against the (a, b) segment.
            let o = robust::orient2d(
                robust::Coord { x: a.x, y: a.y },
                robust::Coord { x: b.x, y: b.y },
                robust::Coord { x: p.x, y: p.y },
            );
            // Upward edge with p on its left, or downward edge with p on its right.
            if (b.y > a.y) == (o > 0.0) {
                inside = !inside;
            }
        }
    }
    inside
}

fn orient(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    robust::orient2d(
        robust::Coord { x: a.x, y: a.y },
        robust::Coord { x: b.x, y: b.y },
        robust::Coord { x: c.x, y: c.y },
    )
}

fn on_segment(a: DVec2, b: DVec2, p: DVec2) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Closed segments ab and cd intersect (crossing, touching or collinear overlap).
fn segments_intersect(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    (o1 == 0.0 && on_segment(a, b, c))
        || (o2 == 0.0 && on_segment(a, b, d))
        || (o3 == 0.0 && on_segment(c, d, a))
        || (o4 == 0.0 && on_segment(c, d, b))
}

/// True when any two edges of a face's rings (outer + holes, 2D) intersect, other than
/// consecutive edges of one ring meeting at their shared vertex. O(E^2), fine for the
/// face sizes lane 1 produces; the push/pull lane swaps in a sweep.
pub(crate) fn rings_cross(rings: &[Vec<DVec2>]) -> bool {
    let mut segs: Vec<(usize, usize, DVec2, DVec2)> = Vec::new();
    for (r, ring) in rings.iter().enumerate() {
        for i in 0..ring.len() {
            segs.push((r, i, ring[i], ring[(i + 1) % ring.len()]));
        }
    }
    for x in 0..segs.len() {
        for y in x + 1..segs.len() {
            let (r1, i1, a, b) = segs[x];
            let (r2, i2, c, d) = segs[y];
            if r1 == r2 {
                let n = rings[r1].len();
                if i2 == i1 + 1 || (i1 == 0 && i2 == n - 1) {
                    // Consecutive: only a fold-back (collinear overlap) is a problem.
                    let (shared, p, q) = if i2 == i1 + 1 { (b, a, d) } else { (a, b, c) };
                    if orient(p, shared, q) == 0.0 && (p - shared).dot(q - shared) > 0.0 {
                        return true;
                    }
                    continue;
                }
            }
            if segments_intersect(a, b, c, d) {
                return true;
            }
        }
    }
    false
}

/// Holes inside the outer ring and not inside each other (assumes no crossings).
pub(crate) fn holes_contained(rings: &[Vec<DVec2>]) -> Result<(), usize> {
    for h in 1..rings.len() {
        if !point_in_ring(rings[h][0], &rings[0]) {
            return Err(h);
        }
        for k in 1..rings.len() {
            if k != h && point_in_ring(rings[h][0], &rings[k]) {
                return Err(h);
            }
        }
    }
    Ok(())
}

/// Per-shell accumulator: faces, edges, vertices, hole count, closed.
type ShellAcc = (
    BTreeSet<FaceId>,
    BTreeSet<EdgeId>,
    BTreeSet<VertexId>,
    usize,
    bool,
);

pub fn validate(b: &Brep) -> Result<TopologyReport, TopologyError> {
    let mut v: Vec<Violation> = Vec::new();

    // --- referential integrity + local consistency ---------------------------------
    for (hid, h) in &b.half_edges {
        let (Some(next), Some(prev)) = (b.half_edges.get(h.next), b.half_edges.get(h.prev)) else {
            v.push(Violation::DanglingKey("half-edge next/prev"));
            continue;
        };
        if next.prev != hid || prev.next != hid {
            v.push(Violation::NextPrevMismatch(hid));
        }
        if next.lp != h.lp {
            v.push(Violation::HalfEdgeWrongLoop(hid));
        }
        if !b.vertices.contains_key(h.origin) {
            v.push(Violation::DanglingKey("half-edge origin"));
        }
        if !b.loops.contains_key(h.lp) {
            v.push(Violation::DanglingKey("half-edge loop"));
        }
        match b.edges.get(h.edge) {
            None => v.push(Violation::DanglingKey("half-edge edge")),
            Some(e) => {
                if e.he != hid && e.twin != Some(hid) {
                    v.push(Violation::HalfEdgeNotInEdge(hid));
                }
            }
        }
        if let Some(t) = h.twin {
            match b.half_edges.get(t) {
                None => v.push(Violation::DanglingKey("half-edge twin")),
                Some(th) => {
                    if th.twin != Some(hid) {
                        v.push(Violation::TwinNotSymmetric(hid));
                    }
                    if th.edge != h.edge {
                        v.push(Violation::TwinOnOtherEdge(hid));
                    }
                    if b.half_edges.contains_key(th.next)
                        && (th.origin != next.origin || b.half_edges[th.next].origin != h.origin)
                    {
                        v.push(Violation::TwinNotOpposite(hid));
                    }
                }
            }
        }
    }
    if !v.is_empty() {
        // Walking loops on corrupt links would only add noise.
        return Err(TopologyError {
            violations: v,
            report: TopologyReport::default(),
        });
    }

    for (eid, e) in &b.edges {
        let ok_he = b
            .half_edges
            .get(e.he)
            .is_some_and(|h| h.edge == eid && h.twin == e.twin);
        let ok_tw = e.twin.is_none_or(|t| {
            b.half_edges
                .get(t)
                .is_some_and(|h| h.edge == eid && h.twin == Some(e.he))
        });
        if !ok_he || !ok_tw {
            v.push(Violation::EdgeHalfEdgeMismatch(eid));
            continue;
        }
        let (p, q) = b.edge_points(eid);
        if p.distance(q) < POINT_MERGE {
            v.push(Violation::ZeroLengthEdge(eid));
        }
    }

    for (vid, vx) in &b.vertices {
        if !vx.pos.is_finite() {
            v.push(Violation::NonFiniteVertex(vid));
        }
        if vx.out.is_null() {
            v.push(Violation::IsolatedVertex(vid));
        } else if b.half_edges.get(vx.out).is_none_or(|h| h.origin != vid) {
            v.push(Violation::VertexOutWrong(vid));
        }
    }

    // Directed-edge uniqueness (two half-edges a->b means a non-manifold edge).
    let mut directed: BTreeMap<(VertexId, VertexId), HalfEdgeId> = BTreeMap::new();
    for (hid, h) in &b.half_edges {
        let key = (h.origin, b.half_edges[h.next].origin);
        if directed.insert(key, hid).is_some() {
            v.push(Violation::DuplicateDirectedEdge(hid));
        }
    }

    // --- loops and faces --------------------------------------------------------
    let mut loop_owner: BTreeMap<LoopId, FaceId> = BTreeMap::new();
    for (fid, f) in &b.faces {
        if !b.loops.contains_key(f.outer) {
            v.push(Violation::FaceWithoutOuterLoop(fid));
            continue;
        }
        for l in f.loops() {
            loop_owner.insert(l, fid);
        }
    }
    for (lid, l) in &b.loops {
        if loop_owner.get(&lid) != Some(&l.face) {
            v.push(if b.faces.contains_key(l.face) {
                Violation::LoopNotInFace(lid)
            } else {
                Violation::LoopWrongFace(lid)
            });
        }
        // Closure: walking next from `first` returns to `first`, visiting only this loop.
        let mut he = l.first;
        let mut steps = 0usize;
        let mut closed = false;
        while steps <= b.half_edges.len() {
            let Some(h) = b.half_edges.get(he) else { break };
            if h.lp != lid {
                break;
            }
            steps += 1;
            he = h.next;
            if he == l.first {
                closed = true;
                break;
            }
        }
        if !closed {
            v.push(Violation::LoopNotClosed(lid));
        } else if steps < 3 {
            v.push(Violation::LoopTooShort(lid));
        }
    }
    // Every half-edge must be reachable from its loop's `first` (no orphan cycles).
    let reachable: usize = b.loops.keys().map(|l| b.half_edges_of_loop(l).len()).sum();
    if reachable != b.half_edges.len() {
        v.push(Violation::DanglingKey(
            "half-edge not reachable from its loop",
        ));
    }

    let mut holes_total = 0usize;
    for (fid, f) in &b.faces {
        if !b.loops.contains_key(f.outer) {
            continue;
        }
        let plane = f.plane;
        let mut worst = 0.0f64;
        for l in f.loops() {
            for p in b.loop_points(l) {
                worst = worst.max(plane.signed_distance(p).abs());
            }
        }
        if worst.is_nan() || worst > PLANARITY {
            v.push(Violation::NonPlanar {
                face: fid,
                deviation: worst,
            });
        }
        let outer = b.loop_points(f.outer);
        let c = outer.iter().copied().sum::<glam::DVec3>() / outer.len().max(1) as f64;
        if newell(&outer, c).dot(plane.normal) <= LENGTH_EPS * LENGTH_EPS {
            v.push(Violation::OuterLoopWinding(fid));
        }
        let outer2: Vec<DVec2> = outer.iter().map(|&p| plane.to_local(p)).collect();
        if area2(&outer2) <= 0.0 {
            v.push(Violation::OuterLoopWinding(fid));
        }
        let mut rings2 = vec![outer2];
        for &h in &f.holes {
            holes_total += 1;
            let ring: Vec<DVec2> = b
                .loop_points(h)
                .iter()
                .map(|&p| plane.to_local(p))
                .collect();
            if area2(&ring) >= 0.0 {
                v.push(Violation::HoleWinding { face: fid, hole: h });
            }
            rings2.push(ring);
        }
        if rings_cross(&rings2) {
            v.push(Violation::SelfIntersectingFace(fid));
        } else if let Err(i) = holes_contained(&rings2) {
            let hole = f.holes[i - 1];
            if point_in_ring(rings2[i][0], &rings2[0]) {
                v.push(Violation::NestedHoles { face: fid });
            } else {
                v.push(Violation::HoleOutsideOuter { face: fid, hole });
            }
        }
    }

    // --- shells and Euler-Poincare ------------------------------------------------
    let face_ids: Vec<FaceId> = b.faces.keys().collect();
    let index: BTreeMap<FaceId, usize> =
        face_ids.iter().enumerate().map(|(i, &f)| (f, i)).collect();
    let mut parent: Vec<usize> = (0..face_ids.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for e in b.edges.values() {
        if let Some(t) = e.twin {
            let fa = index[&b.loops[b.half_edges[e.he].lp].face];
            let fb = index[&b.loops[b.half_edges[t].lp].face];
            let (ra, rb) = (find(&mut parent, fa), find(&mut parent, fb));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut shells: BTreeMap<usize, ShellAcc> = BTreeMap::new();
    for (i, &fid) in face_ids.iter().enumerate() {
        let root = find(&mut parent, i);
        let entry = shells
            .entry(root)
            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new(), 0, true));
        entry.0.insert(fid);
        let f = &b.faces[fid];
        entry.3 += f.holes.len();
        for l in f.loops() {
            for he in b.half_edges_of_loop(l) {
                let h = &b.half_edges[he];
                entry.1.insert(h.edge);
                entry.2.insert(h.origin);
                if h.twin.is_none() {
                    entry.4 = false;
                }
            }
        }
    }
    let mut shell_reports = Vec::new();
    for (si, (_, (faces, edges, verts, holes, closed))) in shells.into_iter().enumerate() {
        let chi = verts.len() as i64 - edges.len() as i64 + faces.len() as i64 - holes as i64;
        let mut genus = None;
        if closed {
            if chi % 2 != 0 {
                v.push(Violation::EulerOdd { shell: si, chi });
            } else if chi > 2 {
                v.push(Violation::EulerTooLarge { shell: si, chi });
            } else {
                genus = Some((2 - chi) / 2);
            }
        } else if chi > 2 {
            v.push(Violation::EulerTooLarge { shell: si, chi });
        }
        shell_reports.push(ShellReport {
            faces: faces.len(),
            edges: edges.len(),
            vertices: verts.len(),
            holes,
            closed,
            euler: chi,
            genus,
        });
    }

    let report = TopologyReport {
        vertices: b.vertices.len(),
        edges: b.edges.len(),
        faces: b.faces.len(),
        loops: b.loops.len(),
        holes: holes_total,
        half_edges: b.half_edges.len(),
        watertight: !b.half_edges.is_empty() && b.half_edges.values().all(|h| h.twin.is_some()),
        shells: shell_reports,
    };
    if v.is_empty() {
        Ok(report)
    } else {
        Err(TopologyError {
            violations: v,
            report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::PolyFace;
    use glam::DVec3;

    fn cube() -> Brep {
        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
        let pts = [
            p(0., 0., 0.),
            p(1., 0., 0.),
            p(1., 1., 0.),
            p(0., 1., 0.),
            p(0., 0., 1.),
            p(1., 0., 1.),
            p(1., 1., 1.),
            p(0., 1., 1.),
        ];
        let f = |v: [usize; 4]| PolyFace {
            outer: v.to_vec(),
            holes: vec![],
        };
        Brep::from_polygons(
            &pts,
            &[
                f([0, 3, 2, 1]),
                f([4, 5, 6, 7]),
                f([0, 1, 5, 4]),
                f([2, 3, 7, 6]),
                f([1, 2, 6, 5]),
                f([3, 0, 4, 7]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn cube_is_valid_genus_zero() {
        let r = validate(&cube()).unwrap();
        assert!(r.watertight);
        assert_eq!(r.shells.len(), 1);
        assert_eq!(r.shells[0].euler, 2);
        assert_eq!(r.shells[0].genus, Some(0));
    }

    #[test]
    fn open_box_is_valid_but_not_watertight() {
        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
        let pts = [
            p(0., 0., 0.),
            p(1., 0., 0.),
            p(1., 1., 0.),
            p(0., 1., 0.),
            p(0., 0., 1.),
            p(1., 0., 1.),
            p(1., 1., 1.),
            p(0., 1., 1.),
        ];
        let f = |v: [usize; 4]| PolyFace {
            outer: v.to_vec(),
            holes: vec![],
        };
        let b = Brep::from_polygons(
            &pts,
            &[
                f([0, 3, 2, 1]),
                f([0, 1, 5, 4]),
                f([2, 3, 7, 6]),
                f([1, 2, 6, 5]),
                f([3, 0, 4, 7]),
            ],
        )
        .unwrap();
        let r = validate(&b).unwrap();
        assert!(!r.watertight);
        assert_eq!(r.shells[0].genus, None);
    }

    #[test]
    fn detects_broken_links() {
        let mut b = cube();
        let (h0, h1) = {
            let mut it = b.half_edges.keys();
            (it.next().unwrap(), it.nth(5).unwrap()) // h1 is on another face
        };
        b.half_edges[h0].next = h1;
        assert!(validate(&b).is_err());

        let mut b = cube();
        let h = b.half_edges.keys().next().unwrap();
        b.half_edges[h].twin = None;
        let e = validate(&b).unwrap_err();
        assert!(!e.violations.is_empty());
    }

    #[test]
    fn detects_non_planar_and_winding() {
        let mut b = cube();
        let v0 = b.vertices.keys().next().unwrap();
        b.vertices[v0].pos.z -= 0.01;
        let e = validate(&b).unwrap_err();
        assert!(
            e.violations
                .iter()
                .any(|x| matches!(x, Violation::NonPlanar { .. }))
        );

        let mut b = cube();
        let f0 = b.faces.keys().next().unwrap();
        b.faces[f0].plane.normal = -b.faces[f0].plane.normal;
        b.faces[f0].plane.v = -b.faces[f0].plane.v;
        let e = validate(&b).unwrap_err();
        assert!(
            e.violations
                .iter()
                .any(|x| matches!(x, Violation::OuterLoopWinding(_)))
        );
    }

    #[test]
    fn ring_crossings() {
        let sq = vec![
            DVec2::ZERO,
            DVec2::new(4.0, 0.0),
            DVec2::splat(4.0),
            DVec2::new(0.0, 4.0),
        ];
        let hole = vec![
            DVec2::ONE,
            DVec2::new(1.0, 2.0),
            DVec2::splat(2.0),
            DVec2::new(2.0, 1.0),
        ];
        assert!(!rings_cross(&[sq.clone(), hole.clone()]));
        assert_eq!(holes_contained(&[sq.clone(), hole.clone()]), Ok(()));
        let poking = vec![
            DVec2::new(3.0, 1.0),
            DVec2::new(3.0, 2.0),
            DVec2::new(5.0, 2.0),
            DVec2::new(5.0, 1.0),
        ];
        assert!(rings_cross(&[sq.clone(), poking]));
        let bowtie = vec![
            DVec2::ZERO,
            DVec2::ONE,
            DVec2::new(1.0, 0.0),
            DVec2::new(0.0, 1.0),
        ];
        assert!(rings_cross(&[bowtie]));
        let outside = vec![
            DVec2::splat(5.0),
            DVec2::new(5.0, 6.0),
            DVec2::splat(6.0),
            DVec2::new(6.0, 5.0),
        ];
        assert_eq!(holes_contained(&[sq, outside]), Err(1));
    }

    #[test]
    fn point_in_ring_basics() {
        let sq = [DVec2::ZERO, DVec2::X, DVec2::ONE, DVec2::Y];
        assert!(point_in_ring(DVec2::splat(0.5), &sq));
        assert!(!point_in_ring(DVec2::splat(1.5), &sq));
        assert!(!point_in_ring(DVec2::new(-0.1, 0.5), &sq));
    }
}
