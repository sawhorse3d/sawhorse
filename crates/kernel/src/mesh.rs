//! View mesh for the viewport: flat-shaded (vertices not shared between faces), f32
//! positions in metres, one face id per triangle, plus edge segments for line drawing.

use glam::{DVec2, DVec3};
use sawhorse_tessellate::{TessellateError, triangulate};

use crate::model::Model;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewMesh {
    /// xyz per render vertex.
    pub positions: Vec<f32>,
    /// Three per triangle.
    pub indices: Vec<u32>,
    /// One per triangle: running face number across all objects (objects in order, faces
    /// in order), then unconsumed sketch regions.
    pub face_ids: Vec<u32>,
    /// Edge segments as xyz pairs.
    pub edges: Vec<f32>,
}

impl ViewMesh {
    fn push_face(
        &mut self,
        face_id: u32,
        outer: &[DVec3],
        holes: &[Vec<DVec3>],
        to2d: impl Fn(DVec3) -> DVec2,
    ) -> Result<(), TessellateError> {
        let o2: Vec<DVec2> = outer.iter().map(|&p| to2d(p)).collect();
        let h2: Vec<Vec<DVec2>> = holes
            .iter()
            .map(|h| h.iter().map(|&p| to2d(p)).collect())
            .collect();
        let base = (self.positions.len() / 3) as u32;
        for p in outer.iter().chain(holes.iter().flatten()) {
            self.positions.extend([p.x as f32, p.y as f32, p.z as f32]);
        }
        for t in triangulate(&o2, &h2)? {
            self.indices.extend(t.map(|i| base + i));
            self.face_ids.push(face_id);
        }
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
}

pub fn view_mesh(model: &Model) -> Result<ViewMesh, TessellateError> {
    let mut m = ViewMesh::default();
    let mut face_id = 0u32;
    for o in model.objects() {
        let b = o.brep();
        for (_, f) in b.faces() {
            let outer = b.loop_points(f.outer);
            let holes: Vec<Vec<DVec3>> = f.holes.iter().map(|&h| b.loop_points(h)).collect();
            let plane = f.plane;
            m.push_face(face_id, &outer, &holes, |p| plane.to_local(p))?;
            face_id += 1;
        }
        for (e, _) in b.edges() {
            let (p, q) = b.edge_points(e);
            m.push_edge(p, q);
        }
    }
    for s in model.sketches() {
        let plane = *s.plane();
        for r in s.regions() {
            if s.is_consumed(r) {
                continue;
            }
            let (o, h) = s.region_rings(r);
            let outer: Vec<DVec3> = o.iter().map(|&q| plane.to_world(q)).collect();
            let holes: Vec<Vec<DVec3>> = h
                .iter()
                .map(|ring| ring.iter().map(|&q| plane.to_world(q)).collect())
                .collect();
            m.push_face(face_id, &outer, &holes, |p| plane.to_local(p))?;
            face_id += 1;
        }
        // Edges that only bound consumed regions are drawn by their solid instead.
        let mut hidden = std::collections::BTreeSet::new();
        for r in s.consumed().values() {
            for ring in std::iter::once(&r.outer).chain(r.holes.iter()) {
                for i in 0..ring.len() {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    hidden.insert((a.min(b), a.max(b)));
                }
            }
        }
        for (_, [a, b]) in s.edges() {
            if hidden.contains(&(a.min(b), a.max(b))) {
                continue;
            }
            if let (Some(p), Some(q)) = (s.vertex(a), s.vertex(b)) {
                m.push_edge(plane.to_world(p), plane.to_world(q));
            }
        }
    }
    Ok(m)
}
