//! Soft, smooth and hidden edges (lane 15).
//!
//! A curved surface drawn with flat faces (a cylinder of 24 sides, a rounded slab, a turned
//! leg) reads as a surface when the edges between its faces are **soft** (not drawn in the
//! edge overlay) and **smooth** (the shading runs across them, as if the faces were one
//! curved face). SketchUp stores both flags per edge; Sawhorse keeps them per edge too:
//!
//! - on a solid, by the **pair of persistent face ids** the edge lies between, so a flag
//!   follows its edge through every op that keeps those faces (moves, copies, components,
//!   push/pull of other faces), and goes when either face goes or the faces stop meeting;
//! - on an imported non-solid mesh, by the **pair of vertex indices** of the edge.
//!
//! A third flag, **hidden**, takes an edge out of the view altogether (the Eraser with
//! Shift). None of the flags changes geometry: they are view data, saved in `.shz`, hashed
//! only when present (every older document keeps its hash), carried into component
//! definitions with the solid. Edges with a flag are not picked unless a query asks for
//! them (the Eraser with Ctrl+Shift, to unsoften), and soft edges are still inference
//! lines.
//!
//! Ops: [`crate::model::Op::SetEdgeFlags`] (the Eraser: given edges, set and clear bits) and
//! [`crate::model::Op::SoftenEdges`] (the Styles pane: every edge of the given solids and
//! meshes whose faces meet within an angle). Both are one undo step.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;

use crate::ids::{EdgeId, FaceId, VertexId};
use crate::meshbuild::LooseMesh;
use crate::object::Object;
use crate::tolerance::POINT_MERGE;

/// Not drawn in the edge overlay (still an inference line).
pub const SOFT: u8 = 1;
/// The shading runs across it: the faces on both sides share their normals at its ends.
pub const SMOOTH: u8 = 2;
/// Not drawn at all, never picked unless asked for.
pub const HIDDEN: u8 = 4;
/// Every flag bit.
pub const ALL: u8 = SOFT | SMOOTH | HIDDEN;

/// Largest angle (degrees) the Soften control accepts.
pub const MAX_SOFTEN_ANGLE: f64 = 180.0;

/// One edge, by the solid or mesh it belongs to and its two end points (world metres,
/// within 0.01 mm), as the UI holds an edge it picked.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct EdgeAt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<u64>,
    pub a: [f64; 3],
    pub b: [f64; 3],
}

/// The unordered key of a pair (smaller first).
pub fn pair<T: Ord + Copy>(a: T, b: T) -> (T, T) {
    if a <= b { (a, b) } else { (b, a) }
}

/// The flags a word list names, for messages: "soft and smooth", "hidden", "hard".
pub fn words(flags: u8) -> String {
    let mut w = Vec::new();
    if flags & SOFT != 0 {
        w.push("soft");
    }
    if flags & SMOOTH != 0 {
        w.push("smooth");
    }
    if flags & HIDDEN != 0 {
        w.push("hidden");
    }
    match w.len() {
        0 => "hard".into(),
        1 => w[0].into(),
        _ => format!("{} and {}", w[..w.len() - 1].join(", "), w[w.len() - 1]),
    }
}

/// An edge, the face-id pair it lies between and its two outward face normals.
type EdgeSide = (EdgeId, Option<(u64, u64)>, [DVec3; 2]);

impl Object {
    /// Soft, smooth and hidden edges, by the pair of persistent face ids they lie between.
    pub fn edge_flags(&self) -> &BTreeMap<(u64, u64), u8> {
        &self.edge_flags
    }

    /// The face-id pair an edge lies between; `None` when both sides are the same face.
    pub fn edge_key(&self, e: EdgeId) -> Option<(u64, u64)> {
        let b = &self.brep;
        let edge = b.edges.get(e)?;
        let side = |h| {
            let he = b.half_edges.get(h)?;
            let lp = b.loops.get(he.lp)?;
            b.faces.get(lp.face).map(|f| f.id)
        };
        let x = side(edge.he)?;
        let y = side(edge.twin?)?;
        (x != y).then(|| pair(x, y))
    }

