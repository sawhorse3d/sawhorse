//! Curves (lane 23, ADR-0013): what the Circle, Arc and Polygon tools draw is **one
//! curve**, an ordered chain of the straight edges that approximate it, as in SketchUp.
//!
//! # In a sketch
//!
//! A sketch keeps its curves beside its graph ([`crate::sketch::Sketch`]): each is a chain
//! of vertex ids (closed for a circle or a polygon), a kind, the number of segments it was
//! drawn with and, for a circle, an arc or a polygon, the circle it lies on ([`Round`]: the
//! centre and radius in the sketch frame, and the start angle and signed sweep of its
//! points about the centre, counter-clockwise about the plane's normal). An edge belongs to
//! at most one curve. The graph's own rules (merges, splits, welds) never look at curves;
//! after every edit the curves are **settled** against the graph:
//!
//! 1. a curve segment another edge split (a crossing, a T-junction) takes the new vertices
//!    into the chain: the segment is still the curve's, as a run of straight pieces;
//! 2. at a **junction** (a chain vertex where another edge meets the curve) the curve is
//!    split: an open curve into two, a closed one only from its second junction on (one
//!    line ending on a circle leaves the circle whole, two make two arcs). A circle or an
//!    arc splits into arcs on the same circle, with their own angles; a polygon or a
//!    polyline into polylines;
//! 3. a curve that **lost** an edge or a vertex (erased, consumed by a push, merged away,
//!    claimed by an older curve it now coincides with) becomes polyline pieces of what is
//!    left, never a chain with a hole in it.
//!
//! The first piece keeps the curve's id; the others get new ones. Ids come from a counter
//! the sketch keeps (saved with it), so a curve's id is stable through undo and redo,
//! `.shz` save and load, and moves.
//!
//! # On a solid
//!
//! Pushing a region bounded by a curve into a solid leaves the curve as the rim of both caps
//! ([`RimCurve`]), keyed the way lane 15 keys edge flags: each rim edge by the pair of
//! persistent face ids it lies between (a cap and one side wall). The key follows the edge
//! through moves, copies, mirrors and component placements with no bookkeeping; after every
//! edit a rim edge whose key no longer names exactly one edge drops out and the rim splits
//! there (circle rims into arcs, polygon rims into polylines). A rim's circle is read from
//! its points when asked for (so a scaled solid's rim needs nothing), and a rim scaled out of
//! round reads as a polyline.

use std::collections::{BTreeMap, BTreeSet};

/// The largest curve id a file may hold (lane 23b): 2^53 - 1. Ids reach the app as JSON
/// numbers, which JavaScript reads exactly only up to here, and a counter this far below
/// `u64::MAX` can never wrap. A `.shz` with a curve id, or a sketch's curve counter, beyond
/// it is refused on open ([`crate::snapshot`]).
pub const MAX_CURVE_ID: u64 = (1 << 53) - 1;
use std::f64::consts::TAU;

use glam::{DVec2, DVec3};

use crate::ids::{EdgeId, SVertexId, VertexId};
use crate::object::Object;
use crate::sketch::point_segment_distance;
use crate::tolerance::{CURVE_FIT, POINT_MERGE};

/// What a curve is.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum CurveKind {
    /// Closed, on a circle (the Circle tool).
    Circle,
    /// Open, on a circle (the Arc tools, or a piece of a split circle).
    Arc,
    /// Closed, its corners on a circle (the Polygon tool).
    Polygon,
    /// A plain chain of edges: what a split polygon or a damaged curve leaves.
    Polyline,
}

impl CurveKind {
    /// On a circle (circle, arc, polygon).
    pub fn is_round(self) -> bool {
        !matches!(self, CurveKind::Polyline)
    }

    /// Whether the kind is closed (`None`: either).
    pub fn closes(self) -> Option<bool> {
        match self {
            CurveKind::Circle | CurveKind::Polygon => Some(true),
            CurveKind::Arc => Some(false),
            CurveKind::Polyline => None,
        }
    }

