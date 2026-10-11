//! Binary STL, hand-written: 80-byte header, u32 count, 50 bytes per triangle.
//! Millimetres, Z-up (slicers assume both).

use crate::{ExportError, ExportMesh, MM_PER_M};

const HEADER: &[u8] = b"Sawhorse binary STL; units mm; Z up";

pub fn write_binary(mesh: &ExportMesh<'_>) -> Result<Vec<u8>, ExportError> {
    mesh.check()?;
    let mut out = Vec::with_capacity(84 + mesh.triangles.len() * 50);
    let mut header = [0u8; 80];
    header[..HEADER.len()].copy_from_slice(HEADER);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(mesh.triangles.len() as u32).to_le_bytes());
    for t in mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.positions[i as usize] * MM_PER_M);
        let n = (b - a).cross(c - a).normalize_or_zero();
        for v in [n, a, b, c] {
            for x in [v.x, v.y, v.z] {
                out.extend_from_slice(&(x as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    #[test]
    fn layout_and_units() {
        let p = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z];
        let t = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let bytes = write_binary(&ExportMesh {
            positions: &p,
            triangles: &t,
            watertight: true,
        })
        .unwrap();
        assert_eq!(bytes.len(), 84 + 4 * 50);
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 4);
        // Second triangle, vertex b = (1 m, 0, 0) -> 1000 mm.
        let off = 84 + 50 + 12 + 12;
        assert_eq!(
            f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()),
            1000.0
        );
    }

    #[test]
    fn refuses_open_shells() {
        let p = [DVec3::ZERO, DVec3::X, DVec3::Y];
        let t = [[0, 1, 2]];
        assert!(matches!(
            write_binary(&ExportMesh {
                positions: &p,
                triangles: &t,
                watertight: false
            }),
            Err(ExportError::NotWatertight)
        ));
    }
}
