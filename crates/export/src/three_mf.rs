//! 3MF through the `threemf` crate. 3MF's default unit is the millimetre.

use std::io::Cursor;

use crate::{ExportError, ExportMesh, MM_PER_M};

pub fn write(mesh: &ExportMesh<'_>) -> Result<Vec<u8>, ExportError> {
    use threemf::model::{Mesh, Triangle, Triangles, Vertex, Vertices};
    mesh.check()?;
    let m = Mesh {
        vertices: Vertices {
            vertex: mesh
                .positions
                .iter()
                .map(|p| {
                    let p = *p * MM_PER_M;
                    Vertex {
                        x: p.x,
                        y: p.y,
                        z: p.z,
                    }
                })
                .collect(),
        },
        triangles: Triangles {
            triangle: mesh
                .triangles
                .iter()
                .map(|t| Triangle {
                    v1: t[0] as usize,
                    v2: t[1] as usize,
                    v3: t[2] as usize,
                })
                .collect(),
        },
    };
    let mut buf = Cursor::new(Vec::new());
    threemf::write(&mut buf, m).map_err(|e| ExportError::ThreeMf(e.to_string()))?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    #[test]
    fn writes_a_zip_with_the_model() {
        let p = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z];
        let t = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let bytes = write(&ExportMesh {
            positions: &p,
            triangles: &t,
            watertight: true,
        })
        .unwrap();
        assert_eq!(&bytes[..2], b"PK", "3MF is a zip container");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("3D/model.model"));
        assert!(text.contains("[Content_Types].xml"));
    }
}