    /// The flags of one edge of the B-rep (0: a hard edge).
    pub fn flags_of(&self, e: EdgeId) -> u8 {
        if self.edge_flags.is_empty() {
            return 0;
        }
        self.edge_key(e)
            .and_then(|k| self.edge_flags.get(&k).copied())
            .unwrap_or(0)
    }

    /// The edge with end points `a` and `b` (either way round, within 0.01 mm).
    pub fn edge_at(&self, a: DVec3, b: DVec3) -> Option<EdgeId> {
        let near = |p: DVec3, q: DVec3| p.distance(q) <= POINT_MERGE;
        self.brep.edges().map(|(e, _)| e).find(|&e| {
            let (p, q) = self.brep.edge_points(e);
            (near(p, a) && near(q, b)) || (near(p, b) && near(q, a))
        })
    }

    /// Set and clear flag bits on the edge between faces `key`. Returns whether it changed.
    pub(crate) fn set_edge_flags(&mut self, key: (u64, u64), set: u8, clear: u8) -> bool {
        let old = self.edge_flags.get(&key).copied().unwrap_or(0);
        let new = (old & !clear) | set;
        if new == old {
            return false;
        }
        if new == 0 {
            self.edge_flags.remove(&key);
        } else {
            self.edge_flags.insert(key, new);
        }
        true
    }

    /// After an edit: flags stay only on face pairs that still meet along an edge.
    pub(crate) fn settle_edges(&mut self) {
        // Lane 23: the rims (keyed the same way) settle with the flags.
        self.settle_curves();
        if self.edge_flags.is_empty() {
            return;
        }
        let live: BTreeSet<(u64, u64)> = self
            .brep
            .edges()
            .filter_map(|(e, _)| self.edge_key(e))
            .collect();
        self.edge_flags.retain(|k, _| live.contains(k));
    }

    /// Each edge's two outward face normals and its face-id key (`None` for an edge
    /// within one face).
    fn edge_sides(&self) -> Vec<EdgeSide> {
        let b = &self.brep;
        let normal_of = |h: Option<crate::ids::HalfEdgeId>| {
            h.and_then(|h| b.half_edges.get(h))
                .and_then(|h| b.loops.get(h.lp))
                .and_then(|l| b.faces.get(l.face))
                .map_or(DVec3::ZERO, |f| f.plane.normal)
        };
        b.edges()
            .map(|(e, edge)| {
                (
                    e,
                    self.edge_key(e),
                    [normal_of(Some(edge.he)), normal_of(edge.twin)],
                )
            })
            .collect()
    }

    /// Soften by angle (the Styles pane): every edge whose two faces turn by at most
    /// `angle` degrees gets `flags` (SOFT and / or SMOOTH), every other edge loses SOFT and
    /// SMOOTH. Hidden edges stay hidden. Returns how many edges changed.
    pub(crate) fn soften_by_angle(&mut self, angle: f64, flags: u8) -> usize {
        let limit = angle.to_radians().cos();
        let mut changed = 0;
        for (_, key, [n, m]) in self.edge_sides() {
            let Some(key) = key else { continue };
            // The turn between the faces is the angle between their outward normals.
            let within = n.dot(m) >= limit - 1e-12;
            let (set, clear) = if within {
                (flags, (SOFT | SMOOTH) & !flags)
            } else {
                (0, SOFT | SMOOTH)
            };
            if self.set_edge_flags(key, set, clear) {
                changed += 1;
            }
        }
        changed
    }

