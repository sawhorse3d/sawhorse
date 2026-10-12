//! Drawing on an object's face: the Object tier of ADR-0001.
//!
//! Extruding a region consumes it from the sketch; the solid's faces belong to the object,
//! and drawing on them goes through these edits instead of a sketch:
//!
//! - [`split_face`]: a cut path (open polyline) whose two ends lie on the same boundary
//!   loop of the face (on a vertex or an edge, within `POINT_MERGE`) and whose interior
//!   points lie strictly inside the face. The face becomes two faces on the same plane; an
//!   end on an edge splits that edge in both faces that share it.
//! - [`imprint`]: a closed loop strictly inside the face becomes a coplanar sub-face, and the
//!   parent face gains the matching hole loop (twinned with the sub-face's outer loop).
//!   Existing holes inside the loop move to the sub-face.
//!
//! New edges are flagged as user-drawn. Both work on [`Polys`] (plain data) and leave the
//! input untouched on refusal; the caller rebuilds the B-rep and validates (transactional).
//!
//! Also here, because push/pull and booleans need them: [`merge_coplanar`] (dissolve edges
//! between coplanar faces, except user-drawn ones) and [`heal_vertices`] (drop vertices
//! that sit straight through on an edge chain).

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

use sawhorse_tessellate::signed_area;

use crate::brep::{PolyFace, Polys};
use crate::error::FaceEditError;
use crate::plane::Plane;
use crate::sketch::{orient, point_segment_distance};
use crate::tolerance::{AREA_EPS, PLANARITY, POINT_MERGE, in_bounds};
use crate::validate::{point_in_ring, segments_intersect};

/// Where a path end sits on the face boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Loc {
    /// On the vertex at position `pos` of ring `ring`.
    Vertex { ring: usize, pos: usize },
    /// On the edge from `ring[pos]` to `ring[pos + 1]`, at parameter `t` (2D point `at`).
    Edge {
        ring: usize,
        pos: usize,
        t: f64,
        at: DVec2,
    },
}

impl Loc {
    fn ring(&self) -> usize {
        match *self {
            Loc::Vertex { ring, .. } | Loc::Edge { ring, .. } => ring,
        }
    }
}

/// A face seen in its own plane frame.
struct FaceView {
    plane: Plane,
    rings: Vec<Vec<usize>>,
    rings2: Vec<Vec<DVec2>>,
}

impl FaceView {
    fn new(p: &Polys, fi: usize) -> Result<FaceView, FaceEditError> {
        let f = p
            .faces
            .get(fi)
            .ok_or(FaceEditError::NoSuchFace(fi as u64))?;
        let plane = p.plane(fi).ok_or(FaceEditError::Degenerate)?;
        let rings: Vec<Vec<usize>> = f.rings().cloned().collect();
        let rings2 = rings
            .iter()
            .map(|r| r.iter().map(|&i| plane.to_local(p.points[i])).collect())
            .collect();
        Ok(FaceView {
            plane,
            rings,
            rings2,
        })
    }

    fn segments(&self) -> impl Iterator<Item = (usize, usize, DVec2, DVec2)> + '_ {
        self.rings2.iter().enumerate().flat_map(|(r, ring)| {
            (0..ring.len()).map(move |m| (r, m, ring[m], ring[(m + 1) % ring.len()]))
        })
    }

    fn inside(&self, q: DVec2) -> bool {
        point_in_ring(q, &self.rings2[0]) && !self.rings2[1..].iter().any(|h| point_in_ring(q, h))
    }

    fn clearance(&self, q: DVec2) -> f64 {
        self.segments()
            .map(|(_, _, a, b)| point_segment_distance(q, a, b))
            .fold(f64::INFINITY, f64::min)
    }

    /// Snap `q` to a boundary vertex, else to an edge interior, within `POINT_MERGE`.
    fn locate(&self, q: DVec2) -> Option<Loc> {
        let mut best: Option<(f64, Loc)> = None;
        for (r, ring) in self.rings2.iter().enumerate() {
            for (pos, &v) in ring.iter().enumerate() {
                let d = v.distance(q);
                if d <= POINT_MERGE && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, Loc::Vertex { ring: r, pos }));
                }
            }
        }
        if best.is_some() {
            return best.map(|(_, l)| l);
        }
        for (r, pos, a, b) in self.segments() {
            let ab = b - a;
            let t = (q - a).dot(ab) / ab.length_squared();
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            let at = a + ab * t;
            let d = at.distance(q);
            if d <= POINT_MERGE
                && at.distance(a) > POINT_MERGE
                && at.distance(b) > POINT_MERGE
                && best.is_none_or(|(bd, _)| d < bd)
            {
                best = Some((
                    d,
                    Loc::Edge {
                        ring: r,
                        pos,
                        t,
                        at,
                    },
                ));
            }
        }
        best.map(|(_, l)| l)
    }

    fn position(&self, l: Loc) -> DVec2 {
        match l {
            Loc::Vertex { ring, pos } => self.rings2[ring][pos],
            Loc::Edge { at, .. } => at,
        }
    }

    /// Boundary segment (ring `r`, edge `m`) is one a path end at `l` may touch.
    fn touches(&self, l: Loc, r: usize, m: usize) -> bool {
        match l {
            Loc::Vertex { ring, pos } => {
                let n = self.rings2[ring].len();
                r == ring && (m == pos || m == (pos + n - 1) % n)
            }
            Loc::Edge { ring, pos, .. } => r == ring && m == pos,
        }
    }

    /// Path points to the face frame, refusing off-plane, non-finite and repeated points.
    fn to_2d(&self, pts: &[DVec3], closed: bool) -> Result<Vec<DVec2>, FaceEditError> {
        let mut out = Vec::with_capacity(pts.len());
        for p in pts {
            if !(in_bounds(p.x) && in_bounds(p.y) && in_bounds(p.z)) {
                return Err(FaceEditError::OutOfBounds);
            }
            if self.plane.signed_distance(*p).abs() > PLANARITY {
                return Err(FaceEditError::NotOnPlane);
            }
            out.push(self.plane.to_local(*p));
        }
        if closed && out.len() > 3 && out[0].distance(out[out.len() - 1]) <= POINT_MERGE {
            out.pop();
        }
        let n = out.len();
        let pairs = if closed { n } else { n - 1 };
        if (0..pairs).any(|k| out[k].distance(out[(k + 1) % n]) <= POINT_MERGE) {
            return Err(FaceEditError::Degenerate);
        }
        Ok(out)
    }
}

