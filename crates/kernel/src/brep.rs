//! Half-edge boundary representation with hole loops.
//!
//! - A [`Face`] is one outer [`Loop`] plus any number of hole loops, and a stored [`Plane`].
//!   The outer loop runs counter-clockwise seen from the face normal; holes run clockwise.
//! - Every [`HalfEdge`] belongs to exactly one loop, has `next`/`prev` within it, and an
//!   optional `twin` (the opposite direction on the neighbouring face).
//! - An [`Edge`] owns one half-edge (open boundary) or two twinned ones.
//! - A shell is watertight when every half-edge has a twin.
//!
//! Collections are slotmaps; iteration order is insertion order, so everything derived
//! from a B-rep (hashes, meshes, exports) is deterministic.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;
use slotmap::{Key, SlotMap};

use crate::error::BuildError;
use crate::ids::{EdgeId, FaceId, HalfEdgeId, LoopId, VertexId};
use crate::plane::Plane;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub pos: DVec3,
    /// Some half-edge leaving this vertex (null for an isolated vertex, which the
    /// validator rejects).
    pub out: HalfEdgeId,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HalfEdge {
    pub origin: VertexId,
    pub twin: Option<HalfEdgeId>,
    pub next: HalfEdgeId,
    pub prev: HalfEdgeId,
    pub lp: LoopId,
    pub edge: EdgeId,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub he: HalfEdgeId,
    pub twin: Option<HalfEdgeId>,
    /// Drawn by the user (split_face / imprint), as opposed to made by the kernel (extrude
    /// side walls, push/pull rails). Coplanar merge never dissolves a user edge, so an
    /// imprint survives being pushed in and pulled back flush.
    pub user: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loop {
    pub first: HalfEdgeId,
    pub face: FaceId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub outer: LoopId,
    pub holes: Vec<LoopId>,
    pub plane: Plane,
}

impl Face {
    /// Outer loop first, then holes.
    pub fn loops(&self) -> impl Iterator<Item = LoopId> + '_ {
        std::iter::once(self.outer).chain(self.holes.iter().copied())
    }
}

/// Input to [`Brep::from_polygons`]: vertex indices into a point list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PolyFace {
    pub outer: Vec<usize>,
    pub holes: Vec<Vec<usize>>,
    /// The face's stored plane; `None` fits one from the outer loop (Newell). Edits keep
    /// the original plane of a face they reshape, so its frame never drifts.
    pub plane: Option<Plane>,
}

impl PolyFace {
    pub fn new(outer: Vec<usize>, holes: Vec<Vec<usize>>) -> PolyFace {
        PolyFace {
            outer,
            holes,
            plane: None,
        }
    }

    /// Outer ring first, then holes.
    pub fn rings(&self) -> impl Iterator<Item = &Vec<usize>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }
}

/// A B-rep as plain data: points, indexed faces with holes and planes, and the user-drawn
/// edges (as sorted index pairs). Every topology edit in the kernel is
/// `to_polys` -> edit the data -> `Brep::build` -> validate, which keeps the half-edge
/// surgery in one audited place (`build`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Polys {
    pub points: Vec<DVec3>,
    pub faces: Vec<PolyFace>,
    pub user_edges: BTreeSet<(usize, usize)>,
}

impl Polys {
    /// Undirected edge key.
    pub fn key(a: usize, b: usize) -> (usize, usize) {
        (a.min(b), a.max(b))
    }

    pub fn is_user(&self, a: usize, b: usize) -> bool {
        self.user_edges.contains(&Self::key(a, b))
    }

    /// Plane of face `i` (stored, or fitted from its outer loop).
    pub fn plane(&self, i: usize) -> Option<Plane> {
        let f = &self.faces[i];
        f.plane.or_else(|| {
            let pts: Vec<DVec3> = f.outer.iter().map(|&k| self.points[k]).collect();
            Plane::fit(&pts).ok()
        })
    }