    /// The word for it ("circle", "arc", "polygon", "curve").
    pub fn word(self) -> &'static str {
        match self {
            CurveKind::Circle => "circle",
            CurveKind::Arc => "arc",
            CurveKind::Polygon => "polygon",
            CurveKind::Polyline => "curve",
        }
    }
}

/// The circle a round curve lies on, in its sketch's 2D frame: centre and radius (metres),
/// the angle of its first point about the centre and the signed angle its points turn
/// through, segment by segment (radians, counter-clockwise about the plane's normal; a
/// closed curve turns through a whole turn).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Round {
    pub centre: DVec2,
    pub radius: f64,
    pub start: f64,
    pub sweep: f64,
}

/// A curve of a sketch: its kind, its chain of vertices (in order; a closed chain does not
/// repeat its first vertex), the number of segments it was drawn with (a circle's or a
/// polygon's count, an arc's or a polyline's edges) and, for a round kind, its circle.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub kind: CurveKind,
    pub chain: Vec<SVertexId>,
    pub closed: bool,
    pub segments: u32,
    pub round: Option<Round>,
}

impl Curve {
    /// The chain's edges as vertex pairs, in order (a closed chain's last joins the first).
    pub fn pairs(&self) -> impl Iterator<Item = (SVertexId, SVertexId)> + '_ {
        let n = self.chain.len();
        let m = if self.closed { n } else { n.saturating_sub(1) };
        (0..m).map(move |i| (self.chain[i], self.chain[(i + 1) % n]))
    }

    /// Number of edges.
    pub fn edge_count(&self) -> usize {
        let n = self.chain.len();
        if self.closed { n } else { n.saturating_sub(1) }
    }

    /// The segment count to show: a circle's or a polygon's as drawn, otherwise its edges.
    pub fn shown_segments(&self) -> u32 {
        match self.kind {
            CurveKind::Circle | CurveKind::Polygon => self.segments,
            _ => self.edge_count() as u32,
        }
    }
}

/// A curve as plain data (in [`crate::sketch::RawSketch`], so in `.shz` files): its id, kind,
/// vertex indices in order, whether it closes, its segment count and its circle (absent for
/// a polyline).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct RawCurve {
    pub id: u64,
    pub kind: CurveKind,
    pub vertices: Vec<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    pub segments: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub centre: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep: Option<f64>,
}

impl RawCurve {
    /// The circle, when all four of its numbers are given (`Err` when only some are).
    pub fn round(&self) -> Result<Option<Round>, String> {
        match (self.centre, self.radius, self.start, self.sweep) {
            (Some(c), Some(radius), Some(start), Some(sweep)) => Ok(Some(Round {
                centre: DVec2::from_array(c),
                radius,
                start,
                sweep,
            })),
            (None, None, None, None) => Ok(None),
            _ => Err(format!("curve {}: its circle is incomplete", self.id)),
        }
    }
}

/// What an op says a stroke is (the Circle, Arc and Polygon tools; an import): the kind and,
/// for a circle, an arc or a polygon, its centre (world metres). The stroke's points must lie
/// on a circle about that centre.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct CurveSpec {
    pub kind: CurveKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub centre: Option<[f64; 3]>,
}

/// How far a point may sit off `r`'s circle ([`CURVE_FIT`] plus rounding of its size).
pub fn fit_tolerance(r: &Round) -> f64 {
    CURVE_FIT + 1e-9 * (r.centre.length() + r.radius)
}

/// The signed angle (radians, counter-clockwise) from `a` to `b` about `c`.
fn step(c: DVec2, a: DVec2, b: DVec2) -> f64 {
    let (u, v) = (a - c, b - c);
    u.perp_dot(v).atan2(u.dot(v))
}

/// The start angle of `pts` about `centre` and the signed angle they turn through segment
/// by segment (a closed chain's last segment back to the first point included). `None` when
/// a point is at the centre or something is not finite.
pub fn fit_angles(centre: DVec2, pts: &[DVec2], closed: bool) -> Option<(f64, f64)> {
    let p0 = *pts.first()?;
    let d0 = p0 - centre;
    if !(d0.is_finite() && d0.length() > 0.0) {
        return None;
    }
    let n = pts.len();
    let m = if closed { n } else { n - 1 };
    let mut sweep = 0.0;
    for i in 0..m {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if a == centre || b == centre {
            return None;
        }
        let s = step(centre, a, b);
        if !s.is_finite() {
            return None;
        }
        sweep += s;
    }
    let start = d0.y.atan2(d0.x);
    (start.is_finite() && sweep.is_finite()).then_some((start, sweep))
}