    /// Shading normals for each face's loop vertices (lane 15): the face's own normal,
    /// averaged with the faces it meets across smooth edges around that vertex. `None` when
    /// the solid has no smooth edge (flat shading, the face normal everywhere).
    pub fn smooth_normals(&self) -> Option<BTreeMap<(FaceId, VertexId), DVec3>> {
        if !self.edge_flags.values().any(|f| f & SMOOTH != 0) {
            return None;
        }
        let b = &self.brep;
        // Per vertex, the faces joined across smooth edges at it (union-find by face).
        let mut joins: BTreeMap<VertexId, Vec<(FaceId, FaceId)>> = BTreeMap::new();
        for (e, edge) in b.edges() {
            if self.flags_of(e) & SMOOTH == 0 {
                continue;
            }
            let Some(twin) = edge.twin else { continue };
            let fa = b.loops[b.half_edges[edge.he].lp].face;
            let fb = b.loops[b.half_edges[twin].lp].face;
            for v in [b.half_edges[edge.he].origin, b.dest(edge.he)] {
                joins.entry(v).or_default().push((fa, fb));
            }
        }
        let mut out = BTreeMap::new();
        for (v, pairs) in joins {
            let faces: Vec<FaceId> = b.faces_around_vertex(v).into_iter().collect();
            let mut root: BTreeMap<FaceId, FaceId> = faces.iter().map(|&f| (f, f)).collect();
            fn find(root: &mut BTreeMap<FaceId, FaceId>, f: FaceId) -> FaceId {
                let mut x = f;
                while root.get(&x).is_some_and(|&p| p != x) {
                    x = root[&x];
                }
                root.insert(f, x);
                x
            }
            for (fa, fb) in pairs {
                if !(root.contains_key(&fa) && root.contains_key(&fb)) {
                    continue;
                }
                let (ra, rb) = (find(&mut root, fa), find(&mut root, fb));
                if ra != rb {
                    root.insert(ra.max(rb), ra.min(rb));
                }
            }
            for &f in &faces {
                let r = find(&mut root, f);
                let mut sum = DVec3::ZERO;
                for &g in &faces {
                    if find(&mut root, g) == r {
                        sum += b.faces[g].plane.normal;
                    }
                }
                let n = sum.normalize_or_zero();
                if n != DVec3::ZERO {
                    out.insert((f, v), n);
                }
            }
        }
        Some(out)
    }
}

