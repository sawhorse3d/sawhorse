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

/// What [`Sketch::with_moved_vertices`] returns: the sketch, the live regions any redrawn
/// edge bounds, and the further pieces of the curves the move broke up, as (piece, curve).
pub(crate) type MovedSketch = (Sketch, Vec<RegionId>, Vec<(u64, u64)>);

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

/// Lane 53 (#65): what a long stroke's insertion looks up, by place: a grid of square cells
/// (side `cell`, at least four merge distances) holding each vertex in its own cell and each
/// edge in the cells of points sampled along it at most half a cell apart
/// ([`sample_keys`]), and a table from each vertex pair to the edge joining it.
///
/// A query looks in the cells around the samples of its point or segment (the eight around
/// each, too). A vertex within the merge distance of a point of the query is within half a
/// cell of one of its samples, so in a cell next to that sample's; an edge within the merge
/// distance of the query, or crossing it, has a point there within a quarter cell of one of
/// its own samples, whose cell is next to the query sample's. So a lookup holds every vertex
/// and edge the full scan would find (and more). Entries are only ever added: a vertex never
/// moves while a stroke goes in, an edge split or taken out keeps its old entries (its pieces
/// lie within the old one), and each candidate is tested on what the sketch holds now. A
/// table entry for an edge taken out, or split, is tested the same way.
///
/// It is a third grid beside the repair's `PointGrid` and `EdgeGrid` because those are built
/// once over a fixed set and answer one question each (the first close pair in scan order);
/// this one grows as the stroke goes in (every split and new edge enters it) and answers
/// several questions about one point or segment at a time.
#[derive(Debug)]
pub(crate) struct Lookup {
    cell: f64,
    verts: std::collections::HashMap<(i64, i64), Vec<SVertexId>>,
    edges: std::collections::HashMap<(i64, i64), Vec<SEdgeId>>,
    pairs: std::collections::HashMap<(SVertexId, SVertexId), SEdgeId>,
    /// Vertex and edge entries held (each a key in a cell's list, stale ones included).
    entries: usize,
    /// Most entries it may hold ([`lookup_ceiling`]); past it the lookup is dropped.
    ceiling: usize,
    /// Queries that came back with more than a quarter of the sketch ([`LOOKUP_POOR`]).
    poor: std::cell::Cell<u32>,
}

/// Lane 53 (#65): a [`Lookup`] whose queries come back with more than a quarter of the
/// sketch this many times, once the sketch holds 4,096 vertices and edges (under that the
/// scans cost little either way), is dropped and the scans take over: its cells are too big
/// for the sketch (an import of 3,000 parallel lines 5 m long and 1 cm apart, in cells of 5 m, took
/// 0.27 s through it and 0.21 s with the scans).
pub const LOOKUP_POOR: u32 = 16;

/// Lane 53 (#65): a stroke is inserted through a [`Lookup`] only when its segments times
/// the sketch's vertices, edges and segments after it come to at least this (a cheap first
/// test: the cost rule, [`LOOKUP_BUILD_COST`], never accepts less than about 72,000); under it the scans
/// are cheaper (measured in a release build: a 65-point zigzag into an empty sketch took
/// 0.24 ms through a lookup and 0.08 ms with the scans, 250 points about the same, 0.9 ms
/// and 0.8 ms; 65 points beside 2,000 edges 13.8 ms against 15.9 ms).
pub const LOOKUP_MIN_WORK: usize = 65_536;

/// Lane 53 (#65): what entering one vertex or edge sample in a [`Lookup`] costs, and what
/// one query costs before it finds anything (finding nine cells), counted in visits of one
/// vertex or edge by the scans; calibrated in a release build. A lookup is built only when
/// its estimated cost, building it and its queries, is at most half the scans'. Beside 250
/// lines 0.52 m long, 3 mm apart, a stroke of 128 or 512 segments of 0.5 mm built one with
/// cells as big as the lines, whose queries found most of them, and took 1.4 times as long
/// as the scans; it now scans.
pub const LOOKUP_BUILD_COST: f64 = 10.0;
pub const LOOKUP_QUERY_COST: f64 = 360.0;

/// Lane 53 (#65): and only for strokes of at least this many segments: building a lookup
/// costs about as much as one pass over the sketch with a large constant, which a stroke of a
/// few dozen segments does not win back (measured in a release build, a lookup against the
/// scans: 16 segments beside 1,920 to 38,400 vertices and edges 1 to 5% slower, 32 segments
/// level, 64 segments 9 to 20% faster, 1,024 segments three to five times faster; the 24
/// segment circle the Circle tool draws always scans).
pub const LOOKUP_MIN_SEGMENTS: usize = 48;

/// Lane 53 (#65): a cell of a [`Lookup`] holding more than this many vertices or edges, and
/// more than an eighth of all it holds, is crowded: the lookup is dropped and the scans take
/// over. A stroke that crosses itself thousands of times packs a few cells with most of the
/// sketch, and a lookup that answers every query with most of the sketch costs more than the
/// scans (a hostile 4,650-point import that ends with 145,088 edges took 30 s through it, 17
/// s with the scans).
pub const LOOKUP_CELL_MAX: usize = 256;

/// Lane 53 (#65): most entries a [`Lookup`] for a sketch of `size` vertices and edges and a
/// stroke of `segs` segments may hold: four for each of them, and 65,536 more. An entry is a
/// key of 8 bytes in a cell's list, a cell (at most one per entry) a slot of about 48 bytes
/// in a map, and each edge a slot in the table of vertex pairs, so a lookup takes at most
/// about 250 bytes per vertex, edge and segment, and usually far less (beside 19,000 lines
/// of 1 m and a stroke of 5,000 segments the process peaked 1.6 MB above the scans'). The
/// lookup it replaced registered each edge in nine cells around every sample, with no such
/// bound: 686 MB beside 19,000 lines of 1 m.
pub fn lookup_ceiling(size: usize, segs: usize) -> usize {
    4usize
        .saturating_mul(size.saturating_add(segs))
        .saturating_add(65_536)
}

/// Lane 53 (#65): samples [`sample_keys`] takes along a segment `len` long in cells of side
/// `cell` (at most half a cell apart, both ends included); `usize::MAX` when not finite or
/// past what a `usize` holds.
fn samples(len: f64, cell: f64) -> usize {
    samples_within(len, cell, usize::MAX)
}

/// [`samples`] with `max` for the largest `usize` (the tests pass `u32::MAX`, as a 32-bit
/// target such as wasm32 has it): a segment of a kilometre in cells of 40 micrometres needs
/// 5e7 samples, and one over about 86 km more than a 32-bit `usize` holds (such a stroke is
/// out of bounds, but an import's lengths are counted before its strokes are checked).
fn samples_within(len: f64, cell: f64, max: usize) -> usize {
    let steps = (len / (0.5 * cell)).ceil();
    if steps.is_finite() && steps >= 0.0 && steps < (max as f64) - 2.0 {
        (steps as usize).saturating_add(2).min(max)
    } else {
        max
    }
}

impl Lookup {
    /// A lookup for inserting `strokes` into `s`, or `None` (insert with the scans) for
    /// strokes whose work is under `min_work` ([`LOOKUP_MIN_WORK`]; the tests that hold the
    /// two ways to one result pass 0), or when no cell size keeps the entries the sketch and
    /// the strokes need within [`lookup_ceiling`]. The entries are counted before anything is
    /// built (one pass over the sketch's edges and the strokes' segments, nothing allocated),
    /// for three cell sizes: the strokes' mean segment, the sketch's mean edge, and between
    /// them; the one needing the fewest is used.
    fn for_strokes<'a>(
        s: &Sketch,
        strokes: impl Iterator<Item = (&'a [DVec2], bool)> + Clone,
        min_work: usize,
    ) -> Option<Lookup> {
        let seg_lens = |f: &mut dyn FnMut(f64)| {
            for (pts, closed) in strokes.clone() {
                let n = pts.len();
                let k = if closed { n } else { n.saturating_sub(1) };
                for i in 0..k.min(n) {
                    f(pts[i].distance(pts[(i + 1) % n]));
                }
            }
        };
        let (mut segs, mut total) = (0usize, 0.0f64);
        seg_lens(&mut |d| {
            segs += 1;
            total += d;
        });
        if !total.is_finite() {
            return None;
        }
        let size = s.vertices.len() + s.edges.len();
        let work = segs.saturating_mul(segs + size);
        if segs == 0 || work < min_work || (min_work > 0 && segs < LOOKUP_MIN_SEGMENTS) {
            return None;
        }
        let edge_len = |[a, b]: &[SVertexId; 2]| s.vertices[*a].distance(s.vertices[*b]);
        let floor = Self::floor();
        let stroke_mean = (total / segs as f64).max(floor);
        let sketch_mean = if s.edges.is_empty() {
            stroke_mean
        } else {
            (s.edges.values().map(edge_len).sum::<f64>() / s.edges.len() as f64).max(floor)
        };
        let ceiling = lookup_ceiling(size, segs);
        let need = |cell: f64| {
            let mut n = s.vertices.len();
            for e in s.edges.values() {
                n = n.saturating_add(samples(edge_len(e), cell));
            }
            seg_lens(&mut |d| n = n.saturating_add(samples(d, cell)));
            n
        };
        let cells = [stroke_mean, (stroke_mean * sketch_mean).sqrt(), sketch_mean];
        if min_work == 0 {
            // The tests that hold the two ways to one result: the cell needing the fewest
            // entries, whatever it would cost.
            let (n, cell) = cells
                .into_iter()
                .map(|c| (need(c), c))
                .min_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)))?;
            return (n <= ceiling)
                .then(|| Self::build(s, cell, ceiling))
                .flatten();
        }
        // What a lookup would cost against the scans it saves, in visits of one vertex or
        // edge ([`LOOKUP_BUILD_COST`]): building it, and for each segment its queries, each
        // finding about the entries in nine cells of the box the sketch and the strokes
        // cover; the scans visit every vertex and edge four times a segment. The cheapest cell
        // is used, and only when the lookup costs at most half the scans.
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in s.vertices.values() {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        for (pts, _) in strokes.clone() {
            for p in pts {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        let extent = (hi - lo).max(DVec2::ZERO);
        let cost = |n: usize, cell: f64| -> f64 {
            let over = ((extent.x / cell).ceil() + 1.0) * ((extent.y / cell).ceil() + 1.0);
            let found = (9.0 * n as f64 / over.max(1.0)).min(n as f64);
            LOOKUP_BUILD_COST * n as f64 + segs as f64 * (LOOKUP_QUERY_COST + 4.0 * found)
        };
        let (n, cell, c) = cells
            .into_iter()
            .map(|cell| {
                let n = need(cell);
                (n, cell, cost(n, cell))
            })
            .filter(|(n, _, c)| *n <= ceiling && c.is_finite())
            .min_by(|x, y| x.2.total_cmp(&y.2).then(x.1.total_cmp(&y.1)))?;
        let scans = 4.0 * segs as f64 * (size as f64 + segs as f64 / 2.0);
        if 2.0 * c > scans {
            return None;
        }
        let _ = n;
        Self::build(s, cell, ceiling)
    }

    /// A lookup of `s` in cells of side `cell` (at least [`Self::floor`]), holding at most
    /// `ceiling` entries, or `None` when the sketch alone would pass it or crowd a cell.
    fn build(s: &Sketch, cell: f64, ceiling: usize) -> Option<Lookup> {
        let mut l = Lookup {
            cell,
            verts: Default::default(),
            edges: Default::default(),
            pairs: Default::default(),
            entries: 0,
            ceiling,
            poor: Default::default(),
        };
        for (v, p) in &s.vertices {
            if !l.add_vertex(v, *p) {
                return None;
            }
        }
        for (e, [a, b]) in &s.edges {
            if !l.add_edge(e, *a, *b, s.vertices[*a], s.vertices[*b]) {
                return None;
            }
        }
        Some(l)
    }

    /// The smallest cell: four merge distances, so a point within the merge distance of a
    /// point within a quarter cell of a sample is in that sample's cell or one next to it.
    fn floor() -> f64 {
        4.0 * SKETCH_MERGE
    }

    fn key(&self, p: DVec2) -> (i64, i64) {
        (
            (p.x / self.cell).floor() as i64,
            (p.y / self.cell).floor() as i64,
        )
    }

    /// Note a query that came back with `found` of the sketch's `size` vertices and edges.
    fn found(&self, found: usize, size: usize) {
        if size >= 4096 && found.saturating_mul(4) > size {
            self.poor.set(self.poor.get() + 1);
        }
    }

    /// Too many queries came back with too much ([`LOOKUP_POOR`]).
    fn poor(&self) -> bool {
        self.poor.get() >= LOOKUP_POOR
    }

    /// A cell's list of `len` entries is crowded ([`LOOKUP_CELL_MAX`]).
    fn crowded(&self, len: usize) -> bool {
        len > LOOKUP_CELL_MAX && len.saturating_mul(8) > self.entries
    }

    /// Enter vertex `v` at `p`; false when that crowds its cell or passes the ceiling.
    fn add_vertex(&mut self, v: SVertexId, p: DVec2) -> bool {
        let k = self.key(p);
        let list = self.verts.entry(k).or_default();
        list.push(v);
        let len = list.len();
        self.entries += 1;
        self.entries <= self.ceiling && !self.crowded(len)
    }

    /// Enter edge `e` (`a`-`b`, at `pa`-`pb`); false when it would pass the ceiling or crowd
    /// a cell.
    fn add_edge(&mut self, e: SEdgeId, a: SVertexId, b: SVertexId, pa: DVec2, pb: DVec2) -> bool {
        let room = self.ceiling.saturating_sub(self.entries);
        let Some(keys) = sample_keys(pa, pb, self.cell, room) else {
            return false;
        };
        self.entries += keys.len();
        for k in keys {
            let list = self.edges.entry(k).or_default();
            list.push(e);
            let len = list.len();
            if self.crowded(len) {
                return false;
            }
        }
        self.pairs.insert((a.min(b), a.max(b)), e);
        true
    }

    /// The cells to look in for the point or segment `a`..`b`: around each of its samples,
    /// each once. `None` when it takes more samples than `most` (a segment longer than a few
    /// thousand cells: its query then scans, which is cheaper).
    fn query_cells(&self, a: DVec2, b: DVec2, most: usize) -> Option<Vec<(i64, i64)>> {
        let keys = sample_keys(a, b, self.cell, most)?;
        let mut out = Vec::with_capacity(9 * keys.len());
        for (x, y) in keys {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    out.push((x + dx, y + dy));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        Some(out)
    }

    /// Most samples one query takes before it scans instead: 4,096, or an eighth of the
    /// sketch's vertices and edges when that is more.
    fn query_most(size: usize) -> usize {
        (size / 8).max(4096)
    }

    /// The vertices in the cells around the samples of `a`..`b` (each once: a vertex is in
    /// one cell), or `None` for a query that scans instead.
    fn vertices_near(
        &self,
        a: DVec2,
        b: DVec2,
        live: &SlotMap<SVertexId, DVec2>,
        size: usize,
    ) -> Option<Vec<SVertexId>> {
        let cells = self.query_cells(a, b, Self::query_most(size))?;
        let out: Vec<SVertexId> = cells
            .iter()
            .filter_map(|c| self.verts.get(c))
            .flatten()
            .copied()
            .filter(|v| live.contains_key(*v))
            .collect();
        self.found(out.len(), size);
        Some(out)
    }

    /// The edges in the cells around the samples of `a`..`b`, each once, or `None` for a
    /// query that scans instead.
    fn edges_near(
        &self,
        a: DVec2,
        b: DVec2,
        live: &SlotMap<SEdgeId, [SVertexId; 2]>,
        size: usize,
    ) -> Option<Vec<SEdgeId>> {
        let cells = self.query_cells(a, b, Self::query_most(size))?;
        let mut out: Vec<SEdgeId> = cells
            .iter()
            .filter_map(|c| self.edges.get(c))
            .flatten()
            .copied()
            .filter(|e| live.contains_key(*e))
            .collect();
        out.sort_unstable();
        out.dedup();
        self.found(out.len(), size);
        Some(out)
    }

    /// The edge joining `a` and `b`, if it is still there and still joins them.
    fn edge_between(
        &self,
        a: SVertexId,
        b: SVertexId,
        live: &SlotMap<SEdgeId, [SVertexId; 2]>,
    ) -> Option<SEdgeId> {
        let e = *self.pairs.get(&(a.min(b), a.max(b)))?;
        let [c, d] = *live.get(e)?;
        ((c == a && d == b) || (c == b && d == a)).then_some(e)
    }

    /// Entries held (tests).
    #[cfg(test)]
    fn entries(&self) -> usize {
        self.entries
    }
}

/// The nearest of `vs` within the merge distance of `p`, ties to the smaller key: the
/// scan [`Sketch::snap_with`] makes (lane 53: over every vertex, or a lookup's candidates).
fn nearest_vertex<'a>(
    vs: impl Iterator<Item = (SVertexId, &'a DVec2)>,
    p: DVec2,
) -> Option<(f64, SVertexId)> {
    vs.map(|(k, q)| (q.distance(p), k))
        .filter(|(d, _)| *d <= SKETCH_MERGE)
        .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)))
}