/// The scale a 2x2 linear map (columns: the images of the frame's axes) applies to every
/// length, when it is a similarity (equal column lengths, square to each other, within one
/// part in 1e9); `None` otherwise or when it is singular.
pub fn similarity_scale(cu: DVec2, cv: DVec2) -> Option<f64> {
    let (a, b) = (cu.length(), cv.length());
    let s = a.max(b);
    if !(s.is_finite() && s > 0.0) {
        return None;
    }
    ((a - b).abs() <= 1e-9 * s && cu.dot(cv).abs() <= 1e-9 * s * s).then_some(0.5 * (a + b))
}

/// Check a round curve's points against its circle (the validator's rule): every point on
/// the circle within [`fit_tolerance`], or on the straight segment joining its two
/// neighbours along the chain (a vertex another edge split a segment at), or, at either end
/// of an open chain, no further from the centre than the radius (a split's end on a chord);
/// the start angle points at the first point; the points turn one way round the centre, by
/// the stored sweep, and a closed chain once round exactly.
pub fn check_round(r: &Round, pts: &[DVec2], closed: bool) -> Result<(), String> {
    if !(r.centre.is_finite()
        && r.radius.is_finite()
        && r.radius > 0.0
        && r.start.is_finite()
        && r.sweep.is_finite())
    {
        return Err("a curve's circle is not finite".into());
    }
    let n = pts.len();
    if n < 2 {
        return Err("a curve has fewer than two points".into());
    }
    let tol = fit_tolerance(r);
    for i in 0..n {
        let p = pts[i];
        let d = p.distance(r.centre);
        if (d - r.radius).abs() <= tol {
            continue;
        }
        let interior = closed || (i > 0 && i + 1 < n);
        if interior {
            let (a, b) = (pts[(i + n - 1) % n], pts[(i + 1) % n]);
            if point_segment_distance(p, a, b) <= tol {
                continue;
            }
        } else if d <= r.radius + tol {
            continue;
        }
        return Err(format!(
            "curve point {p} is {:.3e} m off its circle (radius {})",
            (d - r.radius).abs(),
            r.radius
        ));
    }
    let (_, sweep) = fit_angles(r.centre, pts, closed).ok_or("a curve's point is at its centre")?;
    let u = DVec2::from_angle(r.start);
    let q = pts[0] - r.centre;
    if u.perp_dot(q).abs() > tol || u.dot(q) < -tol {
        return Err("a curve's start angle does not point at its first point".into());
    }
    if (sweep - r.sweep).abs() > 1e-6 * (1.0 + sweep.abs()) {
        return Err(format!(
            "a curve turns through {sweep} rad but says {}",
            r.sweep
        ));
    }
    if closed && (sweep.abs() - TAU).abs() > 1e-6 {
        return Err(format!(
            "a closed curve turns through {sweep} rad, not once round"
        ));
    }
    if !closed && sweep.abs() > TAU + 1e-6 {
        return Err(format!("an arc turns through {sweep} rad"));
    }
    let m = if closed { n } else { n - 1 };
    for i in 0..m {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if a.distance(r.centre) <= tol || b.distance(r.centre) <= tol {
            continue;
        }
        let s = step(r.centre, a, b);
        if s.abs() > 1e-9 && s.signum() != sweep.signum() {
            return Err("a curve turns back on itself".into());
        }
    }
    Ok(())
}

/// A curve on a solid (lane 23): the rim a pushed circle, arc or polygon leaves on each cap.
/// `keys` are its edges in order, each named by the pair of persistent face ids it lies
/// between (smaller first). Its circle is read from its points ([`Object::curve_info`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RimCurve {
    pub kind: CurveKind,
    pub closed: bool,
    pub segments: u32,
    pub keys: Vec<(u64, u64)>,
}

