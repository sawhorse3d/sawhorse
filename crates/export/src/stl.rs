//! Binary STL, hand-written: 80-byte header, u32 count, 50 bytes per triangle.
//! Millimetres, Z-up (slicers assume both). STL has no units field, so the header says it.

use sawhorse_kernel::Model;

use crate::{ExportError, ExportMesh, MM_PER_M, Scope, solid_triangles};

const HEADER: &[u8] = b"Sawhorse binary STL; units mm; Z up";

fn header(out: &mut Vec<u8>, count: usize) {
    let mut header = [0u8; 80];
    header[..HEADER.len()].copy_from_slice(HEADER);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(count as u32).to_le_bytes());
}

fn triangle(out: &mut Vec<u8>, [a, b, c]: [glam::DVec3; 3]) {
    let [a, b, c] = [a, b, c].map(|p| p * MM_PER_M);
    let n = (b - a).cross(c - a).normalize_or_zero();
    for v in [n, a, b, c] {
        for x in [v.x, v.y, v.z] {
            out.extend_from_slice(&(x as f32).to_le_bytes());
        }
    }
    out.extend_from_slice(&0u16.to_le_bytes());
}

pub fn write_binary(mesh: &ExportMesh<'_>) -> Result<Vec<u8>, ExportError> {
    mesh.check()?;
    let mut out = Vec::with_capacity(84 + mesh.triangles.len() * 50);
    header(&mut out, mesh.triangles.len());
    for t in mesh.triangles {
        triangle(&mut out, t.map(|i| mesh.positions[i as usize]));
    }
    Ok(out)
}

/// Every solid in `scope` in one binary STL (one shell per solid). Refuses a scope holding
/// an imported non-solid mesh (see [`Scope::printable`]).
pub fn document(model: &Model, scope: &Scope) -> Result<Vec<u8>, ExportError> {
    let solids = scope.printable(model)?;
    let mut tris = Vec::new();
    for (_, o) in solids {
        let (pts, t, _) = solid_triangles(o)?;
        tris.extend(t.iter().map(|t| t.map(|i| pts[i as usize])));
    }
    let mut out = Vec::with_capacity(84 + tris.len() * 50);
    header(&mut out, tris.len());
    for t in tris {
        triangle(&mut out, t);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
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

    #[test]
    fn document_is_every_solid_and_refuses_meshes() {
        let m = fixtures::mixed_model();
        assert!(matches!(
            document(&m, &Scope::visible(&m)),
            Err(ExportError::NotSolid(..))
        ));
        let solids = Scope {
            objects: vec![1, 2],
            meshes: vec![],
        };
        let bytes = document(&m, &solids).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 24);
        assert_eq!(bytes.len(), 84 + 24 * 50);
        // Every normal is a unit vector and points away from its solid's centre.
        for k in 0..24 {
            let f = |o: usize| {
                f32::from_le_bytes(
                    bytes[84 + k * 50 + o..84 + k * 50 + o + 4]
                        .try_into()
                        .unwrap(),
                )
            };
            let n = DVec3::new(f(0).into(), f(4).into(), f(8).into());
            assert!((n.length() - 1.0).abs() < 1e-6);
        }
    }
}