/// Exact: segments `k` and `k + 1` of a polyline fold back onto each other at their joint.
fn folds_back(p: DVec2, joint: DVec2, q: DVec2) -> bool {
    orient(p, joint, q) == 0.0 && (p - joint).dot(q - joint) > 0.0
}

/// The polyline (closed: as a loop) never touches itself except consecutive segments at
/// their shared point.
fn polyline_simple(pts: &[DVec2], closed: bool) -> bool {
    let n = pts.len();
    let segs = if closed { n } else { n - 1 };
    let seg = |k: usize| (pts[k], pts[(k + 1) % n]);
    for i in 0..segs {
        for j in i + 1..segs {
            let adjacent = j == i + 1 || (closed && i == 0 && j == segs - 1);
            let (a, b) = seg(i);
            let (c, d) = seg(j);
            if adjacent {
                let fold = if j == i + 1 {
                    folds_back(a, b, d)
                } else {
                    folds_back(b, a, c)
                };
                if fold {
                    return false;
                }
            } else if segments_intersect(a, b, c, d) {
                return false;
            }
        }
    }
    true
}

/// Insert point `x` into edge a->b of whichever ring holds that directed edge, and into
/// b->a of the twin ring. User flags carry over to both halves.
fn split_polys_edge(p: &mut Polys, a: usize, b: usize, x: usize) {
    for f in &mut p.faces {
        for r in std::iter::once(&mut f.outer).chain(f.holes.iter_mut()) {
            let n = r.len();
            for k in 0..n {
                let (u, v) = (r[k], r[(k + 1) % n]);
                if (u, v) == (a, b) || (u, v) == (b, a) {
                    r.insert(k + 1, x);
                    break;
                }
            }
        }
    }
    if p.user_edges.remove(&Polys::key(a, b)) {
        p.user_edges.insert(Polys::key(a, x));
        p.user_edges.insert(Polys::key(x, b));
    }
}

fn ring2(p: &Polys, plane: &Plane, ring: &[usize]) -> Vec<DVec2> {
    ring.iter().map(|&i| plane.to_local(p.points[i])).collect()
}

/// Cyclic slice of `ring` from position `i` to position `j` inclusive.
fn arc(ring: &[usize], i: usize, j: usize) -> Vec<usize> {
    let n = ring.len();
    let mut out = vec![ring[i]];
    let mut k = i;
    while k != j {
        k = (k + 1) % n;
        out.push(ring[k]);
    }
    out
}