    /// Directed edge (a, b) -> (face index, ring index).
    pub fn directed_edges(&self) -> BTreeMap<(usize, usize), (usize, usize)> {
        let mut m = BTreeMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for (ri, r) in f.rings().enumerate() {
                for k in 0..r.len() {
                    m.insert((r[k], r[(k + 1) % r.len()]), (fi, ri));
                }
            }
        }
        m
    }

    /// Drop points no face uses and renumber (keeps order). Returns old -> new index.
    pub fn compact(&mut self) -> Vec<Option<usize>> {
        let mut used = vec![false; self.points.len()];
        for f in &self.faces {
            for r in f.rings() {
                for &i in r {
                    used[i] = true;
                }
            }
        }
        let mut map = vec![None; self.points.len()];
        let mut pts = Vec::new();
        for (i, p) in self.points.iter().enumerate() {
            if used[i] {
                map[i] = Some(pts.len());
                pts.push(*p);
            }
        }
        for f in &mut self.faces {
            for r in std::iter::once(&mut f.outer).chain(f.holes.iter_mut()) {
                for i in r.iter_mut() {
                    *i = map[*i].expect("used point");
                }
            }
        }
        self.user_edges = self
            .user_edges
            .iter()
            .filter_map(|&(a, b)| Some(Self::key(map[a]?, map[b]?)))
            .collect();
        self.points = pts;
        map
    }
}

#[derive(Debug, Clone, Default)]
pub struct Brep {
    pub(crate) vertices: SlotMap<VertexId, Vertex>,
    pub(crate) half_edges: SlotMap<HalfEdgeId, HalfEdge>,
    pub(crate) edges: SlotMap<EdgeId, Edge>,
    pub(crate) loops: SlotMap<LoopId, Loop>,
    pub(crate) faces: SlotMap<FaceId, Face>,
}

impl Brep {
    /// Build a B-rep from indexed polygons. Twins are paired by matching directed edges
    /// (a->b with b->a); a directed edge used twice is a typed error. Face planes are fitted
    /// from the outer loop.
    pub fn from_polygons(points: &[DVec3], faces: &[PolyFace]) -> Result<Brep, BuildError> {
        let b = Self::build(&Polys {
            points: points.to_vec(),
            faces: faces.to_vec(),
            user_edges: BTreeSet::new(),
        })?;
        b.debug_validate("from_polygons");
        Ok(b)
    }

    /// Build from plain data without validating (callers validate; speculative edits use
    /// this so a bad candidate is a value, not a debug panic). Vertex order follows
    /// `points`, face order follows `faces`.
    pub fn build(polys: &Polys) -> Result<Brep, BuildError> {
        let (points, faces) = (&polys.points, &polys.faces);
        let mut b = Brep::default();
        let vids: Vec<VertexId> = points
            .iter()
            .map(|&pos| {
                b.vertices.insert(Vertex {
                    pos,
                    out: HalfEdgeId::null(),
                })
            })
            .collect();
        // Directed edge (from, to) -> half-edge, in a BTreeMap so pairing order is deterministic.
        let mut directed: BTreeMap<(usize, usize), HalfEdgeId> = BTreeMap::new();

        for (fi, pf) in faces.iter().enumerate() {
            let rings: Vec<&Vec<usize>> =
                std::iter::once(&pf.outer).chain(pf.holes.iter()).collect();
            for ring in &rings {
                if ring.len() < 3 {
                    return Err(BuildError::LoopTooShort { face: fi });
                }
                if ring.iter().any(|&i| i >= points.len()) {
                    return Err(BuildError::BadIndex { face: fi });
                }
                let distinct: BTreeSet<usize> = ring.iter().copied().collect();
                if distinct.len() != ring.len() {
                    return Err(BuildError::RepeatedVertex { face: fi });
                }
            }
            let plane = match pf.plane {
                Some(p) => p,
                None => {
                    let outer_pts: Vec<DVec3> = pf.outer.iter().map(|&i| points[i]).collect();
                    Plane::fit(&outer_pts).map_err(|_| BuildError::DegenerateFace { face: fi })?
                }
            };
            let fid = b.faces.insert(Face {
                outer: LoopId::null(),
                holes: Vec::new(),
                plane,
            });
            for (ri, ring) in rings.iter().enumerate() {
                let lid = b.loops.insert(Loop {
                    first: HalfEdgeId::null(),
                    face: fid,
                });
                let hes: Vec<HalfEdgeId> = ring
                    .iter()
                    .map(|&i| {
                        b.half_edges.insert(HalfEdge {
                            origin: vids[i],
                            twin: None,
                            next: HalfEdgeId::null(),
                            prev: HalfEdgeId::null(),
                            lp: lid,
                            edge: EdgeId::null(),
                        })
                    })
                    .collect();
                let n = hes.len();
                for k in 0..n {
                    let he = &mut b.half_edges[hes[k]];
                    he.next = hes[(k + 1) % n];
                    he.prev = hes[(k + n - 1) % n];
                    let key = (ring[k], ring[(k + 1) % n]);
                    if directed.insert(key, hes[k]).is_some() {
                        return Err(BuildError::DuplicateDirectedEdge {
                            from: key.0,
                            to: key.1,
                        });
                    }
                }
                b.loops[lid].first = hes[0];
                if ri == 0 {
                    b.faces[fid].outer = lid;
                } else {
                    b.faces[fid].holes.push(lid);
                }
            }
        }

        // Pair twins and create edges.
        for (&(from, to), &he) in &directed {
            if !b.half_edges[he].edge.is_null() {
                continue;
            }
            let twin = directed.get(&(to, from)).copied();
            let user = polys.user_edges.contains(&Polys::key(from, to));
            let eid = b.edges.insert(Edge { he, twin, user });
            b.half_edges[he].edge = eid;
            b.half_edges[he].twin = twin;
            if let Some(t) = twin {
                b.half_edges[t].edge = eid;
                b.half_edges[t].twin = Some(he);
            }
        }
        let outs: Vec<(VertexId, HalfEdgeId)> =
            b.half_edges.iter().map(|(id, h)| (h.origin, id)).collect();
        for (v, he) in outs {
            if b.vertices[v].out.is_null() {
                b.vertices[v].out = he;
            }
        }
        Ok(b)
    }

