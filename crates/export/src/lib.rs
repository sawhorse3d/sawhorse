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
