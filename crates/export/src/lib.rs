//! Exporters (ADR-0002). Everything here reads the kernel [`Model`] and writes bytes; no IO.
//!
//! | Format | Module | Units, axes | Takes |
//! |---|---|---|---|
//! | STL (binary) | [`stl`] | mm, Z up | watertight solids only (typed refusal otherwise) |
//! | 3MF | [`three_mf`] | mm, Z up | watertight solids only; one object per solid |
//! | glTF 2.0 binary (GLB) | [`gltf`] | m, Y up | solids and imported meshes; a node per object; face ids as `_FACE_ID` |
//! | Wavefront OBJ | [`obj`] | mm, Z up | solids and meshes; `o` and `g` per object, `usemtl` + MTL |
//! | DXF (ASCII) | [`dxf`] | mm | `3DFACE` + `LINE` for the model; `LWPOLYLINE` outline of a face |
//! | SVG | [`svg`] | mm (parallel views and outlines) | hidden-line drawing of a view; outline of a face |
//!
//! A [`Scope`] says what to export: every visible solid and mesh, or a selection.
//! Printable formats refuse a scope holding anything that is not a watertight solid: a
//! printable file made from an open shell is a lie, so the exporter says no instead.

use glam::DVec3;
use sawhorse_kernel::Model;
use sawhorse_kernel::meshbuild::LooseMesh;
use sawhorse_kernel::object::Object;

pub mod dxf;
pub mod gltf;
pub mod obj;
pub mod stl;
pub mod svg;
pub mod three_mf;

/// Metres to millimetres: the kernel is metres, most formats are millimetres.
pub const MM_PER_M: f64 = 1000.0;

/// A triangle mesh to export: shared vertices, CCW-outward triangles, metres.
#[derive(Debug, Clone, Copy)]
pub struct ExportMesh<'a> {
    pub positions: &'a [DVec3],
    pub triangles: &'a [[u32; 3]],
    /// From the kernel's validator; printable exporters refuse open shells.
    pub watertight: bool,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExportError {
    #[error("the model is not watertight; fix open edges before exporting a printable file")]
    NotWatertight,
    #[error(
        "{0} is not a solid ({1}); STL and 3MF take watertight solids only: select the solids to export, or delete it"
    )]
    NotSolid(String, String),
    #[error("nothing to export")]
    Empty,
    #[error("no object #{0}")]
    NoSuchObject(u64),
    #[error("no imported mesh #{0}")]
    NoSuchMesh(u64),
    #[error("triangle {0} references a missing vertex")]
    BadIndex(usize),
    #[error("3MF writer: {0}")]
    ThreeMf(String),
    #[error("zip writer: {0}")]
    Zip(String),
    #[error("could not triangulate the model: {0}")]
    Tessellate(String),
    #[error("bad view for a drawing: {0}")]
    BadView(String),
    #[error("an outline needs at least one closed ring of three or more points")]
    BadOutline,
}

impl ExportMesh<'_> {
    pub(crate) fn check(&self) -> Result<(), ExportError> {
        if !self.watertight {
            return Err(ExportError::NotWatertight);
        }
        if self.triangles.is_empty() {
            return Err(ExportError::Empty);
        }
        let n = self.positions.len() as u32;
        if let Some(i) = self
            .triangles
            .iter()
            .position(|t| t.iter().any(|&v| v >= n))
        {
            return Err(ExportError::BadIndex(i));
        }
        Ok(())
    }
}

/// What to export: objects and imported meshes by public id, in this order.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct Scope {
    #[serde(default)]
    pub objects: Vec<u64>,
    #[serde(default)]
    pub meshes: Vec<u64>,
}

impl Scope {
    /// Every visible solid and every visible imported mesh.
    pub fn visible(model: &Model) -> Scope {
        Scope {
            objects: model
                .object_ids()
                .into_iter()
                .filter(|&o| model.is_visible(o))
                .collect(),
            meshes: model
                .meshes()
                .into_iter()
                .map(|(id, _)| id)
                .filter(|&m| model.is_mesh_visible(m))
                .collect(),
        }
    }

    /// The solids, in order (refusing unknown ids).
    pub fn solids<'m>(&self, model: &'m Model) -> Result<Vec<(u64, &'m Object)>, ExportError> {
        self.objects
            .iter()
            .map(|&id| {
                model
                    .object(id)
                    .map(|o| (id, o))
                    .ok_or(ExportError::NoSuchObject(id))
            })
            .collect()
    }

    /// The imported meshes, in order (refusing unknown ids).
    pub fn loose<'m>(&self, model: &'m Model) -> Result<Vec<(u64, &'m LooseMesh)>, ExportError> {
        self.meshes
            .iter()
            .map(|&id| {
                model
                    .mesh(id)
                    .map(|m| (id, m))
                    .ok_or(ExportError::NoSuchMesh(id))
            })
            .collect()
    }

    /// Solids only, for printable formats: refuses (naming it) any mesh in the scope.
    pub fn printable<'m>(&self, model: &'m Model) -> Result<Vec<(u64, &'m Object)>, ExportError> {
        if let Some((_, m)) = self.loose(model)?.first() {
            let why = m.note.strip_prefix("not a solid: ").unwrap_or(&m.note);
            return Err(ExportError::NotSolid(
                format!("the imported mesh \"{}\"", m.name),
                why.to_string(),
            ));
        }
        let solids = self.solids(model)?;
        if solids.is_empty() {
            return Err(ExportError::Empty);
        }
        if let Some((id, _)) = solids.iter().find(|(_, o)| !o.is_watertight()) {
            return Err(ExportError::NotSolid(
                object_name(model, *id),
                "open edges".into(),
            ));
        }
        Ok(solids)
    }
}

