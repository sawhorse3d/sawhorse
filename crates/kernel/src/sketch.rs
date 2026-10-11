//! The sticky tier: a persistent planar graph on a plane (ADR-0001).
//!
//! # Insertion rules
//!
//! Every edit is a set of straight segments inserted with [`Sketch::insert_segment`] (a
//! rectangle is four of them, applied as one transaction). For each segment:
//!
//! 1. An endpoint within [`POINT_MERGE`] of an existing vertex merges into it.
//! 2. Otherwise an endpoint within `POINT_MERGE` of an edge's interior splits that edge
//!    there (T-junction); the new vertex sits on the edge's line so the edge stays straight.
//! 3. Existing vertices within `POINT_MERGE` of the new segment split it (the other kind of
//!    T-junction, and the ends of collinear overlaps).
//! 4. Proper crossings split both segments at the crossing point.
//! 5. Pieces that duplicate an existing edge are skipped, so collinear overlaps normalise
//!    to non-overlapping edges.
//!
//! The graph invariant afterwards ([`Sketch::check`]): every edge is longer than
//! `POINT_MERGE`, no two edges cross or overlap, and no vertex lies within `POINT_MERGE`
//! of an edge it is not an end of. Only genuinely degenerate input is refused: a segment
//! shorter than `POINT_MERGE` (before or after merging) or a non-finite / out-of-range
//! coordinate.
//!
//! # Regions
//!
//! After each edit the regions are recomputed as the minimal closed circuits of the graph:
//! edges are sorted by angle around each vertex and the faces of the planar embedding are
//! walked. Edges with the same face on both sides (dangling strokes, bridges between
//! loops) bound nothing and are ignored. A circuit that revisits a vertex is split there
//! into simple rings. A counter-clockwise ring is a region's outer boundary; a clockwise
//! ring is a hole of the smallest region strictly containing it. That is "a closed coplanar
//! loop auto-forms a face".
//!
//! Regions have **stable ids** ([`RegionId`]): re-detection keeps the id of the old region
//! with the same boundary, or else of the old region whose anchor (an interior point) the
//! new region contains. So a region keeps its id when an unrelated stroke is drawn, when its
//! edges gain T-junction vertices, and (one half of it) when a stroke splits it.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec2;
use slotmap::SlotMap;

use sawhorse_tessellate::{interior_point, signed_area};

use crate::error::SketchError;
use crate::ids::{RegionId, SEdgeId, SVertexId};
use crate::plane::Plane;
use crate::tolerance::{POINT_MERGE, in_bounds};
use crate::validate::point_in_ring;

/// A face of the sketch graph: one counter-clockwise outer ring and clockwise hole rings.
/// Rings are canonical (rotated to start at their smallest vertex id); holes are sorted.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Vec<SVertexId>,
    pub holes: Vec<Vec<SVertexId>>,
    /// A point strictly inside the region, in the sketch frame. Kept across edits while it
    /// stays inside; it is how re-detection recognises the region.
    pub anchor: DVec2,
    /// A hole touches the outer ring or another hole at a vertex. Such a region renders but
    /// cannot become a solid (the solid would be non-manifold along that vertex's rail).
    pub pinched: bool,
    /// Extruded into a solid. A consumed region whose edges all bound other regions stays in
    /// the graph (so it keeps its id and does not reappear as a fresh region), but it is not
    /// drawn and cannot be extruded again. A consumed region with an edge to the outside
    /// simply leaves the sketch.
    pub consumed: bool,
}

/// Canonical boundary of a region (outer ring, sorted hole rings).
pub type BoundaryKey = (Vec<SVertexId>, Vec<Vec<SVertexId>>);

impl Region {
    pub fn boundary_key(&self) -> BoundaryKey {
        (self.outer.clone(), self.holes.clone())
    }

    /// Undirected edges (vertex pairs, smaller id first) on the region's rings.
    pub fn boundary_edges(&self) -> BTreeSet<(SVertexId, SVertexId)> {
        let mut out = BTreeSet::new();
        for ring in std::iter::once(&self.outer).chain(self.holes.iter()) {
            for i in 0..ring.len() {
                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                out.insert((a.min(b), a.max(b)));
            }
        }
        out
    }
}

fn canonical(ring: &[SVertexId]) -> Vec<SVertexId> {
    let Some((i, _)) = ring.iter().enumerate().min_by_key(|(_, v)| **v) else {
        return Vec::new();
    };
    ring[i..].iter().chain(ring[..i].iter()).copied().collect()
}

pub(crate) fn orient(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    robust::orient2d(
        robust::Coord { x: a.x, y: a.y },
        robust::Coord { x: b.x, y: b.y },
        robust::Coord { x: c.x, y: c.y },
    )
}

pub(crate) fn point_segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Segments properly cross (interiors intersect at a single point).
pub(crate) fn proper_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let o1 = orient(a, b, c);
    let o2 = orient(a, b, d);
    let o3 = orient(c, d, a);
    let o4 = orient(c, d, b);
    o1 * o2 < 0.0 && o3 * o4 < 0.0
}