/// Union-find root.
fn root_of(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Smoothing groups from face pairs joined by smooth edges: each run of two or more faces
/// gets a group number from 1 (in face order), a face on its own 0 ("off").
fn groups_from(n: usize, joins: impl Iterator<Item = (usize, usize)>) -> Vec<u32> {
    let mut parent: Vec<usize> = (0..n).collect();
    for (a, b) in joins {
        let (ra, rb) = (root_of(&mut parent, a), root_of(&mut parent, b));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    }
    let mut size = vec![0usize; n];
    for i in 0..n {
        let r = root_of(&mut parent, i);
        size[r] += 1;
    }
    let mut number: BTreeMap<usize, u32> = BTreeMap::new();
    (0..n)
        .map(|i| {
            let r = root_of(&mut parent, i);
            if size[r] < 2 {
                return 0;
            }
            let next = number.len() as u32 + 1;
            *number.entry(r).or_insert(next)
        })
        .collect()
}

impl Object {
    /// OBJ smoothing groups (lane 15), one per face in face order: faces joined across
    /// smooth edges share a number from 1, other faces 0. `None` without smooth edges.
    pub fn smoothing_groups(&self) -> Option<Vec<u32>> {
        if !self.edge_flags.values().any(|f| f & SMOOTH != 0) {
            return None;
        }
        let ids = self.brep.persistent_face_ids();
        let at: BTreeMap<u64, usize> = ids.iter().enumerate().map(|(k, &id)| (id, k)).collect();
        let joins: Vec<(usize, usize)> = self
            .edge_flags
            .iter()
            .filter(|(_, f)| **f & SMOOTH != 0)
            .filter_map(|(&(a, b), _)| Some((*at.get(&a)?, *at.get(&b)?)))
            .collect();
        Some(groups_from(ids.len(), joins.into_iter()))
    }
}

impl LooseMesh {
    /// OBJ smoothing groups per face, as [`Object::smoothing_groups`].
    pub fn smoothing_groups(&self) -> Option<Vec<u32>> {
        if !self.edge_flags.values().any(|f| f & SMOOTH != 0) {
            return None;
        }
        let joins: Vec<(usize, usize)> = self
            .edge_faces()
            .into_iter()
            .filter(|(k, f)| f.len() == 2 && self.flags_between(k.0, k.1) & SMOOTH != 0)
            .map(|(_, f)| (f[0], f[1]))
            .collect();
        Some(groups_from(self.faces.len(), joins.into_iter()))
    }

    /// The faces on each side of every face edge, by vertex pair (an open boundary has one).
    pub fn edge_faces(&self) -> BTreeMap<(u32, u32), Vec<usize>> {
        let mut out: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for r in f {
                for k in 0..r.len() {
                    let key = pair(r[k], r[(k + 1) % r.len()]);
                    if key.0 != key.1 {
                        let v = out.entry(key).or_default();
                        if !v.contains(&fi) {
                            v.push(fi);
                        }
                    }
                }
            }
        }
        out
    }

    /// The flags of the edge between vertices `a` and `b` (0: hard or no such edge).
    pub fn flags_between(&self, a: u32, b: u32) -> u8 {
        self.edge_flags.get(&pair(a, b)).copied().unwrap_or(0)
    }

    /// The vertex pair of the edge with end points `a` and `b` (either way round, within
    /// 0.01 mm), among the faces' edges and the loose lines.
    pub fn edge_at(&self, a: DVec3, b: DVec3) -> Option<(u32, u32)> {
        let near = |p: DVec3, q: DVec3| p.distance(q) <= POINT_MERGE;
        let at = |i: u32| self.positions[i as usize];
        self.edge_faces()
            .into_keys()
            .chain(self.lines.iter().map(|l| pair(l[0], l[1])))
            .find(|&(x, y)| {
                (near(at(x), a) && near(at(y), b)) || (near(at(x), b) && near(at(y), a))
            })
    }

    /// Set and clear flag bits on the edge between vertices `key`. Returns whether it changed.
    pub(crate) fn set_edge_flags(&mut self, key: (u32, u32), set: u8, clear: u8) -> bool {
        let old = self.edge_flags.get(&key).copied().unwrap_or(0);
        let new = (old & !clear) | set;
        if new == old {
            return false;
        }
        if new == 0 {
            self.edge_flags.remove(&key);
        } else {
            self.edge_flags.insert(key, new);
        }
        true
    }

    /// The normal each face's triangles are wound around (from its first triangle that has
    /// area), so smoothing agrees with what is drawn.
    fn face_normals(&self) -> Vec<DVec3> {
        let (tris, owner) = self.triangles();
        let mut out = vec![DVec3::ZERO; self.faces.len()];
        for (t, f) in tris.iter().zip(owner) {
            let slot = &mut out[f as usize];
            if *slot != DVec3::ZERO {
                continue;
            }
            let p = |i: u32| self.positions[i as usize];
            *slot = (p(t[1]) - p(t[0]))
                .cross(p(t[2]) - p(t[0]))
                .normalize_or_zero();
        }
        out
    }

    /// Soften by angle, as [`Object::soften_by_angle`]; only edges between two faces.
    pub(crate) fn soften_by_angle(&mut self, angle: f64, flags: u8) -> usize {
        let limit = angle.to_radians().cos();
        let normals = self.face_normals();
        let mut changed = 0;
        for (key, faces) in self.edge_faces() {
            let (set, clear) =
                if faces.len() == 2 && normals[faces[0]].dot(normals[faces[1]]) >= limit - 1e-12 {
                    (flags, (SOFT | SMOOTH) & !flags)
                } else {
                    (0, SOFT | SMOOTH)
                };
            if self.set_edge_flags(key, set, clear) {
                changed += 1;
            }
        }
        changed
    }

    /// Shading normals per (face index, vertex index), as [`Object::smooth_normals`].
    /// `None` when no edge is smooth.
    pub fn smooth_normals(&self) -> Option<BTreeMap<(usize, u32), DVec3>> {
        if !self.edge_flags.values().any(|f| f & SMOOTH != 0) {
            return None;
        }
        let normals = self.face_normals();
        let mut around: BTreeMap<u32, BTreeSet<usize>> = BTreeMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for &v in f.iter().flatten() {
                around.entry(v).or_default().insert(fi);
            }
        }
        let mut joins: BTreeMap<u32, Vec<(usize, usize)>> = BTreeMap::new();
        for (key, faces) in self.edge_faces() {
            if faces.len() == 2 && self.flags_between(key.0, key.1) & SMOOTH != 0 {
                for v in [key.0, key.1] {
                    joins.entry(v).or_default().push((faces[0], faces[1]));
                }
            }
        }
        let mut out = BTreeMap::new();
        for (v, pairs) in joins {
            let faces: Vec<usize> = around
                .get(&v)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default();
            let mut root: BTreeMap<usize, usize> = faces.iter().map(|&f| (f, f)).collect();
            fn find(root: &mut BTreeMap<usize, usize>, f: usize) -> usize {
                let mut x = f;
                while root.get(&x).is_some_and(|&p| p != x) {
                    x = root[&x];
                }
                root.insert(f, x);
                x
            }
            for (fa, fb) in pairs {
                let (ra, rb) = (find(&mut root, fa), find(&mut root, fb));
                if ra != rb {
                    root.insert(ra.max(rb), ra.min(rb));
                }
            }
            for &f in &faces {
                let r = find(&mut root, f);
                let mut sum = DVec3::ZERO;
                for &g in &faces {
                    if find(&mut root, g) == r {
                        sum += normals[g];
                    }
                }
                let n = sum.normalize_or_zero();
                if n != DVec3::ZERO {
                    out.insert((f, v), n);
                }
            }
        }
        Some(out)
    }
}