impl RimCurve {
    /// The segment count to show (see [`Curve::shown_segments`]).
    pub fn shown_segments(&self) -> u32 {
        match self.kind {
            CurveKind::Circle | CurveKind::Polygon => self.segments,
            _ => self.keys.len() as u32,
        }
    }
}

/// A curve as the UI shows it (world coordinates): its kind as it reads now (a rim scaled
/// out of round reads as a polyline), whether it closes, its segment count, its centre,
/// radius and sweep (radians) when it is round, its plane's normal, and its points in order.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CurveInfo {
    pub kind: CurveKind,
    pub closed: bool,
    pub segments: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub centre: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sweep: Option<f64>,
    pub normal: [f64; 3],
    pub points: Vec<[f64; 3]>,
}

/// The circle through three points in space: (centre, unit normal), `None` when they are
/// (nearly) in a line.
pub fn circle_through(a: DVec3, b: DVec3, c: DVec3) -> Option<(DVec3, DVec3)> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let d = 2.0 * n.length_squared();
    let scale = ab.length_squared().max(ac.length_squared());
    if !(d.is_finite() && d > 1e-24 * scale * scale) {
        return None;
    }
    let centre = a + (ac.length_squared() * n.cross(ab) + ab.length_squared() * ac.cross(n)) / d;
    centre.is_finite().then(|| (centre, n.normalize()))
}

/// The circle a chain of points lies on in space: through three of them spread along it,
/// with every point within [`CURVE_FIT`] (and rounding) of it and of its plane. `None` when
/// there are fewer than three points or they do not lie on one circle.
pub fn round_of_points(pts: &[DVec3], closed: bool) -> Option<(DVec3, DVec3, f64)> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let (i, j, k) = if closed {
        (0, n / 3, (2 * n) / 3)
    } else {
        (0, n / 2, n - 1)
    };
    let (c, normal) = circle_through(pts[i], pts[j], pts[k])?;
    let radius = c.distance(pts[i]);
    let tol = CURVE_FIT + 1e-9 * (c.length() + radius);
    pts.iter()
        .all(|p| (p.distance(c) - radius).abs() <= tol && (*p - c).dot(normal).abs() <= tol)
        .then_some((c, normal, radius))
}

/// The signed angle `pts` turn through about `centre` across `normal`, segment by segment.
pub fn sweep_about(centre: DVec3, normal: DVec3, pts: &[DVec3], closed: bool) -> f64 {
    let n = pts.len();
    let m = if closed { n } else { n.saturating_sub(1) };
    (0..m)
        .map(|i| {
            let (u, v) = (pts[i] - centre, pts[(i + 1) % n] - centre);
            u.cross(v).dot(normal).atan2(u.dot(v))
        })
        .sum()
}

/// One rim edge found by its key: the edge and its two vertices.
type KeyedEdge = (EdgeId, VertexId, VertexId);

impl Object {
    /// The curves on this solid (lane 23), by id.
    pub fn curves(&self) -> &BTreeMap<u64, RimCurve> {
        &self.curves
    }

    /// Every edge with a face-id key, by key (a key two edges share is listed twice).
    fn keyed_edges(&self) -> BTreeMap<(u64, u64), Vec<KeyedEdge>> {
        let b = self.brep();
        let mut out: BTreeMap<(u64, u64), Vec<KeyedEdge>> = BTreeMap::new();
        for (e, edge) in b.edges() {
            if let Some(k) = self.edge_key(e) {
                let (p, q) = (b.half_edges[edge.he].origin, b.dest(edge.he));
                out.entry(k).or_default().push((e, p, q));
            }
        }
        out
    }