/// Split face `fi` along `path`. Returns `(fi, new_face_index)`: the first piece keeps the
/// face's index, the second is appended.
pub fn split_face(
    p: &mut Polys,
    fi: usize,
    path: &[DVec3],
) -> Result<(usize, usize), FaceEditError> {
    let view = FaceView::new(p, fi)?;
    if path.len() < 2 {
        return Err(FaceEditError::TooFewPoints(2));
    }
    let mut p2 = view.to_2d(path, false)?;
    let last = p2.len() - 1;
    let s = view.locate(p2[0]).ok_or(FaceEditError::NotOnBoundary)?;
    let e = view.locate(p2[last]).ok_or(FaceEditError::NotOnBoundary)?;
    if s.ring() != e.ring() {
        return Err(FaceEditError::DoesNotSplit);
    }
    let same = match (s, e) {
        (Loc::Vertex { pos: a, .. }, Loc::Vertex { pos: b, .. }) => a == b,
        (Loc::Edge { pos: a, at: x, .. }, Loc::Edge { pos: b, at: y, .. }) => {
            a == b && x.distance(y) <= POINT_MERGE
        }
        _ => false,
    };
    if same {
        return Err(FaceEditError::Degenerate);
    }
    p2[0] = view.position(s);
    p2[last] = view.position(e);
    if (0..last).any(|k| p2[k].distance(p2[k + 1]) <= POINT_MERGE) {
        return Err(FaceEditError::Degenerate);
    }
    for q in &p2[1..last] {
        if !view.inside(*q) || view.clearance(*q) <= POINT_MERGE {
            return Err(FaceEditError::NotInside);
        }
    }
    if !polyline_simple(&p2, false) {
        return Err(FaceEditError::SelfIntersecting);
    }
    for k in 0..last {
        let (a, b) = (p2[k], p2[k + 1]);
        if !view.inside(0.5 * (a + b)) {
            return Err(FaceEditError::NotInside);
        }
        for (r, m, c, d) in view.segments() {
            if !segments_intersect(a, b, c, d) {
                continue;
            }
            let allowed =
                (k == 0 && view.touches(s, r, m)) || (k == last - 1 && view.touches(e, r, m));
            if !allowed {
                return Err(FaceEditError::CrossesBoundary);
            }
            if orient(c, d, a) == 0.0 && orient(c, d, b) == 0.0 {
                return Err(FaceEditError::NotInside); // runs along the boundary
            }
        }
        // Exact tests miss a cut that grazes a boundary vertex by 1e-16 m: every boundary
        // vertex but the cut's own end vertex keeps the merge distance from the cut.
        for (r, ring) in view.rings2.iter().enumerate() {
            for (m, &q) in ring.iter().enumerate() {
                let own = (k == 0 && s == Loc::Vertex { ring: r, pos: m })
                    || (k == last - 1 && e == Loc::Vertex { ring: r, pos: m });
                if !own && point_segment_distance(q, a, b) <= POINT_MERGE {
                    return Err(FaceEditError::CrossesBoundary);
                }
            }
        }
    }

    // Build on a copy.
    let mut q = p.clone();
    let ring_ix = s.ring();
    let ring = &view.rings[ring_ix];
    let n = ring.len();
    let mut on_edge: Vec<(usize, f64, bool)> = Vec::new(); // (edge pos, t, is_start)
    let mut sv = None;
    let mut ev = None;
    for (l, is_start) in [(s, true), (e, false)] {
        match l {
            Loc::Vertex { pos, .. } => {
                if is_start {
                    sv = Some(ring[pos]);
                } else {
                    ev = Some(ring[pos]);
                }
            }
            Loc::Edge { pos, t, .. } => on_edge.push((pos, t, is_start)),
        }
    }
    on_edge.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
    let mut prev_split: Option<(usize, usize)> = None; // (edge pos, inserted point)
    for (pos, t, is_start) in on_edge {
        let (a, b) = (ring[pos], ring[(pos + 1) % n]);
        let (a3, b3) = (p.points[a], p.points[b]);
        let x = q.points.len();
        // a + (b - a) t, not glam's a (1 - t) + b t: it is exact in every coordinate where
        // the edge does not move (a split on an axis-aligned edge stays on its planes), so
        // later booleans see exact coincidence instead of 1e-17 m near misses.
        q.points.push(a3 + (b3 - a3) * t);
        let from = match prev_split {
            Some((pp, xp)) if pp == pos => xp,
            _ => a,
        };
        split_polys_edge(&mut q, from, b, x);
        prev_split = Some((pos, x));
        if is_start {
            sv = Some(x);
        } else {
            ev = Some(x);
        }
    }
    let (sv, ev) = (sv.expect("start located"), ev.expect("end located"));
    let interior: Vec<usize> = p2[1..last]
        .iter()
        .map(|&q2| {
            q.points.push(view.plane.to_world(q2));
            q.points.len() - 1
        })
        .collect();
    let face = q.faces[fi].clone();
    let rings: Vec<Vec<usize>> = face.rings().cloned().collect();
    let cut = &rings[ring_ix];
    let i = cut.iter().position(|&v| v == sv).expect("start on ring");
    let j = cut.iter().position(|&v| v == ev).expect("end on ring");
    let mut c1 = arc(cut, i, j);
    c1.extend(interior.iter().rev());
    let mut c2 = arc(cut, j, i);
    c2.extend(interior.iter());
    let plane = view.plane;
    let a1 = signed_area(&ring2(&q, &plane, &c1));
    let a2 = signed_area(&ring2(&q, &plane, &c2));
    let others: Vec<Vec<usize>> = rings
        .iter()
        .enumerate()
        .filter(|(k, _)| *k != ring_ix && *k != 0)
        .map(|(_, r)| r.clone())
        .collect();
    let inside_ring = |ring: &[usize], probe: &[usize]| {
        point_in_ring(plane.to_local(q.points[probe[0]]), &ring2(&q, &plane, ring))
    };
    let (kept, added) = if ring_ix == 0 {
        if a1 <= AREA_EPS || a2 <= AREA_EPS {
            return Err(FaceEditError::Degenerate);
        }
        let (h1, h2): (Vec<_>, Vec<_>) = others.into_iter().partition(|h| inside_ring(&c1, h));
        (
            PolyFace {
                outer: c1,
                holes: h1,
                plane: Some(plane),
                id: face.id,
            },
            PolyFace {
                outer: c2,
                holes: h2,
                plane: Some(plane),
                id: None,
            },
        )
    } else {
        let (cap, hole) = if a1 > AREA_EPS && a2 < -AREA_EPS {
            (c1, c2)
        } else if a2 > AREA_EPS && a1 < -AREA_EPS {
            (c2, c1)
        } else {
            return Err(FaceEditError::Degenerate);
        };
        let (in_cap, mut rest): (Vec<_>, Vec<_>) =
            others.into_iter().partition(|h| inside_ring(&cap, h));
        rest.push(hole);
        (
            PolyFace {
                outer: face.outer.clone(),
                holes: rest,
                plane: Some(plane),
                id: face.id,
            },
            PolyFace {
                outer: cap,
                holes: in_cap,
                plane: Some(plane),
                id: None,
            },
        )
    };
    q.faces[fi] = kept;
    q.faces.push(added);
    let chain: Vec<usize> = std::iter::once(sv)
        .chain(interior.iter().copied())
        .chain(std::iter::once(ev))
        .collect();
    for w in chain.windows(2) {
        q.user_edges.insert(Polys::key(w[0], w[1]));
    }
    let new = q.faces.len() - 1;
    *p = q;
    Ok((fi, new))
}

