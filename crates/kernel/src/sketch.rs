//! The sticky tier: a persistent planar graph on a plane (ADR-0001).
//!
//! # Insertion rules
//!
//! Every edit is a set of straight segments inserted with [`Sketch::insert_segment`] (a
//! rectangle is four of them, applied as one transaction). For each segment:
//!
//! 1. An endpoint within [`SKETCH_MERGE`] of an existing vertex merges into it.
//! 2. Otherwise an endpoint within `SKETCH_MERGE` of an edge's interior splits that edge
//!    there (T-junction); the new vertex sits on the edge's line so the edge stays straight.
//! 3. Existing vertices within `SKETCH_MERGE` of the new segment split it (the other kind of
//!    T-junction, and the ends of collinear overlaps).
//! 4. Proper crossings split both segments at the crossing point.
//! 5. Pieces that duplicate an existing edge are skipped, so collinear overlaps normalise
//!    to non-overlapping edges.
//!
//! (The merge distance is [`SKETCH_MERGE`]: the kernel's `POINT_MERGE` plus the 1 nm noise
//! floor, so what a sketch keeps still clears `POINT_MERGE` when a solid made from it is
//! measured in world coordinates.)
//!
//! The graph invariant afterwards ([`Sketch::check`]): every edge is longer than
//! `SKETCH_MERGE`, no two edges cross or overlap, and no vertex lies within `SKETCH_MERGE`
//! of an edge it is not an end of. Only genuinely degenerate input is refused: a segment
//! shorter than `SKETCH_MERGE` (before or after merging) or a non-finite / out-of-range
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

use crate::curve::{Curve, CurveKind, RawCurve, Round, check_round, fit_angles};
use crate::error::SketchError;
use crate::ids::{RegionId, SEdgeId, SVertexId};
use crate::plane::Plane;
use crate::tolerance::{SKETCH_MERGE, in_bounds};
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

/// A sketch as plain data (see [`Sketch::to_raw`]).
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct RawSketch {
    /// Vertex positions in the sketch plane's frame (metres).
    pub vertices: Vec<[f64; 2]>,
    /// Edges as vertex index pairs.
    pub edges: Vec<[u32; 2]>,
    pub regions: Vec<RawRegion>,
    /// Lane 23: the sketch's curves (absent when it has none, so older files read as they
    /// were), and the next id one gets (absent: one past the largest).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curves: Vec<crate::curve::RawCurve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_curve: Option<u64>,
}

/// A region as plain data: rings as vertex indices, a point inside, its flags.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct RawRegion {
    pub outer: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<u32>>,
    pub anchor: [f64; 2],
    #[serde(default)]
    pub pinched: bool,
    #[serde(default)]
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