impl crate::model::Model {
    /// Some visible solid or mesh has a smooth edge (the view mesh then carries normals).
    pub fn has_smooth_edges(&self) -> bool {
        self.objects()
            .into_iter()
            .any(|(id, o)| self.is_visible(id) && o.edge_flags.values().any(|f| f & SMOOTH != 0))
            || self.meshes().into_iter().any(|(id, m)| {
                self.is_mesh_visible(id) && m.edge_flags.values().any(|f| f & SMOOTH != 0)
            })
    }

    /// [`crate::model::Op::SetEdgeFlags`].
    pub(crate) fn set_edge_flags(
        &mut self,
        edges: &[EdgeAt],
        set: u8,
        clear: u8,
    ) -> Result<crate::model::OpOutcome, crate::error::KernelError> {
        use crate::error::{DocumentError, FaceEditError};
        use crate::ids::{MeshId, ObjectId, from_public};
        let bad =
            |m: &str| -> crate::error::KernelError { DocumentError::EdgeFlags(m.into()).into() };
        if edges.is_empty() {
            return Err(bad("pick at least one edge"));
        }
        if (set | clear) & !ALL != 0 || set & clear != 0 || set | clear == 0 {
            return Err(bad(
                "flags are 1 (soft), 2 (smooth) and 4 (hidden); set and clear different ones",
            ));
        }
        // Resolve every edge first: one that is not there refuses the whole op.
        let mut on_objects: Vec<(ObjectId, (u64, u64))> = Vec::new();
        let mut on_meshes: Vec<(MeshId, (u32, u32))> = Vec::new();
        for e in edges {
            let (a, b) = (DVec3::from_array(e.a), DVec3::from_array(e.b));
            if !(a.is_finite() && b.is_finite()) {
                return Err(FaceEditError::OutOfBounds.into());
            }
            match (e.object, e.mesh) {
                (Some(id), None) => {
                    let k = from_public::<ObjectId>(id);
                    if !self.objects.contains_key(k) {
                        return Err(FaceEditError::NoSuchObject(id).into());
                    }
                    self.editable_object(k)?;
                    let o = &self.objects[k];
                    let key = o
                        .edge_at(a, b)
                        .and_then(|x| o.edge_key(x))
                        .ok_or(FaceEditError::NoSuchEdge)?;
                    on_objects.push((k, key));
                }
                (None, Some(id)) => {
                    let k = from_public::<MeshId>(id);
                    if !self.meshes.contains_key(k) {
                        return Err(DocumentError::NoSuchMesh(id).into());
                    }
                    self.editable_mesh(k)?;
                    let key = self.meshes[k]
                        .edge_at(a, b)
                        .ok_or(FaceEditError::NoSuchEdge)?;
                    on_meshes.push((k, key));
                }
                _ => return Err(bad("an edge names one solid or one mesh")),
            }
        }
        let mut changed = 0;
        for (k, key) in on_objects {
            if self.objects[k].set_edge_flags(key, set, clear) {
                changed += 1;
                self.touch(k);
            }
        }
        for (k, key) in on_meshes {
            if self.meshes[k].set_edge_flags(key, set, clear) {
                changed += 1;
                self.dirty.touched_meshes.insert(k);
                let parent = self.meshes[k].member.parent;
                self.mark(parent);
            }
        }
        Ok(crate::model::OpOutcome::EdgesSet { changed })
    }