    /// The vertex chain of a rim's keys, edge by edge (`None` where a key does not name
    /// exactly one edge or two consecutive edges do not meet).
    fn rim_chain(
        keys: &[(u64, u64)],
        closed: bool,
        by_key: &BTreeMap<(u64, u64), Vec<KeyedEdge>>,
    ) -> Option<Vec<VertexId>> {
        let one = |k: &(u64, u64)| match by_key.get(k).map(Vec::as_slice) {
            Some([e]) => Some(*e),
            _ => None,
        };
        let first = one(keys.first()?)?;
        let mut chain: Vec<VertexId> = Vec::new();
        if keys.len() == 1 {
            return (!closed).then(|| vec![first.1, first.2]);
        }
        let second = one(&keys[1])?;
        // Orient the first edge so it ends where the second one is.
        let (a, b) = if second.1 == first.2 || second.2 == first.2 {
            (first.1, first.2)
        } else if second.1 == first.1 || second.2 == first.1 {
            (first.2, first.1)
        } else {
            return None;
        };
        chain.push(a);
        chain.push(b);
        for k in &keys[1..] {
            let (_, p, q) = one(k)?;
            let last = *chain.last()?;
            let next = if p == last {
                q
            } else if q == last {
                p
            } else {
                return None;
            };
            chain.push(next);
        }
        if closed {
            if chain.last() != chain.first() {
                return None;
            }
            chain.pop();
        }
        let distinct: BTreeSet<VertexId> = chain.iter().copied().collect();
        (distinct.len() == chain.len()).then_some(chain)
    }

    /// After an edit (lane 23): every rim edge whose key still names exactly one edge stays,
    /// in order; where one dropped out the rim splits there (a circle or an arc into arcs, a
    /// polygon or a polyline into polylines; pieces too short to be curves go). The first
    /// piece keeps the id, the others take new ones.
    pub(crate) fn settle_curves(&mut self) {
        if self.curves.is_empty() {
            return;
        }
        let by_key = self.keyed_edges();
        let usable = |k: &(u64, u64)| by_key.get(k).is_some_and(|v| v.len() == 1);
        let mut claimed: BTreeSet<(u64, u64)> = BTreeSet::new();
        let old = std::mem::take(&mut self.curves);
        // Checked (lane 23b): a hostile file's rim id of u64::MAX overflowed here in a
        // debug build; files are refused beyond MAX_CURVE_ID now, and an id that cannot be
        // had leaves its piece plain edges rather than wrapping onto another curve's.
        let mut next = old.keys().next_back().map_or(Some(1), |k| k.checked_add(1));
        let mut out: BTreeMap<u64, RimCurve> = BTreeMap::new();
        for (id, c) in old {
            if Self::rim_chain(&c.keys, c.closed, &by_key).is_some()
                && c.keys.iter().all(|k| !claimed.contains(k))
            {
                claimed.extend(c.keys.iter().copied());
                out.insert(id, c);
                continue;
            }
            // Runs of usable keys that meet edge to edge.
            let mut runs: Vec<Vec<(u64, u64)>> = Vec::new();
            let mut cur: Vec<(u64, u64)> = Vec::new();
            for k in &c.keys {
                let ok = usable(k) && !claimed.contains(k) && !cur.contains(k);
                if ok {
                    let mut trial = cur.clone();
                    trial.push(*k);
                    if Self::rim_chain(&trial, false, &by_key).is_some() {
                        cur = trial;
                        continue;
                    }
                    if !cur.is_empty() {
                        runs.push(std::mem::take(&mut cur));
                    }
                    cur.push(*k);
                } else if !cur.is_empty() {
                    runs.push(std::mem::take(&mut cur));
                }
            }
            if !cur.is_empty() {
                runs.push(cur);
            }
            // A closed rim's last run continues into its first.
            if c.closed && runs.len() >= 2 && runs[0][0] == c.keys[0] {
                let last = runs.last().cloned().unwrap_or_default();
                if last.last() == c.keys.last() {
                    let mut joined = last.clone();
                    joined.extend(runs[0].iter().copied());
                    if Self::rim_chain(&joined, false, &by_key).is_some() {
                        runs.pop();
                        runs[0] = joined;
                    }
                }
            }
            let kind = match c.kind {
                CurveKind::Circle | CurveKind::Arc => CurveKind::Arc,
                _ => CurveKind::Polyline,
            };
            let mut first = true;
            for run in runs {
                let min = if kind == CurveKind::Arc { 1 } else { 2 };
                if run.len() < min || run.iter().any(|k| claimed.contains(k)) {
                    continue;
                }
                claimed.extend(run.iter().copied());
                let piece = RimCurve {
                    kind,
                    closed: false,
                    segments: run.len() as u32,
                    keys: run,
                };
                let k = if first {
                    first = false;
                    id
                } else {
                    let Some(n) = next else {
                        continue;
                    };
                    next = n.checked_add(1);
                    n
                };
                out.insert(k, piece);
            }
        }
        self.curves = out;
    }

