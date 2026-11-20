//! 3D booleans behind our own trait (ADR-0001).
//!
//! The engine sees triangle solids with a per-triangle face id; the kernel triangulates
//! operand faces with `sawhorse-tessellate` ([`polys_tri_solid`]), runs the engine, groups
//! output triangles by inherited face id, and re-polygonises them into n-gons with holes,
//! re-fitted planes, coplanar faces merged except across user-drawn edges
//! ([`repolygonise`]); [`combine`] is the whole round trip. Callers validate and commit.
//!
//! Persistent face ids survive the round trip in two steps: every output face inherits the
//! id of the operand face its triangles came from (the engine carries the face id through
//! MeshGL64), and then [`resolve_face_ids`] settles the ambiguous cases by geometry: when
//! one source face comes out in several pieces, the piece that best matches the source
//! (same plane, nearest centroid, closest area) keeps the id; a face that came out without
//! an id takes the id of an unused operand face it matches within tolerance (same plane,
//! centroid within [`ID_MATCH_DISTANCE`], area within [`ID_MATCH_AREA`]).

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};
use sawhorse_tessellate::{signed_area, triangulate};

use crate::brep::{PolyFace, Polys};
use crate::faceops::heal_vertices;
use crate::plane::Plane;
use crate::tolerance::{AREA_EPS, PLANARITY, POINT_MERGE};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoolOp {
    Union,
    Difference,
    Intersection,
}

/// An indexed triangle solid: shared vertices, CCW-outward triangles, one face id each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriSolid {
    pub positions: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
    /// One per triangle: the kernel face the triangle came from.
    pub face_ids: Vec<u32>,
}

