//! Exporters (ADR-0002). Both are gated on watertightness: a printable file from an open
//! shell is a lie, so the exporter refuses with a typed error instead of writing one.
//!
//! - [`stl::write_binary`]: hand-written binary STL, millimetres, Z-up.
//! - [`three_mf::write`]: 3MF via the `threemf` crate, millimetres (3MF's default unit).
//!
//! GLB, DXF and SVG exporters land with the export lane.

use glam::DVec3;

pub mod stl;
pub mod three_mf;

/// Metres to millimetres: the kernel is metres, both formats are millimetres.
pub const MM_PER_M: f64 = 1000.0;

/// A triangle mesh to export: shared vertices, CCW-outward triangles, metres.
#[derive(Debug, Clone, Copy)]
pub struct ExportMesh<'a> {
    pub positions: &'a [DVec3],
    pub triangles: &'a [[u32; 3]],
    /// From the kernel's validator; exporters refuse open shells.
    pub watertight: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("the model is not watertight; fix open edges before exporting a printable file")]
    NotWatertight,
    #[error("nothing to export")]
    Empty,
    #[error("triangle {0} references a missing vertex")]
    BadIndex(usize),
    #[error("3MF writer: {0}")]
    ThreeMf(String),
    #[error("could not triangulate the model: {0}")]
    Tessellate(String),
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

fn with_object<T>(
    object: &sawhorse_kernel::object::Object,
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
pub fn object_stl(object: &sawhorse_kernel::object::Object) -> Result<Vec<u8>, ExportError> {
    with_object(object, stl::write_binary)
}

/// 3MF of one kernel object.
pub fn object_3mf(object: &sawhorse_kernel::object::Object) -> Result<Vec<u8>, ExportError> {
    with_object(object, three_mf::write)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_kernel::{Model, Op};

    #[test]
    fn kernel_box_exports() {
        let mut m = Model::with_ground_sketch();
        m.apply(&Op::AddRectangle {
            sketch: 0,
            x: 0.0,
            y: 0.0,
            w: 0.1,
            h: 0.2,
        })
        .unwrap();
        m.apply(&Op::Extrude {
            sketch: 0,
            region: 0,
            distance: 0.05,
        })
        .unwrap();
        let o = m.object(0).unwrap();
        let stl = object_stl(o).unwrap();
        assert_eq!(u32::from_le_bytes(stl[80..84].try_into().unwrap()), 12);
        assert_eq!(&object_3mf(o).unwrap()[..2], b"PK");
    }
}