    /// The rim's points in order (world), `None` for an unknown id or a broken rim.
    pub fn curve_points(&self, id: u64) -> Option<Vec<DVec3>> {
        let c = self.curves.get(&id)?;
        let chain = Self::rim_chain(&c.keys, c.closed, &self.keyed_edges())?;
        let b = self.brep();
        Some(chain.iter().map(|&v| b.vertices[v].pos).collect())
    }

    /// Each rim edge's (curve id), by edge, for the view and for picking.
    pub fn edge_curves(&self) -> BTreeMap<EdgeId, u64> {
        let mut out = BTreeMap::new();
        if self.curves.is_empty() {
            return out;
        }
        let by_key = self.keyed_edges();
        for (&id, c) in &self.curves {
            for k in &c.keys {
                if let Some([e]) = by_key.get(k).map(Vec::as_slice) {
                    out.insert(e.0, id);
                }
            }
        }
        out
    }

    /// How the UI shows rim `id`: its kind as its points read now (a circle, an arc or a
    /// polygon whose points no longer lie on one circle reads as a curve), centre, radius and
    /// sweep when round, and its points.
    pub fn curve_info(&self, id: u64) -> Option<CurveInfo> {
        let c = self.curves.get(&id)?;
        let pts = self.curve_points(id)?;
        Some(rim_info(c, &pts))
    }

    /// The rims' rules (the validator): every key names exactly one edge, the edges meet in
    /// order (round the loop for a closed rim), no edge is in two rims, and the kinds are
    /// consistent (circles and polygons closed, arcs open, polylines of two edges or more).
    pub fn check_curves(&self) -> Result<(), String> {
        if self.curves.is_empty() {
            return Ok(());
        }
        let by_key = self.keyed_edges();
        let mut seen: BTreeSet<(u64, u64)> = BTreeSet::new();
        for (id, c) in &self.curves {
            if *id == 0 {
                return Err("a solid's curve has id 0".into());
            }
            if let Some(closes) = c.kind.closes()
                && closes != c.closed
            {
                return Err(format!(
                    "solid curve {id}: a {} that is not {}",
                    c.kind.word(),
                    if closes { "closed" } else { "open" }
                ));
            }
            let min = match (c.kind, c.closed) {
                (_, true) => 3,
                (CurveKind::Polyline, false) => 2,
                _ => 1,
            };
            if c.keys.len() < min || c.segments == 0 {
                return Err(format!("solid curve {id} is too short"));
            }
            for k in &c.keys {
                if k.0 >= k.1 || !seen.insert(*k) {
                    return Err(format!("solid curve {id}: an edge twice, or a bad key"));
                }
            }
            if Self::rim_chain(&c.keys, c.closed, &by_key).is_none() {
                return Err(format!("solid curve {id}: its edges do not form one chain"));
            }
        }
        Ok(())
    }
}

/// [`Object::curve_info`] from a rim and its points.
pub fn rim_info(c: &RimCurve, pts: &[DVec3]) -> CurveInfo {
    let round = if c.kind.is_round() {
        round_of_points(pts, c.closed)
    } else {
        None
    };
    let normal = round.map_or_else(|| polyline_normal(pts), |(_, n, _)| n);
    let (kind, centre, radius, sweep) = match round {
        Some((centre, n, radius)) => (
            c.kind,
            Some(centre.to_array()),
            Some(radius),
            Some(sweep_about(centre, n, pts, c.closed)),
        ),
        None if c.kind == CurveKind::Arc && pts.len() == 2 => (CurveKind::Arc, None, None, None),
        None => (CurveKind::Polyline, None, None, None),
    };
    CurveInfo {
        kind,
        closed: c.closed,
        segments: if kind == c.kind {
            c.shown_segments()
        } else {
            c.keys.len() as u32
        },
        centre,
        radius,
        sweep,
        normal: normal.to_array(),
        points: pts.iter().map(|p| p.to_array()).collect(),
    }
}