/// The name an object goes by in exported files: its own, or "Object <id>".
pub fn object_name(model: &Model, id: u64) -> String {
    model
        .object(id)
        .and_then(|o| o.name())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Object {id}"))
}

/// Shared vertices (metres), outward triangles, and the persistent face id of each triangle.
pub type IdTriangles = (Vec<DVec3>, Vec<[u32; 3]>, Vec<u64>);

/// Indexed triangles of a solid with the persistent face id of each triangle.
pub fn solid_triangles(o: &Object) -> Result<IdTriangles, ExportError> {
    let t = o
        .tri_solid()
        .map_err(|e| ExportError::Tessellate(e.to_string()))?;
    let ids = t
        .face_ids
        .iter()
        .map(|&ord| o.face_id_at(ord as usize).unwrap_or(0))
        .collect();
    Ok((t.positions, t.triangles, ids))
}

fn with_object<T>(
    object: &Object,
    f: impl FnOnce(&ExportMesh<'_>) -> Result<T, ExportError>,
) -> Result<T, ExportError> {
    let solid = object
        .tri_solid()
        .map_err(|e| ExportError::Tessellate(e.to_string()))?;
    f(&ExportMesh {
        positions: &solid.positions,
        triangles: &solid.triangles,
        watertight: object.is_watertight(),
    })
}

/// Binary STL of one kernel object.
pub fn object_stl(object: &Object) -> Result<Vec<u8>, ExportError> {
    with_object(object, stl::write_binary)
}

/// Wavefront OBJ of one kernel object.
pub fn object_obj(object: &Object, name: &str) -> Result<String, ExportError> {
    obj::write(object, name)
}

/// 3MF of one kernel object.
pub fn object_3mf(object: &Object) -> Result<Vec<u8>, ExportError> {
    with_object(object, three_mf::write)
}

/// sRGB byte to linear float (glTF colour factors are linear).
pub(crate) fn srgb_to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Model helpers shared by the tests of every writer.
#[cfg(test)]
pub(crate) mod fixtures {
    use sawhorse_kernel::meshbuild::ImportItem;
    use sawhorse_kernel::{Model, Op};

    /// A 100 x 200 x 50 mm box (object 1).
    pub fn box_model() -> Model {
        let mut m = Model::with_ground_sketch();
        for op in [
            Op::AddRectangle {
                sketch: 1,
                x: 0.0,
                y: 0.0,
                w: 0.1,
                h: 0.2,
            },
            Op::Extrude {
                sketch: 1,
                region: 1,
                distance: 0.05,
            },
        ] {
            m.apply(&op).unwrap();
        }
        m
    }

    /// Two named, painted boxes on tags, plus an open (non-solid) imported mesh.
    pub fn mixed_model() -> Model {
        let mut m = box_model();
        let out = m
            .apply(&Op::AddRectangle {
                sketch: 1,
                x: 0.3,
                y: 0.0,
                w: 0.05,
                h: 0.05,
            })
            .unwrap();
        let sawhorse_kernel::OpOutcome::SketchEdited { regions } = out else {
            panic!("{out:?}")
        };
        for op in [
            Op::Extrude {
                sketch: 1,
                region: regions[0],
                distance: 0.02,
            },
            Op::DefineMaterial {
                name: "Oak".into(),
                color: [200, 160, 100, 255],
                texture: Some("oak.jpg".into()),
            },
            Op::SetMaterial {
                object: 1,
                material: Some("Oak".into()),
            },
            Op::RenameObject {
                object: 1,
                name: Some("Side panel".into()),
            },
            Op::RenameObject {
                object: 2,
                name: Some("Block".into()),
            },
            Op::Import {
                items: vec![ImportItem {
                    name: "Sheet".into(),
                    group: None,
                    tag: None,
                    material: None,
                    positions: vec![
                        [0.0, 0.5, 0.0],
                        [0.1, 0.5, 0.0],
                        [0.1, 0.6, 0.0],
                        [0.0, 0.6, 0.0],
                    ],
                    faces: vec![vec![vec![0, 1, 2, 3]]],
                    lines: vec![],
                    merge_coplanar: false,
                    face_labels: vec![],
                }],
                strokes: vec![],
                materials: vec![],
                scenes: vec![],
                hidden_tags: vec![],
                definitions: vec![],
                nodes: vec![],
            },
        ] {
            m.apply(&op).unwrap();
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_box_exports() {
        let m = fixtures::box_model();
        let o = m.object(1).unwrap();
        let stl = object_stl(o).unwrap();
        assert_eq!(u32::from_le_bytes(stl[80..84].try_into().unwrap()), 12);
        assert_eq!(&object_3mf(o).unwrap()[..2], b"PK");
    }

    #[test]
    fn printable_scope_refuses_meshes_by_name() {
        let m = fixtures::mixed_model();
        let all = Scope::visible(&m);
        assert_eq!(all.objects, vec![1, 2]);
        assert_eq!(all.meshes.len(), 1);
        let e = all.printable(&m).unwrap_err();
        let text = e.to_string();
        assert!(text.contains("\"Sheet\" is not a solid"), "{text}");
        assert!(text.contains("open edge"), "{text}");
        let solids_only = Scope {
            objects: vec![2],
            meshes: vec![],
        };
        assert_eq!(solids_only.printable(&m).unwrap().len(), 1);
        assert_eq!(
            Scope::default().printable(&m).unwrap_err(),
            ExportError::Empty
        );
        assert_eq!(
            Scope {
                objects: vec![9],
                meshes: vec![]
            }
            .printable(&m)
            .unwrap_err(),
            ExportError::NoSuchObject(9)
        );
        assert_eq!(object_name(&m, 1), "Side panel");
    }
}
