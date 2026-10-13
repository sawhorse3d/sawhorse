//! View mesh for the viewport: flat-shaded (vertices not shared between faces), f32
//! positions in metres, one part index per triangle, plus edge segments for line drawing.
//!
//! A *part* is something the user can point at: an object's face (by persistent face id,
//! never by ordinal) or a sketch region (by public id). The UI raycasts a triangle, reads
//! its part, and sends the op.

use glam::{DVec2, DVec3};
use sawhorse_tessellate::{TessellateError, triangulate};

use crate::ids::to_public;
use crate::looks::{texture_frame, texture_uv};
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
    /// The colour (sRGB + alpha) of the material the face shows (its own, its solid's or
    /// its container's), when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
    /// The name of that material (lane 10): what the renderer looks its texture up by.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// The face has a material of its own (painted on the face, not inherited).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub own: bool,
    /// How a texture sits on the face, when it was positioned (the texture handles start
    /// from it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uv: Option<crate::looks::UvXform>,
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
            material: None,
            own: false,
            uv: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewMesh {
    /// xyz per render vertex.
    pub positions: Vec<f32>,
    /// Texture coordinates per render vertex (u, v in repeats of the texture), for faces
    /// whose material has a texture the model holds; zero elsewhere (lane 10).
    pub uvs: Vec<f32>,
    /// Three per triangle.
    pub indices: Vec<u32>,
    /// One per triangle: index into `parts`.
    pub face_ids: Vec<u32>,
    /// Edge segments as xyz pairs.
    pub edges: Vec<f32>,
    /// One per edge segment: index into `edge_owners` (what the edge belongs to).
    pub edge_owner: Vec<u32>,
    /// Six per edge segment (lane 10): the outward normals of the two faces that meet at the
    /// edge, zero where there is no face (a stroke, an open boundary). An edge is a profile
    /// where one of its faces turns towards the eye and the other away.
    pub edge_normals: Vec<f32>,
    /// Owners of edges: an object (edges of its solid) or a sketch (its strokes).
    pub edge_owners: Vec<EdgeOwner>,
    pub parts: Vec<MeshPart>,
    /// One per edge segment (lane 15): its soft (1), smooth (2) and hidden (4) flags.
    pub edge_flags: Vec<u8>,
    /// Shading normals, three per render vertex (lane 15), only when some edge in view is
    /// smooth (faces meeting at a smooth edge share normals at its ends); empty otherwise,
    /// and the faces are shaded flat from their triangles.
    pub normals: Vec<f32>,
    /// Fill `normals` (set when some edge in view is smooth).
    want_normals: bool,
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
        uv: impl Fn(DVec3) -> [f64; 2],
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
            let [u, v] = uv(*p);
            self.uvs.extend([u as f32, v as f32]);
            if self.want_normals {
                let n = part.normal;
                self.normals.extend([n[0] as f32, n[1] as f32, n[2] as f32]);
            }
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
        self.push_edge_n(a, b, [DVec3::ZERO; 2], 0);
    }

    fn push_edge_n(&mut self, a: DVec3, b: DVec3, n: [DVec3; 2], flags: u8) {
        self.edge_flags.push(flags);
        self.edges.extend([
            a.x as f32, a.y as f32, a.z as f32, b.x as f32, b.y as f32, b.z as f32,
        ]);
        self.edge_owner
            .push(self.edge_owners.len().saturating_sub(1) as u32);
        for v in n {
            self.edge_normals
                .extend([v.x as f32, v.y as f32, v.z as f32]);
        }
    }

    fn begin_owner(&mut self, kind: &'static str, id: u64) {
        self.edge_owners.push(EdgeOwner { kind, id });
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    fn push_object(&mut self, model: &Model, id: u64, o: &Object) -> Result<(), TessellateError> {
        self.begin_owner("object", id);
        let b = o.brep();
        let default = model.solid_default(o);
        let smooth = if self.want_normals {
            o.smooth_normals()
        } else {
            None
        };
        for (fid, f) in b.faces() {
            let first = self.normals.len();
            let outer = b.loop_points(f.outer);
            let holes: Vec<Vec<DVec3>> = f.holes.iter().map(|&h| b.loop_points(h)).collect();
            let plane = f.plane;
            let paint = o.face_paint().get(&f.id);
            let own = paint.and_then(|p| p.material.as_deref());
            let name = own.or(default);
            let mat = name.and_then(|n| model.materials().get(n));
            let mut part = MeshPart::new(PartKind::Face, id, f.id, &plane);
            part.color = mat.map(|m| m.color);
            part.material = mat.and(name).map(str::to_string);
            part.own = own.is_some();
            part.uv = paint.map(|p| p.uv).filter(|u| !u.is_identity());
            let textured = mat.filter(|m| {
                m.texture
                    .as_deref()
                    .is_some_and(|t| model.textures().contains_key(t))
            });
            match textured {
                Some(m) => {
                    let frame = texture_frame(&plane, &outer);
                    let px = model.texture_px(m);
                    let pos = paint.map(|p| p.uv).unwrap_or_default();
                    self.push_face(
                        part,
                        &outer,
                        &holes,
                        |p| plane.to_local(p),
                        |p| texture_uv(&frame, p, m, px, &pos),
                    )?;
                }
                None => {
                    self.push_face(part, &outer, &holes, |p| plane.to_local(p), |_| [0.0; 2])?
                }
            }
            // Lane 15: across smooth edges the faces share their normals.
            if let Some(map) = &smooth {
                let mut k = first;
                for l in f.loops() {
                    for v in b.loop_vertices(l) {
                        if let Some(n) = map.get(&(fid, v)) {
                            self.normals[k..k + 3]
                                .copy_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
                        }
                        k += 3;
                    }
                }
            }
        }
        let normal_of = |he: Option<crate::ids::HalfEdgeId>| {
            he.and_then(|h| b.half_edge(h))
                .and_then(|h| b.lp(h.lp))
                .and_then(|l| b.face(l.face))
                .map_or(DVec3::ZERO, |f| f.plane.normal)
        };
        for (e, edge) in b.edges() {
            let (p, q) = b.edge_points(e);
            self.push_edge_n(
                p,
                q,
                [normal_of(Some(edge.he)), normal_of(edge.twin)],
                o.flags_of(e),
            );
        }
        Ok(())
    }

    /// An imported non-solid mesh: every face that triangulates (others are skipped: the
    /// mesh is shown as it came), then its edges and loose lines.
    fn push_loose(&mut self, id: u64, mesh: &LooseMesh, material: Option<(&str, [u8; 4])>) {
        self.begin_owner("mesh", id);
        let smooth = if self.want_normals {
            mesh.smooth_normals()
        } else {
            None
        };
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
            // The face's own normal, from the way its first triangle is wound.
            let flat = {
                let p = |i: u32| mesh.positions[i as usize];
                let t = ts[0];
                let n = (p(t[1]) - p(t[0]))
                    .cross(p(t[2]) - p(t[0]))
                    .normalize_or_zero();
                if n == DVec3::ZERO { plane.normal } else { n }
            };
            for t in &ts {
                for &v in t {
                    let next = local.len() as u32;
                    local.entry(v).or_insert_with(|| {
                        let p = mesh.positions[v as usize];
                        self.positions.extend([p.x as f32, p.y as f32, p.z as f32]);
                        self.uvs.extend([0.0, 0.0]);
                        if self.want_normals {
                            let n = smooth
                                .as_ref()
                                .and_then(|m| m.get(&(fi as usize, v)))
                                .copied()
                                .unwrap_or(flat);
                            self.normals.extend([n.x as f32, n.y as f32, n.z as f32]);
                        }
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
            p.color = material.map(|m| m.1);
            p.material = material.map(|m| m.0.to_string());
            self.parts.push(p);
        }
        let mut drawn = std::collections::BTreeSet::new();
        for f in &mesh.faces {
            for r in f {
                for k in 0..r.len() {
                    let (a, b) = (r[k], r[(k + 1) % r.len()]);
                    if drawn.insert((a.min(b), a.max(b))) {
                        self.push_edge_n(
                            mesh.positions[a as usize],
                            mesh.positions[b as usize],
                            [DVec3::ZERO; 2],
                            mesh.flags_between(a, b),
                        );
                    }
                }
            }
        }
        for l in &mesh.lines {
            if drawn.insert((l[0].min(l[1]), l[0].max(l[1]))) {
                self.push_edge_n(
                    mesh.positions[l[0] as usize],
                    mesh.positions[l[1] as usize],
                    [DVec3::ZERO; 2],
                    mesh.flags_between(l[0], l[1]),
                );
            }
        }
    }
}

/// A mesh's material (its own, else its container's) with its colour.
fn mesh_material<'a>(model: &'a Model, mesh: &'a LooseMesh) -> Option<(&'a str, [u8; 4])> {
    let name = mesh
        .material
        .as_deref()
        .or_else(|| model.container_material(mesh.member.parent))?;
    model.materials().get(name).map(|m| (name, m.color))
}

/// Everything: objects (in order, faces in order), then imported meshes, then sketch
/// regions and edges.
pub fn view_mesh(model: &Model) -> Result<ViewMesh, TessellateError> {
    let mut m = ViewMesh {
        want_normals: model.has_smooth_edges(),
        ..ViewMesh::default()
    };
    for (id, o) in model.objects() {
        if model.is_visible(id) {
            m.push_object(model, id, o)?;
        }
    }
    for (id, mesh) in model.meshes() {
        if model.is_mesh_visible(id) {
            m.push_loose(id, mesh, mesh_material(model, mesh));
        }
    }
    for (sid, s) in model.sketches() {
        if !model.is_sketch_visible(sid) {
            continue;
        }
        let plane = *s.plane();
        for (rid, r) in s.regions() {
            let (o, h) = s.region_rings(r);
            let outer: Vec<DVec3> = o.iter().map(|&q| plane.to_world(q)).collect();
            let holes: Vec<Vec<DVec3>> = h
                .iter()
                .map(|ring| ring.iter().map(|&q| plane.to_world(q)).collect())
                .collect();
            let part = MeshPart::new(PartKind::Region, sid, to_public(rid), &plane);
            m.push_face(part, &outer, &holes, |p| plane.to_local(p), |_| [0.0; 2])?;
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
    let mut m = ViewMesh {
        want_normals: o.smooth_normals().is_some(),
        ..ViewMesh::default()
    };
    Some(m.push_object(model, object, o).map(|_| m))
}

/// The texture coordinates face `face` of `object` would get with positioning `uv`, in the
/// order the view mesh lists the face's vertices (outer ring, then holes): the texture
/// handles' live preview. `None` when the face shows no texture the model holds.
pub fn face_uvs(
    model: &Model,
    object: u64,
    face: u64,
    uv: &crate::looks::UvXform,
) -> Option<Vec<f32>> {
    let o = model.object(object)?;
    let b = o.brep();
    let (_, f) = b.faces().find(|(_, f)| f.id == face)?;
    let name = model.face_material(object, face)?;
    let m = model.materials().get(name)?;
    m.texture
        .as_deref()
        .filter(|t| model.textures().contains_key(*t))?;
    let outer = b.loop_points(f.outer);
    let frame = texture_frame(&f.plane, &outer);
    let px = model.texture_px(m);
    let mut out = Vec::new();
    for l in f.loops() {
        for p in b.loop_points(l) {
            let [u, v] = texture_uv(&frame, p, m, px, uv);
            out.extend([u as f32, v as f32]);
        }
    }
    Some(out)
}