/// Intersection point of lines ab and cd (callers have established a proper crossing).
pub(crate) fn line_intersection(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> DVec2 {
    let r = b - a;
    let s = d - c;
    let t = (c - a).perp_dot(s) / r.perp_dot(s);
    a + r * t
}

/// True when `p` is strictly inside the rings (outer, minus holes).
fn inside_rings(p: DVec2, outer: &[DVec2], holes: &[Vec<DVec2>]) -> bool {
    point_in_ring(p, outer) && !holes.iter().any(|h| point_in_ring(p, h))
}

/// Distance from `p` to the nearest ring edge.
fn ring_clearance(p: DVec2, outer: &[DVec2], holes: &[Vec<DVec2>]) -> f64 {
    let mut best = f64::INFINITY;
    for ring in std::iter::once(outer).chain(holes.iter().map(Vec::as_slice)) {
        for i in 0..ring.len() {
            best = best.min(point_segment_distance(
                p,
                ring[i],
                ring[(i + 1) % ring.len()],
            ));
        }
    }
    best
}

/// Split a closed circuit at repeated vertices into simple rings (each of length >= 3).
fn split_at_repeats(c: &[SVertexId]) -> Vec<Vec<SVertexId>> {
    let mut stack: Vec<SVertexId> = Vec::new();
    let mut pos: BTreeMap<SVertexId, usize> = BTreeMap::new();
    let mut pieces = Vec::new();
    for &v in c {
        if let Some(&i) = pos.get(&v) {
            let piece = stack[i..].to_vec();
            for w in &stack[i + 1..] {
                pos.remove(w);
            }
            stack.truncate(i + 1);
            if piece.len() >= 3 {
                pieces.push(piece);
            }
        } else {
            pos.insert(v, stack.len());
            stack.push(v);
        }
    }
    if stack.len() >= 3 {
        pieces.push(stack);
    }
    pieces
}

#[derive(Debug, Clone)]
pub struct Sketch {
    plane: Plane,
    vertices: SlotMap<SVertexId, DVec2>,
    edges: SlotMap<SEdgeId, [SVertexId; 2]>,
    regions: SlotMap<RegionId, Region>,
}

/// A region before it has an id.
struct Detected {
    outer: Vec<SVertexId>,
    holes: Vec<Vec<SVertexId>>,
    pinched: bool,
}

impl Sketch {
    pub fn new(plane: Plane) -> Sketch {
        Sketch {
            plane,
            vertices: SlotMap::with_key(),
            edges: SlotMap::with_key(),
            regions: SlotMap::with_key(),
        }
    }

    // ---- queries -----------------------------------------------------------------------

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
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    /// Live (not consumed) regions in id-slot order (deterministic).
    pub fn regions(&self) -> impl Iterator<Item = (RegionId, &Region)> + '_ {
        self.regions.iter().filter(|(_, r)| !r.consumed)
    }
    /// Every region, consumed ones included.
    pub fn all_regions(&self) -> impl Iterator<Item = (RegionId, &Region)> + '_ {
        self.regions.iter()
    }
    pub fn region(&self, id: RegionId) -> Option<&Region> {
        self.regions.get(id)
    }
    /// Number of live (not consumed) regions.
    pub fn region_count(&self) -> usize {
        self.regions().count()
    }

    /// 2D rings of a region (outer CCW, holes CW) in the sketch plane's frame.
    pub fn region_rings(&self, r: &Region) -> (Vec<DVec2>, Vec<Vec<DVec2>>) {
        let ring = |ids: &[SVertexId]| ids.iter().map(|&v| self.vertices[v]).collect::<Vec<_>>();
        (ring(&r.outer), r.holes.iter().map(|h| ring(h)).collect())
    }

    /// The region containing `p` (sketch frame), if any.
    pub fn region_at(&self, p: DVec2) -> Option<RegionId> {
        self.regions().find_map(|(id, r)| {
            let (o, h) = self.region_rings(r);
            inside_rings(p, &o, &h).then_some(id)
        })
    }

    /// Area of a region (outer minus holes), square metres.
    pub fn region_area(&self, r: &Region) -> f64 {
        let (o, h) = self.region_rings(r);
        signed_area(&o) + h.iter().map(|h| signed_area(h)).sum::<f64>()
    }

    // ---- edits -------------------------------------------------------------------------

    /// Insert one segment (sketch frame). Returns the ids of the regions bounded by any of
    /// the new edges, sorted.
    pub fn insert_segment(&mut self, a: DVec2, b: DVec2) -> Result<Vec<RegionId>, SketchError> {
        self.insert_polyline(&[a, b], false)
    }

    /// Insert a polyline (closed: last point joins the first) as one transaction. Returns
    /// the regions bounded by any new edge, sorted.
    pub fn insert_polyline(
        &mut self,
        points: &[DVec2],
        closed: bool,
    ) -> Result<Vec<RegionId>, SketchError> {
        let (next, new_edges) = self.with_polyline(points, closed)?;
        let touched = next.regions_touching(&new_edges);
        *self = next;
        Ok(touched)
    }

    /// Add an axis-aligned rectangle (in the plane's u/v frame) with one corner at `corner`
    /// and signed `size`. Returns the ids of the regions inside the rectangle (one, unless
    /// existing strokes divide it), sorted.
    pub fn add_rectangle(
        &mut self,
        corner: DVec2,
        size: DVec2,
    ) -> Result<Vec<RegionId>, SketchError> {
        if ![corner.x, corner.y, size.x, size.y]
            .iter()
            .all(|c| c.is_finite())
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
        let (next, _) = self.with_polyline(&corners, true)?;
        let mut inside: Vec<RegionId> = next
            .regions
            .iter()
            .filter(|(_, r)| {
                r.anchor.x > x0 && r.anchor.x < x1 && r.anchor.y > y0 && r.anchor.y < y1
            })
            .map(|(id, _)| id)
            .collect();
        inside.sort();
        *self = next;
        Ok(inside)
    }

    /// Extruding consumes a region: its edges that bound no other live region are removed
    /// (with any vertex left without edges), and the region is marked consumed. Edges shared
    /// with a live neighbour stay, because the neighbour still needs them; a region enclosed
    /// entirely by such edges therefore survives as a consumed region.
    pub fn consume_region(&mut self, id: RegionId) -> Result<(), SketchError> {
        let public = crate::ids::to_public(id);
        let r = self
            .regions
            .get(id)
            .ok_or(SketchError::NoSuchRegion(public))?;
        if r.consumed {
            return Err(SketchError::RegionConsumed(public));
        }
        let mine = r.boundary_edges();
        let mut live: BTreeSet<(SVertexId, SVertexId)> = BTreeSet::new();
        for (rid, o) in self.regions() {
            if rid != id {
                live.extend(o.boundary_edges());
            }
        }
        let mut next = self.clone();
        next.regions[id].consumed = true;
        let doomed: Vec<SEdgeId> = next
            .edges
            .iter()
            .filter(|(_, [a, b])| {
                let k = ((*a).min(*b), (*a).max(*b));
                mine.contains(&k) && !live.contains(&k)
            })
            .map(|(e, _)| e)
            .collect();
        for e in doomed {
            next.edges.remove(e);
        }
        next.remove_isolated_vertices();
        next.recompute_regions();
        next.debug_check("consume_region");
        *self = next;
        Ok(())
    }

    /// A solid made from this sketch collapsed back onto it: draw its footprint rings and
    /// release (un-consume) every region inside them. Returns the live regions inside.
    pub fn release_footprint(
        &mut self,
        rings: &[Vec<DVec2>],
    ) -> Result<Vec<RegionId>, SketchError> {
        let mut next = self.clone();
        for ring in rings {
            next.insert_polyline(ring, true)?;
        }
        let Some((outer, holes)) = rings.split_first() else {
            return Ok(Vec::new());
        };
        let mut inside = Vec::new();
        for (id, r) in next.regions.iter_mut() {
            if inside_rings(r.anchor, outer, holes) {
                r.consumed = false;
                inside.push(id);
            }
        }
        next.debug_check("release_footprint");
        *self = next;
        inside.sort();
        Ok(inside)
    }

    /// Erase every edge lying along segment `a`-`b` (both ends within `POINT_MERGE` of it),
    /// drop vertices left without edges, and heal vertices the erase left joining exactly two
    /// collinear edges (a T-junction or crossing whose other stroke is gone), so splitting
    /// then erasing is the identity. Returns the live regions that are new or changed.
    pub fn erase_segment(&mut self, a: DVec2, b: DVec2) -> Result<Vec<RegionId>, SketchError> {
        if !(in_bounds(a.x) && in_bounds(a.y) && in_bounds(b.x) && in_bounds(b.y)) {
            return Err(SketchError::OutOfBounds);
        }
        if a.distance(b) <= POINT_MERGE {
            return Err(SketchError::Degenerate);
        }
        let mut next = self.clone();
        let doomed: Vec<(SEdgeId, [SVertexId; 2])> = next
            .edges
            .iter()
            .filter(|(_, [p, q])| {
                point_segment_distance(next.vertices[*p], a, b) <= POINT_MERGE
                    && point_segment_distance(next.vertices[*q], a, b) <= POINT_MERGE
            })
            .map(|(e, ends)| (e, *ends))
            .collect();
        if doomed.is_empty() {
            return Ok(Vec::new());
        }
        let mut ends: BTreeSet<SVertexId> = BTreeSet::new();
        for (e, [p, q]) in doomed {
            next.edges.remove(e);
            ends.insert(p);
            ends.insert(q);
        }
        next.remove_isolated_vertices();
        for v in ends {
            next.heal_vertex(v);
        }
        next.recompute_regions();
        next.debug_check("erase_segment");
        let changed = next.changed_regions(self);
        *self = next;
        Ok(changed)
    }

    /// Remove `v` when it joins exactly two edges that run straight through it.
    fn heal_vertex(&mut self, v: SVertexId) {
        let Some(&pv) = self.vertices.get(v) else {
            return;
        };
        let incident: Vec<(SEdgeId, SVertexId)> = self
            .edges
            .iter()
            .filter_map(|(e, [p, q])| {
                if *p == v {
                    Some((e, *q))
                } else if *q == v {
                    Some((e, *p))
                } else {
                    None
                }
            })
            .collect();
        let [(e1, u), (e2, w)] = incident[..] else {
            return;
        };
        let (pu, pw) = (self.vertices[u], self.vertices[w]);
        let t = (pv - pu).dot(pw - pu) / pu.distance_squared(pw);
        if t <= 0.0 || t >= 1.0 || point_segment_distance(pv, pu, pw) > POINT_MERGE {
            return;
        }
        if self.edge_between(u, w).is_some() {
            return;
        }
        self.edges.remove(e2);
        self.edges[e1] = [u, w];
        self.vertices.remove(v);
    }

    /// Live regions of `self` that are new or whose boundary differs from `before`.
    fn changed_regions(&self, before: &Sketch) -> Vec<RegionId> {
        let mut out: Vec<RegionId> = self
            .regions()
            .filter(|(id, r)| {
                before
                    .regions
                    .get(*id)
                    .is_none_or(|old| old.boundary_key() != r.boundary_key() || old.consumed)
            })
            .map(|(id, _)| id)
            .collect();
        out.sort();
        out
    }

    /// Clone, insert, re-detect regions. Returns the new sketch and the new edges.
    fn with_polyline(
        &self,
        points: &[DVec2],
        closed: bool,
    ) -> Result<(Sketch, Vec<(SVertexId, SVertexId)>), SketchError> {
        if points.len() < 2 {
            return Err(SketchError::Degenerate);
        }
        if !points.iter().all(|p| in_bounds(p.x) && in_bounds(p.y)) {
            return Err(SketchError::OutOfBounds);
        }
        let n = points.len();
        let segs = if closed { n } else { n - 1 };
        if closed && n < 3 {
            return Err(SketchError::Degenerate);
        }
        let mut next = self.clone();
        let mut new_edges = Vec::new();
        for i in 0..segs {
            new_edges.extend(next.add_segment(points[i], points[(i + 1) % n])?);
        }
        next.recompute_regions();
        next.debug_check("insert");
        Ok((next, new_edges))
    }

    fn debug_check(&self, what: &str) {
        #[cfg(debug_assertions)]
        if let Err(e) = self.check() {
            panic!("sketch invalid after {what}: {e}");
        }
        let _ = what;
    }

    fn regions_touching(&self, new_edges: &[(SVertexId, SVertexId)]) -> Vec<RegionId> {
        let keys: BTreeSet<(SVertexId, SVertexId)> = new_edges
            .iter()
            .map(|&(a, b)| (a.min(b), a.max(b)))
            .collect();
        let mut out: Vec<RegionId> = self
            .regions
            .iter()
            .filter(|(_, r)| !r.boundary_edges().is_disjoint(&keys))
            .map(|(id, _)| id)
            .collect();
        out.sort();
        out
    }

    fn remove_isolated_vertices(&mut self) {
        let used: BTreeSet<SVertexId> = self.edges.values().flatten().copied().collect();
        self.vertices.retain(|v, _| used.contains(&v));
    }

    fn edge_between(&self, a: SVertexId, b: SVertexId) -> Option<SEdgeId> {
        self.edges
            .iter()
            .find(|(_, e)| (e[0] == a && e[1] == b) || (e[0] == b && e[1] == a))
            .map(|(k, _)| k)
    }

    /// Split edge `e` at `pos` (which lies on it); returns the new vertex.
    fn split_edge(&mut self, e: SEdgeId, pos: DVec2) -> SVertexId {
        let [a, b] = self.edges[e];
        let x = self.vertices.insert(pos);
        self.edges[e] = [a, x];
        self.edges.insert([x, b]);
        x
    }

    /// Resolve a segment endpoint: merge into a vertex, split an edge, or make a new vertex.
    fn snap(&mut self, p: DVec2) -> SVertexId {
        let near = self
            .vertices
            .iter()
            .map(|(k, q)| (q.distance(p), k))
            .filter(|(d, _)| *d <= POINT_MERGE)
            .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        if let Some((_, v)) = near {
            return v;
        }
        let on_edge = self
            .edges
            .iter()
            .filter_map(|(e, [a, b])| {
                let (pa, pb) = (self.vertices[*a], self.vertices[*b]);
                let d = point_segment_distance(p, pa, pb);
                (d <= POINT_MERGE).then_some((d, e))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        if let Some((_, e)) = on_edge {
            let [a, b] = self.edges[e];
            let (pa, pb) = (self.vertices[a], self.vertices[b]);
            let ab = pb - pa;
            let t = ((p - pa).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            let proj = pa + ab * t;
            // Never create a piece shorter than the merge distance.
            if proj.distance(pa) <= POINT_MERGE {
                return a;
            }
            if proj.distance(pb) <= POINT_MERGE {
                return b;
            }
            return self.split_edge(e, proj);
        }
        self.vertices.insert(p)
    }

    /// Insert one segment into the graph without re-detecting regions. Returns the edges it
    /// created (pieces that already existed are not repeated).
    fn add_segment(
        &mut self,
        a: DVec2,
        b: DVec2,
    ) -> Result<Vec<(SVertexId, SVertexId)>, SketchError> {
        if a.distance(b) <= POINT_MERGE {
            return Err(SketchError::Degenerate);
        }
        let p = self.snap(a);
        let q = self.snap(b);
        if p == q {
            return Err(SketchError::Degenerate);
        }
        let (pp, qq) = (self.vertices[p], self.vertices[q]);
        let d = qq - pp;
        let len2 = d.length_squared();
        let param = |x: DVec2| (x - pp).dot(d) / len2;

        // Existing vertices on the new segment split it.
        let mut cuts: Vec<(f64, SVertexId)> = self
            .vertices
            .iter()
            .filter(|(v, pos)| {
                *v != p && *v != q && point_segment_distance(**pos, pp, qq) <= POINT_MERGE
            })
            .map(|(v, pos)| (param(*pos), v))
            .collect();
        let on_line: BTreeSet<SVertexId> = cuts.iter().map(|(_, v)| *v).collect();

        // Proper crossings split both. Edges touching the segment at a vertex were handled
        // above and cannot also cross it.
        let mut crossings: Vec<(f64, SEdgeId, DVec2)> = Vec::new();
        for (e, [c, dd]) in &self.edges {
            if [*c, *dd]
                .iter()
                .any(|v| *v == p || *v == q || on_line.contains(v))
            {
                continue;
            }
            let (pc, pd) = (self.vertices[*c], self.vertices[*dd]);
            if proper_cross(pp, qq, pc, pd) {
                let x = line_intersection(pp, qq, pc, pd);
                crossings.push((param(x), e, x));
            }
        }
        crossings.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        // Crossings within the merge distance of each other share one vertex.
        let mut last: Option<(DVec2, SVertexId)> = None;
        for (t, e, x) in crossings {
            let v = match last {
                Some((lx, lv)) if lx.distance(x) <= POINT_MERGE => {
                    let [c, dd] = self.edges[e];
                    self.edges[e] = [c, lv];
                    self.edges.insert([lv, dd]);
                    lv
                }
                _ => {
                    let v = self.split_edge(e, x);
                    cuts.push((t, v));
                    v
                }
            };
            last = Some((self.vertices[v], v));
        }
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));

        let chain: Vec<SVertexId> = std::iter::once(p)
            .chain(cuts.into_iter().map(|(_, v)| v))
            .chain(std::iter::once(q))
            .collect();
        let mut created = Vec::new();
        for w in chain.windows(2) {
            let (u, v) = (w[0], w[1]);
            if u == v || self.edge_between(u, v).is_some() {
                continue;
            }
            self.edges.insert([u, v]);
            created.push((u, v));
        }
        Ok(created)
    }

    // ---- invariants --------------------------------------------------------------------

    /// Sketch-graph invariants: edges join two distinct vertices more than `POINT_MERGE`
    /// apart; no two edges cross, overlap or repeat; no vertex lies within `POINT_MERGE` of
    /// an edge it does not end; no isolated vertices; regions have simple CCW outers, simple
    /// CW holes and an anchor strictly inside. Runs in `Model::validate` and, in debug
    /// builds, after every sketch mutation.
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
        let used: BTreeSet<SVertexId> = edges.iter().flatten().copied().collect();
        if used.len() != self.vertices.len() {
            return Err("isolated sketch vertex".into());
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
        let edge_set: BTreeSet<(SVertexId, SVertexId)> =
            edges.iter().map(|&[a, b]| (a.min(b), a.max(b))).collect();
        for (_, r) in &self.regions {
            for ring in std::iter::once(&r.outer).chain(r.holes.iter()) {
                let distinct: BTreeSet<_> = ring.iter().collect();
                if ring.len() < 3 || distinct.len() != ring.len() {
                    return Err("region ring is not simple".into());
                }
                if !r.boundary_edges().iter().all(|k| edge_set.contains(k)) {
                    return Err("region ring uses a missing edge".into());
                }
            }
            let (o, hs) = self.region_rings(r);
            if signed_area(&o) <= 0.0 || hs.iter().any(|h| signed_area(h) >= 0.0) {
                return Err("region winding is wrong".into());
            }
            if !inside_rings(r.anchor, &o, &hs) {
                return Err("region anchor is not inside the region".into());
            }
        }
        Ok(())
    }

    // ---- region detection --------------------------------------------------------------

    /// Faces of the planar embedding as closed circuits (face on the left), skipping `skip`.
    fn walk(&self, skip: &BTreeSet<SEdgeId>) -> Vec<Vec<SVertexId>> {
        let mut out: BTreeMap<SVertexId, Vec<SVertexId>> = BTreeMap::new();
        for (e, [a, b]) in &self.edges {
            if skip.contains(&e) {
                continue;
            }
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
        // next(u->v) = the edge leaving v just clockwise of (v->u): keeps the face on the left.
        let next = |u: SVertexId, v: SVertexId| -> SVertexId {
            let nbrs = &out[&v];
            let i = nbrs.iter().position(|&w| w == u).expect("twin exists");
            nbrs[(i + nbrs.len() - 1) % nbrs.len()]
        };
        let mut visited: BTreeSet<(SVertexId, SVertexId)> = BTreeSet::new();
        let mut circuits = Vec::new();
        let limit = 2 * self.edges.len() + 2;
        for (&u0, ns) in &out {
            for &v0 in ns {
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
                    if (u, v) == (u0, v0) || cycle.len() > limit {
                        break;
                    }
                }
                circuits.push(cycle);
            }
        }
        circuits
    }

    /// Edges with the same face on both sides: dangling strokes and bridges.
    fn bridges(&self) -> BTreeSet<SEdgeId> {
        let circuits = self.walk(&BTreeSet::new());
        let mut side: BTreeMap<(SVertexId, SVertexId), usize> = BTreeMap::new();
        for (i, c) in circuits.iter().enumerate() {
            for k in 0..c.len() {
                side.insert((c[k], c[(k + 1) % c.len()]), i);
            }
        }
        self.edges
            .iter()
            .filter(|(_, [a, b])| side.get(&(*a, *b)) == side.get(&(*b, *a)))
            .map(|(e, _)| e)
            .collect()
    }

    fn detect(&self) -> Vec<Detected> {
        let circuits = self.walk(&self.bridges());
        let pts = |ring: &[SVertexId]| ring.iter().map(|&v| self.vertices[v]).collect::<Vec<_>>();
        let mut found: Vec<Detected> = Vec::new();
        let mut areas: Vec<f64> = Vec::new();
        let mut loose: Vec<(Vec<SVertexId>, bool)> = Vec::new();
        for c in circuits {
            let pieces = split_at_repeats(&c);
            let signed: Vec<f64> = pieces.iter().map(|p| signed_area(&pts(p))).collect();
            let total: f64 = signed.iter().sum();
            let pinched = pieces.len() > 1;
            let positive: Vec<usize> = (0..pieces.len()).filter(|&i| signed[i] > 0.0).collect();
            if total > 0.0 && positive.len() == 1 {
                let o = positive[0];
                found.push(Detected {
                    outer: canonical(&pieces[o]),
                    holes: (0..pieces.len())
                        .filter(|&i| i != o && signed[i] < 0.0)
                        .map(|i| canonical(&pieces[i]))
                        .collect(),
                    pinched,
                });
                areas.push(signed[o]);
            } else {
                for (i, p) in pieces.into_iter().enumerate() {
                    if signed[i] > 0.0 {
                        found.push(Detected {
                            outer: canonical(&p),
                            holes: Vec::new(),
                            pinched,
                        });
                        areas.push(signed[i]);
                    } else if signed[i] < 0.0 {
                        loose.push((canonical(&p), pinched));
                    }
                }
            }
        }
        // Each loose clockwise ring is a hole of the smallest outer strictly containing it.
        for (hole, pinched) in loose {
            let members: BTreeSet<SVertexId> = hole.iter().copied().collect();
            let probe = self.vertices[hole[0]];
            let mut best: Option<usize> = None;
            for (i, d) in found.iter().enumerate() {
                if d.outer.iter().any(|v| members.contains(v)) {
                    continue;
                }
                if point_in_ring(probe, &pts(&d.outer)) && best.is_none_or(|b| areas[i] < areas[b])
                {
                    best = Some(i);
                }
            }
            if let Some(i) = best {
                found[i].holes.push(hole);
                found[i].pinched |= pinched;
            }
        }
        for d in &mut found {
            d.holes.sort();
        }
        found.sort_by(|a, b| a.outer.cmp(&b.outer).then(a.holes.cmp(&b.holes)));
        found
    }

    /// Re-detect regions, keeping ids of regions that match (same boundary, else the new
    /// region contains the old anchor). Deterministic.
    fn recompute_regions(&mut self) {
        let found = self.detect();
        let mut assigned: Vec<Option<RegionId>> = vec![None; found.len()];
        let mut used: BTreeSet<RegionId> = BTreeSet::new();
        let by_key: BTreeMap<BoundaryKey, RegionId> = self
            .regions
            .iter()
            .map(|(id, r)| (r.boundary_key(), id))
            .collect();
        for (i, d) in found.iter().enumerate() {
            if let Some(&id) = by_key.get(&(d.outer.clone(), d.holes.clone()))
                && used.insert(id)
            {
                assigned[i] = Some(id);
            }
        }
        // A region that only gained or lost holes keeps its id.
        let by_outer: BTreeMap<Vec<SVertexId>, RegionId> = self
            .regions
            .iter()
            .filter(|(id, _)| !used.contains(id))
            .map(|(id, r)| (r.outer.clone(), id))
            .collect();
        for (i, d) in found.iter().enumerate() {
            if assigned[i].is_none()
                && let Some(&id) = by_outer.get(&d.outer)
                && used.insert(id)
            {
                assigned[i] = Some(id);
            }
        }
        let rings: Vec<(Vec<DVec2>, Vec<Vec<DVec2>>)> = found
            .iter()
            .map(|d| {
                let ring = |ids: &[SVertexId]| ids.iter().map(|&v| self.vertices[v]).collect();
                (ring(&d.outer), d.holes.iter().map(|h| ring(h)).collect())
            })
            .collect();
        // Live regions claim first: when an erased edge merges a consumed region into a live
        // neighbour, the merged region is live.
        let mut old: Vec<(bool, RegionId, DVec2)> = self
            .regions
            .iter()
            .filter(|(id, _)| !used.contains(id))
            .map(|(id, r)| (r.consumed, id, r.anchor))
            .collect();
        old.sort_by_key(|(consumed, _, _)| *consumed);
        let old: Vec<(RegionId, DVec2)> = old.into_iter().map(|(_, id, a)| (id, a)).collect();
        for (id, anchor) in old {
            if let Some(i) = (0..found.len())
                .find(|&i| assigned[i].is_none() && inside_rings(anchor, &rings[i].0, &rings[i].1))
            {
                assigned[i] = Some(id);
                used.insert(id);
            }
        }
        // Pieces split off a consumed region stay consumed (they lie under the solid).
        let consumed_areas: Vec<(Vec<DVec2>, Vec<Vec<DVec2>>)> = self
            .regions
            .values()
            .filter(|r| {
                r.consumed
                    && r.outer
                        .iter()
                        .chain(r.holes.iter().flatten())
                        .all(|v| self.vertices.contains_key(*v))
            })
            .map(|r| self.region_rings(r))
            .collect();
        let stale: Vec<RegionId> = self
            .regions
            .keys()
            .filter(|id| !used.contains(id))
            .collect();
        for id in stale {
            self.regions.remove(id);
        }
        for (i, d) in found.into_iter().enumerate() {
            let (o, h) = &rings[i];
            let keep = assigned[i]
                .and_then(|id| self.regions.get(id))
                .map(|r| r.anchor)
                .filter(|&a| inside_rings(a, o, h) && ring_clearance(a, o, h) > POINT_MERGE);
            let Some(anchor) = keep.or_else(|| interior_point(o, h)) else {
                continue; // unreachable for a positive-area ring; drop rather than corrupt
            };
            let consumed = match assigned[i].and_then(|id| self.regions.get(id)) {
                Some(old) => old.consumed,
                None => consumed_areas
                    .iter()
                    .any(|(co, ch)| inside_rings(anchor, co, ch)),
            };
            let region = Region {
                outer: d.outer,
                holes: d.holes,
                anchor,
                pinched: d.pinched,
                consumed,
            };
            match assigned[i] {
                Some(id) => self.regions[id] = region,
                None => {
                    self.regions.insert(region);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sketch() -> Sketch {
        Sketch::new(Plane::GROUND)
    }
    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }
    fn areas(s: &Sketch) -> Vec<f64> {
        let mut a: Vec<f64> = s.regions().map(|(_, r)| s.region_area(r)).collect();
        a.sort_by(f64::total_cmp);
        a
    }

    #[test]
    fn rectangle_makes_one_region() {
        let mut s = sketch();
        let r = s.add_rectangle(DVec2::ZERO, v(2.0, 1.0)).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(s.region_count(), 1);
        let (outer, holes) = s.region_rings(s.region(r[0]).unwrap());
        assert_eq!(outer.len(), 4);
        assert!(holes.is_empty());
        assert!((signed_area(&outer) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn negative_size_is_normalised() {
        let mut s = sketch();
        let r = s.add_rectangle(v(2.0, 1.0), v(-2.0, -1.0)).unwrap();
        let (outer, _) = s.region_rings(s.region(r[0]).unwrap());
        assert!(signed_area(&outer) > 0.0);
    }

    #[test]
    fn nested_rectangle_is_a_hole() {
        let mut s = sketch();
        let big = s.add_rectangle(DVec2::ZERO, DVec2::splat(4.0)).unwrap()[0];
        let inner = s.add_rectangle(DVec2::ONE, DVec2::splat(2.0)).unwrap()[0];
        assert_eq!(s.region_count(), 2);
        let outer_region = s.region(big).unwrap();
        assert_eq!(
            outer_region.holes.len(),
            1,
            "the big region keeps its id and gains a hole"
        );
        let (o, h) = s.region_rings(outer_region);
        assert!((signed_area(&o) - 16.0).abs() < 1e-12);
        assert!(
            (signed_area(&h[0]) + 4.0).abs() < 1e-12,
            "hole is clockwise"
        );
        assert!(s.region(inner).unwrap().holes.is_empty());
    }

    #[test]
    fn shared_edge_is_reused_and_makes_two_regions() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, v(1.0, 1.0)).unwrap();
        s.add_rectangle(v(1.0, 0.0), v(1.0, 1.0)).unwrap();
        assert_eq!(s.edge_count(), 7);
        assert_eq!(s.region_count(), 2);
    }

    #[test]
    fn corner_touching_rectangles() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::ONE).unwrap();
        s.add_rectangle(DVec2::ONE, DVec2::ONE).unwrap();
        assert_eq!(s.vertex_count(), 7);
        assert_eq!(s.region_count(), 2);
    }

    #[test]
    fn duplicate_rectangle_is_idempotent() {
        let mut s = sketch();
        let a = s.add_rectangle(DVec2::ZERO, DVec2::ONE).unwrap();
        let b = s.add_rectangle(DVec2::ZERO, DVec2::ONE).unwrap();
        assert_eq!(a, b);
        assert_eq!(s.edge_count(), 4);
    }

    #[test]
    fn crossing_rectangles_split_into_three_regions() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::splat(2.0)).unwrap();
        let inside = s.add_rectangle(DVec2::ONE, DVec2::splat(2.0)).unwrap();
        assert_eq!(s.region_count(), 3);
        assert_eq!(inside.len(), 2, "the overlap and the new part");
        assert_eq!(areas(&s), vec![1.0, 3.0, 3.0]);
        assert_eq!(s.vertex_count(), 10, "eight corners and two crossings");
    }

    #[test]
    fn t_junction_splits_the_existing_edge() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::splat(2.0)).unwrap();
        s.add_rectangle(v(2.0, 0.5), DVec2::ONE).unwrap();
        assert_eq!(s.region_count(), 2);
        assert_eq!(s.vertex_count(), 8);
        assert_eq!(
            s.edge_count(),
            9,
            "the shared x = 2 edge is split into three"
        );
    }

    #[test]
    fn collinear_overlap_is_normalised() {
        let mut s = sketch();
        s.insert_segment(v(0.0, 0.0), v(2.0, 0.0)).unwrap();
        s.insert_segment(v(1.0, 0.0), v(3.0, 0.0)).unwrap();
        assert_eq!(s.vertex_count(), 4);
        assert_eq!(s.edge_count(), 3);
        s.insert_segment(v(-1.0, 0.0), v(4.0, 0.0)).unwrap();
        assert_eq!((s.vertex_count(), s.edge_count()), (6, 5));
        s.check().unwrap();
        assert_eq!(s.region_count(), 0, "a line bounds nothing");
    }

    #[test]
    fn segment_through_a_vertex_splits_at_it() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::splat(2.0)).unwrap();
        // The diagonal passes exactly through two corners: no new vertices, two regions.
        s.insert_segment(v(0.0, 0.0), v(2.0, 2.0)).unwrap();
        assert_eq!(s.vertex_count(), 4);
        assert_eq!(areas(&s), vec![2.0, 2.0]);
        // Extending a stroke past a vertex splits it there.
        s.insert_segment(v(-1.0, -1.0), v(3.0, 3.0)).unwrap();
        assert_eq!(s.vertex_count(), 6);
        assert_eq!(s.edge_count(), 7);
    }

    #[test]
    fn endpoints_merge_within_tolerance() {
        let mut s = sketch();
        s.insert_segment(v(0.0, 0.0), v(1.0, 0.0)).unwrap();
        s.insert_segment(v(1.0 + 4e-6, 0.0), v(1.0, 1.0)).unwrap();
        s.insert_segment(v(1.0, 1.0 - 3e-6), v(0.0, 3e-6)).unwrap();
        assert_eq!(s.vertex_count(), 3);
        assert_eq!(s.region_count(), 1);
    }

    #[test]
    fn dangling_strokes_and_bridges_bound_nothing() {
        let mut s = sketch();
        let r = s.add_rectangle(DVec2::ZERO, DVec2::splat(4.0)).unwrap()[0];
        // A dangling stroke inside: still one region, rings stay simple.
        s.insert_segment(v(1.0, 1.0), v(2.0, 2.0)).unwrap();
        assert_eq!(s.region_count(), 1);
        assert!(s.region(r).unwrap().holes.is_empty());
        // A square inside joined to the outer square by a bridge: the square is a hole.
        s.add_rectangle(v(2.5, 2.5), DVec2::ONE).unwrap();
        s.insert_segment(v(3.5, 3.5), v(4.0, 4.0)).unwrap();
        assert_eq!(s.region_count(), 2);
        let big = s.region(r).unwrap();
        assert_eq!(big.holes.len(), 1);
        assert!(!big.pinched);
        assert!((s.region_area(big) - 15.0).abs() < 1e-12);
        s.check().unwrap();
    }

    #[test]
    fn hole_touching_the_outer_ring_is_pinched() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::splat(4.0)).unwrap();
        // A triangle whose apex touches the bottom edge (T-junction there).
        s.insert_polyline(&[v(2.0, 0.0), v(3.0, 2.0), v(1.0, 2.0)], true)
            .unwrap();
        assert_eq!(s.region_count(), 2);
        let outer = s
            .regions()
            .map(|(_, r)| r)
            .find(|r| !r.holes.is_empty())
            .unwrap();
        assert!(outer.pinched);
        assert!((s.region_area(outer) - 14.0).abs() < 1e-12);
        s.check().unwrap();
    }

    #[test]
    fn region_ids_survive_unrelated_and_splitting_edits() {
        let mut s = sketch();
        let a = s.add_rectangle(DVec2::ZERO, DVec2::splat(2.0)).unwrap()[0];
        let b = s.add_rectangle(v(5.0, 0.0), DVec2::ONE).unwrap()[0];
        assert_ne!(a, b);
        // A T-junction on a's boundary changes its ring but not its id.
        s.insert_segment(v(2.0, 1.0), v(3.0, 1.0)).unwrap();
        assert!(s.region(a).is_some() && s.region(b).is_some());
        // Splitting a: the half holding a's anchor keeps the id; the other half is new.
        let anchor = s.region(a).unwrap().anchor;
        s.insert_segment(v(0.0, 0.5), v(2.0, 0.5)).unwrap();
        assert_eq!(s.region_count(), 3);
        assert_eq!(s.region_at(anchor), Some(a));
        assert!(s.region(b).is_some());
    }

    #[test]
    fn consuming_an_enclosed_region_keeps_it_hidden() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::splat(2.0)).unwrap();
        s.add_rectangle(DVec2::ONE, DVec2::splat(2.0)).unwrap();
        let overlap = s.region_at(v(1.5, 1.5)).unwrap();
        let edges = s.edge_count();
        s.consume_region(overlap).unwrap();
        assert_eq!(
            s.edge_count(),
            edges,
            "every edge still bounds a live neighbour"
        );
        assert_eq!(s.region_count(), 2);
        assert!(s.region(overlap).unwrap().consumed);
        assert_eq!(s.region_at(v(1.5, 1.5)), None, "not live");
        assert_eq!(
            s.consume_region(overlap),
            Err(SketchError::RegionConsumed(crate::ids::to_public(overlap)))
        );
        // A stroke across it splits it; both halves stay consumed (they lie under the solid).
        s.insert_segment(v(1.0, 1.5), v(2.0, 1.5)).unwrap();
        assert_eq!(s.region_count(), 2);
        assert_eq!(s.all_regions().filter(|(_, r)| r.consumed).count(), 2);
        // Releasing the footprint brings them back.
        let back = s
            .release_footprint(&[vec![v(1.0, 1.0), v(2.0, 1.0), v(2.0, 2.0), v(1.0, 2.0)]])
            .unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(s.region_count(), 4);
        s.check().unwrap();
    }

    #[test]
    fn erase_heals_what_the_stroke_split() {
        let mut s = sketch();
        let r = s.add_rectangle(DVec2::ZERO, DVec2::splat(2.0)).unwrap()[0];
        let before = crate::hash::sketch_hash(&s);
        s.insert_segment(v(-1.0, 1.0), v(3.0, 1.0)).unwrap();
        assert_eq!(s.region_count(), 2);
        assert_eq!(s.vertex_count(), 8);
        let changed = s.erase_segment(v(-1.0, 1.0), v(3.0, 1.0)).unwrap();
        assert_eq!(
            crate::hash::sketch_hash(&s),
            before,
            "split then erase is the identity"
        );
        assert_eq!(s.region_count(), 1);
        assert!(
            s.region(r).is_some(),
            "the half holding the anchor kept the id"
        );
        assert!(changed.contains(&r));
        // Erasing where nothing lies is a no-op, not an error.
        assert_eq!(s.erase_segment(v(5.0, 5.0), v(6.0, 5.0)).unwrap(), vec![]);
        assert_eq!(
            s.erase_segment(v(5.0, 5.0), v(5.0, 5.0)),
            Err(SketchError::Degenerate)
        );
    }

    #[test]
    fn consuming_a_region_keeps_shared_edges() {
        let mut s = sketch();
        let a = s.add_rectangle(DVec2::ZERO, DVec2::ONE).unwrap()[0];
        let b = s.add_rectangle(v(1.0, 0.0), DVec2::ONE).unwrap()[0];
        s.consume_region(a).unwrap();
        assert_eq!(s.region_count(), 1);
        assert!(s.region(b).is_some(), "the neighbour keeps its id");
        assert_eq!(s.edge_count(), 4, "the shared edge stays for the neighbour");
        assert_eq!(s.vertex_count(), 4);
    }

    #[test]
    fn refusals_are_typed_and_leave_the_sketch_alone() {
        let mut s = sketch();
        s.add_rectangle(DVec2::ZERO, DVec2::splat(2.0)).unwrap();
        let before = (s.vertex_count(), s.edge_count(), s.region_count());
        assert_eq!(
            s.add_rectangle(DVec2::ZERO, v(0.0, 1.0)),
            Err(SketchError::Degenerate)
        );
        assert_eq!(
            s.add_rectangle(DVec2::ZERO, v(f64::NAN, 1.0)),
            Err(SketchError::OutOfBounds)
        );
        assert_eq!(
            s.add_rectangle(DVec2::ZERO, v(1e5, 1.0)),
            Err(SketchError::OutOfBounds)
        );
        assert_eq!(
            s.insert_segment(v(0.5, 0.5), v(0.5, 0.5 + 1e-6)),
            Err(SketchError::Degenerate)
        );
        // Both ends merge into the same vertex: zero length after merging.
        assert_eq!(
            s.insert_segment(v(-6e-6, 0.0), v(6e-6, 0.0)),
            Err(SketchError::Degenerate)
        );
        assert_eq!((s.vertex_count(), s.edge_count(), s.region_count()), before);
    }
}