/// The nearest of `es` within the merge distance of `p`, ties to the smaller key.
fn nearest_edge<'a>(
    es: impl Iterator<Item = (SEdgeId, &'a [SVertexId; 2])>,
    vertices: &SlotMap<SVertexId, DVec2>,
    p: DVec2,
) -> Option<(f64, SEdgeId)> {
    es.filter_map(|(e, [a, b])| {
        let (pa, pb) = (vertices[*a], vertices[*b]);
        let d = point_segment_distance(p, pa, pb);
        (d <= SKETCH_MERGE).then_some((d, e))
    })
    .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)))
}

/// The vertices of `vs` (but the segment's ends `p` and `q`) within the merge distance of
/// segment `pp`-`qq`, each with its parameter along it.
fn on_segment<'a>(
    vs: impl Iterator<Item = (SVertexId, &'a DVec2)>,
    p: SVertexId,
    q: SVertexId,
    pp: DVec2,
    qq: DVec2,
    param: impl Fn(DVec2) -> f64,
) -> Vec<(f64, SVertexId)> {
    vs.filter(|(v, pos)| {
        *v != p && *v != q && point_segment_distance(**pos, pp, qq) <= SKETCH_MERGE
    })
    .map(|(v, pos)| (param(*pos), v))
    .collect()
}

/// The edges of `es` (but those with an end `skip` names) that properly cross segment
/// `pp`-`qq`: each with its parameter along it and the crossing point.
fn crossing<'a>(
    es: impl Iterator<Item = (SEdgeId, &'a [SVertexId; 2])>,
    vertices: &SlotMap<SVertexId, DVec2>,
    skip: impl Fn(SVertexId) -> bool,
    pp: DVec2,
    qq: DVec2,
    param: impl Fn(DVec2) -> f64,
) -> Vec<(f64, SEdgeId, DVec2)> {
    let mut out = Vec::new();
    for (e, &[c, dd]) in es {
        if skip(c) || skip(dd) {
            continue;
        }
        let (pc, pd) = (vertices[c], vertices[dd]);
        if proper_cross(pp, qq, pc, pd) {
            let x = line_intersection(pp, qq, pc, pd);
            out.push((param(x), e, x));
        }
    }
    out
}

/// Lane 53 (#65): segments ab and cd do not cross and each end of one is more than `clear`
/// from the other (so they are more than `clear` apart, up to rounding).
fn segments_apart(a: DVec2, b: DVec2, c: DVec2, d: DVec2, clear: f64) -> bool {
    !proper_cross(a, b, c, d)
        && point_segment_distance(a, c, d) > clear
        && point_segment_distance(b, c, d) > clear
        && point_segment_distance(c, a, b) > clear
        && point_segment_distance(d, a, b) > clear
}

/// Lane 53 (#65): most segments one cell of [`Sketch::edges_after_clear_polyline`]'s grid
/// may hold before the polyline counts as too tightly packed to count cheaply.
pub const SURE_CELL: usize = 64;