impl TriSolid {
    /// Signed volume by the divergence theorem (positive for outward-oriented solids),
    /// summed about the solid's own first corner rather than the world origin: a closed
    /// surface's volume is the same about any point, and about one of its own it does not
    /// cancel products of its distance from the origin. (Lane 23's fuzzer: a 0.1 mm
    /// cylinder 6.6 km out, turned 45 degrees, measured 3.5e-6 m3 about the origin, 244
    /// times its 1.4e-8.)
    pub fn volume(&self) -> f64 {
        let Some(r) = self
            .triangles
            .first()
            .map(|t| self.positions[t[0] as usize])
        else {
            return 0.0;
        };
        self.triangles
            .iter()
            .map(|t| {
                let (a, b, c) = (
                    self.positions[t[0] as usize] - r,
                    self.positions[t[1] as usize] - r,
                    self.positions[t[2] as usize] - r,
                );
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BooleanError {
    #[error("operand {0} is not a closed manifold: {1}")]
    NotManifold(usize, String),
    #[error("boolean engine failed: {0}")]
    Engine(String),
    #[error("no boolean engine is compiled in (enable the `manifold` feature)")]
    Unavailable,
    /// The engine's result could not be turned back into valid n-gon faces (a pinched
    /// face, a sliver). The operation is refused; nothing changed.
    #[error("the result has degenerate geometry: {0}")]
    Degenerate(String),
    /// The re-polygonised result failed validation (e.g. two shells touching).
    #[error("the result is not a valid solid: {0}")]
    InvalidResult(String),
}

pub trait BooleanEngine {
    fn run(&self, a: &TriSolid, b: &TriSolid, op: BoolOp) -> Result<TriSolid, BooleanError>;
}

/// `manifold-rust` (pure-Rust Manifold port, Apache-2.0, pinned exactly).
#[cfg(feature = "manifold")]
#[derive(Debug, Default, Clone, Copy)]
pub struct ManifoldEngine;

#[cfg(feature = "manifold")]
mod manifold_adapter {
    use super::*;
    use manifold_rust::manifold::Manifold;
    use manifold_rust::types::{Error, MeshGL64, OpType};

    fn to_manifold(s: &TriSolid, which: usize) -> Result<Manifold, BooleanError> {
        let mesh = MeshGL64 {
            num_prop: 3,
            vert_properties: s.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
            tri_verts: s.triangles.iter().flat_map(|t| t.map(u64::from)).collect(),
            face_id: s.face_ids.iter().map(|&f| u64::from(f)).collect(),
            ..Default::default()
        };
        let m = Manifold::from_mesh_gl64(&mesh);
        match m.status() {
            Error::NoError => Ok(m),
            e => Err(BooleanError::NotManifold(which, format!("{e:?}"))),
        }
    }

    impl BooleanEngine for ManifoldEngine {
        fn run(&self, a: &TriSolid, b: &TriSolid, op: BoolOp) -> Result<TriSolid, BooleanError> {
            let ma = to_manifold(a, 0)?;
            let mb = to_manifold(b, 1)?;
            let op = match op {
                BoolOp::Union => OpType::Add,
                BoolOp::Difference => OpType::Subtract,
                BoolOp::Intersection => OpType::Intersect,
            };
            let r = ma.boolean(&mb, op);
            if r.status() != Error::NoError {
                return Err(BooleanError::Engine(format!("{:?}", r.status())));
            }
            let out = r.get_mesh_gl64(-1);
            let np = out.num_prop as usize;
            let positions = out
                .vert_properties
                .chunks_exact(np)
                .map(|p| DVec3::new(p[0], p[1], p[2]))
                .collect();
            let triangles = out
                .tri_verts
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| t.map(|i| i as u32))
                .collect();
            let face_ids = out.face_id.iter().map(|&f| f as u32).collect();
            Ok(TriSolid {
                positions,
                triangles,
                face_ids,
            })
        }
    }
}

// ---- the kernel side of a boolean: triangulate, run, re-polygonise --------------------

/// The engine compiled into this build (Manifold), or [`BooleanError::Unavailable`].
pub fn default_engine() -> Result<Box<dyn BooleanEngine>, BooleanError> {
    #[cfg(feature = "manifold")]
    {
        Ok(Box::new(ManifoldEngine))
    }
    #[cfg(not(feature = "manifold"))]
    {
        Err(BooleanError::Unavailable)
    }
}

/// Triangle solid of a polygon B-rep: shared vertices in point order, one face id per
/// triangle (`base` + face index). Every point on a face boundary is used by that face's
/// triangles (no T-junctions), so a closed `Polys` gives a manifold mesh.
pub fn polys_tri_solid(p: &Polys, base: u32) -> Result<TriSolid, BooleanError> {
    let mut triangles = Vec::new();
    let mut face_ids = Vec::new();
    for (fi, f) in p.faces.iter().enumerate() {
        let plane = p
            .plane(fi)
            .ok_or_else(|| BooleanError::Degenerate(format!("face {fi} has no plane")))?;
        let flat: Vec<usize> = f.rings().flatten().copied().collect();
        let ring2 = |r: &Vec<usize>| {
            r.iter()
                .map(|&i| plane.to_local(p.points[i]))
                .collect::<Vec<_>>()
        };
        let holes: Vec<Vec<DVec2>> = f.holes.iter().map(ring2).collect();
        let tris = triangulate(&ring2(&f.outer), &holes)
            .map_err(|e| BooleanError::Degenerate(format!("face {fi}: {e}")))?;
        for t in tris {
            triangles.push(t.map(|k| flat[k as usize] as u32));
            face_ids.push(base + fi as u32);
        }
    }
    Ok(TriSolid {
        positions: p.points.clone(),
        triangles,
        face_ids,
    })
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn segment_distance(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Centroid distance within which a re-numbered face is matched to an operand face.
pub const ID_MATCH_DISTANCE: f64 = POINT_MERGE;
/// Relative area difference within which a re-numbered face is matched to an operand face.
pub const ID_MATCH_AREA: f64 = 1e-6;

/// What identifies a face geometrically: its plane, area-weighted centroid and area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceSignature {
    pub plane: Plane,
    pub centroid: DVec3,
    pub area: f64,
}

/// Signature of face `fi` of `p` (outer ring minus holes), or `None` without a plane.
pub fn face_signature(p: &Polys, fi: usize) -> Option<FaceSignature> {
    let plane = p.plane(fi)?;
    let mut area = 0.0;
    let mut c = DVec2::ZERO;
    for r in p.faces[fi].rings() {
        let pts: Vec<DVec2> = r.iter().map(|&i| plane.to_local(p.points[i])).collect();
        let n = pts.len();
        for k in 0..n {
            let (a, b) = (pts[k], pts[(k + 1) % n]);
            let cross = a.perp_dot(b);
            area += cross / 2.0;
            c += (a + b) * cross;
        }
    }
    if area.abs() <= AREA_EPS {
        return None;
    }
    let centroid = plane.to_world(c / (6.0 * area));
    Some(FaceSignature {
        plane,
        centroid,
        area,
    })
}

fn same_plane(a: &Plane, b: &Plane) -> bool {
    a.normal.dot(b.normal) >= 1.0 - 1e-9 && a.signed_distance(b.origin).abs() <= PLANARITY
}

/// How far a face is from a source signature (smaller is better); `None` off its plane.
fn signature_cost(f: &FaceSignature, s: &FaceSignature) -> Option<f64> {
    if !same_plane(&f.plane, &s.plane) {
        return None;
    }
    Some(f.centroid.distance(s.centroid) + (f.area.sqrt() - s.area.max(0.0).sqrt()).abs())
}

/// Settle persistent face ids after a boolean (see the module docs). `sources` holds
/// every operand face that had an id. Faces left without an id get fresh ones from the
/// owning object ([`crate::object::assign_face_ids`]).
pub fn resolve_face_ids(p: &mut Polys, sources: &[(u64, FaceSignature)]) {
    let sigs: Vec<Option<FaceSignature>> =
        (0..p.faces.len()).map(|i| face_signature(p, i)).collect();
    let source = |id: u64| sources.iter().find(|(s, _)| *s == id).map(|(_, sig)| sig);
    // Duplicates: the best match keeps the id.
    let mut by_id: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, f) in p.faces.iter().enumerate() {
        if let Some(id) = f.id {
            by_id.entry(id).or_default().push(i);
        }
    }
    for (id, faces) in by_id {
        if faces.len() < 2 {
            continue;
        }
        let cost = |i: usize| -> (bool, f64, f64) {
            match (sigs[i], source(id)) {
                (Some(f), Some(s)) => match signature_cost(&f, s) {
                    Some(c) => (false, c, -f.area),
                    None => (true, f64::INFINITY, -f.area),
                },
                (Some(f), None) => (true, f64::INFINITY, -f.area),
                _ => (true, f64::INFINITY, 0.0),
            }
        };
        let keep = faces
            .iter()
            .copied()
            .min_by(|&a, &b| {
                cost(a)
                    .partial_cmp(&cost(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("non-empty");
        for &i in &faces {
            if i != keep {
                p.faces[i].id = None;
            }
        }
    }
    // Orphans: match unused source faces within tolerance.
    let used: BTreeSet<u64> = p.faces.iter().filter_map(|f| f.id).collect();
    let mut free: Vec<(u64, FaceSignature)> = sources
        .iter()
        .filter(|(id, _)| !used.contains(id))
        .copied()
        .collect();
    for (face, sig) in p.faces.iter_mut().zip(&sigs) {
        if face.id.is_some() {
            continue;
        }
        let Some(f) = sig else { continue };
        let found = free.iter().position(|(_, s)| {
            same_plane(&f.plane, &s.plane)
                && f.centroid.distance(s.centroid) <= ID_MATCH_DISTANCE
                && (f.area - s.area).abs() <= ID_MATCH_AREA * s.area.abs().max(AREA_EPS)
        });
        if let Some(k) = found {
            face.id = Some(free.remove(k).0);
        }
    }
}

/// An engine result back to n-gons with holes (ADR-0001).
///
/// Triangles are grouped by inherited face id; triangles of different source faces are
/// grouped too when the faces are coplanar and their shared edge does not lie on a
/// user-drawn source edge (`preserved`).
///
/// (Lane 16: [`SegIndex`] finds the preserved edges near a point without trying them all.) Each edge-connected group becomes a face: its
/// boundary edges are chained into loops, the one counter-clockwise loop is the outer, the
/// rest are holes. The face keeps its source
/// plane (`planes[face id]`) when every vertex is on it, else a fitted one. Vertices left
/// running straight through between two faces are healed away, unused points dropped,
/// and output edges lying on preserved source edges are flagged user-drawn again.
///
/// Refuses (typed) a group whose loops touch at a vertex (a pinched face) or that does not
/// have exactly one outer loop.
///
/// `ids[label]` is the persistent id of each source face (or `None`); every output face
/// inherits the id of its group's smallest label. Duplicates are settled by the caller
/// ([`resolve_face_ids`]).
pub fn repolygonise(
    out: &TriSolid,
    planes: &[Plane],
    ids: &[Option<u64>],
    preserved: &[(DVec3, DVec3)],
) -> Result<Polys, BooleanError> {
    let pos = &out.positions;
    let nt = out.triangles.len();
    if out.face_ids.iter().any(|&f| f as usize >= planes.len()) {
        return Err(BooleanError::Degenerate("unknown face id".into()));
    }
    // The engine's simplification can leave a triangle carrying a neighbour's face id
    // after an edge collapse on sliver geometry. A triangle must lie on its source plane
    // (either orientation); otherwise it takes the id of an adjacent triangle whose plane
    // it does lie on, and failing that the result is refused rather than guessed at.
    let labels = relabel(out, planes)?;
    let label = |t: usize| labels[t];
    let mut dir: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for (t, tri) in out.triangles.iter().enumerate() {
        for k in 0..3 {
            if dir.insert((tri[k], tri[(k + 1) % 3]), t).is_some() {
                return Err(BooleanError::Degenerate(
                    "engine output is not manifold".into(),
                ));
            }
        }
    }
    // Lane 16: the preserved edges near a point come from a grid of their boxes (it used to
    // be every preserved edge for every output edge: quadratic on labelled imports).
    let near = SegIndex::new(preserved);
    let on_preserved = |u: usize, v: usize| {
        near.candidates(pos[u]).any(|i| {
            let (a, b) = preserved[i];
            segment_distance(pos[u], a, b) <= PLANARITY
                && segment_distance(pos[v], a, b) <= PLANARITY
        })
    };
    // A source face can come out flipped (the second operand of a difference): orient each
    // label's plane the way its output triangles actually face (area-weighted vote).
    let mut vote: Vec<f64> = vec![0.0; planes.len()];
    for (t, tri) in out.triangles.iter().enumerate() {
        let [a, b, c] = tri.map(|i| pos[i as usize]);
        vote[label(t)] += (b - a).cross(c - a).dot(planes[label(t)].normal);
    }
    let flip: Vec<Option<bool>> = vote.iter().map(|&v| Some(v < 0.0)).collect();
    let planes: Vec<Plane> = planes
        .iter()
        .zip(&flip)
        .map(|(p, f)| {
            if f == &Some(true) {
                Plane {
                    origin: p.origin,
                    normal: -p.normal,
                    u: p.u,
                    v: -p.v,
                }
            } else {
                *p
            }
        })
        .collect();
    let coplanar = |a: usize, b: usize| {
        let (pa, pb) = (planes[a], planes[b]);
        pa.normal.dot(pb.normal) >= 1.0 - 1e-10 && pa.signed_distance(pb.origin).abs() <= PLANARITY
    };
    let mut parent: Vec<usize> = (0..nt).collect();
    for (&(u, v), &t) in &dir {
        let Some(&s) = dir.get(&(v, u)) else {
            return Err(BooleanError::Degenerate(
                "engine output is not closed".into(),
            ));
        };
        if t >= s {
            continue;
        }
        let (la, lb) = (label(t), label(s));
        if la == lb || (coplanar(la, lb) && !on_preserved(u as usize, v as usize)) {
            let (ra, rb) = (find(&mut parent, t), find(&mut parent, s));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for t in 0..nt {
        let r = find(&mut parent, t);
        groups.entry(r).or_default().push(t);
    }
    let mut faces: Vec<(usize, PolyFace)> = Vec::new();
    for tris in groups.values() {
        let lab = tris
            .iter()
            .map(|&t| label(t))
            .min()
            .expect("non-empty group");
        let plane = planes[lab];
        let edges: BTreeSet<(usize, usize)> = tris
            .iter()
            .flat_map(|&t| {
                let tri = out.triangles[t];
                (0..3).map(move |k| (tri[k] as usize, tri[(k + 1) % 3] as usize))
            })
            .collect();
        // Boundary edges, chained. A vertex with two ways on is a pinch: refused below.
        let mut next: BTreeMap<usize, usize> = BTreeMap::new();
        for &(u, v) in &edges {
            if !edges.contains(&(v, u)) && next.insert(u, v).is_some() {
                return Err(BooleanError::Degenerate(
                    "a face of the result touches itself at a vertex".into(),
                ));
            }
        }
        let p2 = |i: usize| plane.to_local(pos[i]);
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut loops: Vec<Vec<usize>> = Vec::new();
        for &start in next.keys() {
            if seen.contains(&start) {
                continue;
            }
            let mut lp = Vec::new();
            let mut v = start;
            while seen.insert(v) {
                lp.push(v);
                v = *next
                    .get(&v)
                    .ok_or_else(|| BooleanError::Degenerate("open face boundary".into()))?;
            }
            if v != start {
                return Err(BooleanError::Degenerate("tangled face boundary".into()));
            }
            loops.push(lp);
        }
        let all: Vec<usize> = loops.iter().flatten().copied().collect();
        let distinct: BTreeSet<usize> = all.iter().copied().collect();
        if distinct.len() != all.len() {
            return Err(BooleanError::Degenerate(
                "a face of the result touches itself at a vertex".into(),
            ));
        }
        let areas: Vec<f64> = loops
            .iter()
            .map(|l| signed_area(&l.iter().map(|&i| p2(i)).collect::<Vec<_>>()))
            .collect();
        let outers: Vec<usize> = (0..loops.len()).filter(|&i| areas[i] > 0.0).collect();
        if outers.len() != 1 {
            return Err(BooleanError::Degenerate(format!(
                "a face of the result has {} outer loops",
                outers.len()
            )));
        }
        let o = outers[0];
        let mut holes: Vec<Vec<usize>> = (0..loops.len())
            .filter(|&i| i != o)
            .map(|i| loops[i].clone())
            .collect();
        holes.sort();
        let on_plane = all
            .iter()
            .all(|&i| plane.signed_distance(pos[i]).abs() <= PLANARITY);
        faces.push((
            lab,
            PolyFace {
                outer: loops[o].clone(),
                holes,
                plane: on_plane.then_some(plane),
                id: ids.get(lab).copied().flatten(),
            },
        ));
    }
    faces.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.outer.iter().min().cmp(&b.1.outer.iter().min()))
    });
    let mut p = Polys {
        points: pos.clone(),
        faces: faces.into_iter().map(|(_, f)| f).collect(),
        user_edges: BTreeSet::new(),
    };
    for f in &p.faces {
        for r in f.rings() {
            for k in 0..r.len() {
                let (u, v) = (r[k], r[(k + 1) % r.len()]);
                if on_preserved(u, v) {
                    p.user_edges.insert(Polys::key(u, v));
                }
            }
        }
    }
    let every: BTreeSet<usize> = (0..p.points.len()).collect();
    heal_vertices(&mut p, &every);
    p.compact();
    Ok(p)
}

/// Per-triangle source label, corrected where the engine left a triangle off its plane.
fn relabel(out: &TriSolid, planes: &[Plane]) -> Result<Vec<usize>, BooleanError> {
    let pos = &out.positions;
    let tol = POINT_MERGE;
    let on = |t: usize, l: usize| {
        out.triangles[t]
            .iter()
            .all(|&i| planes[l].signed_distance(pos[i as usize]).abs() <= tol)
    };
    let mut labels: Vec<usize> = out.face_ids.iter().map(|&f| f as usize).collect();
    let mut by_edge: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for (t, tri) in out.triangles.iter().enumerate() {
        for k in 0..3 {
            by_edge.insert((tri[k], tri[(k + 1) % 3]), t);
        }
    }
    for t in 0..out.triangles.len() {
        if on(t, labels[t]) {
            continue;
        }
        let tri = out.triangles[t];
        let neighbours: Vec<usize> = (0..3)
            .filter_map(|k| by_edge.get(&(tri[(k + 1) % 3], tri[k])).map(|&s| labels[s]))
            .collect();
        match neighbours.into_iter().find(|&l| on(t, l)) {
            Some(l) => labels[t] = l,
            None => {
                return Err(BooleanError::Degenerate(
                    "the engine's result left a triangle off every neighbouring source face".into(),
                ));
            }
        }
    }
    Ok(labels)
}

/// `a op b` for two closed polygon B-reps (see [`repolygonise`]). `Ok(None)` when the
/// result is empty. User-drawn edges of either operand survive where the result still
/// has an edge along them.
///
/// Both operands first go through the plane-snapping pass ([`crate::snap`]: nearly
/// coplanar faces made exactly coplanar, vertices re-solved onto them), and so does the
/// re-polygonised result. Since lane 81 (#124), sheets of no thickness the engine leaves
/// where faces touch are then cancelled (`cancel_fins`); a result that is nothing but
/// such sheets is empty.
pub fn combine(
    a: &Polys,
    b: &Polys,
    op: BoolOp,
    engine: &dyn BooleanEngine,
) -> Result<Option<Polys>, BooleanError> {
    Ok(combine_parts(a, b, op, engine)?.result)
}

/// What [`combine_parts`] gives: the result, and, when sheets were cancelled from it, the
/// result as it was before that step (lane 81, #124).
#[derive(Debug, Clone)]
pub struct Combined {
    /// `None`: the result is empty.
    pub result: Option<Polys>,
    /// The result without the sheets cancelled, when they were.
    pub before_fins: Option<Polys>,
}

impl Combined {
    /// `build` (rebuild and validate) the result; `Ok(None)` when it is empty. When sheets
    /// were cancelled from it and it does not build, the result as it was before that step
    /// is built instead and its answer returned. That result is exactly the one the kernel
    /// built before lane 81, so a boolean the step cannot save is refused with the words it
    /// had before (the step's own result often fails in a later, structural check, an open
    /// shell where the old one failed as a vertex on a face), and a boolean the step saves
    /// is unchanged: only a failure falls back, and the engine runs once.
    pub fn build<T, E>(self, mut build: impl FnMut(Polys) -> Result<T, E>) -> Result<Option<T>, E> {
        let Some(p) = self.result else {
            return Ok(None);
        };
        match build(p) {
            Ok(t) => Ok(Some(t)),
            Err(e) => match self.before_fins {
                Some(q) => build(q).map(Some),
                None => Err(e),
            },
        }
    }
}

thread_local! {
    static FINS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Turn the cancelling of sheets (lane 81) on or off for this thread (default on). For tests
/// only: the code without the step, to compare a refusal with.
pub fn set_fin_cancelling(on: bool) {
    FINS.with(|f| f.set(on));
}

/// [`combine`], with the result as it was before sheets were cancelled from it, if they
/// were (see [`Combined::build`]).
pub fn combine_parts(
    a: &Polys,
    b: &Polys,
    op: BoolOp,
    engine: &dyn BooleanEngine,
) -> Result<Combined, BooleanError> {
    let (mut a, mut b) = (a.clone(), b.clone());
    crate::snap::snap_planes(&mut [&mut a, &mut b]);
    let (a, b) = (&a, &b);
    let ta = polys_tri_solid(a, 0)?;
    let tb = polys_tri_solid(b, a.faces.len() as u32)?;
    let mut planes = Vec::with_capacity(a.faces.len() + b.faces.len());
    for (p, n) in [(a, a.faces.len()), (b, b.faces.len())] {
        for i in 0..n {
            planes.push(
                p.plane(i)
                    .ok_or_else(|| BooleanError::Degenerate(format!("face {i} has no plane")))?,
            );
        }
    }
    let mut preserved = Vec::new();
    let mut ids = Vec::with_capacity(planes.len());
    let mut sources = Vec::new();
    for p in [a, b] {
        for &(u, v) in &p.user_edges {
            preserved.push((p.points[u], p.points[v]));
        }
        for (i, f) in p.faces.iter().enumerate() {
            ids.push(f.id);
            if let (Some(id), Some(sig)) = (f.id, face_signature(p, i)) {
                sources.push((id, sig));
            }
        }
    }
    let raw = engine.run(&ta, &tb, op)?;
    // Welded first (slivers closed); if the weld left the surface unusable, the engine's
    // own output as it was.
    let mut welded = raw.clone();
    let was_welded = crate::snap::weld_output(&mut welded) > 0;
    let out = if was_welded { welded } else { raw.clone() };
    let empty = Combined {
        result: None,
        before_fins: None,
    };
    if out.triangles.is_empty() || out.volume().abs() <= AREA_EPS * POINT_MERGE {
        return Ok(empty);
    }
    let mut res = match repolygonise(&out, &planes, &ids, &preserved) {
        Ok(r) => r,
        Err(e) if was_welded => repolygonise(&raw, &planes, &ids, &preserved).map_err(|_| e)?,
        Err(e) => return Err(e),
    };
    // Fins are cancelled on a result whose every face came out on its own source plane. A
    // face that needed a fitted plane means the engine or the weld moved corners (an
    // overlap at the merge distance welded in some places and not others), and there the
    // sheet is not the only thing wrong: such a result is cancelled only when nothing is
    // left, a result that is all sheet (an overlap thinner than the merge distance, whose
    // two faces were welded onto each other).
    let on_source_planes = res.faces.iter().all(|f| f.plane.is_some());
    crate::snap::snap_planes(&mut [&mut res]);
    let fins = if FINS.with(|f| f.get()) {
        fin_clusters(&res)
    } else {
        Vec::new()
    };
    // The result as it was before the step is kept beside the step's own, so a caller whose
    // validation refuses the step's result builds that one instead and refuses with its
    // words (`Combined::build`): only results the step saves change.
    let mut before_fins = None;
    if !fins.is_empty() {
        let mut cancelled = res.clone();
        if cancel_fins(&mut cancelled, &fins) {
            if cancelled.faces.is_empty() {
                return Ok(empty);
            }
            if on_source_planes {
                before_fins = Some(std::mem::replace(&mut res, cancelled));
            }
        }
    }
    resolve_face_ids(&mut res, &sources);
    if let Some(b) = &mut before_fins {
        resolve_face_ids(b, &sources);
    }
    Ok(Combined {
        result: Some(res),
        before_fins,
    })
}

/// Two faces of one plane facing opposite ways overlap in area: the same test as the
/// validator's face-on-face check ([`crate::intersect`], check 4), restricted to faces that
/// face opposite ways. Contours are taken in `a`'s plane.
fn fin_pair(p: &Polys, a: usize, b: usize, pa: &Plane, pb: &Plane) -> bool {
    if pa.normal.dot(pb.normal) > -(1.0 - 1e-9)
        || p.faces[b]
            .rings()
            .flatten()
            .any(|&v| pa.signed_distance(p.points[v]).abs() > POINT_MERGE)
    {
        return false;
    }
    let shared = sawhorse_tessellate::overlay::intersection(
        &face_contours(p, a, pa),
        &face_contours(p, b, pa),
    );
    sawhorse_tessellate::overlay::area(&shared) > AREA_EPS
}

fn face_contours(p: &Polys, f: usize, pl: &Plane) -> Vec<Vec<[f64; 2]>> {
    p.faces[f]
        .rings()
        .map(|r| {
            r.iter()
                .map(|&v| pl.to_local(p.points[v]).to_array())
                .collect()
        })
        .collect()
}

/// Lane 81 (#124): cancel the zero-thickness fins the engine can leave where two operand
/// faces touch face to face. Manifold resolves coplanar faces by symbolic perturbation, and
/// for touching faces that can keep both faces over the contact area, facing opposite ways:
/// a sheet of no thickness hinged to the true result (a shelf end on a dado's floor, the
/// two faces of boards that only touch). The validator refuses such a surface as faces
/// overlapping, or an edge through a face; the solid it bounds is the same without the
/// sheet. Here the faces of each such plane are summed as signed areas: what one facing
/// covers minus what the other covers, each way. The pieces must land on the faces' own
/// vertices (a vertex the overlay cannot match leaves everything as it was, and the
/// validator refuses it as before). Every changed surface is one the validator's
/// face-on-face check refuses, so a result that was accepted cannot change. Returns
/// whether anything changed.
fn cancel_fins(p: &mut Polys, clusters: &[Vec<usize>]) -> bool {
    if clusters.is_empty() {
        return false;
    }
    let planes: Vec<Option<Plane>> = (0..p.faces.len()).map(|i| p.plane(i)).collect();
    let mut replaced: BTreeMap<usize, Vec<PolyFace>> = BTreeMap::new();
    for members in clusters {
        match sum_plane(p, members, &planes) {
            Some(faces) => {
                replaced.insert(members[0], faces);
            }
            None => return false,
        }
    }
    let gone: BTreeSet<usize> = clusters.iter().flatten().copied().collect();
    let mut touched: BTreeSet<usize> = gone
        .iter()
        .flat_map(|&f| p.faces[f].rings().flatten().copied().collect::<Vec<_>>())
        .collect();
    let old = std::mem::take(&mut p.faces);
    for (i, f) in old.into_iter().enumerate() {
        if let Some(new) = replaced.remove(&i) {
            p.faces.extend(new);
        } else if !gone.contains(&i) {
            p.faces.push(f);
        }
    }
    drop_spikes(p);
    touched.extend(close_t_junctions(p));
    let edges: BTreeSet<(usize, usize)> = p
        .directed_edges()
        .keys()
        .map(|&(a, b)| Polys::key(a, b))
        .collect();
    p.user_edges.retain(|e| edges.contains(e));
    heal_vertices(p, &touched);
    p.compact();
    true
}

/// The faces of `p` in fins ([`fin_pair`]), grouped: faces joined by overlapping across
/// their plane are one group, in face order, each group in face order; none when there is
/// no fin.
fn fin_clusters(p: &Polys) -> Vec<Vec<usize>> {
    let n = p.faces.len();
    let planes: Vec<Option<Plane>> = (0..n).map(|i| p.plane(i)).collect();
    // Faces by their normal up to sign, then by offset, so only near pairs are tried.
    // (Faces facing one way, faces facing the other), each as (offset, face).
    type Side = Vec<(f64, usize)>;
    let mut by_normal: BTreeMap<[u64; 3], (Side, Side)> = BTreeMap::new();
    for (i, pl) in planes.iter().enumerate() {
        let Some(pl) = pl else { continue };
        let nn = pl.normal;
        let first = [nn.x, nn.y, nn.z]
            .into_iter()
            .find(|c| *c != 0.0)
            .unwrap_or(0.0);
        // (Adding zero turns -0.0 into 0.0, so a normal and its negation share a key.)
        let c = (if first < 0.0 { -nn } else { nn }) + DVec3::ZERO;
        let e = by_normal
            .entry([c.x.to_bits(), c.y.to_bits(), c.z.to_bits()])
            .or_default();
        let side = if first < 0.0 { &mut e.1 } else { &mut e.0 };
        side.push((c.dot(pl.origin), i));
    }
    let boxes: Vec<(DVec3, DVec3)> = p
        .faces
        .iter()
        .map(|f| {
            f.rings().flatten().fold(
                (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
                |(lo, hi), &v| (lo.min(p.points[v]), hi.max(p.points[v])),
            )
        })
        .collect();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut found = false;
    for (front, back) in by_normal.values_mut() {
        back.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for &(da, a) in front.iter() {
            let start = back.partition_point(|&(d, _)| d < da - 2.0 * POINT_MERGE);
            for &(db, b) in &back[start..] {
                if db > da + 2.0 * POINT_MERGE {
                    break;
                }
                let (x, y) = (a.min(b), a.max(b));
                let ((lo1, hi1), (lo2, hi2)) = (boxes[x], boxes[y]);
                if lo1.cmple(hi2 + POINT_MERGE).all()
                    && lo2.cmple(hi1 + POINT_MERGE).all()
                    && let (Some(px), Some(py)) = (&planes[x], &planes[y])
                    && fin_pair(p, x, y, px, py)
                {
                    let (rx, ry) = (find(&mut parent, x), find(&mut parent, y));
                    if rx != ry {
                        parent[rx.max(ry)] = rx.min(ry);
                    }
                    found = true;
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut clusters: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        clusters.entry(r).or_default().push(i);
    }
    clusters.into_values().filter(|v| v.len() > 1).collect()
}

/// The points the faces of a [`Polys`] use, sorted by x, to find those on a segment.
struct UsedPoints<'a> {
    points: &'a [DVec3],
    by_x: Vec<usize>,
}

impl<'a> UsedPoints<'a> {
    fn new(p: &'a Polys) -> UsedPoints<'a> {
        let used: BTreeSet<usize> = p
            .faces
            .iter()
            .flat_map(|f| f.rings().flatten().copied())
            .collect();
        let mut by_x: Vec<usize> = used.into_iter().collect();
        by_x.sort_by(|&a, &b| p.points[a].x.total_cmp(&p.points[b].x).then(a.cmp(&b)));
        UsedPoints {
            points: &p.points,
            by_x,
        }
    }

    /// `ring` with every used point that lies inside one of its edges (within
    /// [`PLANARITY`]) put in, in order along the edge.
    fn with_points_on(&self, ring: &[usize]) -> Vec<usize> {
        let pts = self.points;
        let mut full = Vec::with_capacity(ring.len());
        for k in 0..ring.len() {
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            full.push(a);
            let (pa, pb) = (pts[a], pts[b]);
            let (lo, hi) = (pa.x.min(pb.x) - PLANARITY, pa.x.max(pb.x) + PLANARITY);
            let start = self.by_x.partition_point(|&w| pts[w].x < lo);
            let ab = pb - pa;
            let len2 = ab.length_squared();
            if len2 == 0.0 {
                continue;
            }
            let mut on: Vec<(f64, usize)> = self.by_x[start..]
                .iter()
                .take_while(|&&w| pts[w].x <= hi)
                .copied()
                .filter(|&w| w != a && w != b)
                .filter_map(|w| {
                    let t = (pts[w] - pa).dot(ab) / len2;
                    (t > 0.0 && t < 1.0 && segment_distance(pts[w], pa, pb) <= PLANARITY)
                        .then_some((t, w))
                })
                .collect();
            on.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            full.extend(on.into_iter().map(|(_, w)| w));
        }
        full
    }
}

/// After fins go, a face whose edge a cancelled face's corner sat on (a T-junction the fin
/// covered) gets that corner put in, so the edges on both sides match; healing then takes
/// out the corners that run straight through. Returns the points put in.
fn close_t_junctions(p: &mut Polys) -> BTreeSet<usize> {
    let rings: Vec<Vec<Vec<usize>>> = {
        let used = UsedPoints::new(p);
        p.faces
            .iter()
            .map(|f| f.rings().map(|r| used.with_points_on(r)).collect())
            .collect()
    };
    let mut put_in = BTreeSet::new();
    for (f, new) in p.faces.iter_mut().zip(rings) {
        for (r, n) in std::iter::once(&mut f.outer)
            .chain(f.holes.iter_mut())
            .zip(new)
        {
            if n.len() != r.len() {
                let had: BTreeSet<usize> = r.iter().copied().collect();
                put_in.extend(n.iter().copied().filter(|v| !had.contains(v)));
                *r = n;
            }
        }
    }
    put_in
}

/// A fin can be hinged to a face along a line the face's ring runs out along and back (a
/// spike of no area, whose two sides were the fin's edges). With the fin gone the spike
/// has nothing on its other side: its tip goes, until no ring turns straight back. A ring
/// that was nothing but spikes (a face or hole of no area) goes with them.
fn drop_spikes(p: &mut Polys) {
    let pts = p.points.clone();
    let between = |x: usize, a: usize, b: usize| {
        x != a && x != b && segment_distance(pts[x], pts[a], pts[b]) <= PLANARITY
    };
    for f in &mut p.faces {
        for r in std::iter::once(&mut f.outer).chain(f.holes.iter_mut()) {
            loop {
                let n = r.len();
                if n < 3 {
                    break;
                }
                let tip = (0..n).find(|&k| {
                    let (u, v, w) = (r[(k + n - 1) % n], r[k], r[(k + 1) % n]);
                    u == w || between(w, u, v) || between(u, v, w)
                });
                let Some(k) = tip else { break };
                let (u, w) = (r[(k + n - 1) % n], r[(k + 1) % n]);
                if u == w {
                    // Out and back to the same vertex: the tip and one copy of it go.
                    let (a, b) = (k, (k + 1) % n);
                    let (hi, lo) = (a.max(b), a.min(b));
                    r.remove(hi);
                    r.remove(lo);
                } else {
                    r.remove(k);
                }
            }
        }
        f.holes.retain(|h| h.len() >= 3);
    }
    p.faces.retain(|f| f.outer.len() >= 3);
}

/// The faces `members` (one plane, both facings) summed as signed areas: each facing keeps
/// what it covers and the other does not. `None` when a piece's corner is not one of the
/// members' vertices.
fn sum_plane(p: &Polys, members: &[usize], planes: &[Option<Plane>]) -> Option<Vec<PolyFace>> {
    use sawhorse_tessellate::overlay;
    let reference = planes[members[0]]?;
    let facing = |f: usize| planes[f].is_some_and(|pl| pl.normal.dot(reference.normal) > 0.0);
    let union_of = |side: bool| -> Vec<Vec<[f64; 2]>> {
        let mut acc: Vec<Vec<[f64; 2]>> = Vec::new();
        for &f in members.iter().filter(|&&f| facing(f) == side) {
            let c = face_contours(p, f, &reference);
            acc = if acc.is_empty() {
                c
            } else {
                overlay::union(&acc, &c).into_iter().flatten().collect()
            };
        }
        acc
    };
    let (front, back) = (union_of(true), union_of(false));
    let verts: Vec<usize> = members
        .iter()
        .flat_map(|&f| p.faces[f].rings().flatten().copied().collect::<Vec<_>>())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let used = UsedPoints::new(p);
    let mut out = Vec::new();
    for (side, keep, minus) in [(true, &front, &back), (false, &back, &front)] {
        let shapes = if minus.is_empty() {
            vec![keep.clone()]
        } else if keep.is_empty() {
            Vec::new()
        } else {
            overlay::difference(keep, minus)
        };
        for shape in shapes {
            if shape.is_empty() || overlay::area(std::slice::from_ref(&shape)) <= AREA_EPS {
                continue;
            }
            let mut rings: Vec<Vec<usize>> = Vec::new();
            for c in &shape {
                let mut ring: Vec<usize> = Vec::new();
                for q in c {
                    let q = DVec2::from(*q);
                    let (d, v) = verts
                        .iter()
                        .map(|&v| (reference.to_local(p.points[v]).distance(q), v))
                        .min_by(|a, b| a.0.total_cmp(&b.0))?;
                    if d > POINT_MERGE {
                        return None;
                    }
                    if ring.last() != Some(&v) {
                        ring.push(v);
                    }
                }
                while ring.len() > 1 && ring.first() == ring.last() {
                    ring.pop();
                }
                // Corners the overlay dropped as collinear go back, so the neighbouring
                // faces' edges still meet this face's.
                let mut full = used.with_points_on(&ring);
                if full.len() < 3 {
                    return None;
                }
                let area = signed_area(
                    &full
                        .iter()
                        .map(|&v| reference.to_local(p.points[v]))
                        .collect::<Vec<_>>(),
                );
                // Outer counter-clockwise, holes clockwise, seen from the facing kept.
                let want_ccw = (rings.is_empty()) == side;
                if (area > 0.0) != want_ccw {
                    full.reverse();
                }
                rings.push(full);
            }
            // The member of that facing it overlaps most gives its plane and id.
            let owner = members
                .iter()
                .copied()
                .filter(|&f| facing(f) == side)
                .max_by(|&a, &b| {
                    let o = |f: usize| {
                        overlay::area(&overlay::intersection(
                            &face_contours(p, f, &reference),
                            &shape,
                        ))
                    };
                    o(a).total_cmp(&o(b)).then(b.cmp(&a))
                })?;
            let plane = planes[owner].filter(|pl| {
                rings
                    .iter()
                    .flatten()
                    .all(|&v| pl.signed_distance(p.points[v]).abs() <= PLANARITY)
            });
            let outer = rings.remove(0);
            out.push(PolyFace {
                outer,
                holes: rings,
                plane,
                id: p.faces[owner].id,
            });
        }
    }
    Some(out)
}

#[cfg(all(test, feature = "manifold"))]
mod tests {
    use super::*;

    /// Axis-aligned box with shared vertices, two triangles per side, face id per side.
    pub(crate) fn cube(min: DVec3, size: f64, face_base: u32) -> TriSolid {
        let p = |x: f64, y: f64, z: f64| min + DVec3::new(x, y, z) * size;
        let positions = vec![
            p(0., 0., 0.),
            p(1., 0., 0.),
            p(1., 1., 0.),
            p(0., 1., 0.),
            p(0., 0., 1.),
            p(1., 0., 1.),
            p(1., 1., 1.),
            p(0., 1., 1.),
        ];
        let quads: [[u32; 4]; 6] = [
            [0, 3, 2, 1], // bottom (-z)
            [4, 5, 6, 7], // top (+z)
            [0, 1, 5, 4], // front (-y)
            [2, 3, 7, 6], // back (+y)
            [1, 2, 6, 5], // right (+x)
            [3, 0, 4, 7], // left (-x)
        ];
        let mut triangles = Vec::new();
        let mut face_ids = Vec::new();
        for (i, q) in quads.iter().enumerate() {
            triangles.push([q[0], q[1], q[2]]);
            triangles.push([q[0], q[2], q[3]]);
            face_ids.extend([face_base + i as u32; 2]);
        }
        TriSolid {
            positions,
            triangles,
            face_ids,
        }
    }

    #[test]
    fn cube_volume() {
        assert!((cube(DVec3::ZERO, 2.0, 0).volume() - 8.0).abs() < 1e-12);
    }

    /// A 0.1 mm cube 6.6 km out measures its own volume (lane 23): about the world origin
    /// its triangles' products cancel to rounding far above 1e-12 m3.
    #[test]
    fn a_tiny_cube_far_out_measures_its_volume() {
        let v = cube(DVec3::new(-1983.4556, 4664.0710, -4661.6253), 1e-4, 0).volume();
        assert!((v - 1e-12).abs() < 1e-20, "volume {v}");
    }

    #[test]
    fn manifold_difference_and_union() {
        let a = cube(DVec3::ZERO, 2.0, 0);
        let b = cube(DVec3::splat(1.0), 2.0, 100);
        let d = ManifoldEngine.run(&a, &b, BoolOp::Difference).unwrap();
        assert!(
            (d.volume() - 7.0).abs() < 1e-9,
            "difference volume {}",
            d.volume()
        );
        let u = ManifoldEngine.run(&a, &b, BoolOp::Union).unwrap();
        assert!(
            (u.volume() - 15.0).abs() < 1e-9,
            "union volume {}",
            u.volume()
        );
        let i = ManifoldEngine.run(&a, &b, BoolOp::Intersection).unwrap();
        assert!(
            (i.volume() - 1.0).abs() < 1e-9,
            "intersection volume {}",
            i.volume()
        );
        // Face ids survive the engine: every output triangle traces to an operand face.
        assert!(d.face_ids.iter().all(|&f| f < 6 || (100..106).contains(&f)));
        assert_eq!(d.face_ids.len(), d.triangles.len());
    }

    #[test]
    fn open_operand_is_a_typed_error() {
        let mut a = cube(DVec3::ZERO, 1.0, 0);
        a.triangles.pop();
        a.face_ids.pop();
        let b = cube(DVec3::ZERO, 1.0, 0);
        assert!(matches!(
            ManifoldEngine.run(&a, &b, BoolOp::Union),
            Err(BooleanError::NotManifold(0, _))
        ));
    }
}

/// Segments binned by their boxes (grown by `PLANARITY`) in a uniform grid: the segments
/// whose box may hold a point (lane 16). Few segments are simply all tried.
struct SegIndex {
    cell: f64,
    origin: DVec3,
    bins: std::collections::HashMap<(i64, i64, i64), Vec<u32>>,
    big: Vec<u32>,
    all: usize,
}

impl SegIndex {
    fn new(segs: &[(DVec3, DVec3)]) -> SegIndex {
        let mut ix = SegIndex {
            cell: 1.0,
            origin: DVec3::ZERO,
            bins: std::collections::HashMap::new(),
            big: Vec::new(),
            all: segs.len(),
        };
        if segs.len() < 32 {
            return ix;
        }
        let lo = segs
            .iter()
            .fold(DVec3::splat(f64::INFINITY), |m, s| m.min(s.0).min(s.1));
        let mut lens: Vec<f64> = segs
            .iter()
            .map(|s| (s.1 - s.0).abs().max_element())
            .collect();
        if !lo.is_finite() || lens.iter().any(|l| !l.is_finite()) {
            return ix;
        }
        let mid = lens.len() / 2;
        lens.select_nth_unstable_by(mid, f64::total_cmp);
        ix.cell = (lens[mid] * 2.0).max(PLANARITY * 1e3);
        ix.origin = lo - DVec3::splat(ix.cell);
        for (i, s) in segs.iter().enumerate() {
            let (a, z) = (
                ix.at(s.0.min(s.1) - DVec3::splat(PLANARITY)),
                ix.at(s.0.max(s.1) + DVec3::splat(PLANARITY)),
            );
            if (z.0 - a.0 + 1) * (z.1 - a.1 + 1) * (z.2 - a.2 + 1) > 64 {
                ix.big.push(i as u32);
                continue;
            }
            for x in a.0..=z.0 {
                for y in a.1..=z.1 {
                    for w in a.2..=z.2 {
                        ix.bins.entry((x, y, w)).or_default().push(i as u32);
                    }
                }
            }
        }
        ix.all = 0;
        ix
    }

    fn at(&self, p: DVec3) -> (i64, i64, i64) {
        let q = (p - self.origin) / self.cell;
        (q.x.floor() as i64, q.y.floor() as i64, q.z.floor() as i64)
    }

    /// Every segment whose grown box may hold `p` (all of them when the index is off).
    fn candidates(&self, p: DVec3) -> Box<dyn Iterator<Item = usize> + '_> {
        if self.all > 0 {
            return Box::new(0..self.all);
        }
        let cell = self
            .bins
            .get(&self.at(p))
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        Box::new(self.big.iter().chain(cell.iter()).map(|&i| i as usize))
    }
}