    /// [`crate::model::Op::SoftenEdges`].
    pub(crate) fn soften_edges(
        &mut self,
        objects: &[u64],
        meshes: &[u64],
        angle: f64,
        soft: bool,
        smooth: bool,
    ) -> Result<crate::model::OpOutcome, crate::error::KernelError> {
        use crate::error::{DocumentError, FaceEditError};
        use crate::ids::{MeshId, ObjectId, from_public};
        let bad =
            |m: &str| -> crate::error::KernelError { DocumentError::EdgeFlags(m.into()).into() };
        if objects.is_empty() && meshes.is_empty() {
            return Err(bad("select the solids or meshes to soften first"));
        }
        if !(angle.is_finite() && (0.0..=MAX_SOFTEN_ANGLE).contains(&angle)) {
            return Err(bad("the angle is 0 to 180 degrees"));
        }
        let mut ok = Vec::new();
        for &id in objects {
            let k = from_public::<ObjectId>(id);
            if !self.objects.contains_key(k) {
                return Err(FaceEditError::NoSuchObject(id).into());
            }
            self.editable_object(k)?;
            if !ok.contains(&k) {
                ok.push(k);
            }
        }
        let mut okm = Vec::new();
        for &id in meshes {
            let k = from_public::<MeshId>(id);
            if !self.meshes.contains_key(k) {
                return Err(DocumentError::NoSuchMesh(id).into());
            }
            self.editable_mesh(k)?;
            if !okm.contains(&k) {
                okm.push(k);
            }
        }
        let flags = if soft { SOFT } else { 0 } | if smooth { SMOOTH } else { 0 };
        let mut changed = 0;
        for k in ok {
            let n = self.objects[k].soften_by_angle(angle, flags);
            if n > 0 {
                changed += n;
                self.touch(k);
            }
        }
        for k in okm {
            let n = self.meshes[k].soften_by_angle(angle, flags);
            if n > 0 {
                changed += n;
                self.dirty.touched_meshes.insert(k);
                let parent = self.meshes[k].member.parent;
                self.mark(parent);
            }
        }
        Ok(crate::model::OpOutcome::EdgesSet { changed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_read_like_a_tooltip() {
        assert_eq!(words(0), "hard");
        assert_eq!(words(SOFT), "soft");
        assert_eq!(words(SOFT | SMOOTH), "soft and smooth");
        assert_eq!(words(ALL), "soft, smooth and hidden");
        assert_eq!(pair(5u64, 2), (2, 5));
    }
}
