//! The sticky tier: a persistent planar graph on a plane (ADR-0001).
//!
//! Regions are recomputed after every insertion as the minimal closed circuits of the
//! graph (angular sort of edges at each vertex, then a face walk). A counter-clockwise
//! circuit is a region's outer boundary; a clockwise circuit strictly inside a region is
//! one of its holes. That is "a closed coplanar loop auto-forms a face".
//!
//! Lane 1 implements only `add_rectangle` (needed for the extrude spike): corners merge
//! with existing vertices within POINT_MERGE and an edge that already exists is reused,
//! but an edit whose edges would touch or cross existing edges anywhere else is refused
//! with [`SketchError::IntersectionUnsupported`]. Splitting at T-junctions and crossings,
//! and collinear-overlap normalisation, arrive with the draw-tools lane.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec2;
use slotmap::SlotMap;

use crate::error::SketchError;
use crate::ids::{SEdgeId, SVertexId};
use crate::plane::Plane;
use crate::tolerance::{AREA_EPS, POINT_MERGE, in_bounds};
use crate::validate::point_in_ring;

/// A face of the sketch graph: one counter-clockwise outer circuit and clockwise holes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Region {
    pub outer: Vec<SVertexId>,
    pub holes: Vec<Vec<SVertexId>>,
}

/// Canonical identity of a region: its outer circuit rotated to start at the smallest id.
pub type RegionKey = Vec<SVertexId>;

impl Region {
    pub fn key(&self) -> RegionKey {
        canonical(&self.outer)
    }
}

fn canonical(ring: &[SVertexId]) -> Vec<SVertexId> {
    let Some((i, _)) = ring.iter().enumerate().min_by_key(|(_, v)| **v) else {
        return Vec::new();
    };
    ring[i..].iter().chain(ring[..i].iter()).copied().collect()
}

#[derive(Debug, Clone)]
pub struct Sketch {
    plane: Plane,
    vertices: SlotMap<SVertexId, DVec2>,
    edges: SlotMap<SEdgeId, [SVertexId; 2]>,
    regions: Vec<Region>,
    /// Regions that were extruded into objects, with the exact rings at that time.
    consumed: BTreeMap<RegionKey, Region>,
}

fn orient(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    robust::orient2d(
        robust::Coord { x: a.x, y: a.y },
        robust::Coord { x: b.x, y: b.y },
        robust::Coord { x: c.x, y: c.y },
    )
}

fn point_segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Segments properly cross (interiors intersect at a single point).
fn proper_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let o1 = orient(a, b, c);
    let o2 = orient(a, b, d);
    let o3 = orient(c, d, a);
    let o4 = orient(c, d, b);
    o1 * o2 < 0.0 && o3 * o4 < 0.0
}

impl Sketch {
    pub fn new(plane: Plane) -> Sketch {
        Sketch {
            plane,
            vertices: SlotMap::with_key(),
            edges: SlotMap::with_key(),
            regions: Vec::new(),
            consumed: BTreeMap::new(),
        }
    }

