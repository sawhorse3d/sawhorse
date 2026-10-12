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
    /// Signed volume by the divergence theorem (positive for outward-oriented solids).
    pub fn volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let (a, b, c) = (
                    self.positions[t[0] as usize],
                    self.positions[t[1] as usize],
                    self.positions[t[2] as usize],
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
/// user-drawn source edge (`preserved`). Each edge-connected group becomes a face: its
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
    let on_preserved = |u: usize, v: usize| {
        preserved.iter().any(|&(a, b)| {
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
/// re-polygonised result.
pub fn combine(
    a: &Polys,
    b: &Polys,
    op: BoolOp,
    engine: &dyn BooleanEngine,
) -> Result<Option<Polys>, BooleanError> {
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
    if out.triangles.is_empty() || out.volume().abs() <= AREA_EPS * POINT_MERGE {
        return Ok(None);
    }
    let mut res = match repolygonise(&out, &planes, &ids, &preserved) {
        Ok(r) => r,
        Err(e) if was_welded => repolygonise(&raw, &planes, &ids, &preserved).map_err(|_| e)?,
        Err(e) => return Err(e),
    };
    crate::snap::snap_planes(&mut [&mut res]);
    resolve_face_ids(&mut res, &sources);
    Ok(Some(res))
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