    /// Plain-data copy: points in vertex order, faces in face order with their stored
    /// planes, user edges.
    pub fn to_polys(&self) -> Polys {
        let vindex: BTreeMap<VertexId, usize> = self
            .vertices
            .keys()
            .enumerate()
            .map(|(i, k)| (k, i))
            .collect();
        let ring =
            |l: LoopId| -> Vec<usize> { self.loop_vertices(l).iter().map(|v| vindex[v]).collect() };
        let faces = self
            .faces
            .values()
            .map(|f| PolyFace {
                outer: ring(f.outer),
                holes: f.holes.iter().map(|&h| ring(h)).collect(),
                plane: Some(f.plane),
            })
            .collect();
        let user_edges = self
            .edges
            .values()
            .filter(|e| e.user)
            .map(|e| {
                let h = &self.half_edges[e.he];
                Polys::key(vindex[&h.origin], vindex[&self.dest(e.he)])
            })
            .collect();
        Polys {
            points: self.vertices.values().map(|v| v.pos).collect(),
            faces,
            user_edges,
        }
    }

    // ---- queries -------------------------------------------------------------------

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }
    pub fn loop_count(&self) -> usize {
        self.loops.len()
    }
    pub fn half_edge_count(&self) -> usize {
        self.half_edges.len()
    }

    pub fn vertex(&self, id: VertexId) -> Option<&Vertex> {
        self.vertices.get(id)
    }
    pub fn half_edge(&self, id: HalfEdgeId) -> Option<&HalfEdge> {
        self.half_edges.get(id)
    }
    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(id)
    }
    pub fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.get(id)
    }
    pub fn lp(&self, id: LoopId) -> Option<&Loop> {
        self.loops.get(id)
    }

    pub fn vertices(&self) -> impl Iterator<Item = (VertexId, &Vertex)> {
        self.vertices.iter()
    }
    pub fn edges(&self) -> impl Iterator<Item = (EdgeId, &Edge)> {
        self.edges.iter()
    }
    pub fn faces(&self) -> impl Iterator<Item = (FaceId, &Face)> {
        self.faces.iter()
    }
    pub fn face_ids(&self) -> Vec<FaceId> {
        self.faces.keys().collect()
    }

    /// Destination vertex of a half-edge (origin of its `next`).
    pub fn dest(&self, he: HalfEdgeId) -> VertexId {
        self.half_edges[self.half_edges[he].next].origin
    }

    /// Half-edges of a loop in order. Stops after `half_edge_count` steps, so a corrupt
    /// (non-closing) loop cannot hang; the validator reports it.
    pub fn half_edges_of_loop(&self, lp: LoopId) -> Vec<HalfEdgeId> {
        let mut out = Vec::new();
        let Some(l) = self.loops.get(lp) else {
            return out;
        };
        let mut he = l.first;
        for _ in 0..=self.half_edges.len() {
            if !self.half_edges.contains_key(he) {
                break;
            }
            out.push(he);
            he = self.half_edges[he].next;
            if he == l.first {
                break;
            }
        }
        out
    }

    pub fn loop_vertices(&self, lp: LoopId) -> Vec<VertexId> {
        self.half_edges_of_loop(lp)
            .into_iter()
            .map(|h| self.half_edges[h].origin)
            .collect()
    }

    pub fn loop_points(&self, lp: LoopId) -> Vec<DVec3> {
        self.loop_vertices(lp)
            .into_iter()
            .map(|v| self.vertices[v].pos)
            .collect()
    }

    /// Both end points of an edge.
    pub fn edge_points(&self, e: EdgeId) -> (DVec3, DVec3) {
        let he = self.edges[e].he;
        (
            self.vertices[self.half_edges[he].origin].pos,
            self.vertices[self.dest(he)].pos,
        )
    }

    /// Faces that use vertex `v` (sorted, deduplicated).
    pub fn faces_around_vertex(&self, v: VertexId) -> BTreeSet<FaceId> {
        self.half_edges
            .values()
            .filter(|h| h.origin == v)
            .map(|h| self.loops[h.lp].face)
            .collect()
    }

    /// All vertices of a face (outer and holes), sorted.
    pub fn face_vertices(&self, f: FaceId) -> BTreeSet<VertexId> {
        self.faces[f]
            .loops()
            .flat_map(|l| self.loop_vertices(l))
            .collect()
    }

    // ---- mutations (each one re-validates in debug builds) -----------------------------

    /// Move vertices by `delta` and refit the planes of every face touching them.
    pub fn translate_vertices(&mut self, ids: &BTreeSet<VertexId>, delta: DVec3) {
        let mut touched: BTreeSet<FaceId> = BTreeSet::new();
        for &v in ids {
            self.vertices[v].pos += delta;
            touched.extend(self.faces_around_vertex(v));
        }
        for f in touched {
            self.refit_plane(f);
        }
        self.debug_validate("translate_vertices");
    }

    /// Refit a face's plane from its outer loop, keeping the stored frame's orientation.
    pub fn refit_plane(&mut self, f: FaceId) {
        let pts = self.loop_points(self.faces[f].outer);
        if let Ok(p) = Plane::fit(&pts) {
            self.faces[f].plane = p;
        }
    }

    /// In debug builds, run the full validator and panic on any violation, naming the
    /// mutation. Release builds rely on validate-before-commit in `Model::apply`.
    #[inline]
    pub(crate) fn debug_validate(&self, what: &str) {
        #[cfg(debug_assertions)]
        if let Err(e) = crate::validate::validate(self) {
            panic!("B-rep invalid after {what}: {e}");
        }
        let _ = what;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn unit_cube() -> (Vec<DVec3>, Vec<PolyFace>) {
        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
        let pts = vec![
            p(0., 0., 0.),
            p(1., 0., 0.),
            p(1., 1., 0.),
            p(0., 1., 0.),
            p(0., 0., 1.),
            p(1., 0., 1.),
            p(1., 1., 1.),
            p(0., 1., 1.),
        ];
        let f = |v: [usize; 4]| PolyFace::new(v.to_vec(), vec![]);
        let faces = vec![
            f([0, 3, 2, 1]),
            f([4, 5, 6, 7]),
            f([0, 1, 5, 4]),
            f([2, 3, 7, 6]),
            f([1, 2, 6, 5]),
            f([3, 0, 4, 7]),
        ];
        (pts, faces)
    }

    #[test]
    fn cube_counts_and_twins() {
        let (pts, faces) = unit_cube();
        let b = Brep::from_polygons(&pts, &faces).unwrap();
        assert_eq!(
            (
                b.vertex_count(),
                b.edge_count(),
                b.face_count(),
                b.half_edge_count()
            ),
            (8, 12, 6, 24)
        );
        assert!(b.half_edges.values().all(|h| h.twin.is_some()));
        // Normals point outward.
        let top = b.faces().find(|(_, f)| f.plane.normal.z > 0.5).unwrap().1;
        assert!((top.plane.signed_distance(DVec3::new(0.5, 0.5, 1.0))).abs() < 1e-12);
    }

    #[test]
    fn inconsistent_orientation_is_typed() {
        let (pts, mut faces) = unit_cube();
        faces[1].outer.reverse();
        assert!(matches!(
            Brep::from_polygons(&pts, &faces),
            Err(BuildError::DuplicateDirectedEdge { .. })
        ));
    }

    #[test]
    fn bad_input_is_typed() {
        let (pts, mut faces) = unit_cube();
        faces[0].outer = vec![0, 1];
        assert_eq!(
            Brep::from_polygons(&pts, &faces).unwrap_err(),
            BuildError::LoopTooShort { face: 0 }
        );
        let (pts, mut faces) = unit_cube();
        faces[0].outer[0] = 99;
        assert_eq!(
            Brep::from_polygons(&pts, &faces).unwrap_err(),
            BuildError::BadIndex { face: 0 }
        );
    }
}