    pub fn plane(&self) -> &Plane {
        &self.plane
    }
    pub fn vertices(&self) -> impl Iterator<Item = (SVertexId, DVec2)> + '_ {
        self.vertices.iter().map(|(k, v)| (k, *v))
    }
    pub fn edges(&self) -> impl Iterator<Item = (SEdgeId, [SVertexId; 2])> + '_ {
        self.edges.iter().map(|(k, e)| (k, *e))
    }
    pub fn vertex(&self, v: SVertexId) -> Option<DVec2> {
        self.vertices.get(v).copied()
    }
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }
    pub fn consumed(&self) -> &BTreeMap<RegionKey, Region> {
        &self.consumed
    }
    pub fn is_consumed(&self, r: &Region) -> bool {
        self.consumed.contains_key(&r.key())
    }

    /// 2D rings of a region (outer CCW, holes CW) in the sketch plane's frame.
    pub fn region_rings(&self, r: &Region) -> (Vec<DVec2>, Vec<Vec<DVec2>>) {
        let ring = |ids: &[SVertexId]| ids.iter().map(|&v| self.vertices[v]).collect::<Vec<_>>();
        (ring(&r.outer), r.holes.iter().map(|h| ring(h)).collect())
    }

    pub(crate) fn mark_consumed(&mut self, r: &Region) {
        self.consumed.insert(r.key(), r.clone());
    }

    pub(crate) fn unmark_consumed(&mut self, key: &RegionKey) {
        self.consumed.remove(key);
    }

    /// Add an axis-aligned rectangle (in the plane's u/v frame) with one corner at
    /// `corner` and signed `size`. Returns the index of the rectangle's region.
    pub fn add_rectangle(&mut self, corner: DVec2, size: DVec2) -> Result<usize, SketchError> {
        if ![
            corner.x,
            corner.y,
            size.x,
            size.y,
            corner.x + size.x,
            corner.y + size.y,
        ]
        .iter()
        .all(|&c| in_bounds(c))
        {
            return Err(SketchError::OutOfBounds);
        }
        if size.x.abs() <= POINT_MERGE || size.y.abs() <= POINT_MERGE {
            return Err(SketchError::Degenerate);
        }
        let (x0, x1) = (
            corner.x.min(corner.x + size.x),
            corner.x.max(corner.x + size.x),
        );
        let (y0, y1) = (
            corner.y.min(corner.y + size.y),
            corner.y.max(corner.y + size.y),
        );
        let corners = [
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ];

        // Plan: resolve each corner to an existing vertex (merge) or a new one. All checks
        // run before any mutation.
        let mut plan: [Option<SVertexId>; 4] = [None; 4];
        for (i, c) in corners.iter().enumerate() {
            plan[i] = self
                .vertices
                .iter()
                .find(|(_, p)| p.distance(*c) <= POINT_MERGE)
                .map(|(k, _)| k);
            if plan[i].is_none()
                && self.edges.values().any(|[a, b]| {
                    point_segment_distance(*c, self.vertices[*a], self.vertices[*b]) <= POINT_MERGE
                })
            {
                return Err(SketchError::IntersectionUnsupported);
            }
        }
        let pos = |i: usize| plan[i].map_or(corners[i], |v| self.vertices[v]);
        for i in 0..4 {
            let j = (i + 1) % 4;
            let (p, q) = (pos(i), pos(j));
            if let (Some(a), Some(b)) = (plan[i], plan[j])
                && self
                    .edges
                    .values()
                    .any(|e| (e[0] == a && e[1] == b) || (e[0] == b && e[1] == a))
            {
                continue; // reuse an identical edge
            }
            for [a, b] in self.edges.values() {
                let (pa, pb) = (self.vertices[*a], self.vertices[*b]);
                if proper_cross(p, q, pa, pb) {
                    return Err(SketchError::IntersectionUnsupported);
                }
                let new_ids = [plan[i], plan[j]];
                // An existing vertex on the new edge (T-junction or collinear overlap)...
                for (x, px) in [(*a, pa), (*b, pb)] {
                    if !new_ids.contains(&Some(x))
                        && point_segment_distance(px, p, q) <= POINT_MERGE
                    {
                        return Err(SketchError::IntersectionUnsupported);
                    }
                }
                // ...or a new endpoint on the existing edge.
                for (id, pt) in [(plan[i], p), (plan[j], q)] {
                    let shared = id.is_some_and(|v| v == *a || v == *b);
                    if !shared && point_segment_distance(pt, pa, pb) <= POINT_MERGE {
                        return Err(SketchError::IntersectionUnsupported);
                    }
                }
            }
        }

        // Commit on a copy so a late refusal leaves `self` untouched.
        let mut next = self.clone();
        let ids: Vec<SVertexId> = (0..4)
            .map(|i| plan[i].unwrap_or_else(|| next.vertices.insert(corners[i])))
            .collect();
        for i in 0..4 {
            let (a, b) = (ids[i], ids[(i + 1) % 4]);
            if !next
                .edges
                .values()
                .any(|e| (e[0] == a && e[1] == b) || (e[0] == b && e[1] == a))
            {
                next.edges.insert([a, b]);
            }
        }
        next.recompute_regions();
        // Consumed regions must survive unchanged (the solid was made from them).
        for (key, before) in &next.consumed {
            if !next.regions.iter().any(|r| &r.key() == key && r == before) {
                return Err(SketchError::EditsConsumedRegion);
            }
        }
        let key = canonical(&ids);
        let idx = next
            .regions
            .iter()
            .position(|r| r.key() == key)
            .ok_or(SketchError::IntersectionUnsupported)?;
        #[cfg(debug_assertions)]
        if let Err(e) = next.check() {
            panic!("sketch invalid after add_rectangle: {e}");
        }
        *self = next;
        Ok(idx)
    }

    /// Sketch-graph invariants: edges join two distinct, distant vertices; no two edges
    /// touch or cross except at a shared endpoint; regions are CCW with CW holes. Runs in
    /// `Model::validate` and, in debug builds, after every sketch mutation.
    pub fn check(&self) -> Result<(), String> {
        let edges: Vec<[SVertexId; 2]> = self.edges.values().copied().collect();
        for &[a, b] in &edges {
            let (Some(pa), Some(pb)) = (self.vertices.get(a), self.vertices.get(b)) else {
                return Err("edge references a missing vertex".into());
            };
            if a == b || pa.distance(*pb) <= POINT_MERGE {
                return Err("zero-length sketch edge".into());
            }
        }
        for i in 0..edges.len() {
            for j in i + 1..edges.len() {
                let [a, b] = edges[i];
                let [c, d] = edges[j];
                let (pa, pb, pc, pd) = (
                    self.vertices[a],
                    self.vertices[b],
                    self.vertices[c],
                    self.vertices[d],
                );
                if (a == c && b == d) || (a == d && b == c) {
                    return Err("duplicate sketch edge".into());
                }
                if proper_cross(pa, pb, pc, pd) {
                    return Err("sketch edges cross".into());
                }
                for (x, px, seg) in [
                    (a, pa, (c, d)),
                    (b, pb, (c, d)),
                    (c, pc, (a, b)),
                    (d, pd, (a, b)),
                ] {
                    if x != seg.0
                        && x != seg.1
                        && point_segment_distance(px, self.vertices[seg.0], self.vertices[seg.1])
                            <= POINT_MERGE
                    {
                        return Err("sketch vertex lies on another edge".into());
                    }
                }
            }
        }
        for r in &self.regions {
            let (o, hs) = self.region_rings(r);
            if sawhorse_tessellate::signed_area(&o) <= 0.0
                || hs
                    .iter()
                    .any(|h| sawhorse_tessellate::signed_area(h) >= 0.0)
            {
                return Err("region winding is wrong".into());
            }
        }
        Ok(())
    }

    /// Minimal closed circuits -> regions with holes. Deterministic.
    fn recompute_regions(&mut self) {
        // Outgoing directed edges per vertex, sorted counter-clockwise by angle.
        let mut out: BTreeMap<SVertexId, Vec<SVertexId>> = BTreeMap::new();
        for [a, b] in self.edges.values() {
            out.entry(*a).or_default().push(*b);
            out.entry(*b).or_default().push(*a);
        }
        for (v, nbrs) in out.iter_mut() {
            let o = self.vertices[*v];
            nbrs.sort_by(|p, q| {
                let dp = self.vertices[*p] - o;
                let dq = self.vertices[*q] - o;
                dp.y.atan2(dp.x).total_cmp(&dq.y.atan2(dq.x)).then(p.cmp(q))
            });
        }
        // next(u->v) = the outgoing edge at v just clockwise of (v->u).
        let next = |u: SVertexId, v: SVertexId| -> SVertexId {
            let nbrs = &out[&v];
            let i = nbrs.iter().position(|&w| w == u).expect("twin exists");
            nbrs[(i + nbrs.len() - 1) % nbrs.len()]
        };
        let mut visited: BTreeSet<(SVertexId, SVertexId)> = BTreeSet::new();
        let mut ccw: Vec<(Vec<SVertexId>, f64)> = Vec::new();
        let mut cw: Vec<Vec<SVertexId>> = Vec::new();
        let starts: Vec<(SVertexId, SVertexId)> = out
            .iter()
            .flat_map(|(&u, ns)| ns.iter().map(move |&v| (u, v)))
            .collect();
        for (u0, v0) in starts {
            if visited.contains(&(u0, v0)) {
                continue;
            }
            let mut cycle = Vec::new();
            let (mut u, mut v) = (u0, v0);
            loop {
                visited.insert((u, v));
                cycle.push(u);
                let w = next(u, v);
                (u, v) = (v, w);
                if (u, v) == (u0, v0) || cycle.len() > 2 * self.edges.len() + 2 {
                    break;
                }
            }
            let pts: Vec<DVec2> = cycle.iter().map(|&k| self.vertices[k]).collect();
            let area = sawhorse_tessellate::signed_area(&pts);
            if area > AREA_EPS {
                ccw.push((cycle, area));
            } else if area < -AREA_EPS {
                cw.push(cycle);
            }
        }
        // Each clockwise circuit is a hole of the smallest CCW circuit strictly containing it.
        let mut regions: Vec<Region> = ccw
            .iter()
            .map(|(c, _)| Region {
                outer: canonical(c),
                holes: Vec::new(),
            })
            .collect();
        for hole in cw {
            let members: BTreeSet<SVertexId> = hole.iter().copied().collect();
            let mut best: Option<(usize, f64)> = None;
            for (i, (outer, area)) in ccw.iter().enumerate() {
                if outer.iter().any(|v| members.contains(v)) {
                    continue;
                }
                let ring: Vec<DVec2> = outer.iter().map(|&k| self.vertices[k]).collect();
                if point_in_ring(self.vertices[hole[0]], &ring)
                    && best.is_none_or(|(_, a)| *area < a)
                {
                    best = Some((i, *area));
                }
            }
            if let Some((i, _)) = best {
                regions[i].holes.push(canonical(&hole));
            }
        }
        for r in &mut regions {
            r.holes.sort();
        }
        regions.sort();
        self.regions = regions;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sketch() -> Sketch {
        Sketch::new(Plane::GROUND)
    }

    #[test]
    fn rectangle_makes_one_region() {
        let mut s = sketch();
        let r = s.add_rectangle(DVec2::ZERO, DVec2::new(2.0, 1.0)).unwrap();
        assert_eq!(s.regions().len(), 1);
        let (outer, holes) = s.region_rings(&s.regions()[r]);
        assert_eq!(outer.len(), 4);
        assert!(holes.is_empty());
        assert!((sawhorse_tessellate::signed_area(&outer) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn negative_size_is_normalised() {
        let mut s = sketch();
        s.add_rectangle(DVec2::new(2.0, 1.0), DVec2::new(-2.0, -1.0))
            .unwrap();
        let (outer, _) = s.region_rings(&s.regions()[0]);
        assert!(sawhorse_tessellate::signed_area(&outer) > 0.0);
    }

    #[test]
    fn nested_rectangle_is_a_hole() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::splat(4.0)).unwrap();
        let inner = s.add_rectangle(DVec2::ONE, DVec2::splat(2.0)).unwrap();
        assert_eq!(s.regions().len(), 2);
        let outer_region = s.regions().iter().find(|r| !r.holes.is_empty()).unwrap();
        let (o, h) = s.region_rings(outer_region);
        assert!((sawhorse_tessellate::signed_area(&o) - 16.0).abs() < 1e-12);
        assert!(
            (sawhorse_tessellate::signed_area(&h[0]) + 4.0).abs() < 1e-12,
            "hole is clockwise"
        );
        assert!(s.regions()[inner].holes.is_empty());
    }

    #[test]
    fn shared_edge_is_reused_and_makes_two_regions() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::new(1.0, 1.0)).unwrap();
        s.add_rectangle(DVec2::new(1.0, 0.0), DVec2::new(1.0, 1.0))
            .unwrap();
        assert_eq!(s.edges().count(), 7);
        assert_eq!(s.regions().len(), 2);
    }

    #[test]
    fn corner_touching_rectangles() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::ONE).unwrap();
        s.add_rectangle(DVec2::ONE, DVec2::ONE).unwrap();
        assert_eq!(s.vertices().count(), 7);
        assert_eq!(s.regions().len(), 2);
    }

    #[test]
    fn duplicate_rectangle_is_idempotent() {
        let mut s = sketch();
        let a = s.add_rectangle(DVec2::ZERO, DVec2::ONE).unwrap();
        let b = s.add_rectangle(DVec2::ZERO, DVec2::ONE).unwrap();
        assert_eq!(a, b);
        assert_eq!(s.edges().count(), 4);
    }

    #[test]
    fn refusals_are_typed() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::splat(2.0)).unwrap();
        // Crossing.
        assert_eq!(
            s.add_rectangle(DVec2::ONE, DVec2::splat(2.0)),
            Err(SketchError::IntersectionUnsupported)
        );
        // T-junction: corner on an existing edge.
        assert_eq!(
            s.add_rectangle(DVec2::new(2.0, 0.5), DVec2::ONE),
            Err(SketchError::IntersectionUnsupported)
        );
        // Collinear overlap along a shared vertex.
        assert_eq!(
            s.add_rectangle(DVec2::ZERO, DVec2::new(1.0, -1.0)),
            Err(SketchError::IntersectionUnsupported)
        );
        assert_eq!(
            s.add_rectangle(DVec2::ZERO, DVec2::new(0.0, 1.0)),
            Err(SketchError::Degenerate)
        );
        assert_eq!(
            s.add_rectangle(DVec2::ZERO, DVec2::new(f64::NAN, 1.0)),
            Err(SketchError::OutOfBounds)
        );
        assert_eq!(
            s.add_rectangle(DVec2::ZERO, DVec2::new(1e5, 1.0)),
            Err(SketchError::OutOfBounds)
        );
        assert_eq!(s.regions().len(), 1, "refusals do not touch the sketch");
    }
}