/// A plane normal for points (Newell's method), +Z when they are in a line.
pub fn polyline_normal(pts: &[DVec3]) -> DVec3 {
    let n = pts.len();
    let mut s = DVec3::ZERO;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        s += DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    let l = s.length();
    if l > POINT_MERGE * POINT_MERGE {
        s / l
    } else {
        DVec3::Z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(c: DVec2, r: f64, n: usize, a0: f64, sweep: f64, closed: bool) -> Vec<DVec2> {
        let m = if closed { n } else { n + 1 };
        (0..m)
            .map(|k| c + r * DVec2::from_angle(a0 + sweep * k as f64 / n as f64))
            .collect()
    }

    #[test]
    fn angles_of_a_circle_and_an_arc() {
        let c = DVec2::new(1.0, 2.0);
        let pts = ring(c, 0.5, 24, 0.3, TAU, true);
        let (s, w) = fit_angles(c, &pts, true).unwrap();
        assert!((s - 0.3).abs() < 1e-12 && (w - TAU).abs() < 1e-9);
        let arc = ring(c, 0.5, 12, 1.0, -2.0, false);
        let (s, w) = fit_angles(c, &arc, false).unwrap();
        assert!((s - 1.0).abs() < 1e-12 && (w + 2.0).abs() < 1e-9);
        let r = Round {
            centre: c,
            radius: 0.5,
            start: s,
            sweep: w,
        };
        check_round(&r, &arc, false).unwrap();
        // A point moved off the circle fails.
        let mut bad = arc.clone();
        bad[5] += DVec2::new(0.01, 0.0);
        assert!(check_round(&r, &bad, false).is_err());
        // A chord point (another edge split a segment) is fine.
        let mut split = arc.clone();
        split.insert(3, 0.5 * (arc[2] + arc[3]));
        check_round(&r, &split, false).unwrap();
        // So is an end on a chord.
        let mut end = arc.clone();
        end[0] = 0.5 * (arc[0] + arc[1]);
        let (s2, w2) = fit_angles(c, &end, false).unwrap();
        check_round(
            &Round {
                start: s2,
                sweep: w2,
                ..r
            },
            &end,
            false,
        )
        .unwrap();
    }

    #[test]
    fn similarity() {
        assert_eq!(
            similarity_scale(DVec2::new(2.0, 0.0), DVec2::new(0.0, 2.0)),
            Some(2.0)
        );
        assert!(similarity_scale(DVec2::new(0.0, -3.0), DVec2::new(3.0, 0.0)).is_some());
        assert_eq!(
            similarity_scale(DVec2::new(2.0, 0.0), DVec2::new(0.0, 1.0)),
            None
        );
        assert_eq!(similarity_scale(DVec2::ZERO, DVec2::ZERO), None);
    }

    #[test]
    fn circle_from_points_in_space() {
        let c = DVec3::new(1.0, -2.0, 3.0);
        let pts: Vec<DVec3> = (0..24)
            .map(|k| {
                let a = TAU * k as f64 / 24.0;
                c + DVec3::new(0.0, a.cos(), a.sin()) * 0.25
            })
            .collect();
        let (cc, n, r) = round_of_points(&pts, true).unwrap();
        assert!(cc.distance(c) < 1e-12 && (r - 0.25).abs() < 1e-12);
        assert!((n.x.abs() - 1.0).abs() < 1e-12);
        assert!((sweep_about(cc, n, &pts, true).abs() - TAU).abs() < 1e-9);
        // An ellipse is not a circle.
        let squashed: Vec<DVec3> = pts
            .iter()
            .map(|p| DVec3::new(p.x, p.y * 2.0, p.z))
            .collect();
        assert!(round_of_points(&squashed, true).is_none());
    }
}