/// Imprint the closed loop `pts` (strictly inside face `fi`) as a sub-face. Returns the
/// sub-face's index (appended); the parent keeps its index and gains the hole.
pub fn imprint(p: &mut Polys, fi: usize, pts: &[DVec3]) -> Result<usize, FaceEditError> {
    let view = FaceView::new(p, fi)?;
    if pts.len() < 3 {
        return Err(FaceEditError::TooFewPoints(3));
    }
    let mut p2 = view.to_2d(pts, true)?;
    if p2.len() < 3 {
        return Err(FaceEditError::TooFewPoints(3));
    }
    if !polyline_simple(&p2, true) {
        return Err(FaceEditError::SelfIntersecting);
    }
    let area = signed_area(&p2);
    if area.abs() <= AREA_EPS {
        return Err(FaceEditError::Degenerate);
    }
    for q in &p2 {
        if !view.inside(*q) || view.clearance(*q) <= POINT_MERGE {
            return Err(FaceEditError::NotInside);
        }
    }
    let n = p2.len();
    // Exactly simple is not enough: a corner within tolerance of a side it does not belong
    // to (a spike doubling back, a waist pinched to a hair) would weld into a solid whose
    // boundary touches itself. Refuse it like a crossing (fuzz, 2026-10-11).
    for i in 0..n {
        for k in 0..n {
            if k == i || (k + 1) % n == i {
                continue;
            }
            if point_segment_distance(p2[i], p2[k], p2[(k + 1) % n]) <= POINT_MERGE {
                return Err(FaceEditError::SelfIntersecting);
            }
        }
    }
    for k in 0..n {
        let (a, b) = (p2[k], p2[(k + 1) % n]);
        if view
            .segments()
            .any(|(_, _, c, d)| segments_intersect(a, b, c, d))
            || view
                .rings2
                .iter()
                .flatten()
                .any(|&q| point_segment_distance(q, a, b) <= POINT_MERGE)
        {
            return Err(FaceEditError::CrossesBoundary);
        }
    }
    if area < 0.0 {
        p2.reverse();
    }
    let mut q = p.clone();
    let idx: Vec<usize> = p2
        .iter()
        .map(|&v| {
            q.points.push(view.plane.to_world(v));
            q.points.len() - 1
        })
        .collect();
    let parent = q.faces[fi].clone();
    let (inner, mut outer_holes): (Vec<Vec<usize>>, Vec<Vec<usize>>) = parent
        .holes
        .iter()
        .cloned()
        .partition(|h| point_in_ring(view.plane.to_local(q.points[h[0]]), &p2));
    let mut hole = idx.clone();
    hole.reverse();
    outer_holes.push(hole);
    q.faces[fi] = PolyFace {
        outer: parent.outer.clone(),
        holes: outer_holes,
        plane: Some(view.plane),
        id: parent.id,
    };
    q.faces.push(PolyFace {
        outer: idx.clone(),
        holes: inner,
        plane: Some(view.plane),
        id: None,
    });
    for k in 0..n {
        q.user_edges.insert(Polys::key(idx[k], idx[(k + 1) % n]));
    }
    let sub = q.faces.len() - 1;
    *p = q;
    Ok(sub)
}

/// The vertex at `q` (within `POINT_MERGE`), if any.
fn vertex_at(p: &Polys, q: DVec3) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for (i, &x) in p.points.iter().enumerate() {
        let d = x.distance(q);
        if d <= POINT_MERGE && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| i)
}

/// Merge faces `fa` and `fb` (coplanar, same facing): every edge they share stops being
/// user-drawn and is dissolved. Returns the surviving face's index.
fn dissolve_between(p: &mut Polys, fa: usize, fb: usize) -> Result<usize, FaceEditError> {
    if !(coplanar(p, fa, fb) && coplanar(p, fb, fa)) {
        return Err(FaceEditError::WouldOpenSolid);
    }
    let directed = p.directed_edges();
    let shared: Vec<(usize, usize)> = directed
        .iter()
        .filter(|&(&(u, v), &(f, _))| {
            f == fa && directed.get(&(v, u)).is_some_and(|&(g, _)| g == fb)
        })
        .map(|(&(u, v), _)| Polys::key(u, v))
        .collect();
    if shared.is_empty() {
        return Err(FaceEditError::NoSuchEdge);
    }
    let mut q = p.clone();
    for k in &shared {
        q.user_edges.remove(k);
    }
    let before = q.faces.len();
    let keep = fa.min(fb);
    let id = q.faces[keep].id;
    merge_coplanar(&mut q, true);
    if q.faces.len() >= before {
        return Err(FaceEditError::WouldPinch);
    }
    let survivor = q
        .faces
        .iter()
        .position(|f| f.id.is_some() && f.id == id)
        .unwrap_or(keep.min(q.faces.len() - 1));
    *p = q;
    Ok(survivor)
}