/// Lane 53 (#65): the cells (side `cell`) of points sampled along segment ab at most half a
/// cell apart, ends included, each once (every point of the segment is within a quarter cell
/// of one); `None` when that takes more than `most` samples ([`samples`]), before anything
/// is allocated.
fn sample_keys(a: DVec2, b: DVec2, cell: f64, most: usize) -> Option<Vec<(i64, i64)>> {
    let n = samples(a.distance(b), cell);
    if n > most {
        return None;
    }
    let steps = n - 1;
    let mut out: Vec<(i64, i64)> = Vec::with_capacity(n);
    for k in 0..=steps {
        let p = a + (b - a) * (k as f64 / steps as f64);
        let key = ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
        if out.last() != Some(&key) {
            out.push(key);
        }
    }
    out.sort_unstable();
    out.dedup();
    Some(out)
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

/// #11: a stroke already in the sketch that is to become a curve (see
/// [`Sketch::add_curve`], whose arguments these are).
#[derive(Debug, Clone)]
pub(crate) struct NewCurve {
    pub pts: Vec<DVec2>,
    pub closed: bool,
    pub kind: CurveKind,
    pub centre: Option<DVec2>,
    pub segments: u32,
}

/// #11: a sketch's vertices binned in square cells twice `reach` wide, so the vertices
/// within `reach` of a point are in its cell or the eight around it: [`Sketch::vertex_near`]
/// for many points without a scan of every vertex per point. A point too far out for a
/// cell number (or not finite) is scanned for, as before.
struct VertexGrid {
    cell: f64,
    reach: f64,
    bins: std::collections::HashMap<(i64, i64), Vec<SVertexId>>,
}

impl VertexGrid {
    /// Cell numbers stay well inside `i64` (and exact in `f64`) below this.
    const FAR: f64 = 1e15;

    fn new(s: &Sketch, reach: f64) -> VertexGrid {
        let mut g = VertexGrid {
            cell: 2.0 * reach,
            reach,
            bins: std::collections::HashMap::new(),
        };
        for (k, p) in s.vertices() {
            if let Some(c) = g.at(p) {
                g.bins.entry(c).or_default().push(k);
            }
        }
        g
    }

    fn at(&self, p: DVec2) -> Option<(i64, i64)> {
        let q = p / self.cell;
        (q.x.abs() < Self::FAR && q.y.abs() < Self::FAR)
            .then(|| (q.x.floor() as i64, q.y.floor() as i64))
    }

    /// The vertex nearest `p` within the reach, ties to the smaller id: what
    /// [`Sketch::vertex_near`] finds.
    fn near(&self, s: &Sketch, p: DVec2) -> Option<SVertexId> {
        let Some((x, y)) = self.at(p) else {
            return s.vertex_near(p, self.reach);
        };
        let mut best: Option<(f64, SVertexId)> = None;
        for cx in x - 1..=x + 1 {
            for cy in y - 1..=y + 1 {
                for &k in self.bins.get(&(cx, cy)).into_iter().flatten() {
                    let d = s.vertices[k].distance(p);
                    let better = best.is_none_or(|(bd, bk)| {
                        d.total_cmp(&bd).then(k.cmp(&bk)) == std::cmp::Ordering::Less
                    });
                    if d <= self.reach && better {
                        best = Some((d, k));
                    }
                }
            }
        }
        best.map(|(_, k)| k)
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

    /// Lane 53 (#65): the number of edges this sketch holds after inserting the polyline,
    /// when that is certain without inserting it; `None` when it is not (or the polyline is
    /// refused for another reason, which inserting it will say).
    ///
    /// It is certain for a polyline that is clear of itself and of the sketch: every segment
    /// longer than twice the merge distance, every two segments that do not share an end more
    /// than that apart, two that do share one each with its far end more than that from the
    /// other, and every vertex and edge of the sketch more than that from every segment.
    /// Then inserting it merges, splits and repairs nothing: each segment snaps to new
    /// vertices or to the previous segment's end, no vertex lies on it and no edge crosses
    /// it, so it adds exactly one edge, and the repair finds every rule held. The count is the
    /// sketch's edges plus the segments. (The clearance is twice what the insertion tests, so
    /// a rounding in these distances can only make a polyline count as not clear.)
    ///
    /// Segments are found near each other through a grid, so this costs about as much as the
    /// polyline is long; a polyline packed so tightly that the grid stops being cheap (more
    /// than [`SURE_CELL`] segments in one cell, or more cells than a budget in its length and
    /// the sketch's) is not counted.
    pub(crate) fn edges_after_clear_polyline(
        &self,
        points: &[DVec2],
        closed: bool,
    ) -> Option<usize> {
        let segs = Self::polyline_segments(points, closed).ok()?;
        if !points.iter().all(|p| p.is_finite()) {
            return None;
        }
        let n = points.len();
        let clear = 2.0 * SKETCH_MERGE;
        let seg = |i: usize| (points[i], points[(i + 1) % n]);
        let mut total = 0.0;
        for i in 0..segs {
            let (a, b) = seg(i);
            let len = a.distance(b);
            if len <= clear {
                return None;
            }
            total += len;
        }
        // The cell: about the mean segment's length, and never under four clearances. Each
        // segment is entered in the cells of its samples ([`sample_keys`]), and each lookup
        // looks in the cells around a sample, as [`Lookup`] does: two segments (or a vertex
        // or an edge and a segment) within the clearance of each other are then found.
        let cell = (total / segs as f64).max(4.0 * clear);
        // Samples along the segments and edges: a few a segment of about the mean length.
        let mut budget = 8 * (segs + self.edges.len()) + 4096;
        let mut grid: std::collections::HashMap<(i64, i64), Vec<u32>> =
            std::collections::HashMap::new();
        let mut keys_of: Vec<Vec<(i64, i64)>> = Vec::with_capacity(segs);
        for i in 0..segs {
            let (a, b) = seg(i);
            let keys = sample_keys(a, b, cell, budget)?;
            budget -= keys.len();
            for &c in &keys {
                let v = grid.entry(c).or_default();
                v.push(i as u32);
                if v.len() > SURE_CELL {
                    return None;
                }
            }
            keys_of.push(keys);
        }
        let around = |keys: &[(i64, i64)]| {
            let mut out: Vec<(i64, i64)> = keys
                .iter()
                .flat_map(|&(x, y)| {
                    (-1..=1).flat_map(move |dx| (-1..=1).map(move |dy| (x + dx, y + dy)))
                })
                .collect();
            out.sort_unstable();
            out.dedup();
            out
        };
        // Segments i < j share an end when they are neighbours along the polyline: j right
        // after i (sharing i's far end), or, closed, the last and the first (sharing the
        // first point). (A cell holds at most SURE_CELL segments, so the pairs cost at most
        // nine times SURE_CELL for each cell a segment takes.)
        for (i, keys) in keys_of.iter().enumerate() {
            let mut near: Vec<usize> = around(keys)
                .iter()
                .filter_map(|c| grid.get(c))
                .flatten()
                .map(|&j| j as usize)
                .filter(|&j| j > i)
                .collect();
            near.sort_unstable();
            near.dedup();
            for j in near {
                let ((a, b), (c, d)) = (seg(i), seg(j));
                let ok = if j == i + 1 {
                    point_segment_distance(a, c, d) > clear
                        && point_segment_distance(d, a, b) > clear
                } else if closed && i == 0 && j == segs - 1 {
                    point_segment_distance(b, c, d) > clear
                        && point_segment_distance(c, a, b) > clear
                } else {
                    segments_apart(a, b, c, d, clear)
                };
                if !ok {
                    return None;
                }
            }
        }
        drop(keys_of);
        // The sketch's own vertices and edges, against the segments near them: only those in
        // the polyline's box widened by the clearance, the rest at the cost of a comparison
        // (a stroke beside a sketch of 40,000 vertices and edges looked each up in the grid).
        // Every vertex of a sketch ends one of its edges (`Sketch::check` refuses an isolated
        // vertex), and each edge's ends are tested against the segments below, so this loop
        // decides nothing alone; it keeps the test of a lone point cheap and plain.
        let (mut lo, mut hi) = (points[0], points[0]);
        for p in points {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let (lo, hi) = (lo - DVec2::splat(clear), hi + DVec2::splat(clear));
        let outside = |a: DVec2, b: DVec2| {
            a.x.max(b.x) < lo.x || a.x.min(b.x) > hi.x || a.y.max(b.y) < lo.y || a.y.min(b.y) > hi.y
        };
        let key = |x: f64| (x / cell).floor() as i64;
        for p in self.vertices.values().filter(|p| !outside(**p, **p)) {
            for c in around(&[(key(p.x), key(p.y))]) {
                for &i in grid.get(&c).map(Vec::as_slice).unwrap_or(&[]) {
                    let (a, b) = seg(i as usize);
                    if point_segment_distance(*p, a, b) <= clear {
                        return None;
                    }
                }
            }
        }
        for [u, w] in self.edges.values() {
            let (c, d) = (self.vertices[*u], self.vertices[*w]);
            if outside(c, d) {
                continue;
            }
            let keys = sample_keys(c, d, cell, budget)?;
            budget -= keys.len();
            for k in around(&keys) {
                for &i in grid.get(&k).map(Vec::as_slice).unwrap_or(&[]) {
                    let (a, b) = seg(i as usize);
                    if !segments_apart(a, b, c, d, clear) {
                        return None;
                    }
                }
            }
        }
        Some(self.edges.len() + segs)
    }

    /// Lane 16 (C3): insert many polylines as one transaction. The graph is built, repaired
    /// once and its regions detected once; an import of 5,000 lines used to copy the sketch,
    /// repair it and detect every region once per line. All or nothing: the first stroke the
    /// sketch refuses refuses them all (the caller then inserts them one at a time).
    pub fn insert_polylines(&mut self, strokes: &[(Vec<DVec2>, bool)]) -> Result<(), SketchError> {
        self.insert_polylines_by(strokes, None)
    }

    /// [`Self::insert_polylines`], through a [`Lookup`] or with the scans (as
    /// [`Self::with_polyline_by`]).
    fn insert_polylines_by(
        &mut self,
        strokes: &[(Vec<DVec2>, bool)],
        lookup: Option<bool>,
    ) -> Result<(), SketchError> {
        let mut next = self.clone();
        let all = strokes.iter().map(|(p, c)| (&p[..], *c));
        let mut look = match lookup {
            Some(false) => None,
            Some(true) => Lookup::for_strokes(&next, all, 0),
            None => Lookup::for_strokes(&next, all, LOOKUP_MIN_WORK),
        };
        for (points, closed) in strokes {
            let segs = Self::polyline_segments(points, *closed)?;
            let n = points.len();
            for i in 0..segs {
                next.add_segment_with(points[i], points[(i + 1) % n], &mut look)?;
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
        self.with_polyline_by(points, closed, None)
    }

    /// [`Self::with_polyline`]: through a [`Lookup`] when the stroke is long enough for one
    /// to pay (`None`), or (for the tests that hold the two ways to one result) always
    /// through one (`Some(true)`) or always with the scans of every vertex and edge.
    fn with_polyline_by(
        &self,
        points: &[DVec2],
        closed: bool,
        lookup: Option<bool>,
    ) -> Result<(Sketch, Vec<(SVertexId, SVertexId)>), SketchError> {
        Self::polyline_segments(points, closed)?;
        let one = std::iter::once((points, closed));
        let mut look = match lookup {
            Some(false) => None,
            Some(true) => Lookup::for_strokes(self, one, 0),
            None => Lookup::for_strokes(self, one, LOOKUP_MIN_WORK),
        };
        self.with_polyline_through(points, closed, &mut look)
    }

    /// [`Self::with_polyline_by`] through `look` (`None`: the scans), which may be dropped on
    /// the way ([`Self::looked_up`]).
    fn with_polyline_through(
        &self,
        points: &[DVec2],
        closed: bool,
        look: &mut Option<Lookup>,
    ) -> Result<(Sketch, Vec<(SVertexId, SVertexId)>), SketchError> {
        let segs = Self::polyline_segments(points, closed)?;
        let n = points.len();
        let mut next = self.clone();
        let mut new_edges = Vec::new();
        for i in 0..segs {
            new_edges.extend(next.add_segment_with(points[i], points[(i + 1) % n], look)?);
        }
        next.repair()?;
        // The edges still there after the repair. For a long stroke (lane 53) one set of the
        // sketch's edges instead of a scan of them for each new one (`edge_between` either way
        // round is this pair); for a short one the scans, which cost less than the set.
        if new_edges.len() > 64 {
            let live: std::collections::HashSet<(SVertexId, SVertexId)> = next
                .edges
                .values()
                .map(|&[a, b]| (a.min(b), a.max(b)))
                .collect();
            new_edges.retain(|&(a, b)| live.contains(&(a.min(b), a.max(b))));
        } else {
            new_edges.retain(|&(a, b)| next.edge_between(a, b).is_some());
        }
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
        self.split_edge_with(e, pos, &mut None)
    }

    /// [`Self::split_edge`], keeping `look` up to date.
    fn split_edge_with(&mut self, e: SEdgeId, pos: DVec2, look: &mut Option<Lookup>) -> SVertexId {
        let [a, b] = self.edges[e];
        let x = self.vertices.insert(pos);
        self.edges[e] = [a, x];
        let f = self.edges.insert([x, b]);
        self.looked_up(look, &[x], &[e, f]);
        x
    }

    /// Lane 53 (#65): tell `look` about new vertices and new or changed edges; it is dropped
    /// (the scans of every vertex and edge take over) when that would cost past its budget.
    fn looked_up(&self, look: &mut Option<Lookup>, verts: &[SVertexId], edges: &[SEdgeId]) {
        if let Some(l) = look {
            for &v in verts {
                if !l.add_vertex(v, self.vertices[v]) {
                    *look = None;
                    return;
                }
            }
            for &e in edges {
                let [a, b] = self.edges[e];
                if !l.add_edge(e, a, b, self.vertices[a], self.vertices[b]) {
                    *look = None;
                    return;
                }
            }
        }
    }

    /// Lane 53 (#65): the vertices to test against the point or segment `a`..`b` when there
    /// is a lookup: those near it, which hold every vertex within the merge distance of it
    /// (see [`Lookup`]); `None`: test every vertex (no lookup, or a segment too long for it
    /// to be worth looking along). Each candidate is then tested exactly as the full scan
    /// tests it (the scan's own loop runs on `None`), and every use orders what passes by its
    /// own total order, so the results are the full scan's.
    fn vertex_candidates(
        &self,
        a: DVec2,
        b: DVec2,
        look: &Option<Lookup>,
    ) -> Option<Vec<SVertexId>> {
        let size = self.vertices.len() + self.edges.len();
        look.as_ref()
            .and_then(|l| l.vertices_near(a, b, &self.vertices, size))
    }

    /// [`Self::vertex_candidates`] for edges: those near the point or segment, which hold
    /// every edge within the merge distance of it or crossing it.
    fn edge_candidates(&self, a: DVec2, b: DVec2, look: &Option<Lookup>) -> Option<Vec<SEdgeId>> {
        let size = self.vertices.len() + self.edges.len();
        look.as_ref()
            .and_then(|l| l.edges_near(a, b, &self.edges, size))
    }

    /// [`Self::edge_between`], through `look`'s table of vertex pairs when there is one.
    fn edge_between_with(
        &self,
        a: SVertexId,
        b: SVertexId,
        look: &Option<Lookup>,
    ) -> Option<SEdgeId> {
        match look {
            None => self.edge_between(a, b),
            Some(l) => l.edge_between(a, b, &self.edges),
        }
    }

    /// Resolve a segment endpoint: merge into a vertex, split an edge, or make a new vertex.
    fn snap_with(&mut self, p: DVec2, look: &mut Option<Lookup>) -> SVertexId {
        let near = match self.vertex_candidates(p, p, look) {
            Some(c) => nearest_vertex(c.iter().map(|&k| (k, &self.vertices[k])), p),
            None => nearest_vertex(self.vertices.iter(), p),
        };
        if let Some((_, v)) = near {
            return v;
        }
        let on_edge = match self.edge_candidates(p, p, look) {
            Some(c) => nearest_edge(c.iter().map(|&e| (e, &self.edges[e])), &self.vertices, p),
            None => nearest_edge(self.edges.iter(), &self.vertices, p),
        };
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
            return self.split_edge_with(e, proj, look);
        }
        let v = self.vertices.insert(p);
        self.looked_up(look, &[v], &[]);
        v
    }

    /// Insert one segment into the graph without re-detecting regions. Returns the edges it
    /// created (pieces that already existed are not repeated).
    fn add_segment(
        &mut self,
        a: DVec2,
        b: DVec2,
    ) -> Result<Vec<(SVertexId, SVertexId)>, SketchError> {
        self.add_segment_with(a, b, &mut None)
    }

    /// [`Self::add_segment`], looking things up through `look` when there is one (lane 53,
    /// #65: a long stroke's insertion scanned every vertex and edge for each segment, so it
    /// grew with the square of the stroke). The same tests on every candidate that can pass
    /// them, so the same sketch.
    fn add_segment_with(
        &mut self,
        a: DVec2,
        b: DVec2,
        look: &mut Option<Lookup>,
    ) -> Result<Vec<(SVertexId, SVertexId)>, SketchError> {
        if a.distance(b) <= SKETCH_MERGE {
            return Err(SketchError::Degenerate);
        }
        if look.as_ref().is_some_and(Lookup::poor) {
            *look = None;
        }
        let p = self.snap_with(a, look);
        let q = self.snap_with(b, look);
        if p == q {
            return Err(SketchError::Degenerate);
        }
        let (pp, qq) = (self.vertices[p], self.vertices[q]);
        let d = qq - pp;
        let len2 = d.length_squared();
        let param = |x: DVec2| (x - pp).dot(d) / len2;

        // Existing vertices on the new segment split it.
        let mut cuts: Vec<(f64, SVertexId)> = match self.vertex_candidates(pp, qq, look) {
            Some(c) => on_segment(
                c.iter().map(|&k| (k, &self.vertices[k])),
                p,
                q,
                pp,
                qq,
                param,
            ),
            None => on_segment(self.vertices.iter(), p, q, pp, qq, param),
        };
        let on_line: BTreeSet<SVertexId> = cuts.iter().map(|(_, v)| *v).collect();

        // Proper crossings split both. Edges touching the segment at a vertex were handled
        // above and cannot also cross it.
        let skip = |v: SVertexId| v == p || v == q || on_line.contains(&v);
        let mut crossings: Vec<(f64, SEdgeId, DVec2)> = match self.edge_candidates(pp, qq, look) {
            Some(c) => crossing(
                c.iter().map(|&e| (e, &self.edges[e])),
                &self.vertices,
                skip,
                pp,
                qq,
                param,
            ),
            None => crossing(self.edges.iter(), &self.vertices, skip, pp, qq, param),
        };
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
                        if u != w && self.edge_between_with(u, w, look).is_none() {
                            let f = self.edges.insert([u, w]);
                            self.looked_up(look, &[], &[f]);
                        }
                    }
                    lv
                }
                _ => {
                    let v = self.split_edge_with(e, x, look);
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
            if u == v || self.edge_between_with(u, v, look).is_some() {
                continue;
            }
            let f = self.edges.insert([u, v]);
            self.looked_up(look, &[], &[f]);
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
    /// sketch, the live regions bounded by any redrawn edge, sorted, and (lane 43, #7) the
    /// further pieces of every curve the move broke up, as (piece, curve) id pairs (a curve
    /// keeps its id on its first piece).
    pub(crate) fn with_moved_vertices(
        &self,
        to: &BTreeMap<SVertexId, DVec2>,
        anchor: &dyn Fn(DVec2) -> DVec2,
        round: &dyn Fn(&Round) -> Option<(DVec2, f64)>,
    ) -> Result<MovedSketch, SketchError> {
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
        let split = next.settle_traced();
        next.debug_check("move");
        let touched = next.live_regions_touching(&made);
        Ok((next, touched, split))
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
        self.settle_traced();
    }

    /// [`Self::settle`], returning the pieces curves split into (see
    /// [`Self::settle_curves`]).
    fn settle_traced(&mut self) -> Vec<(u64, u64)> {
        self.recompute_regions();
        self.settle_curves()
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
    /// A curve that splits keeps its id on its first piece; each further piece gets a new id.
    /// Returns those further pieces as (piece, the curve it came from), in id order (lane
    /// 43, #7: what a move reports a curve it broke up as); the list itself is kept nowhere.
    fn settle_curves(&mut self) -> Vec<(u64, u64)> {
        let mut split_from = Vec::new();
        if self.curves.is_empty() {
            return split_from;
        }
        let adj = self.adjacency();
        let mut claimed: BTreeSet<(SVertexId, SVertexId)> = BTreeSet::new();
        let none = BTreeSet::new();
        let old = std::mem::take(&mut self.curves);
        let mut next = self.next_curve;
        let mut out: BTreeMap<u64, Curve> = BTreeMap::new();
        for (id, c) in old {
            for (key, piece) in self.settle_curve(id, &c, &adj, &none, &mut claimed, &mut next) {
                if key != id {
                    split_from.push((key, id));
                }
                out.insert(key, piece);
            }
        }
        self.curves = out;
        self.next_curve = next;
        split_from
    }

    /// Each vertex's neighbours along edges.
    fn adjacency(&self) -> BTreeMap<SVertexId, Vec<SVertexId>> {
        let mut adj: BTreeMap<SVertexId, Vec<SVertexId>> = BTreeMap::new();
        for [a, b] in self.edges.values() {
            adj.entry(*a).or_default().push(*b);
            adj.entry(*b).or_default().push(*a);
        }
        adj
    }

    /// One curve of [`Self::settle_curves`], `id`: its pieces, keyed by id (the first keeps
    /// `id`, each further one takes `next`, which moves on). An edge in `held` or `claimed`
    /// is an older curve's; the edges this curve takes go into `claimed`.
    fn settle_curve(
        &self,
        id: u64,
        c: &Curve,
        adj: &BTreeMap<SVertexId, Vec<SVertexId>>,
        held: &BTreeSet<(SVertexId, SVertexId)>,
        claimed: &mut BTreeSet<(SVertexId, SVertexId)>,
        next: &mut u64,
    ) -> Vec<(u64, Curve)> {
        let mut out = Vec::new();
        let (pieces, lost) = self.curve_pieces(c, adj, held, claimed);
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
                *next += 1;
                *next - 1
            };
            out.push((
                key,
                Curve {
                    kind,
                    chain,
                    closed,
                    segments,
                    round,
                },
            ));
        }
        out
    }

    /// The pieces of curve `c` in the graph as it is now (chains, closed), and whether it
    /// lost an edge or a point. Claims the edges it keeps; an edge in `held` or already
    /// claimed is another curve's.
    fn curve_pieces(
        &self,
        c: &Curve,
        adj: &BTreeMap<SVertexId, Vec<SVertexId>>,
        held: &BTreeSet<(SVertexId, SVertexId)>,
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
                    !held.contains(&k) && !claimed.contains(&k) && mine.insert(k)
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
        self.add_curve_traced(pts, closed, kind, centre, segments)
            .map(|(id, _)| id)
    }

    /// [`Self::add_curve`], also returning (lane 43, #7) the ids of the curve's further
    /// pieces where it split as it settled, in id order (empty when it is one piece).
    pub(crate) fn add_curve_traced(
        &mut self,
        pts: &[DVec2],
        closed: bool,
        kind: CurveKind,
        centre: Option<DVec2>,
        segments: u32,
    ) -> Option<(u64, Vec<u64>)> {
        let curve = self.new_curve(pts, closed, kind, centre, segments, |p| {
            self.vertex_near(p, 2.0 * SKETCH_MERGE)
        })?;
        let id = self.next_curve;
        let before = self.curves.clone();
        let next_before = self.next_curve;
        self.next_curve += 1;
        self.curves.insert(id, curve);
        let split = self.settle_curves();
        // The new curve's pieces must fit their circle; a stroke that does not (points the
        // insertion moved too far) stays plain edges, never a curve that lies.
        let mine: Vec<u64> = self.curves.keys().copied().filter(|&k| k >= id).collect();
        let fits = mine.iter().all(|k| self.round_fits(&self.curves[k]));
        if !fits {
            self.curves = before;
            self.next_curve = next_before;
            return None;
        }
        let first = mine.first().copied()?;
        let more = split
            .iter()
            .filter(|&&(piece, from)| from == id && piece != first)
            .map(|&(piece, _)| piece)
            .collect();
        Some((first, more))
    }

    /// The curve a stroke through `pts` makes (see [`Self::add_curve`]), its points found by
    /// `near` (the vertex within twice the merge distance); `None` when it makes none.
    fn new_curve(
        &self,
        pts: &[DVec2],
        closed: bool,
        kind: CurveKind,
        centre: Option<DVec2>,
        segments: u32,
        near: impl Fn(DVec2) -> Option<SVertexId>,
    ) -> Option<Curve> {
        let mut chain: Vec<SVertexId> = Vec::with_capacity(pts.len());
        for &p in pts {
            let v = near(p)?;
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
        Some(Curve {
            kind,
            chain,
            closed,
            segments,
            round,
        })
    }

    /// Whether [`Self::settle_curves`] would leave every curve as it is, with the same
    /// counter: each settles to itself, one piece with its own id.
    fn curves_settled(&self, adj: &BTreeMap<SVertexId, Vec<SVertexId>>) -> bool {
        let none = BTreeSet::new();
        let mut claimed = BTreeSet::new();
        let mut next = self.next_curve;
        self.curves.iter().all(|(&id, c)| {
            let pieces = self.settle_curve(id, c, adj, &none, &mut claimed, &mut next);
            matches!(pieces.as_slice(), [(k, p)] if *k == id && p == c)
        })
    }

    /// Whether a curve's points fit its circle (a curve that is not round fits).
    fn round_fits(&self, c: &Curve) -> bool {
        c.round.is_none_or(|r| {
            let at: Vec<DVec2> = c.chain.iter().map(|&v| self.vertices[v]).collect();
            check_round(&r, &at, c.closed).is_ok()
        })
    }

    /// #11: [`Self::add_curve`] for each of `curves` in order (an import's curves), leaving
    /// the same sketch and returning the same ids. One at a time scanned every vertex for
    /// each point and settled every curve in the sketch for each curve, so a file of n
    /// curves took time in n squared. Here the points are found through one grid of the
    /// vertices, and only the new curve is settled: the graph does not change as curves are
    /// added, a settled curve settles again to itself, and an older curve never loses an
    /// edge to a newer one, so what settling every curve did to the older ones was nothing.
    /// The new curve is settled as the last of them, against the edges the others hold
    /// once settled (its own pieces' edges, after it, never the edges of a piece it
    /// dropped), its further pieces numbered as one at a time numbers them. A sketch whose
    /// curves would not settle to themselves (only a hand-edited file holds one: a curve
    /// running through a junction, say) takes them one at a time, since there the first
    /// curve's settle changed the older ones.
    pub(crate) fn add_curves(&mut self, curves: &[NewCurve]) -> Vec<Option<u64>> {
        let adj = self.adjacency();
        if !self.curves_settled(&adj) {
            return curves
                .iter()
                .map(|c| self.add_curve(&c.pts, c.closed, c.kind, c.centre, c.segments))
                .collect();
        }
        let grid = VertexGrid::new(self, 2.0 * SKETCH_MERGE);
        let key = |(a, b): (SVertexId, SVertexId)| (a.min(b), a.max(b));
        let mut held: BTreeSet<(SVertexId, SVertexId)> = self
            .curves
            .values()
            .flat_map(|c| c.pairs().map(key))
            .collect();
        let mut ids = Vec::with_capacity(curves.len());
        for c in curves {
            let Some(curve) = self.new_curve(&c.pts, c.closed, c.kind, c.centre, c.segments, |p| {
                grid.near(self, p)
            }) else {
                ids.push(None);
                continue;
            };
            let id = self.next_curve;
            let mut next = id + 1;
            let mut taken = BTreeSet::new();
            let pieces = self.settle_curve(id, &curve, &adj, &held, &mut taken, &mut next);
            // The check `add_curve` makes, kept the same (a settle already makes a piece off
            // its circle a polyline, so it holds today); a curve that kept no edge spends its id.
            if !pieces.iter().all(|(_, p)| self.round_fits(p)) {
                ids.push(None);
                continue;
            }
            self.next_curve = next;
            ids.push(pieces.first().map(|(k, _)| *k));
            for (k, p) in pieces {
                held.extend(p.pairs().map(key));
                self.curves.insert(k, p);
            }
        }
        ids
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
            if c.id > crate::curve::MAX_CURVE_ID {
                return Err("a curve id is beyond 2^53 - 1".into());
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
            Some(n) if n > crate::curve::MAX_CURVE_ID => {
                return Err("the curve counter is beyond 2^53 - 1".into());
            }
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
    use std::f64::consts::TAU;

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

    /// #11: a small, fixed random sequence (splitmix64), so the comparison below needs no
    /// new dependency and every seed replays.
    struct Mix(u64);

    impl Mix {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        fn unit(&mut self) -> f64 {
            (self.next() >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// One stroke of a random import: its points and, when it is one, its curve.
    fn random_stroke(r: &mut Mix, made: &[NewCurve], spread: f64) -> NewCurve {
        let grid = |r: &mut Mix| v(r.below(9) as f64 * spread, r.below(9) as f64 * spread);
        let radii = [0.003, 0.02, 0.05, 0.1, 0.25];
        let plain = |pts: Vec<DVec2>| NewCurve {
            pts,
            closed: false,
            kind: CurveKind::Polyline,
            centre: None,
            segments: 0,
        };
        match r.below(10) {
            // Again, exactly or within the merge distance: the second one loses its edges
            // to the first, or lands on the same points.
            0 if !made.is_empty() => {
                let mut c = made[r.below(made.len() as u64) as usize].clone();
                let d = [0.0, 5e-6, 3e-5][r.below(3) as usize];
                c.pts.iter_mut().for_each(|p| p.x += d);
                c
            }
            // A circle or a polygon, sometimes square to the axes, sometimes about a centre
            // a little off (so it does not fit).
            0..=2 => {
                let c = grid(r);
                let rad = radii[r.below(5) as usize];
                let n = [3, 4, 5, 6, 8, 12, 24, 33][r.below(8) as usize];
                let a0 = [0.0, TAU / 8.0, r.unit() * TAU][r.below(3) as usize];
                let pts = (0..n)
                    .map(|k| c + rad * DVec2::from_angle(a0 + TAU * k as f64 / n as f64))
                    .collect();
                let off = [0.0, 0.0, 1e-5, 3e-5, 1e-3][r.below(5) as usize];
                let kind = if r.below(3) == 0 {
                    CurveKind::Polygon
                } else {
                    CurveKind::Circle
                };
                NewCurve {
                    pts,
                    closed: true,
                    kind,
                    centre: Some(c + v(off, 0.0)),
                    segments: n as u32,
                }
            }
            // An arc either way round, of one segment or more.
            3 => {
                let c = grid(r);
                let rad = radii[r.below(5) as usize];
                let n = [1, 1, 2, 3, 6, 16][r.below(6) as usize];
                let a0 = r.unit() * TAU;
                let sweep = (0.1 + r.unit() * 6.0) * if r.below(2) == 0 { 1.0 } else { -1.0 };
                let pts = (0..=n)
                    .map(|k| c + rad * DVec2::from_angle(a0 + sweep * k as f64 / n as f64))
                    .collect();
                NewCurve {
                    pts,
                    closed: false,
                    kind: CurveKind::Arc,
                    centre: Some(c),
                    segments: n as u32,
                }
            }
            // A polyline curve on the grid: open, closed, P-shaped (back to its second
            // point) or doubled back on its last edge.
            4 => {
                let n = 2 + r.below(4) as usize;
                let mut pts: Vec<DVec2> = (0..n).map(|_| grid(r)).collect();
                let shape = r.below(4);
                if shape == 0 && n >= 4 {
                    pts.push(pts[1]);
                } else if shape == 1 {
                    pts.push(pts[n - 2]);
                }
                let closed = shape == 2 && n > 2;
                let segments = if closed { pts.len() } else { pts.len() - 1 } as u32;
                NewCurve {
                    pts,
                    closed,
                    kind: CurveKind::Polyline,
                    centre: None,
                    segments,
                }
            }
            // An arc of one or two segments over a chord of an earlier circle or polygon.
            5 => {
                let round: Vec<&NewCurve> = made
                    .iter()
                    .filter(|c| c.closed && c.centre.is_some())
                    .collect();
                if round.is_empty() {
                    return plain(vec![grid(r), grid(r)]);
                }
                let c = round[r.below(round.len() as u64) as usize];
                let n = c.pts.len();
                let i = r.below(n as u64) as usize;
                let take = 1 + r.below(2) as usize;
                NewCurve {
                    pts: (0..=take).map(|j| c.pts[(i + j) % n]).collect(),
                    closed: false,
                    kind: CurveKind::Arc,
                    centre: c.centre,
                    segments: take as u32,
                }
            }
            // A spur from a point of an earlier stroke (a junction on it).
            6 if !made.is_empty() => {
                let c = &made[r.below(made.len() as u64) as usize];
                let p = c.pts[r.below(c.pts.len() as u64) as usize];
                plain(vec![p, grid(r) + v(0.013, 0.007) * spread])
            }
            // A plain line across whatever is there (no curve: segments 0).
            _ => plain(vec![grid(r), grid(r)]),
        }
    }

    /// Strokes into the sketch as an import puts them: all together, else one at a time
    /// (those that go in). Returns the curves among those that went in.
    fn insert_like_an_import(s: &mut Sketch, strokes: &[NewCurve]) -> Vec<NewCurve> {
        let list: Vec<(Vec<DVec2>, bool)> =
            strokes.iter().map(|c| (c.pts.clone(), c.closed)).collect();
        let landed: Vec<bool> = if s.insert_polylines(&list).is_ok() {
            vec![true; strokes.len()]
        } else {
            list.iter()
                .map(|(p, closed)| s.insert_polyline(p, *closed).is_ok())
                .collect()
        };
        strokes
            .iter()
            .zip(landed)
            .filter(|(c, ok)| *ok && c.segments > 0)
            .map(|(c, _)| c.clone())
            .collect()
    }

    fn one_at_a_time(s: &mut Sketch, curves: &[NewCurve]) -> Vec<Option<u64>> {
        curves
            .iter()
            .map(|c| s.add_curve(&c.pts, c.closed, c.kind, c.centre, c.segments))
            .collect()
    }

    /// #11: adding an import's curves together leaves exactly what adding them one at a
    /// time left (the same curves, ids, points and angles, the same next id, the same model
    /// hash) and returns the same ids, over 1,000 random imports into sketches that may
    /// hold curves already: circles, polygons, arcs (of one segment too, and over another
    /// curve's chord) and polylines (P-shaped and doubled back too), crossing, touching,
    /// with spurs at their points, repeated (exactly or within the merge distance), about
    /// centres a little off, crowded or spread out. Each kind of outcome must turn up:
    /// curves that split, curves that stay plain edges, and imports where neither happens.
    #[test]
    fn curves_added_together_match_curves_added_one_at_a_time() {
        let (mut split, mut refused, mut clean) = (0, 0, 0);
        for seed in 0..1000u64 {
            let mut r = Mix(seed);
            // Strokes crowded on a 0.1 grid (most cross), or spread out on a 1.0 one.
            let spread = if seed % 2 == 0 { 0.1 } else { 1.0 };
            let mut s = sketch();
            // Curves already there, added one at a time as a draw adds them.
            let mut made: Vec<NewCurve> = Vec::new();
            for _ in 0..r.below(4) {
                let c = random_stroke(&mut r, &made, spread);
                if s.insert_polyline(&c.pts, c.closed).is_ok() && c.segments > 0 {
                    s.add_curve(&c.pts, c.closed, c.kind, c.centre, c.segments);
                }
                made.push(c);
            }
            let strokes: Vec<NewCurve> = (0..2 + r.below(24))
                .map(|_| {
                    let c = random_stroke(&mut r, &made, spread);
                    made.push(c.clone());
                    c
                })
                .collect();
            let curves = insert_like_an_import(&mut s, &strokes);
            let mut a = s.clone();
            let want = one_at_a_time(&mut a, &curves);
            let made_ids = want.iter().flatten().count();
            let none = want.len() - made_ids;
            let pieces = a.curve_count() - s.curve_count();
            refused += usize::from(none > 0);
            split += usize::from(pieces > made_ids);
            clean += usize::from(none == 0 && pieces == made_ids && made_ids > 0);
            let mut b = s.clone();
            let got = b.add_curves(&curves);
            assert_eq!(got, want, "seed {seed}: the ids returned");
            assert_eq!(
                b.curves().collect::<Vec<_>>(),
                a.curves().collect::<Vec<_>>(),
                "seed {seed}: the curves"
            );
            assert_eq!(b.next_curve_id(), a.next_curve_id(), "seed {seed}");
            assert_eq!(
                crate::hash::sketch_hash(&b),
                crate::hash::sketch_hash(&a),
                "seed {seed}"
            );
        }
        eprintln!("imports with a curve split {split}, with plain edges {refused}, clean {clean}");
        assert!(split > 100, "split {split}");
        assert!(refused > 100, "plain edges {refused}");
        assert!(clean > 50, "clean {clean}");
    }

    /// #11: the vertex grid finds what the scan finds, ties to the smaller id included,
    /// for points on, near, between and far from the vertices (and not finite).
    #[test]
    fn the_vertex_grid_finds_what_the_scan_finds() {
        let reach = 2.0 * SKETCH_MERGE;
        for seed in 0..400u64 {
            let mut r = Mix(seed ^ 0x5eed);
            let mut s = sketch();
            for _ in 0..1 + r.below(12) {
                let c = random_stroke(&mut r, &[], 0.1);
                let _ = s.insert_polyline(&c.pts, c.closed);
            }
            let grid = VertexGrid::new(&s, reach);
            let verts: Vec<DVec2> = s.vertices().map(|(_, p)| p).collect();
            if verts.is_empty() {
                continue;
            }
            for _ in 0..200 {
                let base = verts[r.below(verts.len() as u64) as usize];
                let d = [0.0, 1e-6, 1e-5, reach, reach * (1.0 + 1e-12), 2e-5, 0.3];
                let off = d[r.below(d.len() as u64) as usize];
                let p = base + off * DVec2::from_angle(r.unit() * TAU);
                assert_eq!(
                    grid.near(&s, p),
                    s.vertex_near(p, reach),
                    "seed {seed}, {p:?}"
                );
            }
            for p in [
                v(f64::NAN, 0.0),
                v(f64::INFINITY, 1.0),
                v(1e300, -1e300),
                verts[0] + v(0.5 * reach, 0.5 * reach),
            ] {
                assert_eq!(grid.near(&s, p), s.vertex_near(p, reach), "{p:?}");
            }
        }
    }

    /// #11: a point exactly as far from two vertices (each within the reach) finds the one
    /// with the smaller id through the grid, as the scan does, whichever vertex came first.
    #[test]
    fn the_vertex_grid_breaks_a_tie_to_the_smaller_id() {
        let reach = 2.0 * SKETCH_MERGE;
        // Powers of two, so the two distances are exactly equal: 2^-15 m apart, the point
        // half way (2^-16 m, under the reach, from each).
        let h = 2f64.powi(-15);
        for flip in [false, true] {
            let mut s = sketch();
            let (a, b) = if flip { (h, 0.0) } else { (0.0, h) };
            s.insert_polyline(&[v(0.0, a), v(-1.0, a)], false).unwrap();
            s.insert_polyline(&[v(0.0, b), v(1.0, b)], false).unwrap();
            let p = v(0.0, 0.5 * h);
            let grid = VertexGrid::new(&s, reach);
            let want = s.vertex_near(p, reach);
            assert!(want.is_some());
            assert_eq!(s.vertex(want.unwrap()).unwrap().distance(p), 0.5 * h);
            assert_eq!(grid.near(&s, p), want, "flip {flip}");
        }
    }

    /// #11: a sketch whose curves would not settle to themselves, as only a hand-edited
    /// file holds (here a circle running whole through two junctions, which a settle splits
    /// into two arcs), takes an import's curves one at a time: the same curves, ids, next id
    /// and sketch hash as `add_curve` one at a time, the circle split as that splits it.
    #[test]
    fn curves_added_to_a_sketch_a_file_left_unsettled_match_one_at_a_time() {
        let n = 12;
        let ring: Vec<DVec2> = (0..n)
            .map(|k| 0.1 * DVec2::from_angle(TAU * k as f64 / n as f64))
            .collect();
        let mut s = sketch();
        s.insert_polyline(&ring, true).unwrap();
        // Two spurs of two segments out from opposite points: junctions there.
        for k in [0, 6] {
            s.insert_polyline(&[ring[k], 1.5 * ring[k], 2.0 * ring[k]], false)
                .unwrap();
        }
        // On a settled sketch the circle is two arcs at once.
        let mut drawn = s.clone();
        drawn
            .add_curve(&ring, true, CurveKind::Circle, Some(DVec2::ZERO), n as u32)
            .unwrap();
        assert_eq!(drawn.curve_count(), 2);
        // The file a hand edit leaves: the same circle as one curve, which opens.
        let chain: Vec<SVertexId> = ring.iter().map(|&p| s.vertex_at(p).unwrap()).collect();
        let at: Vec<DVec2> = chain.iter().map(|&k| s.vertices[k]).collect();
        let (start, sweep) = fit_angles(DVec2::ZERO, &at, true).unwrap();
        s.curves.insert(
            1,
            Curve {
                kind: CurveKind::Circle,
                chain,
                closed: true,
                segments: n as u32,
                round: Some(Round {
                    centre: DVec2::ZERO,
                    radius: 0.1,
                    start,
                    sweep,
                }),
            },
        );
        s.next_curve = 2;
        let s = Sketch::from_raw(Plane::GROUND, &s.to_raw()).unwrap();
        assert!(!s.curves_settled(&s.adjacency()));
        // An import's curves: a polyline along one spur, and an arc on three of the
        // circle's points, which loses its edges to the circle.
        let curves = vec![
            NewCurve {
                pts: vec![ring[6], 1.5 * ring[6], 2.0 * ring[6]],
                closed: false,
                kind: CurveKind::Polyline,
                centre: None,
                segments: 2,
            },
            NewCurve {
                pts: ring[1..=3].to_vec(),
                closed: false,
                kind: CurveKind::Arc,
                centre: Some(DVec2::ZERO),
                segments: 2,
            },
        ];
        let mut a = s.clone();
        let want = one_at_a_time(&mut a, &curves);
        let mut b = s.clone();
        assert_eq!(b.add_curves(&curves), want);
        assert_eq!(
            b.curves().collect::<Vec<_>>(),
            a.curves().collect::<Vec<_>>()
        );
        assert_eq!(b.next_curve_id(), a.next_curve_id());
        assert_eq!(crate::hash::sketch_hash(&b), crate::hash::sketch_hash(&a));
        // The circle is two arcs, as the first curve's settle left it.
        let arcs = b
            .curves()
            .filter(|(_, c)| c.kind == CurveKind::Arc && c.round.is_some_and(|r| r.radius == 0.1))
            .count();
        assert_eq!(arcs, 2, "{:?}", b.curves().collect::<Vec<_>>());
    }
}

/// Lane 53 (#65): a stroke inserted through the [`Lookup`] leaves exactly the sketch the scans
/// of every vertex and edge leave (the same vertices and edges under the same keys, the same
/// regions and curves, the same new edges), on hostile strokes and sketches.
#[cfg(test)]
mod lookup_tests {
    use super::*;
    use proptest::prelude::*;

    /// Everything a sketch holds, keys and bits included.
    fn fingerprint(s: &Sketch) -> String {
        let v: Vec<_> = s
            .vertices
            .iter()
            .map(|(k, p)| (k, p.x.to_bits(), p.y.to_bits()))
            .collect();
        let e: Vec<_> = s.edges.iter().collect();
        format!(
            "{v:?}\n{e:?}\n{:?}\n{:?}\n{}",
            s.regions, s.curves, s.next_curve
        )
    }

    /// A stroke: `n` points from `start`, each step `len` long in direction `a`, turning by
    /// `turn` a step; `kind` sends some steps back onto an earlier point (exactly, or within
    /// or just past the merge distance), along a segment already drawn, or folding back.
    fn stroke(
        start: DVec2,
        steps: &[(f64, f64, u8, f64)],
        scale: f64,
        existing: &[DVec2],
    ) -> Vec<DVec2> {
        let mut p = start;
        let mut a = 0.0f64;
        let mut pts = vec![p];
        for &(len, turn, kind, r) in steps {
            a += turn;
            let pick = |m: usize| (a.abs() * 997.0) as usize % m.max(1);
            p = match kind {
                0 => pts[pick(pts.len())] + DVec2::from_angle(a * 3.0) * r,
                1 if !existing.is_empty() => {
                    existing[pick(existing.len())] + DVec2::from_angle(a) * r
                }
                2 => {
                    let back = pts[pts.len().saturating_sub(2)];
                    if back == p {
                        p + DVec2::from_angle(a) * len * scale
                    } else {
                        let along = back - p;
                        back + along * 0.5 + along.perp().normalize() * r
                    }
                }
                _ => p + DVec2::from_angle(a) * len * scale,
            };
            pts.push(p);
        }
        pts
    }

    fn step() -> impl Strategy<Value = (f64, f64, u8, f64)> {
        (
            prop_oneof![
                Just(1e-5),
                Just(1.0e-5 + 2e-9),
                Just(2e-5),
                Just(1e-4),
                1e-3f64..2e-2,
                Just(0.5),
            ],
            prop_oneof![-0.4f64..0.4, -3.2f64..3.2],
            0u8..12,
            prop_oneof![
                Just(0.0),
                Just(5e-6),
                Just(1e-5),
                Just(1.0e-5 + 2e-9),
                Just(1.5e-5),
                Just(3e-5)
            ],
        )
    }

    /// A sketch: rectangles, a long diagonal (more cells than a lookup will take, at the
    /// small steps), and a stroke already there, all near the origin or `far` out.
    fn base(rects: &[(f64, f64, f64, f64)], diagonal: bool, far: f64, old: &[DVec2]) -> Sketch {
        let mut s = Sketch::new(Plane::GROUND);
        let o = DVec2::splat(far);
        for &(x, y, w, h) in rects {
            let _ = s.add_rectangle(o + DVec2::new(x, y), DVec2::new(w, h));
        }
        if diagonal {
            let _ = s.insert_polyline(
                &[o + DVec2::new(-20.0, -19.0), o + DVec2::new(20.0, 21.0)],
                false,
            );
        }
        if old.len() >= 2 {
            let pts: Vec<DVec2> = old.iter().map(|&p| p + o).collect();
            let _ = s.insert_polyline(&pts, false);
        }
        s
    }

    fn case() -> impl Strategy<Value = (Sketch, Vec<Vec<DVec2>>, Vec<bool>)> {
        (
            prop::collection::vec(
                (
                    -0.05f64..0.05,
                    -0.05f64..0.05,
                    0.001f64..0.06,
                    0.001f64..0.06,
                ),
                0..3,
            ),
            any::<bool>(),
            prop_oneof![Just(0.0), Just(-3.3), Just(9000.0)],
            prop::collection::vec(step(), 0..80),
            prop::collection::vec(
                (
                    prop::collection::vec(step(), 1..300),
                    -0.05f64..0.05,
                    -0.05f64..0.05,
                    any::<bool>(),
                    prop_oneof![Just(1.0), Just(0.1), Just(5.0)],
                ),
                1..4,
            ),
        )
            .prop_map(|(rects, diagonal, far, old_steps, strokes)| {
                let old = stroke(DVec2::ZERO, &old_steps, 1.0, &[]);
                let s = base(&rects, diagonal, far, &old);
                let there: Vec<DVec2> = s.vertices.values().copied().collect();
                let o = DVec2::splat(far);
                let mut all = Vec::new();
                let mut closed = Vec::new();
                for (steps, x, y, c, scale) in strokes {
                    all.push(stroke(o + DVec2::new(x, y), &steps, scale, &there));
                    closed.push(c);
                }
                (s, all, closed)
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]
        #[test]
        fn a_stroke_through_the_lookup_is_the_stroke_scanned((s, strokes, closed) in case()) {
            for (pts, &c) in strokes.iter().zip(&closed) {
                let fast = s.with_polyline_by(pts, c, Some(true));
                let slow = s.with_polyline_by(pts, c, Some(false));
                match (fast, slow) {
                    (Ok((a, ea)), Ok((b, eb))) => {
                        prop_assert_eq!(fingerprint(&a), fingerprint(&b));
                        prop_assert_eq!(ea, eb);
                    }
                    (a, b) => prop_assert_eq!(a.err(), b.err()),
                }
            }
            let list: Vec<(Vec<DVec2>, bool)> =
                strokes.iter().cloned().zip(closed.iter().copied()).collect();
            let (mut a, mut b) = (s.clone(), s.clone());
            let (ra, rb) = (
                a.insert_polylines_by(&list, Some(true)),
                b.insert_polylines_by(&list, Some(false)),
            );
            prop_assert_eq!(&ra, &rb);
            if ra.is_ok() {
                prop_assert_eq!(fingerprint(&a), fingerprint(&b));
            }
        }
    }
}

/// Lane 53 (#65): [`Sketch::edges_after_clear_polyline`], the count of a stroke's edges that
/// is certain before it is inserted: which strokes it counts, and that a count it gives is
/// the count inserting gives.
#[cfg(test)]
mod sure_count_tests {
    use super::*;
    use proptest::prelude::*;

    /// The issue's stroke: an open zigzag, a millimetre a step.
    fn zigzag(n: usize) -> Vec<[f64; 3]> {
        (0..n)
            .map(|k| [0.001 * k as f64, 0.0005 * (k % 2) as f64, 0.0])
            .collect()
    }

    /// A random walk in the sketch's frame: half of them hostile (steps of a few sizes in any
    /// direction, sometimes back over itself, sometimes within the merge distance of where it
    /// has been), half gentle (steps of 1 to 20 mm, now and then 50 mm, turning by up to 45
    /// degrees, now and then jumping to near a point it has been at or folding back past the
    /// point before, at distances either side of the merge distance and the clearance).
    fn walk() -> impl Strategy<Value = (Vec<DVec2>, bool)> {
        prop_oneof![hostile_walk(), gentle_walk()]
    }

    fn gentle_walk() -> impl Strategy<Value = (Vec<DVec2>, bool)> {
        (
            prop::collection::vec(
                (
                    prop_oneof![8 => 0.001f64..0.02, 1 => Just(0.05)],
                    -0.8f64..0.8,
                    0u8..40,
                    prop_oneof![
                        Just(0.0),
                        Just(5e-6),
                        Just(1.5e-5),
                        Just(2.05e-5),
                        Just(3e-5),
                        Just(1e-4)
                    ],
                ),
                1..60,
            ),
            any::<bool>(),
            -0.05f64..0.05,
            -0.05f64..0.05,
            0.0f64..std::f64::consts::TAU,
        )
            .prop_map(|(steps, closed, x, y, a0)| {
                let mut p = DVec2::new(x, y);
                let mut a = a0;
                let mut pts = vec![p];
                for (len, turn, kind, r) in steps {
                    a += turn;
                    let back = pts[pts.len().saturating_sub(2)];
                    p = match kind {
                        0 => {
                            pts[(a.abs() * 1000.0) as usize % pts.len()]
                                + DVec2::from_angle(a * 7.0) * r
                        }
                        // Folding back past the point before, `r` beside the segment it left.
                        1 if back != p => {
                            let along = back - p;
                            back + along * 0.5 + along.perp().normalize() * r
                        }
                        _ => p + DVec2::from_angle(a) * len,
                    };
                    pts.push(p);
                }
                (pts, closed)
            })
    }

    fn hostile_walk() -> impl Strategy<Value = (Vec<DVec2>, bool)> {
        (
            prop::collection::vec(
                (
                    prop_oneof![
                        Just(1e-5),
                        Just(2.5e-5),
                        Just(1e-4),
                        Just(1e-3),
                        0.0f64..0.05
                    ],
                    0.0f64..std::f64::consts::TAU,
                    0u8..8,
                ),
                1..120,
            ),
            any::<bool>(),
            -0.05f64..0.05,
            -0.05f64..0.05,
        )
            .prop_map(|(steps, closed, x, y)| {
                let mut p = DVec2::new(x, y);
                let mut pts = vec![p];
                for (len, a, kind) in steps {
                    p = match kind {
                        // Back to a point already there, or within the merge distance of it.
                        0 => pts[(a * 1000.0) as usize % pts.len()],
                        1 => pts[(a * 1000.0) as usize % pts.len()] + DVec2::from_angle(a) * 1e-5,
                        _ => p + DVec2::from_angle(a) * len,
                    };
                    pts.push(p);
                }
                (pts, closed)
            })
    }

    /// A sketch with up to three rectangles, near the walk's start or across it.
    fn sketch_with(rects: &[(f64, f64, f64, f64)]) -> Sketch {
        let mut s = Sketch::new(Plane::GROUND);
        for &(x, y, w, h) in rects {
            let _ = s.add_rectangle(DVec2::new(x, y), DVec2::new(w, h));
        }
        s
    }

    /// Up to three rectangles, among the walks or (half the time) a metre away.
    fn rects() -> impl Strategy<Value = Vec<(f64, f64, f64, f64)>> {
        (
            prop::collection::vec(
                (
                    -0.06f64..0.06,
                    -0.06f64..0.06,
                    0.001f64..0.05,
                    0.001f64..0.05,
                ),
                0..3,
            ),
            any::<bool>(),
        )
            .prop_map(|(r, away)| {
                let dx = if away { 1.0 } else { 0.0 };
                r.into_iter()
                    .map(|(x, y, w, h)| (x + dx, y, w, h))
                    .collect()
            })
    }

    /// One case: when the count is said to be certain, inserting the stroke gives exactly that
    /// count. Returns whether it was said.
    fn certain_is_exact(pts: &[DVec2], closed: bool, rects: &[(f64, f64, f64, f64)]) -> bool {
        let s = sketch_with(rects);
        let sure = s.edges_after_clear_polyline(pts, closed);
        let mut t = s.clone();
        let r = t.insert_polyline(pts, closed);
        if let Some(n) = sure {
            assert!(
                r.is_ok(),
                "{r:?} for {pts:?} closed {closed} beside {rects:?}"
            );
            assert_eq!(
                t.edge_count(),
                n,
                "{pts:?} closed {closed} beside {rects:?}"
            );
        }
        sure.is_some()
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 2000, ..ProptestConfig::default() })]
        /// Whenever the count is said to be certain, inserting the stroke gives exactly that
        /// count (random seed; `PROPTEST_CASES` for longer runs).
        #[test]
        fn a_certain_count_is_the_count((pts, closed) in walk(), rects in rects()) {
            certain_is_exact(&pts, closed, &rects);
        }
    }

    /// The same on a fixed seed, counting: both answers come up often (so the property above is
    /// not passing by never being sure).
    #[test]
    fn a_certain_count_is_the_count_on_a_fixed_seed() {
        use proptest::strategy::ValueTree;
        let mut runner = proptest::test_runner::TestRunner::deterministic();
        let (mut sure, mut not) = (0, 0);
        for _ in 0..3000 {
            let ((pts, closed), rects) = (walk(), rects()).new_tree(&mut runner).unwrap().current();
            if certain_is_exact(&pts, closed, &rects) {
                sure += 1;
            } else {
                not += 1;
            }
        }
        println!("certain {sure}, not certain {not}");
        assert!(sure >= 300 && not >= 300, "sure {sure}, not {not}");
    }

    /// The shapes the count is certain for, and the ones it is not.
    #[test]
    fn which_strokes_are_counted() {
        let empty = sketch_with(&[]);
        let zig: Vec<DVec2> = zigzag(500).iter().map(|p| DVec2::new(p[0], p[1])).collect();
        assert_eq!(empty.edges_after_clear_polyline(&zig, false), Some(499));
        // Next to a rectangle far away: its 4 edges and the stroke's.
        let far = sketch_with(&[(-1.0, -1.0, 0.4, 0.1)]);
        assert_eq!(far.edges_after_clear_polyline(&zig, false), Some(503));
        // Closed square ring.
        let sq = [
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 0.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(0.0, 1.0),
        ];
        assert_eq!(empty.edges_after_clear_polyline(&sq, true), Some(4));
        // Across the rectangle, on its edge, within the clearance of it: not counted.
        let across = sketch_with(&[(0.1, -0.01, 0.01, 0.02)]);
        assert_eq!(across.edges_after_clear_polyline(&zig, false), None);
        let on = sketch_with(&[(0.0, 0.0005 + 1.5 * SKETCH_MERGE, 0.5, 0.01)]);
        assert_eq!(on.edges_after_clear_polyline(&zig, false), None);
        // Folding back over itself, crossing itself, a step under twice the merge distance.
        let back = [DVec2::ZERO, DVec2::new(1.0, 0.0), DVec2::new(0.5, 1e-6)];
        assert_eq!(empty.edges_after_clear_polyline(&back, false), None);
        let cross = [
            DVec2::ZERO,
            DVec2::new(1.0, 1.0),
            DVec2::new(1.0, 0.0),
            DVec2::new(0.0, 1.0),
        ];
        assert_eq!(empty.edges_after_clear_polyline(&cross, false), None);
        let short = [
            DVec2::ZERO,
            DVec2::new(1.5 * SKETCH_MERGE, 0.0),
            DVec2::new(1.0, 0.0),
        ];
        assert_eq!(empty.edges_after_clear_polyline(&short, false), None);
        // Too few points, a point out of bounds.
        assert_eq!(empty.edges_after_clear_polyline(&sq[..1], false), None);
        assert_eq!(
            empty.edges_after_clear_polyline(&[DVec2::ZERO, DVec2::new(2e4, 0.0)], false),
            None
        );
        // Thousands of segments packed into one spot: not counted (no cheap way to be sure).
        let packed: Vec<DVec2> = (0..5000)
            .map(|k| DVec2::from_angle(k as f64 * 2.4) * 0.01)
            .collect();
        assert_eq!(empty.edges_after_clear_polyline(&packed, false), None);
    }
}

/// Lane 53 (#65): what a [`Lookup`] costs, beside edges much longer than the stroke's
/// segments, and that a lookup dropped on the way leaves what the scans leave.
#[cfg(test)]
mod lookup_cost_tests {
    use super::*;

    /// `k` parallel lines `len` long, 1 cm apart, inserted as one import.
    fn lines(k: usize, len: f64) -> Sketch {
        let mut s = Sketch::new(Plane::GROUND);
        let ls: Vec<(Vec<DVec2>, bool)> = (0..k)
            .map(|i| {
                let y = i as f64 * 0.01;
                (vec![DVec2::new(0.0, y), DVec2::new(len, y)], false)
            })
            .collect();
        s.insert_polylines(&ls).unwrap();
        s
    }

    /// A zigzag of `n` segments of about 0.5 mm, a metre below the lines.
    fn zig(n: usize, y: f64) -> Vec<DVec2> {
        (0..=n)
            .map(|i| DVec2::new(i as f64 * 0.0005, y + (i % 2) as f64 * 0.0001))
            .collect()
    }

    fn fingerprint(s: &Sketch) -> String {
        let v: Vec<_> = s
            .vertices
            .iter()
            .map(|(k, p)| (k, p.x.to_bits(), p.y.to_bits()))
            .collect();
        let e: Vec<_> = s.edges.iter().collect();
        format!("{v:?}\n{e:?}\n{:?}", s.regions)
    }

    /// The review's case: 2,000 lines of 1 m and a stroke of 1,000 segments of 0.5 mm. With
    /// a cell of the stroke's segment the lines alone need 4,000 samples each, 8 million in
    /// all (what the lookup before this took, up to its budget, before it gave up); the
    /// lookup is decided before anything is built, and holds no more than its ceiling all
    /// the way through, and the stroke goes in exactly as with the scans.
    #[test]
    fn long_edges_beside_a_short_stroke_stay_under_the_ceiling() {
        let s = lines(2000, 1.0);
        let pts = zig(1000, -1.0);
        let size = s.vertices.len() + s.edges.len();
        let ceiling = lookup_ceiling(size, 1000);
        let at_stroke_cell: usize = s
            .edges
            .values()
            .map(|[a, b]| samples(s.vertices[*a].distance(s.vertices[*b]), 0.0005))
            .sum();
        assert!(at_stroke_cell > 20 * ceiling, "{at_stroke_cell} {ceiling}");
        let mut look = Lookup::for_strokes(&s, std::iter::once((&pts[..], false)), 0);
        if let Some(l) = &look {
            assert!(l.entries() <= ceiling, "{} > {ceiling}", l.entries());
        }
        let fast = s.with_polyline_through(&pts, false, &mut look).unwrap();
        if let Some(l) = &look {
            assert!(l.entries() <= ceiling, "{} > {ceiling}", l.entries());
        }
        let slow = s.with_polyline_by(&pts, false, Some(false)).unwrap();
        assert_eq!(fingerprint(&fast.0), fingerprint(&slow.0));
        assert_eq!(fast.1, slow.1);
    }

    /// Lines of 1 m and a stroke of 3,000 short segments packed into one of their cells: the
    /// lookup is built (with the lines' cell), the stroke crowds that cell past an eighth of
    /// all it holds, the lookup is dropped half way and the scans finish: the same sketch.
    #[test]
    fn a_lookup_crowded_on_the_way_is_dropped_and_changes_nothing() {
        let s = lines(200, 1.0);
        let pts = zig(1500, 0.5005);
        let mut look = Lookup::for_strokes(&s, std::iter::once((&pts[..], false)), 0);
        assert!(look.is_some(), "a lookup is worth building here");
        let fast = s.with_polyline_through(&pts, false, &mut look).unwrap();
        assert!(look.is_none(), "dropped on the way");
        let slow = s.with_polyline_by(&pts, false, Some(false)).unwrap();
        assert_eq!(fingerprint(&fast.0), fingerprint(&slow.0));
        assert_eq!(fast.1, slow.1);
    }

    /// A lookup whose ceiling runs out part way through a stroke (forced here: room for ten
    /// more entries) is dropped there, and the stroke goes in exactly as with the scans.
    #[test]
    fn a_lookup_out_of_room_part_way_is_dropped_and_changes_nothing() {
        // The rectangle clear of the stroke, so nothing splits: running out of room is the
        // only way the lookup can be dropped here.
        let mut s = Sketch::new(Plane::GROUND);
        s.add_rectangle(DVec2::new(-0.01, -1.0), DVec2::new(0.3, 0.0002))
            .unwrap();
        let pts = zig(600, 0.0);
        // Cells of 2 mm: four of the stroke's points to a cell, none crowded.
        let mut look = Lookup::build(&s, 0.002, usize::MAX);
        let l = look.as_mut().unwrap();
        l.ceiling = l.entries() + 10;
        let fast = s.with_polyline_through(&pts, false, &mut look).unwrap();
        assert!(look.is_none(), "dropped on the way");
        let slow = s.with_polyline_by(&pts, false, Some(false)).unwrap();
        assert_eq!(fingerprint(&fast.0), fingerprint(&slow.0));
        assert_eq!(fast.1, slow.1);
    }

    /// A query along a segment far longer than the cells scans instead of taking a cell per
    /// sample: nothing is allocated for it (a 10 km segment in cells of 0.5 mm would be 40
    /// million samples).
    #[test]
    fn a_very_long_query_scans_instead() {
        let s = Sketch::new(Plane::GROUND);
        let pts = zig(1000, 0.0);
        let l = Lookup::for_strokes(&s, std::iter::once((&pts[..], false)), 0).unwrap();
        let (a, b) = (DVec2::new(-5000.0, 0.0), DVec2::new(5000.0, 0.0));
        assert!(sample_keys(a, b, l.cell, 4096).is_none());
        assert!(l.query_cells(a, b, Lookup::query_most(0)).is_none());
        assert!(l.vertices_near(a, b, &s.vertices, 0).is_none());
        assert!(l.edges_near(a, b, &s.edges, 0).is_none());
        // A short one is looked up.
        let c = DVec2::new(0.001, 0.0);
        assert!(
            l.query_cells(DVec2::ZERO, c, Lookup::query_most(0))
                .is_some()
        );
    }
}

/// Lane 53 (#65): the lookup against the scans where it is easiest to get wrong: steps of one
/// to a few merge distances, so the cell is its floor (four merge distances), with every point
/// hugging a cell border (on it, a rounding either side, or a fraction of the merge distance
/// either side), beside short edges that hug the same borders. Any cell a lookup forgot, a
/// neighbour ring it dropped or samples spaced too far apart lose a candidate the scans find,
/// and the sketch comes out different (keys included).
#[cfg(test)]
mod lookup_border_tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::strategy::ValueTree;

    fn fingerprint(s: &Sketch) -> String {
        let v: Vec<_> = s
            .vertices
            .iter()
            .map(|(k, p)| (k, p.x.to_bits(), p.y.to_bits()))
            .collect();
        let e: Vec<_> = s.edges.iter().collect();
        format!("{v:?}\n{e:?}\n{:?}", s.regions)
    }

    /// The cell a lookup takes for steps this short.
    const CELL: f64 = 4.0 * SKETCH_MERGE;

    /// A point on the lattice of half cells (`i`, `j`: every other line a cell border), so a
    /// step is shorter than a cell and the cell its floor, moved by one of the offsets that
    /// matter: none, a rounding, a fraction of the merge distance, or about one.
    fn hug(i: i32, j: i32, dx: u8, dy: u8) -> DVec2 {
        const OFF: [f64; 8] = [0.0, 1e-13, -1e-13, 0.2, -0.2, 0.45, -0.45, 0.9];
        DVec2::new(
            i as f64 * 0.5 * CELL + OFF[dx as usize % 8] * SKETCH_MERGE,
            j as f64 * 0.5 * CELL + OFF[dy as usize % 8] * SKETCH_MERGE,
        )
    }

    /// A walk over the lattice half a cell a step, and edges already there: most half a cell
    /// long, some two to twelve cells long, along a border or across the cells.
    fn case() -> impl Strategy<Value = (Vec<DVec2>, Vec<(DVec2, DVec2)>)> {
        (
            prop::collection::vec((-1i32..=1, -1i32..=1, any::<u8>(), any::<u8>()), 48..160),
            prop::collection::vec(
                (
                    -6i32..6,
                    -6i32..6,
                    -1i32..=1,
                    -1i32..=1,
                    any::<u8>(),
                    any::<u8>(),
                    prop_oneof![Just(1i32), 4i32..24],
                ),
                0..24,
            ),
            -3i32..3,
            -3i32..3,
            (any::<bool>(), 8i32..60, any::<u8>()),
        )
            .prop_map(|(steps, old, i0, j0, (along, k, off))| {
                let (mut i, mut j) = (i0, j0);
                let mut pts = vec![hug(i, j, 0, 0)];
                for (di, dj, dx, dy) in steps {
                    let (di, dj) = if di == 0 && dj == 0 { (1, 0) } else { (di, dj) };
                    i += di;
                    j += dj;
                    pts.push(hug(i, j, dx, dy));
                }
                let mut old: Vec<(DVec2, DVec2)> = old
                    .into_iter()
                    .map(|(a, b, di, dj, dx, dy, k)| {
                        // Mostly one step, sometimes a long edge along or across the cells.
                        let (di, dj) = if di == 0 && dj == 0 { (0, 1) } else { (di, dj) };
                        (hug(a, b, dx, dy), hug(a + k * di, b + k * dj, dy, dx))
                    })
                    .collect();
                if along {
                    // A long edge on a cell border, and the stroke running along it under
                    // the merge distance away: every point of the stroke needs that edge
                    // found, wherever it falls between the edge's samples.
                    old.push((hug(i0, j0, 0, 0), hug(i0 + k, j0, 0, 0)));
                    pts = (0..k)
                        .map(|t| {
                            hug(i0, j0, 0, 0)
                                + DVec2::new(
                                    (t as f64 + 0.3) * 0.5 * CELL,
                                    if (t + off as i32) % 2 == 0 {
                                        0.45
                                    } else {
                                        -0.45
                                    } * SKETCH_MERGE,
                                )
                        })
                        .collect();
                }
                (pts, old)
            })
    }

    /// One case: the stroke through a lookup and through the scans leave the same sketch,
    /// keys included, or both refuse it the same way. The lookup takes the cell it picks, or
    /// (`floor`) the smallest cell, whose borders the points hug whatever the edges' lengths.
    fn same_both_ways(pts: &[DVec2], old: &[(DVec2, DVec2)], floor: bool) {
        let mut s = Sketch::new(Plane::GROUND);
        for &(a, b) in old {
            let _ = s.insert_polyline(&[a, b], false);
        }
        if floor {
            let mut look = Lookup::build(&s, Lookup::floor(), usize::MAX);
            assert!(look.is_some());
            let fast = s.with_polyline_through(pts, false, &mut look);
            let slow = s.with_polyline_by(pts, false, Some(false));
            match (fast, slow) {
                (Ok((a, ea)), Ok((b, eb))) => {
                    assert_eq!(fingerprint(&a), fingerprint(&b), "{pts:?} beside {old:?}");
                    assert_eq!(ea, eb);
                }
                (a, b) => assert_eq!(a.err(), b.err()),
            }
            return;
        }
        let short: Vec<(DVec2, DVec2)> = old
            .iter()
            .copied()
            .filter(|(a, b)| a.distance(*b) < CELL)
            .collect();
        let mut s = Sketch::new(Plane::GROUND);
        for &(a, b) in &short {
            let _ = s.insert_polyline(&[a, b], false);
        }
        let old = &short[..];
        let look = Lookup::for_strokes(&s, std::iter::once((pts, false)), 0);
        if let Some(l) = &look {
            assert!(
                l.cell < 1.25 * CELL,
                "a cell near its floor here: {}",
                l.cell
            );
        }
        let fast = s.with_polyline_by(pts, false, Some(true));
        let slow = s.with_polyline_by(pts, false, Some(false));
        match (fast, slow) {
            (Ok((a, ea)), Ok((b, eb))) => {
                assert_eq!(fingerprint(&a), fingerprint(&b), "{pts:?} beside {old:?}");
                assert_eq!(ea, eb);
            }
            (a, b) => assert_eq!(a.err(), b.err()),
        }
    }

    /// On a fixed seed, in every `cargo test`, half the cases at the cell the lookup picks
    /// (short edges only) and half at the smallest cell (long edges too). Planted faults in
    /// the lookup are caught here: a dropped neighbour column, a cell under the merge
    /// distance, edges looked up without their neighbour ring, samples four times too far
    /// apart.
    #[test]
    fn strokes_hugging_cell_borders_on_a_fixed_seed() {
        let mut runner = proptest::test_runner::TestRunner::deterministic();
        for k in 0..400 {
            let (pts, old) = case().new_tree(&mut runner).unwrap().current();
            same_both_ways(&pts, &old, k % 2 == 1);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 200, ..ProptestConfig::default() })]
        /// The same on a random seed (`PROPTEST_CASES` for longer runs).
        #[test]
        fn strokes_hugging_cell_borders((pts, old) in case(), floor in any::<bool>()) {
            same_both_ways(&pts, &old, floor);
        }
    }
}

/// Lane 53 (#65): when a lookup is built, and the arithmetic of its samples.
#[cfg(test)]
mod lookup_decision_tests {
    use super::*;

    fn circles(n: usize) -> Sketch {
        let mut s = Sketch::new(Plane::GROUND);
        let st: Vec<(Vec<DVec2>, bool)> = (0..n)
            .map(|i| {
                let c = DVec2::new((i % 40) as f64 * 0.1, (i / 40) as f64 * 0.1);
                let ring = (0..24)
                    .map(|k| c + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 24.0) * 0.04)
                    .collect();
                (ring, true)
            })
            .collect();
        s.insert_polylines(&st).unwrap();
        s
    }

    fn zig(segs: usize, y: f64) -> Vec<DVec2> {
        (0..=segs)
            .map(|i| DVec2::new(i as f64 * 0.001, y + (i % 2) as f64 * 0.0005))
            .collect()
    }

    fn decided(s: &Sketch, pts: &[DVec2]) -> bool {
        Lookup::for_strokes(s, std::iter::once((pts, false)), LOOKUP_MIN_WORK).is_some()
    }

    /// Beside 400 circles a stroke of 47 segments scans and one of 48 or 512 is looked up;
    /// a stroke beside 250 lines 0.52 m long, 3 mm apart, of 128 segments of 0.5 mm scans
    /// (a lookup there would find most of the lines in every query), one of 2,048 does not.
    #[test]
    fn a_lookup_is_built_only_where_it_pays() {
        let s = circles(400);
        assert!(!decided(&s, &zig(47, -1.0)));
        assert!(decided(&s, &zig(48, -1.0)));
        assert!(decided(&s, &zig(512, -1.0)));
        let mut l = Sketch::new(Plane::GROUND);
        let lines: Vec<(Vec<DVec2>, bool)> = (0..250)
            .map(|j| {
                let x = j as f64 * 0.003;
                (vec![DVec2::new(x, 0.0), DVec2::new(x, 0.52)], false)
            })
            .collect();
        l.insert_polylines(&lines).unwrap();
        let near = |segs: usize| -> Vec<DVec2> {
            (0..=segs)
                .map(|q| DVec2::new(0.0005 * q as f64 + 0.0007, -0.05 + 0.0004 * (q % 2) as f64))
                .collect()
        };
        assert!(!decided(&l, &near(128)));
        assert!(decided(&l, &near(2048)));
    }

    /// A query that comes back with more than a quarter of a sketch of 4,096 or more is
    /// counted as poor, and a lookup with [`LOOKUP_POOR`] of them is dropped at the next
    /// segment, the stroke going in as with the scans.
    #[test]
    fn a_lookup_finding_most_of_the_sketch_is_dropped() {
        let s = circles(200);
        let pts = zig(200, -1.0);
        let l = Lookup::for_strokes(&s, std::iter::once((&pts[..], false)), 0).unwrap();
        l.found(100, 4000);
        l.found(1000, 9600);
        assert_eq!(l.poor.get(), 0, "a small sketch, or a quarter or less");
        for _ in 0..LOOKUP_POOR {
            assert!(!l.poor());
            l.found(2401, 9600);
        }
        assert!(l.poor());
        let mut look = Some(l);
        let fast = s.with_polyline_through(&pts, false, &mut look).unwrap();
        assert!(look.is_none(), "dropped");
        let slow = s.with_polyline_by(&pts, false, Some(false)).unwrap();
        assert_eq!(format!("{:?}", fast.0.edges), format!("{:?}", slow.0.edges));
        assert_eq!(fast.1, slow.1);
    }

    /// The samples of a segment never overflow a 32-bit `usize` (wasm32): a segment far over
    /// 86 km in the smallest cells takes as many as `u32::MAX`, never wraps to a few.
    #[test]
    fn samples_fit_a_32_bit_usize() {
        let max = u32::MAX as usize;
        let cell = Lookup::floor();
        for len in [9.0e4, 1.0e5, 2.0e4 * 1.0e4, 1e300, f64::INFINITY, f64::NAN] {
            assert_eq!(samples_within(len, cell, max), max, "{len}");
        }
        for len in [0.0, 1e-5, 0.5, 1.0e3] {
            assert_eq!(samples_within(len, cell, max), samples(len, cell));
            assert!(samples(len, cell) >= 2);
        }
        // Just under and over what 32 bits hold.
        let at = |steps: f64| samples_within(steps * 0.5 * cell, cell, max);
        assert_eq!(at(4.0e9), 4_000_000_002);
        assert_eq!(at(4.3e9), max);
    }
}