/// Intersection point of segments ab and cd (callers have established a proper crossing).
///
/// The parameter along ab comes from the two robust orientations of a and b against cd:
/// a proper crossing puts them on opposite sides, so `t = o_a / (o_a - o_b)` divides by the
/// sum of two magnitudes and lies in [0, 1] whatever the angle. (Lane 22: the line form,
/// `(c - a) x s / (r x s)`, divided by a cross product that rounded to zero for two nearly
/// parallel edges the robust predicate still saw crossing, and put a vertex at infinity;
/// a turned sketch moved onto its own copy found it.)
pub(crate) fn line_intersection(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> DVec2 {
    let (oa, ob) = (orient(c, d, a), orient(c, d, b));
    let den = oa - ob;
    let t = if den != 0.0 && (oa / den).is_finite() {
        (oa / den).clamp(0.0, 1.0)
    } else {
        0.5
    };
    a + (b - a) * t
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

/// The faces of a planar embedding as closed circuits of vertices, each with its face on the
/// left. `adj` lists every vertex's neighbours sorted counter-clockwise around it (each edge
/// once from each end). The next edge after `u -> v` is the one leaving `v` just clockwise of
/// `v -> u`, so a walk turns as far left as it can and traces one face. Shared by the
/// sketch's region detection and the section cap's arrangement ([`crate::arrangement`]).
pub(crate) fn walk_circuits<V: Ord + Copy>(adj: &BTreeMap<V, Vec<V>>) -> Vec<Vec<V>> {
    // next(u->v) = the edge leaving v just clockwise of (v->u): keeps the face on the left.
    let next = |u: V, v: V| -> V {
        let nbrs = &adj[&v];
        let i = nbrs.iter().position(|&w| w == u).expect("twin exists");
        nbrs[(i + nbrs.len() - 1) % nbrs.len()]
    };
    let mut visited: BTreeSet<(V, V)> = BTreeSet::new();
    let mut circuits = Vec::new();
    let limit = adj.values().map(Vec::len).sum::<usize>() + 2;
    for (&u0, ns) in adj {
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

/// A sketch's graph: plane, vertices, edges and regions (see [`Sketch`]).
#[derive(Debug, Clone)]
pub struct SketchData {
    plane: Plane,
    vertices: SlotMap<SVertexId, DVec2>,
    edges: SlotMap<SEdgeId, [SVertexId; 2]>,
    regions: SlotMap<RegionId, Region>,
    /// Lane 23: curves by id (see [`crate::curve`]), and the id the next one gets (always
    /// above every id in use).
    curves: BTreeMap<u64, Curve>,
    next_curve: u64,
    /// [`Sketch::check`]'s answer, once worked out (dropped by any change).
    checked: std::sync::OnceLock<Result<(), String>>,
}

/// A sketch. Lane 16: a shared, copy-on-write handle to its graph, so copies of the model
/// (each edit, the undo ring) share a sketch until it changes, and its invariants are
/// checked once per change instead of on every edit anywhere in the model (the check
/// compares every pair of edges).
#[derive(Debug, Clone)]
pub struct Sketch {
    inner: std::sync::Arc<SketchData>,
    /// The group or component instance it was drawn in (`None`: the model). Strokes only
    /// stick to strokes of the same editing context (see [`crate::nodes`]).
    pub(crate) parent: Option<crate::ids::NodeId>,
}

impl std::ops::Deref for Sketch {
    type Target = SketchData;
    fn deref(&self) -> &SketchData {
        &self.inner
    }
}

impl std::ops::DerefMut for Sketch {
    fn deref_mut(&mut self) -> &mut SketchData {
        let d = std::sync::Arc::make_mut(&mut self.inner);
        d.checked = std::sync::OnceLock::new();
        d
    }
}

/// Edge boxes binned in a uniform grid (lane 16): for each edge, the edges after it whose
/// boxes come within the merge distance, ascending.
struct EdgeGrid {
    cell: f64,
    origin: DVec2,
    bins: std::collections::HashMap<(i64, i64), Vec<u32>>,
    big: Vec<u32>,
    boxes: Vec<(DVec2, DVec2)>,
}

impl EdgeGrid {
    fn new(boxes: &[(DVec2, DVec2)]) -> Option<EdgeGrid> {
        if boxes.len() < 64 {
            return None;
        }
        let lo = boxes
            .iter()
            .fold(DVec2::splat(f64::INFINITY), |m, b| m.min(b.0));
        let mut sides: Vec<f64> = boxes.iter().map(|b| (b.1 - b.0).max_element()).collect();
        if !lo.is_finite() || sides.iter().any(|x| !x.is_finite()) {
            return None;
        }
        let mid = sides.len() / 2;
        sides.select_nth_unstable_by(mid, f64::total_cmp);
        let cell = (sides[mid] * 2.0).max(SKETCH_MERGE * 1e3);
        let mut g = EdgeGrid {
            cell,
            origin: lo - DVec2::splat(cell),
            bins: std::collections::HashMap::new(),
            big: Vec::new(),
            boxes: boxes.to_vec(),
        };
        for (i, b) in boxes.iter().enumerate() {
            let (a, z) = (
                g.at(b.0 - DVec2::splat(SKETCH_MERGE)),
                g.at(b.1 + DVec2::splat(SKETCH_MERGE)),
            );
            if (z.0 - a.0 + 1) * (z.1 - a.1 + 1) > 64 {
                g.big.push(i as u32);
                continue;
            }
            for x in a.0..=z.0 {
                for y in a.1..=z.1 {
                    g.bins.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        Some(g)
    }

    fn at(&self, p: DVec2) -> (i64, i64) {
        let q = (p - self.origin) / self.cell;
        (q.x.floor() as i64, q.y.floor() as i64)
    }

    fn near(&self, i: usize) -> Vec<usize> {
        let b = self.boxes[i];
        let (a, z) = (
            self.at(b.0 - DVec2::splat(SKETCH_MERGE)),
            self.at(b.1 + DVec2::splat(SKETCH_MERGE)),
        );
        let mut out: Vec<u32> = self
            .big
            .iter()
            .copied()
            .filter(|&j| j as usize > i)
            .collect();
        if (z.0 - a.0 + 1).saturating_mul(z.1 - a.1 + 1) > 4096 {
            return (i + 1..self.boxes.len()).collect();
        }
        for x in a.0..=z.0 {
            for y in a.1..=z.1 {
                if let Some(v) = self.bins.get(&(x, y)) {
                    out.extend(v.iter().copied().filter(|&j| j as usize > i));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out.into_iter().map(|j| j as usize).collect()
    }
}

/// The first pair (i, j), i < j, of points within the merge distance, in index order, or
/// None (lane 16: a sweep in x instead of every pair).
fn first_close_pair(verts: &[(SVertexId, DVec2)]) -> Option<(usize, usize)> {
    let mut order: Vec<usize> = (0..verts.len()).collect();
    order.sort_by(|&a, &b| verts[a].1.x.total_cmp(&verts[b].1.x).then(a.cmp(&b)));
    let mut best: Option<(usize, usize)> = None;
    for (n, &i) in order.iter().enumerate() {
        let pi = verts[i].1;
        for &j in &order[n + 1..] {
            let pj = verts[j].1;
            if pj.x - pi.x > SKETCH_MERGE {
                break;
            }
            if pi.distance(pj) <= SKETCH_MERGE {
                let pair = (i.min(j), i.max(j));
                if best.is_none_or(|b| pair < b) {
                    best = Some(pair);
                }
            }
        }
    }
    best
}

/// Points binned in a uniform grid (lane 16): the points whose cells meet a box grown by the
/// merge distance, ascending. Few points are simply all returned.
struct PointGrid {
    cell: f64,
    origin: DVec2,
    bins: std::collections::HashMap<(i64, i64), Vec<u32>>,
    all: usize,
}

impl PointGrid {
    fn new(pts: &[DVec2]) -> PointGrid {
        let mut g = PointGrid {
            cell: 1.0,
            origin: DVec2::ZERO,
            bins: std::collections::HashMap::new(),
            all: pts.len(),
        };
        if pts.len() < 64 {
            return g;
        }
        let lo = pts
            .iter()
            .fold(DVec2::splat(f64::INFINITY), |m, p| m.min(*p));
        let hi = pts
            .iter()
            .fold(DVec2::splat(f64::NEG_INFINITY), |m, p| m.max(*p));
        if !(lo.is_finite() && hi.is_finite()) {
            return g;
        }
        // About one point a cell on average over the occupied area.
        let span = (hi - lo).max_element().max(SKETCH_MERGE);
        g.cell = (span / (pts.len() as f64).sqrt()).max(SKETCH_MERGE * 1e3);
        g.origin = lo - DVec2::splat(g.cell);
        for (i, p) in pts.iter().enumerate() {
            let c = g.at(*p);
            g.bins.entry(c).or_default().push(i as u32);
        }
        g.all = 0;
        g
    }

    fn at(&self, p: DVec2) -> (i64, i64) {
        let q = (p - self.origin) / self.cell;
        (q.x.floor() as i64, q.y.floor() as i64)
    }

    fn near(&self, lo: DVec2, hi: DVec2, pts: &[DVec2]) -> Vec<usize> {
        if self.all > 0 {
            return (0..self.all).collect();
        }
        let (a, z) = (
            self.at(lo - DVec2::splat(SKETCH_MERGE)),
            self.at(hi + DVec2::splat(SKETCH_MERGE)),
        );
        if (z.0 - a.0 + 1).saturating_mul(z.1 - a.1 + 1) > 4096 {
            return (0..pts.len()).collect();
        }
        let mut out: Vec<usize> = Vec::new();
        for x in a.0..=z.0 {
            for y in a.1..=z.1 {
                if let Some(v) = self.bins.get(&(x, y)) {
                    out.extend(v.iter().map(|&i| i as usize));
                }
            }
        }
        out.sort_unstable();
        out
    }
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
            inner: std::sync::Arc::new(SketchData {
                plane,
                vertices: SlotMap::with_key(),
                edges: SlotMap::with_key(),
                regions: SlotMap::with_key(),
                curves: BTreeMap::new(),
                next_curve: 1,
                checked: std::sync::OnceLock::new(),
            }),
            parent: None,
        }
    }

    /// Whether two handles share one sketch's graph.
    pub fn shares_with(&self, other: &Sketch) -> bool {
        std::sync::Arc::ptr_eq(&self.inner, &other.inner)
    }

    // ---- queries -----------------------------------------------------------------------

    /// Bytes the sketch takes (estimated): what the undo history is charged for it.
    pub fn approx_bytes(&self) -> usize {
        512 + self.vertices.len() * 40
            + self.edges.len() * 32
            + self
                .regions
                .values()
                .map(|r| {
                    128 + r.outer.len() * 8
                        + r.holes.iter().map(|h| 24 + h.len() * 8).sum::<usize>()
                })
                .sum::<usize>()
            + self
                .curves
                .values()
                .map(|c| 96 + c.chain.len() * 8)
                .sum::<usize>()
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
        if size.x.abs() <= SKETCH_MERGE || size.y.abs() <= SKETCH_MERGE {
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
        next.settle();
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

    /// Erase a region: every edge of its rings leaves the sketch unless it is on another
    /// region's outer ring (live or consumed), healing the junctions it leaves. So erasing
    /// one of two touching rectangles keeps the other whole, erasing a region that sits in
    /// a hole of another fills that hole, and a region inside this one's hole survives.
    /// Returns the live regions that are new or changed.
    pub fn erase_region(&mut self, id: RegionId) -> Result<Vec<RegionId>, SketchError> {
        let r = self
            .regions
            .get(id)
            .ok_or(SketchError::NoSuchRegion(crate::ids::to_public(id)))?;
        if r.consumed {
            return Err(SketchError::RegionConsumed(crate::ids::to_public(id)));
        }
        let mine = r.boundary_edges();
        let others: BTreeSet<(SVertexId, SVertexId)> = self
            .regions
            .iter()
            .filter(|(k, _)| *k != id)
            .flat_map(|(_, o)| {
                (0..o.outer.len()).map(|i| {
                    let (a, b) = (o.outer[i], o.outer[(i + 1) % o.outer.len()]);
                    (a.min(b), a.max(b))
                })
            })
            .collect();
        let doomed: Vec<(SVertexId, SVertexId)> =
            mine.into_iter().filter(|e| !others.contains(e)).collect();
        if doomed.is_empty() {
            return Err(SketchError::NothingToErase);
        }
        // Remove them all at once (erasing one at a time could heal a junction and fuse a
        // doomed edge with a kept one before its turn came).
        let mut next = self.clone();
        let mut ends: BTreeSet<SVertexId> = BTreeSet::new();
        for (u, w) in doomed {
            if let Some(e) = next.edge_between(u, w) {
                next.edges.remove(e);
                ends.insert(u);
                ends.insert(w);
            }
        }
        next.remove_isolated_vertices();
        for v in ends {
            next.heal_vertex(v);
        }
        next.settle();
        next.debug_check("erase_region");
        let changed = next.changed_regions(self);
        *self = next;
        Ok(changed)
    }

    /// Erase every edge lying along segment `a`-`b` (both ends within `SKETCH_MERGE` of it),
    /// drop vertices left without edges, and heal vertices the erase left joining exactly two
    /// collinear edges (a T-junction or crossing whose other stroke is gone), so splitting
    /// then erasing is the identity. Returns the live regions that are new or changed.
    pub fn erase_segment(&mut self, a: DVec2, b: DVec2) -> Result<Vec<RegionId>, SketchError> {
        if !(in_bounds(a.x) && in_bounds(a.y) && in_bounds(b.x) && in_bounds(b.y)) {
            return Err(SketchError::OutOfBounds);
        }
        if a.distance(b) <= SKETCH_MERGE {
            return Err(SketchError::Degenerate);
        }
        let mut next = self.clone();
        let doomed: Vec<(SEdgeId, [SVertexId; 2])> = next
            .edges
            .iter()
            .filter(|(_, [p, q])| {
                point_segment_distance(next.vertices[*p], a, b) <= SKETCH_MERGE
                    && point_segment_distance(next.vertices[*q], a, b) <= SKETCH_MERGE
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
        next.settle();
        next.debug_check("erase_segment");
        let changed = next.changed_regions(self);
        *self = next;
        Ok(changed)
    }

    /// Remove `v` when it joins exactly two edges that run straight through it. A curve's
    /// point stays (lane 23: it is a point of the curve, and the curve's chain names it).
    fn heal_vertex(&mut self, v: SVertexId) {
        let Some(&pv) = self.vertices.get(v) else {
            return;
        };
        if self.curves.values().any(|c| c.chain.contains(&v)) {
            return;
        }
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
        if t <= 0.0 || t >= 1.0 || point_segment_distance(pv, pu, pw) > SKETCH_MERGE {
            return;
        }
        if self.edge_between(u, w).is_some() {
            return;
        }
        // The healed edge must keep the graph's rules (lane 23): `v` may sit up to the merge
        // distance off the straight line, so another point between the merge distance and
        // twice it from the two edges can be within it of their replacement, and an edge
        // can cross it. Then `v` stays (healing is tidiness, never needed). The fuzzer found
        // it: a 0.1 mm circle erased beside its copy moved 0.14 mm.
        let clear = self.vertices.iter().all(|(x, px)| {
            x == u || x == w || x == v || point_segment_distance(*px, pu, pw) > SKETCH_MERGE
        }) && self.edges.iter().all(|(e, [a, b])| {
            e == e1
                || e == e2
                || [*a, *b].iter().any(|&y| y == u || y == w)
                || !proper_cross(pu, pw, self.vertices[*a], self.vertices[*b])
        });
        if !clear {
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
    /// The number of segments of a polyline the sketch would take, or why not.
    fn polyline_segments(points: &[DVec2], closed: bool) -> Result<usize, SketchError> {
        if points.len() < 2 {
            return Err(SketchError::Degenerate);
        }
        if !points.iter().all(|p| in_bounds(p.x) && in_bounds(p.y)) {
            return Err(SketchError::OutOfBounds);
        }
        let n = points.len();
        if closed && n < 3 {
            return Err(SketchError::Degenerate);
        }
        Ok(if closed { n } else { n - 1 })
    }

    /// Lane 16 (C3): insert many polylines as one transaction. The graph is built, repaired
    /// once and its regions detected once; an import of 5,000 lines used to copy the sketch,
    /// repair it and detect every region once per line. All or nothing: the first stroke the
    /// sketch refuses refuses them all (the caller then inserts them one at a time).
    pub fn insert_polylines(&mut self, strokes: &[(Vec<DVec2>, bool)]) -> Result<(), SketchError> {
        let mut next = self.clone();
        for (points, closed) in strokes {
            let segs = Self::polyline_segments(points, *closed)?;
            let n = points.len();
            for i in 0..segs {
                next.add_segment(points[i], points[(i + 1) % n])?;
            }
        }
        next.repair()?;
        next.settle();
        next.debug_check("insert");
        *self = next;
        Ok(())
    }

    fn with_polyline(
        &self,
        points: &[DVec2],
        closed: bool,
    ) -> Result<(Sketch, Vec<(SVertexId, SVertexId)>), SketchError> {
        let segs = Self::polyline_segments(points, closed)?;
        let n = points.len();
        let mut next = self.clone();
        let mut new_edges = Vec::new();
        for i in 0..segs {
            new_edges.extend(next.add_segment(points[i], points[(i + 1) % n])?);
        }
        next.repair()?;
        new_edges.retain(|&(a, b)| next.edge_between(a, b).is_some());
        next.settle();
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
            .filter(|(d, _)| *d <= SKETCH_MERGE)
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
                (d <= SKETCH_MERGE).then_some((d, e))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        if let Some((_, e)) = on_edge {
            let [a, b] = self.edges[e];
            let (pa, pb) = (self.vertices[a], self.vertices[b]);
            let ab = pb - pa;
            let t = ((p - pa).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            let proj = pa + ab * t;
            // Never create a piece shorter than the merge distance.
            if proj.distance(pa) <= SKETCH_MERGE {
                return a;
            }
            if proj.distance(pb) <= SKETCH_MERGE {
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
        if a.distance(b) <= SKETCH_MERGE {
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
                *v != p && *v != q && point_segment_distance(**pos, pp, qq) <= SKETCH_MERGE
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
            // A crossing within the merge distance of the crossed edge's end is that end:
            // the new segment passes through the existing vertex (bending by less than the
            // merge distance) instead of leaving a sliver piece next to it.
            let [c, dd] = self.edges[e];
            if let Some(end) = [c, dd]
                .into_iter()
                .find(|&w| self.vertices[w].distance(x) <= SKETCH_MERGE)
            {
                cuts.push((param(self.vertices[end]), end));
                last = Some((self.vertices[end], end));
                continue;
            }
            if x.distance(pp) <= SKETCH_MERGE || x.distance(qq) <= SKETCH_MERGE {
                continue;
            }
            let v = match last {
                Some((lx, lv)) if lx.distance(x) <= SKETCH_MERGE => {
                    // Re-route this edge through the shared vertex. A half that already
                    // exists (two crossed edges sharing an end) is the same edge now:
                    // keep one, so near-coincident pieces normalise like overlaps.
                    let [c, dd] = self.edges[e];
                    self.edges.remove(e);
                    for (u, w) in [(c, lv), (lv, dd)] {
                        if u != w && self.edge_between(u, w).is_none() {
                            self.edges.insert([u, w]);
                        }
                    }
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

    /// Settle tolerance clusters the per-segment rules leave behind (a crossing that lands
    /// within the merge distance of an unrelated edge, two new vertices closer than the merge
    /// distance): merge vertices closer than `SKETCH_MERGE`, split an edge at a vertex
    /// within `SKETCH_MERGE` of it, split proper crossings, repeat until the graph satisfies
    /// [`Sketch::check`]'s edge rules. Each step moves geometry by less than the merge
    /// distance. A cluster that does not settle is refused as degenerate input.
    fn repair(&mut self) -> Result<(), SketchError> {
        for _ in 0..64 {
            if !self.repair_step() {
                return Ok(());
            }
        }
        Err(SketchError::Degenerate)
    }

    /// One repair (lane 16: each of the three searches looks only near where it looks, through
    /// a sweep or a grid, instead of at every pair; each still finds the first case in the
    /// order a full scan would, so repairs happen exactly as before).
    fn repair_step(&mut self) -> bool {
        let verts: Vec<(SVertexId, DVec2)> = self.vertices.iter().map(|(k, p)| (k, *p)).collect();
        // 1. Two vertices within the merge distance: the first pair in vertex order.
        if let Some((i, j)) = first_close_pair(&verts) {
            // Lane 23: a curve's point survives a merge with a plain one (its chain names it).
            let on_curve = |v: SVertexId| self.curves.values().any(|c| c.chain.contains(&v));
            let (from, into) = if on_curve(verts[j].0) && !on_curve(verts[i].0) {
                (verts[i].0, verts[j].0)
            } else {
                (verts[j].0, verts[i].0)
            };
            self.merge_vertex(from, into);
            return true;
        }
        let edges: Vec<(SEdgeId, [SVertexId; 2])> =
            self.edges.iter().map(|(e, ab)| (e, *ab)).collect();
        // 2. A vertex on an edge it does not end: the first edge, then its first vertex.
        let pts: Vec<DVec2> = verts.iter().map(|v| v.1).collect();
        let grid = PointGrid::new(&pts);
        for &(e, [a, b]) in &edges {
            let (pa, pb) = (self.vertices[a], self.vertices[b]);
            for k in grid.near(pa.min(pb), pa.max(pb), &pts) {
                let (w, pw) = verts[k];
                if w != a && w != b && point_segment_distance(pw, pa, pb) <= SKETCH_MERGE {
                    self.edges.remove(e);
                    for (u, x) in [(a, w), (w, b)] {
                        if self.edge_between(u, x).is_none() {
                            self.edges.insert([u, x]);
                        }
                    }
                    return true;
                }
            }
        }
        // 3. Two edges crossing: the first pair in edge order.
        let boxes: Vec<(DVec2, DVec2)> = edges
            .iter()
            .map(|&(_, [a, b])| {
                let (pa, pb) = (self.vertices[a], self.vertices[b]);
                (pa.min(pb), pa.max(pb))
            })
            .collect();
        let egrid = EdgeGrid::new(&boxes);
        for i in 0..edges.len() {
            let cands: Vec<usize> = match &egrid {
                Some(g) => g.near(i),
                None => (i + 1..edges.len()).collect(),
            };
            for j in cands {
                let (e1, [a, b]) = edges[i];
                let (e2, [c, d]) = edges[j];
                let (pa, pb, pc, pd) = (
                    self.vertices[a],
                    self.vertices[b],
                    self.vertices[c],
                    self.vertices[d],
                );
                if proper_cross(pa, pb, pc, pd) {
                    let x = self.split_edge(e1, line_intersection(pa, pb, pc, pd));
                    self.edges.remove(e2);
                    for (u, w) in [(c, x), (x, d)] {
                        if self.edge_between(u, w).is_none() {
                            self.edges.insert([u, w]);
                        }
                    }
                    return true;
                }
            }
        }
        false
    }

    /// Re-point `from`'s edges to `into` and drop `from` (self-loops and duplicates go).
    fn merge_vertex(&mut self, from: SVertexId, into: SVertexId) {
        let touched: Vec<(SEdgeId, [SVertexId; 2])> = self
            .edges
            .iter()
            .filter(|(_, ab)| ab.contains(&from))
            .map(|(e, ab)| (e, *ab))
            .collect();
        for (e, [a, b]) in touched {
            self.edges.remove(e);
            let (a, b) = (
                if a == from { into } else { a },
                if b == from { into } else { b },
            );
            if a != b && self.edge_between(a, b).is_none() {
                self.edges.insert([a, b]);
            }
        }
        self.vertices.remove(from);
    }

    // ---- moving geometry (lane 22, ADR-0012) ---------------------------------------------

    /// The vertex at `p` (sketch frame): the nearest one within `SKETCH_MERGE`, ties to the
    /// smaller id.
    pub fn vertex_at(&self, p: DVec2) -> Option<SVertexId> {
        self.vertices
            .iter()
            .map(|(k, q)| (q.distance(p), k))
            .filter(|(d, _)| *d <= SKETCH_MERGE)
            .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)))
            .map(|(_, k)| k)
    }

    /// Whether an edge joins `a` and `b`.
    pub fn has_edge(&self, a: SVertexId, b: SVertexId) -> bool {
        self.edge_between(a, b).is_some()
    }

    /// The vertices that move with region `id` (its rings').
    pub fn region_vertices(&self, id: RegionId) -> Vec<SVertexId> {
        self.regions
            .get(id)
            .map(|r| {
                std::iter::once(&r.outer)
                    .chain(r.holes.iter())
                    .flatten()
                    .copied()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Move vertices within the sketch's plane: `to` holds each moving vertex's new
    /// position (sketch frame). Every edge touching a moving vertex is taken out and drawn
    /// again between its ends' new positions with the insertion rules, so it welds onto a
    /// coincident edge, splits where it crosses or touches another, and goes when it has
    /// shrunk to the merge distance (its ends then merge); the neighbours' edges follow the
    /// vertices they share. A vertex keeps its id while it lives, so a region whose rings
    /// only changed shape keeps its id; `anchor` maps the anchor of a region whose every
    /// vertex moved (a region moved whole keeps its id through a split). Returns the
    /// sketch and the live regions bounded by any redrawn edge, sorted.
    pub(crate) fn with_moved_vertices(
        &self,
        to: &BTreeMap<SVertexId, DVec2>,
        anchor: &dyn Fn(DVec2) -> DVec2,
        round: &dyn Fn(&Round) -> Option<(DVec2, f64)>,
    ) -> Result<(Sketch, Vec<RegionId>), SketchError> {
        for p in to.values() {
            if !(p.is_finite() && in_bounds(p.x) && in_bounds(p.y)) {
                return Err(SketchError::OutOfBounds);
            }
        }
        let mut next = self.clone();
        // Lane 23: curves moved whole keep their circle (mapped); bent ones are polylines.
        let moving: BTreeSet<SVertexId> = to.keys().copied().collect();
        next.map_curves(&moving, round);
        let taken: Vec<(SEdgeId, [SVertexId; 2])> = next
            .edges
            .iter()
            .filter(|(_, [a, b])| to.contains_key(a) || to.contains_key(b))
            .map(|(e, ab)| (e, *ab))
            .collect();
        for (e, _) in &taken {
            next.edges.remove(*e);
        }
        for (&v, &p) in to {
            if let Some(q) = next.vertices.get_mut(v) {
                *q = p;
            }
        }
        for r in next.regions.values_mut() {
            let whole = std::iter::once(&r.outer)
                .chain(r.holes.iter())
                .flatten()
                .all(|v| to.contains_key(v));
            if whole {
                r.anchor = anchor(r.anchor);
            }
        }
        let mut made = Vec::new();
        for (_, [a, b]) in taken {
            let (pa, pb) = (next.vertices[a], next.vertices[b]);
            if pa.distance(pb) <= SKETCH_MERGE {
                continue; // shrunk to nothing: its ends merge in the repair
            }
            match next.add_segment(pa, pb) {
                Ok(e) => made.extend(e),
                // Both ends snapped onto one vertex: nothing left of it.
                Err(SketchError::Degenerate) => {}
                Err(e) => return Err(e),
            }
        }
        next.remove_isolated_vertices();
        next.repair()?;
        made.retain(|&(a, b)| next.edge_between(a, b).is_some());
        next.settle();
        next.debug_check("move");
        let touched = next.live_regions_touching(&made);
        Ok((next, touched))
    }

    /// Draw segments (sketch frame) as one transaction with the insertion rules, skipping
    /// any no longer than the merge distance (a copy can shrink an edge that way when it is
    /// projected into a plane). Returns the sketch and the live regions bounded by any new
    /// edge, sorted; `None` when nothing was drawn (every segment was too short, or each one
    /// lay exactly on an edge already there).
    pub(crate) fn with_segments(
        &self,
        segs: &[(DVec2, DVec2)],
    ) -> Result<Option<(Sketch, Vec<RegionId>)>, SketchError> {
        let mut next = self.clone();
        let mut made = Vec::new();
        for &(a, b) in segs {
            if !(a.is_finite() && b.is_finite())
                || !(in_bounds(a.x) && in_bounds(a.y) && in_bounds(b.x) && in_bounds(b.y))
            {
                return Err(SketchError::OutOfBounds);
            }
            if a.distance(b) <= SKETCH_MERGE {
                continue;
            }
            match next.add_segment(a, b) {
                Ok(e) => made.extend(e),
                Err(SketchError::Degenerate) => {}
                Err(e) => return Err(e),
            }
        }
        if made.is_empty() {
            return Ok(None);
        }
        next.repair()?;
        made.retain(|&(a, b)| next.edge_between(a, b).is_some());
        next.settle();
        next.debug_check("copy");
        let touched = next.live_regions_touching(&made);
        Ok(Some((next, touched)))
    }

    /// Take edges out of the sketch (geometry lifted off its plane): every edge in `carried`
    /// goes unless it bounds a region that stays (one not in `moving`, live or consumed),
    /// which keeps it. Junctions left joining two straight edges heal, as an erase does.
    pub(crate) fn without_edges(
        &self,
        carried: &BTreeSet<(SVertexId, SVertexId)>,
        moving: &BTreeSet<RegionId>,
    ) -> Sketch {
        let mut keep: BTreeSet<(SVertexId, SVertexId)> = BTreeSet::new();
        for (id, r) in &self.regions {
            if !moving.contains(&id) {
                keep.extend(r.boundary_edges());
            }
        }
        let mut next = self.clone();
        let mut ends: BTreeSet<SVertexId> = BTreeSet::new();
        for &(a, b) in carried {
            if keep.contains(&(a.min(b), a.max(b))) {
                continue;
            }
            if let Some(e) = next.edge_between(a, b) {
                next.edges.remove(e);
                ends.insert(a);
                ends.insert(b);
            }
        }
        next.remove_isolated_vertices();
        for v in ends {
            next.heal_vertex(v);
        }
        next.settle();
        next.debug_check("lift");
        next
    }

    /// Live regions bounded by any of `edges`, sorted.
    fn live_regions_touching(&self, edges: &[(SVertexId, SVertexId)]) -> Vec<RegionId> {
        let mut out = self.regions_touching(edges);
        out.retain(|id| self.regions.get(*id).is_some_and(|r| !r.consumed));
        out
    }

    // ---- curves (lane 23, ADR-0013) ------------------------------------------------------

    /// After an edit: regions re-detected, then curves settled against the graph.
    fn settle(&mut self) {
        self.recompute_regions();
        self.settle_curves();
    }

    /// The curves, by id.
    pub fn curves(&self) -> impl Iterator<Item = (u64, &Curve)> + '_ {
        self.curves.iter().map(|(k, c)| (*k, c))
    }

    pub fn curve(&self, id: u64) -> Option<&Curve> {
        self.curves.get(&id)
    }

    pub fn curve_count(&self) -> usize {
        self.curves.len()
    }

    /// The id the next curve gets.
    pub fn next_curve_id(&self) -> u64 {
        self.next_curve
    }

    /// Each curve edge's curve, by vertex pair (smaller id first).
    pub fn edge_curves(&self) -> BTreeMap<(SVertexId, SVertexId), u64> {
        let mut out = BTreeMap::new();
        for (&id, c) in &self.curves {
            for (a, b) in c.pairs() {
                out.insert((a.min(b), a.max(b)), id);
            }
        }
        out
    }

    /// The curves a vertex is a point of.
    pub fn curves_at(&self, v: SVertexId) -> Vec<u64> {
        self.curves
            .iter()
            .filter(|(_, c)| c.chain.contains(&v))
            .map(|(k, _)| *k)
            .collect()
    }

    /// A curve's points in order (sketch frame).
    pub fn curve_points(&self, id: u64) -> Option<Vec<DVec2>> {
        let c = self.curves.get(&id)?;
        c.chain
            .iter()
            .map(|&v| self.vertices.get(v).copied())
            .collect()
    }

    /// How the UI shows curve `id` (world coordinates).
    pub fn curve_info(&self, id: u64) -> Option<crate::curve::CurveInfo> {
        let c = self.curves.get(&id)?;
        let pts = self.curve_points(id)?;
        let p = &self.plane;
        Some(crate::curve::CurveInfo {
            kind: c.kind,
            closed: c.closed,
            segments: c.shown_segments(),
            centre: c.round.map(|r| p.to_world(r.centre).to_array()),
            radius: c.round.map(|r| r.radius),
            sweep: c.round.map(|r| r.sweep),
            normal: p.normal.to_array(),
            points: pts.iter().map(|&q| p.to_world(q).to_array()).collect(),
        })
    }

    /// The vertex nearest `p` within `reach`, ties to the smaller id.
    fn vertex_near(&self, p: DVec2, reach: f64) -> Option<SVertexId> {
        self.vertices
            .iter()
            .map(|(k, q)| (q.distance(p), k))
            .filter(|(d, _)| *d <= reach)
            .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)))
            .map(|(_, k)| k)
    }

    /// The vertices strictly between `u` and `w` along a straight run of edges from one to
    /// the other, `w` included (just `[w]` when an edge joins them): a curve segment another
    /// edge split. `None` when no such run exists.
    fn link(
        &self,
        u: SVertexId,
        w: SVertexId,
        adj: &BTreeMap<SVertexId, Vec<SVertexId>>,
    ) -> Option<Vec<SVertexId>> {
        let nbrs = adj.get(&u)?;
        if nbrs.contains(&w) {
            return Some(vec![w]);
        }
        let (pu, pw) = (*self.vertices.get(u)?, *self.vertices.get(w)?);
        let d = pw - pu;
        let len2 = d.length_squared();
        if len2.is_nan() || len2 <= 0.0 {
            return None;
        }
        let param = |p: DVec2| (p - pu).dot(d) / len2;
        let mut out = Vec::new();
        let (mut cur, mut t) = (u, 0.0f64);
        for _ in 0..=self.vertices.len() {
            let next = adj
                .get(&cur)?
                .iter()
                .filter_map(|&x| {
                    let px = *self.vertices.get(x)?;
                    let tx = if x == w { 1.0 } else { param(px) };
                    let along = x == w
                        || (tx < 1.0 && point_segment_distance(px, pu, pw) <= 2.0 * SKETCH_MERGE);
                    (along && tx > t).then_some((tx, x))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))?;
            out.push(next.1);
            if next.1 == w {
                return Some(out);
            }
            (cur, t) = (next.1, next.0);
        }
        None
    }

    /// Settle every curve against the graph (see [`crate::curve`]): split segments taken in,
    /// curves split at junctions, curves that lost an edge broken into polyline pieces, each
    /// edge in one curve at most (the older curve keeps it), angles re-read from the points.
    fn settle_curves(&mut self) {
        if self.curves.is_empty() {
            return;
        }
        let mut adj: BTreeMap<SVertexId, Vec<SVertexId>> = BTreeMap::new();
        for [a, b] in self.edges.values() {
            adj.entry(*a).or_default().push(*b);
            adj.entry(*b).or_default().push(*a);
        }
        let mut claimed: BTreeSet<(SVertexId, SVertexId)> = BTreeSet::new();
        let old = std::mem::take(&mut self.curves);
        let mut next = self.next_curve;
        let mut out: BTreeMap<u64, Curve> = BTreeMap::new();
        for (id, c) in old {
            let (pieces, lost) = self.curve_pieces(&c, &adj, &mut claimed);
            let split = !(pieces.len() == 1 && pieces[0].1 == c.closed);
            let mut first = true;
            for (chain, closed) in pieces {
                let edges = if closed { chain.len() } else { chain.len() - 1 };
                let (kind, mut round, segments) = if lost {
                    (CurveKind::Polyline, None, edges as u32)
                } else if !split {
                    (c.kind, c.round, c.segments)
                } else {
                    match (c.kind, closed) {
                        (CurveKind::Circle | CurveKind::Arc, false) => {
                            (CurveKind::Arc, c.round, edges as u32)
                        }
                        _ => (CurveKind::Polyline, None, edges as u32),
                    }
                };
                let mut kind = kind;
                if let Some(r) = round.as_mut() {
                    let pts: Vec<DVec2> = chain.iter().map(|&v| self.vertices[v]).collect();
                    // A piece whose points no longer fit its circle (a repair bent it, say a
                    // vertex snapped onto one of its chords zigzags it) is a polyline: the
                    // validator's rules hold for every curve settling leaves.
                    let fits = match fit_angles(r.centre, &pts, closed) {
                        Some((start, sweep)) => {
                            r.start = start;
                            r.sweep = sweep;
                            check_round(r, &pts, closed).is_ok()
                        }
                        None => false,
                    };
                    if !fits {
                        kind = CurveKind::Polyline;
                        round = None;
                    }
                }
                let segments = if kind == CurveKind::Polyline {
                    edges as u32
                } else {
                    segments
                };
                let min = match (kind, closed) {
                    (_, true) => 3,
                    (CurveKind::Polyline, false) => 2,
                    _ => 1,
                };
                if edges < min {
                    continue;
                }
                let key = if first {
                    first = false;
                    id
                } else {
                    next += 1;
                    next - 1
                };
                out.insert(
                    key,
                    Curve {
                        kind,
                        chain,
                        closed,
                        segments,
                        round,
                    },
                );
            }
        }
        self.curves = out;
        self.next_curve = next;
    }

    /// The pieces of curve `c` in the graph as it is now (chains, closed), and whether it
    /// lost an edge or a point. Claims the edges it keeps.
    fn curve_pieces(
        &self,
        c: &Curve,
        adj: &BTreeMap<SVertexId, Vec<SVertexId>>,
        claimed: &mut BTreeSet<(SVertexId, SVertexId)>,
    ) -> (Vec<(Vec<SVertexId>, bool)>, bool) {
        let n = c.chain.len();
        let pairs = c.edge_count();
        let key = |a: SVertexId, b: SVertexId| (a.min(b), a.max(b));
        let mut runs: Vec<Vec<SVertexId>> = Vec::new();
        let mut cur: Vec<SVertexId> = Vec::new();
        let mut lost = false;
        let (mut first_ok, mut last_ok) = (false, false);
        for i in 0..pairs {
            let (u, w) = (c.chain[i], c.chain[(i + 1) % n]);
            let path = self.link(u, w, adj).filter(|p| {
                let mut prev = u;
                let mut mine = BTreeSet::new();
                p.iter().all(|&x| {
                    let k = key(prev, x);
                    prev = x;
                    !claimed.contains(&k) && mine.insert(k)
                })
            });
            match path {
                Some(p) => {
                    let mut prev = u;
                    for &x in &p {
                        claimed.insert(key(prev, x));
                        prev = x;
                    }
                    if cur.is_empty() {
                        cur.push(u);
                    }
                    cur.extend(p);
                    if i == 0 {
                        first_ok = true;
                    }
                    if i + 1 == pairs {
                        last_ok = true;
                    }
                }
                None => {
                    lost = true;
                    if cur.len() >= 2 {
                        runs.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
            }
        }
        if cur.len() >= 2 {
            runs.push(cur);
        }
        // Runs with their closedness: a closed curve that kept every edge is one closed run;
        // one that lost some continues from its last run into its first.
        let mut closed_runs: Vec<(Vec<SVertexId>, bool)> = Vec::new();
        if c.closed && !lost && runs.len() == 1 {
            let mut r = runs.pop().unwrap_or_default();
            r.pop(); // back at the first vertex
            closed_runs.push((r, true));
        } else {
            if c.closed && lost && first_ok && last_ok && runs.len() >= 2 {
                let head = runs.remove(0);
                if let Some(tail) = runs.last_mut() {
                    tail.extend(head.into_iter().skip(1));
                }
            }
            closed_runs.extend(runs.into_iter().map(|r| (r, false)));
        }
        // Split at junctions: a chain point where another edge meets the curve.
        let junction = |v: SVertexId| adj.get(&v).map_or(0, Vec::len) != 2;
        let mut pieces: Vec<(Vec<SVertexId>, bool)> = Vec::new();
        for (run, closed) in closed_runs {
            let at: Vec<usize> = if closed {
                (0..run.len()).filter(|&i| junction(run[i])).collect()
            } else {
                (1..run.len().saturating_sub(1))
                    .filter(|&i| junction(run[i]))
                    .collect()
            };
            if closed {
                if at.len() <= 1 {
                    pieces.push((run, true));
                    continue;
                }
                // Start at the first junction, then cut at each.
                let m = run.len();
                let rot: Vec<SVertexId> = (0..=m).map(|k| run[(at[0] + k) % m]).collect();
                let cuts: Vec<usize> = at.iter().map(|&i| (i + m - at[0]) % m).chain([m]).collect();
                for w in cuts.windows(2) {
                    pieces.push((rot[w[0]..=w[1]].to_vec(), false));
                }
            } else {
                let cuts: Vec<usize> = std::iter::once(0)
                    .chain(at)
                    .chain([run.len() - 1])
                    .collect();
                for w in cuts.windows(2) {
                    pieces.push((run[w[0]..=w[1]].to_vec(), false));
                }
            }
        }
        // A piece back where it started is a loop (a lobe of a figure eight).
        for (p, closed) in &mut pieces {
            if !*closed && p.len() >= 4 && p.first() == p.last() {
                p.pop();
                *closed = true;
            }
        }
        pieces.retain(|(p, closed)| {
            let distinct: BTreeSet<&SVertexId> = p.iter().collect();
            distinct.len() == p.len() && p.len() >= if *closed { 3 } else { 2 }
        });
        (pieces, lost)
    }

    /// Lane 23: make the stroke just drawn through `pts` (sketch frame, as drawn; `closed`
    /// as drawn) a curve of `kind` with `segments`, on the circle about `centre` for a round
    /// kind (radius: the farthest drawn point). Its points are found where the insertion put
    /// them (within twice the merge distance); a stroke that did not land whole, or whose
    /// points do not fit the circle, stays plain edges. Settles at once (a curve drawn across
    /// others splits there). Returns the curve's id, or the first piece's.
    pub(crate) fn add_curve(
        &mut self,
        pts: &[DVec2],
        closed: bool,
        kind: CurveKind,
        centre: Option<DVec2>,
        segments: u32,
    ) -> Option<u64> {
        let mut chain: Vec<SVertexId> = Vec::with_capacity(pts.len());
        for &p in pts {
            let v = self.vertex_near(p, 2.0 * SKETCH_MERGE)?;
            if chain.last() != Some(&v) {
                chain.push(v);
            }
        }
        if closed && chain.len() > 1 && chain.first() == chain.last() {
            chain.pop();
        }
        if chain.len() < if closed { 3 } else { 2 } {
            return None;
        }
        let round = match (kind.is_round(), centre) {
            (true, Some(c)) => {
                let radius = pts.iter().map(|p| p.distance(c)).fold(0.0, f64::max);
                let at: Vec<DVec2> = chain.iter().map(|&v| self.vertices[v]).collect();
                let (start, sweep) = fit_angles(c, &at, closed)?;
                Some(Round {
                    centre: c,
                    radius,
                    start,
                    sweep,
                })
            }
            (true, None) => return None,
            (false, _) => None,
        };
        let id = self.next_curve;
        let before = self.curves.clone();
        let next_before = self.next_curve;
        self.next_curve += 1;
        self.curves.insert(
            id,
            Curve {
                kind,
                chain,
                closed,
                segments,
                round,
            },
        );
        self.settle_curves();
        // The new curve's pieces must fit their circle; a stroke that does not (points the
        // insertion moved too far) stays plain edges, never a curve that lies.
        let mine: Vec<u64> = self.curves.keys().copied().filter(|&k| k >= id).collect();
        let fits = mine.iter().all(|k| {
            let c = &self.curves[k];
            c.round.is_none_or(|r| {
                let at: Vec<DVec2> = c.chain.iter().map(|&v| self.vertices[v]).collect();
                check_round(&r, &at, c.closed).is_ok()
            })
        });
        if !fits {
            self.curves = before;
            self.next_curve = next_before;
            return None;
        }
        mine.first().copied()
    }

    /// Lane 23: erase a whole curve (the Eraser on any of its segments): every edge of it
    /// goes, junctions it leaves joining two straight edges heal, regions are re-detected.
    /// Returns the live regions that are new or changed.
    pub fn erase_curve(&mut self, id: u64) -> Result<Vec<RegionId>, SketchError> {
        let c = self.curves.get(&id).ok_or(SketchError::NoSuchCurve(id))?;
        let pairs: Vec<(SVertexId, SVertexId)> = c.pairs().collect();
        let mut next = self.clone();
        next.curves.remove(&id);
        let mut ends: BTreeSet<SVertexId> = BTreeSet::new();
        for (u, w) in pairs {
            if let Some(e) = next.edge_between(u, w) {
                next.edges.remove(e);
                ends.insert(u);
                ends.insert(w);
            }
        }
        next.remove_isolated_vertices();
        for v in ends {
            next.heal_vertex(v);
        }
        next.settle();
        next.debug_check("erase_curve");
        let changed = next.changed_regions(self);
        *self = next;
        Ok(changed)
    }

    /// Lane 23: Explode Curve: the curve's edges stay as plain edges.
    pub fn explode_curve(&mut self, id: u64) -> Result<usize, SketchError> {
        let n = self
            .curves
            .get(&id)
            .ok_or(SketchError::NoSuchCurve(id))?
            .edge_count();
        self.curves.remove(&id);
        self.debug_check("explode_curve");
        Ok(n)
    }

    /// Lane 23: what a move does to the curves before their points move: a curve whose
    /// every point moves takes `map`'s centre and radius (`None` from it: the map is not a
    /// similarity in the plane, and the curve becomes a polyline); a curve only some of
    /// whose points move is bent and becomes a polyline. Angles are re-read by the settle.
    pub(crate) fn map_curves(
        &mut self,
        moving: &BTreeSet<SVertexId>,
        map: &dyn Fn(&Round) -> Option<(DVec2, f64)>,
    ) {
        for c in self.curves.values_mut() {
            let n = c.chain.iter().filter(|v| moving.contains(v)).count();
            if n == 0 {
                continue;
            }
            let mapped = if n == c.chain.len() {
                c.round.and_then(|r| map(&r))
            } else {
                None
            };
            match (c.round.as_mut(), mapped) {
                (Some(r), Some((centre, radius))) => {
                    r.centre = centre;
                    r.radius = radius;
                }
                (Some(_), None) => {
                    c.kind = CurveKind::Polyline;
                    c.round = None;
                    c.segments = c.edge_count() as u32;
                }
                (None, _) => {}
            }
        }
    }

    /// The curves among `carried` edges: every curve whose edges are all carried.
    pub(crate) fn whole_curves(&self, carried: &BTreeSet<(SVertexId, SVertexId)>) -> Vec<u64> {
        self.curves
            .iter()
            .filter(|(_, c)| {
                c.pairs()
                    .all(|(a, b)| carried.contains(&(a.min(b), a.max(b))))
            })
            .map(|(k, _)| *k)
            .collect()
    }

    /// Check the curves (part of [`Sketch::check`]): ids below the counter, chains of
    /// distinct live vertices joined by edges in order (and back round for a closed one), no
    /// edge in two curves, kinds consistent (circles and polygons closed, arcs open, round
    /// kinds with a circle and polylines without), and every round curve's points on its
    /// circle ([`check_round`]).
    fn check_curves(&self, edge_set: &BTreeSet<(SVertexId, SVertexId)>) -> Result<(), String> {
        let mut seen: BTreeSet<(SVertexId, SVertexId)> = BTreeSet::new();
        for (&id, c) in &self.curves {
            if id == 0 || id >= self.next_curve {
                return Err(format!("curve {id} is not below the sketch's counter"));
            }
            if let Some(closes) = c.kind.closes()
                && closes != c.closed
            {
                return Err(format!(
                    "curve {id}: a {} that does not close as one",
                    c.kind.word()
                ));
            }
            if c.kind.is_round() != c.round.is_some() {
                return Err(format!(
                    "curve {id}: a {} with the wrong parameters",
                    c.kind.word()
                ));
            }
            let min = match (c.kind, c.closed) {
                (_, true) => 3,
                (CurveKind::Polyline, false) => 2,
                _ => 1,
            };
            if c.edge_count() < min || c.segments == 0 {
                return Err(format!("curve {id} is too short"));
            }
            let distinct: BTreeSet<&SVertexId> = c.chain.iter().collect();
            if distinct.len() != c.chain.len() {
                return Err(format!("curve {id} passes a point twice"));
            }
            for (a, b) in c.pairs() {
                let k = (a.min(b), a.max(b));
                if !edge_set.contains(&k) {
                    return Err(format!("curve {id} names an edge that is not there"));
                }
                if !seen.insert(k) {
                    return Err(format!("curve {id} shares an edge with another curve"));
                }
            }
            if let Some(r) = &c.round {
                let at: Vec<DVec2> = c.chain.iter().map(|&v| self.vertices[v]).collect();
                check_round(r, &at, c.closed).map_err(|e| format!("curve {id}: {e}"))?;
            }
        }
        Ok(())
    }

    // ---- plain data (the .shz snapshot) -------------------------------------------------

    /// The sketch as plain data: vertices in slot order, edges and region rings as indices
    /// into them, regions with their anchors and flags.
    pub fn to_raw(&self) -> RawSketch {
        let index: BTreeMap<SVertexId, u32> = self
            .vertices
            .keys()
            .enumerate()
            .map(|(i, k)| (k, i as u32))
            .collect();
        let ring = |r: &[SVertexId]| r.iter().map(|v| index[v]).collect::<Vec<u32>>();
        RawSketch {
            vertices: self.vertices.values().map(|p| p.to_array()).collect(),
            edges: self
                .edges
                .values()
                .map(|[a, b]| [index[a], index[b]])
                .collect(),
            regions: self
                .regions
                .values()
                .map(|r| RawRegion {
                    outer: ring(&r.outer),
                    holes: r.holes.iter().map(|h| ring(h)).collect(),
                    anchor: r.anchor.to_array(),
                    pinched: r.pinched,
                    consumed: r.consumed,
                })
                .collect(),
            curves: self
                .curves
                .iter()
                .map(|(&id, c)| RawCurve {
                    id,
                    kind: c.kind,
                    vertices: ring(&c.chain),
                    closed: c.closed,
                    segments: c.segments,
                    centre: c.round.map(|r| r.centre.to_array()),
                    radius: c.round.map(|r| r.radius),
                    start: c.round.map(|r| r.start),
                    sweep: c.round.map(|r| r.sweep),
                })
                .collect(),
            // The counter once a curve was ever made (absent in older files).
            next_curve: (self.next_curve != 1).then_some(self.next_curve),
        }
    }

    /// Rebuild a sketch from plain data, refusing anything the sketch invariants reject or
    /// whose regions do not match the faces its edges actually form.
    pub fn from_raw(plane: Plane, raw: &RawSketch) -> Result<Sketch, String> {
        let mut s = Sketch::new(plane);
        let keys: Vec<SVertexId> = raw
            .vertices
            .iter()
            .map(|p| {
                let q = DVec2::from_array(*p);
                if !(q.is_finite() && in_bounds(q.x) && in_bounds(q.y)) {
                    return Err("sketch vertex out of bounds".to_string());
                }
                Ok(s.vertices.insert(q))
            })
            .collect::<Result<_, _>>()?;
        let key = |i: u32| {
            keys.get(i as usize)
                .copied()
                .ok_or_else(|| format!("sketch index {i} out of range"))
        };
        for e in &raw.edges {
            s.edges.insert([key(e[0])?, key(e[1])?]);
        }
        for r in &raw.regions {
            let ring = |r: &[u32]| -> Result<Vec<SVertexId>, String> {
                let v: Vec<SVertexId> = r.iter().map(|&i| key(i)).collect::<Result<_, _>>()?;
                Ok(canonical(&v))
            };
            let mut holes: Vec<Vec<SVertexId>> =
                r.holes.iter().map(|h| ring(h)).collect::<Result<_, _>>()?;
            holes.sort();
            s.regions.insert(Region {
                outer: ring(&r.outer)?,
                holes,
                anchor: DVec2::from_array(r.anchor),
                pinched: r.pinched,
                consumed: r.consumed,
            });
        }
        let mut last = 0u64;
        for c in &raw.curves {
            if c.id <= last {
                return Err("curve ids must rise".into());
            }
            last = c.id;
            let chain: Vec<SVertexId> = c
                .vertices
                .iter()
                .map(|&i| key(i))
                .collect::<Result<_, _>>()?;
            s.curves.insert(
                c.id,
                Curve {
                    kind: c.kind,
                    chain,
                    closed: c.closed,
                    segments: c.segments,
                    round: c.round()?,
                },
            );
        }
        s.next_curve = match raw.next_curve {
            Some(n) if n > last => n,
            Some(_) => return Err("the curve counter is not above every curve id".into()),
            None => last + 1,
        };
        s.check()?;
        let mut stored: Vec<BoundaryKey> = s.regions.values().map(Region::boundary_key).collect();
        let mut found: Vec<BoundaryKey> =
            s.detect().into_iter().map(|d| (d.outer, d.holes)).collect();
        stored.sort();
        found.sort();
        if stored != found {
            return Err("the sketch's regions do not match its edges".into());
        }
        Ok(s)
    }

    /// This sketch under an affine map (its group moved): the plane frame is mapped and every
    /// vertex re-expressed in it; a mirror reverses the region rings so outers stay
    /// counter-clockwise. Region flags (consumed, pinched) are kept; region ids are renumbered.
    pub(crate) fn mapped(
        &self,
        x: &crate::transform::Xform,
    ) -> Result<Sketch, crate::error::TransformError> {
        use crate::error::TransformError;
        x.check()?;
        let a = x.to_affine();
        let plane = crate::transform::map_plane(&a, &self.plane).ok_or(TransformError::Singular)?;
        let mut raw = self.to_raw();
        let map = |q: [f64; 2]| -> [f64; 2] {
            let w = a.transform_point3(self.plane.to_world(DVec2::from_array(q)));
            plane.to_local(w).to_array()
        };
        for v in &mut raw.vertices {
            *v = map(*v);
        }
        let mirror = x.determinant() < 0.0;
        for r in &mut raw.regions {
            r.anchor = map(r.anchor);
            if mirror {
                r.outer.reverse();
                for h in &mut r.holes {
                    h.reverse();
                }
            }
        }
        // Lane 23: a round curve keeps its circle when the map is a similarity in the plane
        // (the centre mapped, the radius scaled, the angles read again from the points);
        // otherwise it is a polyline now.
        let at = |q: DVec2| plane.to_local(a.transform_point3(self.plane.to_world(q)));
        let scale = crate::curve::similarity_scale(
            at(DVec2::X) - at(DVec2::ZERO),
            at(DVec2::Y) - at(DVec2::ZERO),
        );
        for c in &mut raw.curves {
            let pts: Vec<DVec2> = c
                .vertices
                .iter()
                .filter_map(|&i| raw.vertices.get(i as usize).map(|p| DVec2::from_array(*p)))
                .collect();
            let fitted = match (c.centre, c.radius, scale) {
                (Some(cc), Some(r), Some(k)) => {
                    let centre = DVec2::from_array(map(cc));
                    fit_angles(centre, &pts, c.closed).map(|(s0, w)| (centre, r * k, s0, w))
                }
                _ => None,
            };
            match fitted {
                Some((centre, radius, start, sweep)) => {
                    c.centre = Some(centre.to_array());
                    c.radius = Some(radius);
                    c.start = Some(start);
                    c.sweep = Some(sweep);
                }
                None if c.centre.is_some() => {
                    c.kind = crate::curve::CurveKind::Polyline;
                    c.segments = if c.closed {
                        c.vertices.len() as u32
                    } else {
                        c.vertices.len().saturating_sub(1) as u32
                    };
                    (c.centre, c.radius, c.start, c.sweep) = (None, None, None, None);
                }
                None => {}
            }
        }
        let mut s = Sketch::from_raw(plane, &raw).map_err(TransformError::InvalidResult)?;
        s.parent = self.parent;
        Ok(s)
    }

    /// The group or component instance it belongs to (public id).
    pub fn parent(&self) -> Option<u64> {
        self.parent.map(crate::ids::to_public)
    }

    // ---- invariants --------------------------------------------------------------------

    /// Sketch-graph invariants: every vertex finite and within 10 km; edges join two
    /// distinct vertices more than `SKETCH_MERGE`
    /// apart; no two edges cross, overlap or repeat; no vertex lies within `SKETCH_MERGE` of
    /// an edge it does not end; no isolated vertices; regions have simple CCW outers, simple
    /// CW holes and an anchor strictly inside. Runs in `Model::validate` and, in debug
    /// builds, after every sketch mutation.
    pub fn check(&self) -> Result<(), String> {
        self.inner.checked.get_or_init(|| self.check_now()).clone()
    }

    /// [`Sketch::check`], worked out. The pairwise edge tests look only at edges whose boxes
    /// come within the merge distance of each other (lane 16: through a grid of edge boxes,
    /// once a sketch has more than a few dozen edges).
    fn check_now(&self) -> Result<(), String> {
        // Lane 22: a vertex the snapshot could not hold is a broken sketch here too.
        if let Some(p) = self
            .vertices
            .values()
            .find(|p| !(p.is_finite() && in_bounds(p.x) && in_bounds(p.y)))
        {
            return Err(format!(
                "sketch vertex {p} is not finite or is beyond 10 km"
            ));
        }
        let edges: Vec<[SVertexId; 2]> = self.edges.values().copied().collect();
        for &[a, b] in &edges {
            let (Some(pa), Some(pb)) = (self.vertices.get(a), self.vertices.get(b)) else {
                return Err("edge references a missing vertex".into());
            };
            if a == b || pa.distance(*pb) <= SKETCH_MERGE {
                return Err("zero-length sketch edge".into());
            }
        }
        let used: BTreeSet<SVertexId> = edges.iter().flatten().copied().collect();
        if used.len() != self.vertices.len() {
            return Err("isolated sketch vertex".into());
        }
        // Candidates per edge from a grid of edge boxes, in index order, so pairs are tried
        // in the order a full scan tries them (and the first problem is the same).
        let grid = EdgeGrid::new(
            &edges
                .iter()
                .map(|&[a, b]| {
                    (
                        self.vertices[a].min(self.vertices[b]),
                        self.vertices[a].max(self.vertices[b]),
                    )
                })
                .collect::<Vec<_>>(),
        );
        for i in 0..edges.len() {
            let cands: Vec<usize> = match &grid {
                Some(g) => g.near(i),
                None => (i + 1..edges.len()).collect(),
            };
            for j in cands {
                if j <= i {
                    continue;
                }
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
                            <= SKETCH_MERGE
                    {
                        return Err(format!(
                            "sketch vertex {px} lies on another edge {} - {}",
                            self.vertices[seg.0], self.vertices[seg.1]
                        ));
                    }
                }
            }
        }
        let edge_set: BTreeSet<(SVertexId, SVertexId)> =
            edges.iter().map(|&[a, b]| (a.min(b), a.max(b))).collect();
        self.check_curves(&edge_set)?;
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
        walk_circuits(&out)
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
                .filter(|&a| inside_rings(a, o, h) && ring_clearance(a, o, h) > SKETCH_MERGE);
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
    fn near_coincident_crossings_next_to_a_shared_vertex() {
        // Fuzz find (2026-10-11): a stroke passing 1.6e-5 m from a crossing vertex cuts two
        // of its edges within the merge distance of each other; both crossings share one
        // vertex, and the two pieces back to the crossing vertex become one edge.
        let mut s = sketch();
        s.insert_segment(v(0.250004, 0.0), v(0.000001, 0.75))
            .unwrap();
        s.insert_polyline(
            &[
                v(1.0, -1.25),
                v(-0.25, 1.0),
                v(0.25, 0.250004),
                v(0.0, 0.25),
            ],
            false,
        )
        .unwrap();
        s.check().unwrap();
    }

    #[test]
    fn crossing_next_to_an_edge_end_uses_the_end() {
        // Fuzz find (2026-10-11): a stroke ending at a vertex 3.6e-5 m from another vertex
        // crossed one of that vertex's edges within the merge distance of it.
        let mut s = sketch();
        s.insert_polyline(
            &[
                v(0.0, 0.0),
                v(0.0, 1.25),
                v(-0.5, 2.707424327303603),
                v(1.25, -2.3936860005670186),
            ],
            true,
        )
        .unwrap();
        s.insert_segment(v(-2.224823054476681, -0.25), v(0.000001, 1.25))
            .unwrap();
        s.check().unwrap();
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