/// Erase the edge from `a` to `b` (world points within `POINT_MERGE` of its two ends). Only
/// an edge between two coplanar faces can go (a line drawn on a face, part of an imprint):
/// the two faces merge, and every other edge they share goes with it, because a face can
/// not hold a dangling line. Anything else is refused ([`FaceEditError::WouldOpenSolid`]).
/// Returns the index of the merged face.
pub fn erase_edge(p: &mut Polys, a: DVec3, b: DVec3) -> Result<usize, FaceEditError> {
    let (Some(ia), Some(ib)) = (vertex_at(p, a), vertex_at(p, b)) else {
        return Err(FaceEditError::NoSuchEdge);
    };
    let directed = p.directed_edges();
    let (Some(&(fa, _)), Some(&(fb, _))) = (directed.get(&(ia, ib)), directed.get(&(ib, ia)))
    else {
        return Err(FaceEditError::NoSuchEdge);
    };
    if fa == fb {
        return Err(FaceEditError::WouldOpenSolid);
    }
    dissolve_between(p, fa, fb)
}

/// Erase face `fi`: allowed when it lies on the same plane as a neighbour across a drawn
/// line (an imprint, one side of a split), which then absorbs it. Returns the index of the
/// face that absorbed it.
pub fn erase_face(p: &mut Polys, fi: usize) -> Result<usize, FaceEditError> {
    if fi >= p.faces.len() {
        return Err(FaceEditError::NoSuchFace(fi as u64));
    }
    let directed = p.directed_edges();
    let mut neighbours: BTreeSet<usize> = BTreeSet::new();
    for (&(u, v), &(f, _)) in &directed {
        if f == fi
            && let Some(&(g, _)) = directed.get(&(v, u))
            && g != fi
            && coplanar(p, fi, g)
            && coplanar(p, g, fi)
        {
            neighbours.insert(g);
        }
    }
    // The smallest coplanar neighbour (the parent of an imprint has the lower index).
    let Some(&g) = neighbours.iter().next() else {
        return Err(FaceEditError::WouldOpenSolid);
    };
    dissolve_between(p, g, fi)
}

