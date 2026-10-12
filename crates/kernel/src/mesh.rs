//! View mesh for the viewport: flat-shaded (vertices not shared between faces), f32
//! positions in metres, one part index per triangle, plus edge segments for line drawing.
//!
//! A *part* is something the user can point at: an object's face (by persistent face id,
//! never by ordinal) or a sketch region (by public id). The UI raycasts a triangle, reads
//! its part, and sends the op.

use glam::{DVec2, DVec3};
use sawhorse_tessellate::{TessellateError, triangulate};

use crate::ids::to_public;
use crate::meshbuild::LooseMesh;
use crate::model::Model;
use crate::object::Object;
use crate::plane::Plane;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// `owner` is an object id, `index` the persistent face id.
    Face,
    /// `owner` is a sketch id, `index` the region id.
    Region,
    /// `owner` is an imported non-solid mesh's id, `index` its face index.
    Mesh,
}

/// One pickable part of the view mesh, with its plane frame (for drawing on it).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MeshPart {
    pub kind: PartKind,
    pub owner: u64,
    pub index: u64,
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    /// The owner's material colour (sRGB + alpha), when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
}

impl MeshPart {
    fn new(kind: PartKind, owner: u64, index: u64, plane: &Plane) -> MeshPart {
        MeshPart {
            kind,
            owner,
            index,
            origin: plane.origin.to_array(),
            normal: plane.normal.to_array(),
            u: plane.u.to_array(),
            v: plane.v.to_array(),
            color: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewMesh {
    /// xyz per render vertex.
    pub positions: Vec<f32>,
    /// Three per triangle.
    pub indices: Vec<u32>,
    /// One per triangle: index into `parts`.
    pub face_ids: Vec<u32>,
    /// Edge segments as xyz pairs.
    pub edges: Vec<f32>,
    /// One per edge segment: index into `edge_owners` (what the edge belongs to).
    pub edge_owner: Vec<u32>,
    /// Owners of edges: an object (edges of its solid) or a sketch (its strokes).
    pub edge_owners: Vec<EdgeOwner>,
    pub parts: Vec<MeshPart>,
}

/// What a run of view-mesh edges belongs to (for window / crossing selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct EdgeOwner {
    /// "object", "sketch" or "mesh".
    pub kind: &'static str,
    pub id: u64,
}

impl ViewMesh {
    fn push_face(
        &mut self,
        part: MeshPart,
        outer: &[DVec3],
        holes: &[Vec<DVec3>],
        to2d: impl Fn(DVec3) -> DVec2,
    ) -> Result<(), TessellateError> {
        let o2: Vec<DVec2> = outer.iter().map(|&p| to2d(p)).collect();
        let h2: Vec<Vec<DVec2>> = holes
            .iter()
            .map(|h| h.iter().map(|&p| to2d(p)).collect())
            .collect();
        let tris = triangulate(&o2, &h2)?;
        let base = (self.positions.len() / 3) as u32;
        for p in outer.iter().chain(holes.iter().flatten()) {
            self.positions.extend([p.x as f32, p.y as f32, p.z as f32]);
        }
        let id = self.parts.len() as u32;
        for t in tris {
            self.indices.extend(t.map(|i| base + i));
            self.face_ids.push(id);
        }
        self.parts.push(part);
        Ok(())
    }

    fn push_edge(&mut self, a: DVec3, b: DVec3) {
        self.edges.extend([
            a.x as f32, a.y as f32, a.z as f32, b.x as f32, b.y as f32, b.z as f32,
        ]);
        self.edge_owner
            .push(self.edge_owners.len().saturating_sub(1) as u32);
    }

    fn begin_owner(&mut self, kind: &'static str, id: u64) {
        self.edge_owners.push(EdgeOwner { kind, id });
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    fn push_object(
        &mut self,
        id: u64,
        o: &Object,
        color: Option<[u8; 4]>,
    ) -> Result<(), TessellateError> {
        self.begin_owner("object", id);
        let b = o.brep();
        for (_, f) in b.faces() {
            let outer = b.loop_points(f.outer);
            let holes: Vec<Vec<DVec3>> = f.holes.iter().map(|&h| b.loop_points(h)).collect();
            let plane = f.plane;
            let mut part = MeshPart::new(PartKind::Face, id, f.id, &plane);
            part.color = color;
            self.push_face(part, &outer, &holes, |p| plane.to_local(p))?;
        }
        for (e, _) in b.edges() {
            let (p, q) = b.edge_points(e);
            self.push_edge(p, q);
        }
        Ok(())
    }

    /// An imported non-solid mesh: every face that triangulates (others are skipped: the
    /// mesh is shown as it came), then its edges and loose lines.
    fn push_loose(&mut self, id: u64, mesh: &LooseMesh, color: Option<[u8; 4]>) {
        self.begin_owner("mesh", id);
        let (tris, owner) = mesh.triangles();
        let mut by_face: std::collections::BTreeMap<u32, Vec<[u32; 3]>> = Default::default();
        for (t, f) in tris.into_iter().zip(owner) {
            by_face.entry(f).or_default().push(t);
        }
        for (fi, ts) in by_face {
            let Some(plane) = mesh.plane(fi as usize) else {
                continue;
            };
            let base = (self.positions.len() / 3) as u32;
            let mut local = std::collections::BTreeMap::new();
            for t in &ts {
                for &v in t {
                    let next = local.len() as u32;
                    local.entry(v).or_insert_with(|| {
                        let p = mesh.positions[v as usize];
                        self.positions.extend([p.x as f32, p.y as f32, p.z as f32]);
                        next
                    });
                }
            }
            let part = self.parts.len() as u32;
            for t in ts {
                self.indices.extend(t.map(|v| base + local[&v]));
                self.face_ids.push(part);
            }
            let mut p = MeshPart::new(PartKind::Mesh, id, u64::from(fi), &plane);
            p.color = color;
            self.parts.push(p);
        }
        let mut drawn = std::collections::BTreeSet::new();
        for f in &mesh.faces {
            for r in f {
                for k in 0..r.len() {
                    let (a, b) = (r[k], r[(k + 1) % r.len()]);
                    if drawn.insert((a.min(b), a.max(b))) {
                        self.push_edge(mesh.positions[a as usize], mesh.positions[b as usize]);
                    }
                }
            }
        }
        for l in &mesh.lines {
            if drawn.insert((l[0].min(l[1]), l[0].max(l[1]))) {
                self.push_edge(mesh.positions[l[0] as usize], mesh.positions[l[1] as usize]);
            }
        }
    }
}

fn material_color(model: &Model, material: Option<&str>) -> Option<[u8; 4]> {
    material
        .and_then(|m| model.materials().get(m))
        .map(|m| m.color)
}

/// Everything: objects (in order, faces in order), then imported meshes, then sketch
/// regions and edges.
pub fn view_mesh(model: &Model) -> Result<ViewMesh, TessellateError> {
    let mut m = ViewMesh::default();
    for (id, o) in model.objects() {
        if model.is_visible(id) {
            m.push_object(id, o, material_color(model, o.material()))?;
        }
    }
    for (id, mesh) in model.meshes() {
        if model.is_mesh_visible(id) {
            m.push_loose(id, mesh, material_color(model, mesh.material.as_deref()));
        }
    }
    for (sid, s) in model.sketches() {
        let plane = *s.plane();
        for (rid, r) in s.regions() {
            let (o, h) = s.region_rings(r);
            let outer: Vec<DVec3> = o.iter().map(|&q| plane.to_world(q)).collect();
            let holes: Vec<Vec<DVec3>> = h
                .iter()
                .map(|ring| ring.iter().map(|&q| plane.to_world(q)).collect())
                .collect();
            let part = MeshPart::new(PartKind::Region, sid, to_public(rid), &plane);
            m.push_face(part, &outer, &holes, |p| plane.to_local(p))?;
        }
        m.begin_owner("sketch", sid);
        for (_, [a, b]) in s.edges() {
            if let (Some(p), Some(q)) = (s.vertex(a), s.vertex(b)) {
                m.push_edge(plane.to_world(p), plane.to_world(q));
            }
        }
    }
    Ok(m)
}

/// One object only (`None` when there is no such object).
pub fn object_view_mesh(model: &Model, object: u64) -> Option<Result<ViewMesh, TessellateError>> {
    let o = model.object(object)?;
    let mut m = ViewMesh::default();
    Some(
        m.push_object(object, o, material_color(model, o.material()))
            .map(|_| m),
    )
}
