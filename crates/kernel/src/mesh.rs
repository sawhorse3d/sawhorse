//! View mesh for the viewport: flat-shaded (vertices not shared between faces), f32
//! positions in metres, one part index per triangle, plus edge segments for line drawing.
//!
//! A *part* is something the user can point at: an object's face (by ordinal) or a sketch
//! region (by public id). The UI raycasts a triangle, reads its part, and sends the op.

use glam::{DVec2, DVec3};
use sawhorse_tessellate::{TessellateError, triangulate};

use crate::ids::to_public;
use crate::model::Model;
use crate::object::Object;
use crate::plane::Plane;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// `owner` is an object id, `index` the face ordinal.
    Face,
    /// `owner` is a sketch id, `index` the region id.
    Region,
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
    pub parts: Vec<MeshPart>,
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
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    fn push_object(&mut self, id: u64, o: &Object) -> Result<(), TessellateError> {
        let b = o.brep();
        for (i, (_, f)) in b.faces().enumerate() {
            let outer = b.loop_points(f.outer);
            let holes: Vec<Vec<DVec3>> = f.holes.iter().map(|&h| b.loop_points(h)).collect();
            let plane = f.plane;
            let part = MeshPart::new(PartKind::Face, id, i as u64, &plane);
            self.push_face(part, &outer, &holes, |p| plane.to_local(p))?;
        }
        for (e, _) in b.edges() {
            let (p, q) = b.edge_points(e);
            self.push_edge(p, q);
        }
        Ok(())
    }
}

/// Everything: objects (in order, faces in order), then sketch regions and edges.
pub fn view_mesh(model: &Model) -> Result<ViewMesh, TessellateError> {
    let mut m = ViewMesh::default();
    for (id, o) in model.objects() {
        m.push_object(id, o)?;
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
    Some(m.push_object(object, o).map(|_| m))
}