/// Two faces' planes are the same plane with the same orientation.
pub fn coplanar(p: &Polys, a: usize, b: usize) -> bool {
    let (Some(pa), Some(pb)) = (p.plane(a), p.plane(b)) else {
        return false;
    };
    pa.normal.dot(pb.normal) >= 1.0 - 1e-10
        && p.faces[b]
            .rings()
            .flatten()
            .all(|&i| pa.signed_distance(p.points[i]).abs() <= PLANARITY)
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Merge each group of faces connected through edges between coplanar faces into one face,
/// dissolving those edges. User-drawn edges are kept (not dissolved) when `keep_user`.
/// Groups whose merged boundary would pinch at a vertex stay unmerged. Vertices left
/// running straight through between the merged edges are healed. Returns the number of
/// edges dissolved. The merged face takes the smallest member's index, plane and persistent id.
pub fn merge_coplanar(p: &mut Polys, keep_user: bool) -> usize {
    let directed = p.directed_edges();
    let nf = p.faces.len();
    let mut parent: Vec<usize> = (0..nf).collect();
    let mut cache: BTreeMap<(usize, usize), bool> = BTreeMap::new();
    for (&(a, b), &(fa, _)) in &directed {
        if a > b {
            continue;
        }
        let Some(&(fb, _)) = directed.get(&(b, a)) else {
            continue;
        };
        if fa == fb || (keep_user && p.is_user(a, b)) {
            continue;
        }
        let key = (fa.min(fb), fa.max(fb));
        let ok = *cache
            .entry(key)
            .or_insert_with(|| coplanar(p, key.0, key.1) && coplanar(p, key.1, key.0));
        if ok {
            let (ra, rb) = (find(&mut parent, fa), find(&mut parent, fb));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for f in 0..nf {
        let r = find(&mut parent, f);
        groups.entry(r).or_default().push(f);
    }
    let mut doomed: BTreeSet<usize> = BTreeSet::new();
    let mut touched: BTreeSet<usize> = BTreeSet::new();
    let mut dissolved = 0;
    for (root, members) in groups {
        if members.len() < 2 {
            continue;
        }
        let Some(plane) = p.plane(root) else { continue };
        let mut edges: BTreeSet<(usize, usize)> = BTreeSet::new();
        for &m in &members {
            for r in p.faces[m].rings() {
                for k in 0..r.len() {
                    edges.insert((r[k], r[(k + 1) % r.len()]));
                }
            }
        }
        let internal: Vec<(usize, usize)> = edges
            .iter()
            .filter(|(a, b)| edges.contains(&(*b, *a)) && (!keep_user || !p.is_user(*a, *b)))
            .copied()
            .collect();
        let boundary: BTreeSet<(usize, usize)> = edges
            .iter()
            .filter(|e| !internal.contains(e))
            .copied()
            .collect();
        // Each vertex must have exactly one outgoing boundary edge (no pinch).
        let mut next: BTreeMap<usize, usize> = BTreeMap::new();
        if boundary.iter().any(|&(a, b)| next.insert(a, b).is_some()) {
            continue;
        }
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut loops: Vec<Vec<usize>> = Vec::new();
        let mut broken = false;
        for &(a0, _) in &boundary {
            if seen.contains(&a0) {
                continue;
            }
            let mut lp = Vec::new();
            let mut v = a0;
            loop {
                if !seen.insert(v) {
                    broken = v != a0;
                    break;
                }
                lp.push(v);
                match next.get(&v) {
                    Some(&w) => v = w,
                    None => {
                        broken = true;
                        break;
                    }
                }
            }
            if broken {
                break;
            }
            loops.push(lp);
        }
        if broken {
            continue;
        }
        let areas: Vec<f64> = loops
            .iter()
            .map(|l| signed_area(&ring2(p, &plane, l)))
            .collect();
        let outers: Vec<usize> = (0..loops.len()).filter(|&i| areas[i] > 0.0).collect();
        if outers.len() != 1 || areas.contains(&0.0) {
            continue;
        }
        let o = outers[0];
        let mut holes: Vec<Vec<usize>> = (0..loops.len())
            .filter(|&i| i != o)
            .map(|i| loops[i].clone())
            .collect();
        holes.sort();
        p.faces[root] = PolyFace {
            outer: loops[o].clone(),
            holes,
            plane: Some(plane),
            id: p.faces[root].id,
        };
        for &m in &members[1..] {
            doomed.insert(m);
        }
        for (a, b) in internal {
            if a < b {
                dissolved += 1;
                p.user_edges.remove(&Polys::key(a, b));
            }
            touched.insert(a);
            touched.insert(b);
        }
    }
    for &d in doomed.iter().rev() {
        p.faces.remove(d);
    }
    heal_vertices(p, &touched);
    p.compact();
    dissolved
}

/// Remove each candidate vertex that joins exactly two edges running straight through it
/// (within `PLANARITY`), in every ring that holds it. The joined edge is user-drawn if both
/// halves were. Points stay in the list (call `compact`).
pub fn heal_vertices(p: &mut Polys, candidates: &BTreeSet<usize>) {
    for &v in candidates {
        let mut nbrs: BTreeSet<usize> = BTreeSet::new();
        let mut uses = 0;
        for f in &p.faces {
            for r in f.rings() {
                let n = r.len();
                for k in 0..n {
                    if r[k] == v {
                        uses += 1;
                        nbrs.insert(r[(k + n - 1) % n]);
                        nbrs.insert(r[(k + 1) % n]);
                    }
                }
            }
        }
        if uses == 0 || nbrs.len() != 2 {
            continue;
        }
        let mut it = nbrs.iter();
        let (u, w) = (*it.next().unwrap(), *it.next().unwrap());
        let (pu, pv, pw) = (p.points[u], p.points[v], p.points[w]);
        let uw = pw - pu;
        let t = (pv - pu).dot(uw) / uw.length_squared();
        if !(t > 0.0 && t < 1.0) || (pu + uw * t).distance(pv) > PLANARITY {
            continue;
        }
        // A ring that would drop below three vertices, or an edge u-w that already exists,
        // means v is not a plain pass-through.
        let exists = p
            .directed_edges()
            .keys()
            .any(|&(a, b)| Polys::key(a, b) == Polys::key(u, w));
        if exists
            || p.faces
                .iter()
                .any(|f| f.rings().any(|r| r.contains(&v) && r.len() <= 3))
        {
            continue;
        }
        let user = p.is_user(u, v) && p.is_user(v, w);
        for f in &mut p.faces {
            for r in std::iter::once(&mut f.outer).chain(f.holes.iter_mut()) {
                r.retain(|&x| x != v);
            }
        }
        p.user_edges.remove(&Polys::key(u, v));
        p.user_edges.remove(&Polys::key(v, w));
        if user {
            p.user_edges.insert(Polys::key(u, w));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::Brep;
    use crate::object::Object;
    use crate::validate::validate;

    fn boxy() -> Polys {
        let o = Object::extrude(
            &Plane::GROUND,
            &[
                DVec2::ZERO,
                DVec2::new(2.0, 0.0),
                DVec2::new(2.0, 1.0),
                DVec2::new(0.0, 1.0),
            ],
            &[],
            1.0,
        )
        .unwrap();
        o.brep().to_polys()
    }

    fn check(p: &Polys) -> crate::validate::TopologyReport {
        let b = Brep::build(p).unwrap();
        let r = validate(&b).unwrap();
        assert!(r.watertight);
        r
    }

    fn v3(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    const TOP: usize = 1;

    #[test]
    fn round_trip_through_polys() {
        let p = boxy();
        let b = Brep::build(&p).unwrap();
        assert_eq!(b.to_polys(), p);
    }

    #[test]
    fn split_top_edge_to_edge() {
        let mut p = boxy();
        let (a, b) = split_face(&mut p, TOP, &[v3(1.0, 0.0, 1.0), v3(1.0, 1.0, 1.0)]).unwrap();
        assert_eq!((a, b), (TOP, 6));
        let r = check(&p);
        assert_eq!((r.vertices, r.edges, r.faces), (10, 15, 7));
        assert_eq!(r.shells[0].genus, Some(0));
        assert_eq!(p.user_edges.len(), 1);
        // The two side walls gained the split vertex.
        assert_eq!(p.faces.iter().filter(|f| f.outer.len() == 5).count(), 2);
    }

    #[test]
    fn split_vertex_to_vertex_with_a_bend() {
        let mut p = boxy();
        split_face(
            &mut p,
            TOP,
            &[v3(0.0, 0.0, 1.0), v3(1.0, 0.5, 1.0), v3(2.0, 1.0, 1.0)],
        )
        .unwrap();
        let r = check(&p);
        assert_eq!((r.vertices, r.edges, r.faces), (9, 14, 7));
        assert_eq!(p.user_edges.len(), 2);
    }

    #[test]
    fn split_twice_on_one_edge() {
        let mut p = boxy();
        // A notch: both ends on the front edge (y = 0).
        split_face(
            &mut p,
            TOP,
            &[
                v3(0.5, 0.0, 1.0),
                v3(0.5, 0.5, 1.0),
                v3(1.5, 0.5, 1.0),
                v3(1.5, 0.0, 1.0),
            ],
        )
        .unwrap();
        let r = check(&p);
        assert_eq!(r.faces, 7);
        let front = p
            .faces
            .iter()
            .find(|f| f.plane.unwrap().normal.y < -0.5)
            .unwrap();
        assert_eq!(
            front.outer.len(),
            6,
            "front wall gained both split points in order"
        );
    }

    #[test]
    fn imprint_then_split_its_hole_ring() {
        let mut p = boxy();
        let sub = imprint(
            &mut p,
            TOP,
            &[
                v3(0.5, 0.25, 1.0),
                v3(1.5, 0.25, 1.0),
                v3(1.5, 0.75, 1.0),
                v3(0.5, 0.75, 1.0),
            ],
        )
        .unwrap();
        assert_eq!(sub, 6);
        let r = check(&p);
        assert_eq!((r.faces, r.holes), (7, 1));
        assert_eq!(r.shells[0].euler, 2);
        assert_eq!(p.user_edges.len(), 4);
        // A cut in the parent from the hole ring back to the hole ring: a cap on one side.
        let (_, cap) = split_face(
            &mut p,
            TOP,
            &[
                v3(0.5, 0.5, 1.0),
                v3(0.25, 0.5, 1.0),
                v3(0.25, 0.9, 1.0),
                v3(1.0, 0.9, 1.0),
                v3(1.0, 0.75, 1.0),
            ],
        )
        .unwrap();
        let r = check(&p);
        assert_eq!(r.faces, 8);
        assert_eq!(
            p.faces[TOP].holes.len(),
            1,
            "the hole grew to include the cap"
        );
        assert!(p.faces[cap].holes.is_empty());
    }

    #[test]
    fn a_cut_grazing_a_hole_corner_is_refused() {
        // Fuzz find (2026-10-11): a diagonal cut passing within 1e-16 m of an imprinted
        // hole corner slipped past the exact crossing tests.
        let mut p = boxy();
        imprint(
            &mut p,
            TOP,
            &[
                v3(0.5, 0.25, 1.0),
                v3(1.5, 0.25, 1.0),
                v3(1.5, 0.75, 1.0),
                v3(0.5, 0.75, 1.0),
            ],
        )
        .unwrap();
        let before = p.clone();
        // From the front edge to the left edge, missing the hole corner (0.5, 0.25) by a
        // hair: exact predicates say "no contact".
        let r = split_face(
            &mut p,
            TOP,
            &[v3(0.75, 0.0, 1.0), v3(0.0, 0.75 + 1e-15, 1.0)],
        );
        assert_eq!(r, Err(FaceEditError::CrossesBoundary));
        assert_eq!(p, before);
        // An imprint whose edge passes 5 um from that corner is refused too.
        let r = imprint(
            &mut p,
            TOP,
            &[
                v3(0.1, 0.1, 1.0),
                v3(0.5 + 5e-6, 0.25 - 5e-6, 1.0),
                v3(0.1, 0.4, 1.0),
            ],
        );
        assert!(r.is_err());
    }

    #[test]
    fn imprint_around_an_existing_hole_takes_it() {
        let mut p = boxy();
        let small = [
            v3(0.8, 0.4, 1.0),
            v3(1.2, 0.4, 1.0),
            v3(1.2, 0.6, 1.0),
            v3(0.8, 0.6, 1.0),
        ];
        imprint(&mut p, TOP, &small).unwrap();
        let big = imprint(
            &mut p,
            TOP,
            &[
                v3(0.5, 0.2, 1.0),
                v3(1.5, 0.2, 1.0),
                v3(1.5, 0.8, 1.0),
                v3(0.5, 0.8, 1.0),
            ],
        )
        .unwrap();
        check(&p);
        assert_eq!(
            p.faces[big].holes.len(),
            1,
            "the small imprint's hole moved inside"
        );
        assert_eq!(p.faces[TOP].holes.len(), 1);
    }

    #[test]
    fn refusals_are_typed_and_leave_the_faces_alone() {
        let p0 = boxy();
        let z = 1.0;
        let cases: Vec<(Vec<DVec3>, FaceEditError)> = vec![
            (vec![v3(1.0, 0.0, z)], FaceEditError::TooFewPoints(2)),
            (
                vec![v3(1.0, 0.0, z), v3(1.0, 0.5, z)],
                FaceEditError::NotOnBoundary,
            ),
            (
                vec![v3(1.0, 0.0, z), v3(1.0, 1.0, z + 0.01)],
                FaceEditError::NotOnPlane,
            ),
            (
                vec![v3(1.0, 0.0, z), v3(f64::NAN, 1.0, z)],
                FaceEditError::OutOfBounds,
            ),
            (
                vec![v3(1.0, 0.0, z), v3(1.0, 1.5, z), v3(1.2, 1.0, z)],
                FaceEditError::NotInside,
            ),
            (
                vec![v3(0.0, 0.0, z), v3(2.0, 0.0, z)],
                FaceEditError::NotInside,
            ),
            (
                vec![
                    v3(0.5, 0.0, z),
                    v3(1.5, 0.8, z),
                    v3(1.5, 0.2, z),
                    v3(0.5, 1.0, z),
                ],
                FaceEditError::SelfIntersecting,
            ),
            (
                vec![v3(0.0, 0.0, z), v3(0.0, 0.0, z)],
                FaceEditError::Degenerate,
            ),
            (
                vec![v3(1.0, 0.0, z), v3(1.0, 0.5, z), v3(1.0 + 1e-6, 0.0, z)],
                FaceEditError::Degenerate,
            ),
        ];
        for (path, want) in cases {
            let mut p = p0.clone();
            assert_eq!(
                split_face(&mut p, TOP, &path),
                Err(want.clone()),
                "{path:?}"
            );
            assert_eq!(p, p0);
        }
        // Cut from the outer loop to a hole: does not split.
        let mut p = boxy();
        imprint(
            &mut p,
            TOP,
            &[
                v3(0.5, 0.25, z),
                v3(1.5, 0.25, z),
                v3(1.5, 0.75, z),
                v3(0.5, 0.75, z),
            ],
        )
        .unwrap();
        let before = p.clone();
        assert_eq!(
            split_face(&mut p, TOP, &[v3(1.0, 0.0, z), v3(1.0, 0.25, z)]),
            Err(FaceEditError::DoesNotSplit)
        );
        assert_eq!(p, before);
        // Imprint refusals.
        let tri = |a: DVec3, b: DVec3, c: DVec3| vec![a, b, c];
        let mut p = p0.clone();
        let cases: Vec<(Vec<DVec3>, FaceEditError)> = vec![
            (
                vec![v3(0.5, 0.5, z), v3(0.6, 0.5, z)],
                FaceEditError::TooFewPoints(3),
            ),
            (
                tri(v3(0.0, 0.5, z), v3(0.5, 0.2, z), v3(0.5, 0.8, z)),
                FaceEditError::NotInside,
            ),
            (
                tri(v3(1.5, 0.5, z), v3(2.5, 0.2, z), v3(2.5, 0.8, z)),
                FaceEditError::NotInside,
            ),
            (
                vec![
                    v3(0.5, 0.2, z),
                    v3(1.5, 0.8, z),
                    v3(1.5, 0.2, z),
                    v3(0.5, 0.8, z),
                ],
                FaceEditError::SelfIntersecting,
            ),
            (
                tri(v3(0.5, 0.5, z), v3(0.6, 0.5, z), v3(0.7, 0.5, z)),
                FaceEditError::SelfIntersecting,
            ),
            (
                tri(v3(0.5, 0.5, z), v3(0.6, 0.5, z), v3(0.6, 0.6, z + 1.0)),
                FaceEditError::NotOnPlane,
            ),
        ];
        for (pts, want) in cases {
            assert_eq!(imprint(&mut p, TOP, &pts), Err(want.clone()), "{pts:?}");
            assert_eq!(p, p0);
        }
    }

    #[test]
    fn merge_dissolves_kernel_edges_but_keeps_user_edges() {
        let mut p = boxy();
        split_face(&mut p, TOP, &[v3(1.0, 0.0, 1.0), v3(1.0, 1.0, 1.0)]).unwrap();
        let split = p.clone();
        assert_eq!(merge_coplanar(&mut p, true), 0, "the cut is user-drawn");
        assert_eq!(p, split);
        assert_eq!(merge_coplanar(&mut p, false), 1);
        check(&p);
        // Merged back, and the split vertices on the side walls healed away.
        let mut back = p.clone();
        let mut orig = boxy();
        back.faces.sort_by(|a, b| a.outer.cmp(&b.outer));
        orig.faces.sort_by(|a, b| a.outer.cmp(&b.outer));
        assert_eq!(back.points, orig.points);
        assert_eq!(back.faces.len(), orig.faces.len());
    }
}
