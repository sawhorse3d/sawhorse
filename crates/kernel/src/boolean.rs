//! 3D booleans behind our own trait (ADR-0001).
//!
//! The engine sees triangle solids with a per-triangle face id; the kernel triangulates
//! operand faces with `sawhorse-tessellate`, runs the engine, groups output triangles by
//! inherited face id, and re-polygonises (that last part lands with the push/pull lane).
//! Today: the trait, the Manifold adapter, and its round-trip tests.

use glam::DVec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BoolOp {
    Union,
    Difference,
    Intersection,
}

/// An indexed triangle solid: shared vertices, CCW-outward triangles, one face id each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriSolid {
    pub positions: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
    /// One per triangle: the kernel face the triangle came from.
    pub face_ids: Vec<u32>,
}

impl TriSolid {
    /// Signed volume by the divergence theorem (positive for outward-oriented solids).
    pub fn volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let (a, b, c) = (
                    self.positions[t[0] as usize],
                    self.positions[t[1] as usize],
                    self.positions[t[2] as usize],
                );
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BooleanError {
    #[error("operand {0} is not a closed manifold: {1}")]
    NotManifold(usize, String),
    #[error("boolean engine failed: {0}")]
    Engine(String),
    #[error("no boolean engine is compiled in (enable the `manifold` feature)")]
    Unavailable,
}

pub trait BooleanEngine {
    fn run(&self, a: &TriSolid, b: &TriSolid, op: BoolOp) -> Result<TriSolid, BooleanError>;
}

/// `manifold-rust` (pure-Rust Manifold port, Apache-2.0, pinned exactly).
#[cfg(feature = "manifold")]
#[derive(Debug, Default, Clone, Copy)]
pub struct ManifoldEngine;

#[cfg(feature = "manifold")]
mod manifold_adapter {
    use super::*;
    use manifold_rust::manifold::Manifold;
    use manifold_rust::types::{Error, MeshGL64, OpType};

    fn to_manifold(s: &TriSolid, which: usize) -> Result<Manifold, BooleanError> {
        let mesh = MeshGL64 {
            num_prop: 3,
            vert_properties: s.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
            tri_verts: s.triangles.iter().flat_map(|t| t.map(u64::from)).collect(),
            face_id: s.face_ids.iter().map(|&f| u64::from(f)).collect(),
            ..Default::default()
        };
        let m = Manifold::from_mesh_gl64(&mesh);
        match m.status() {
            Error::NoError => Ok(m),
            e => Err(BooleanError::NotManifold(which, format!("{e:?}"))),
        }
    }

    impl BooleanEngine for ManifoldEngine {
        fn run(&self, a: &TriSolid, b: &TriSolid, op: BoolOp) -> Result<TriSolid, BooleanError> {
            let ma = to_manifold(a, 0)?;
            let mb = to_manifold(b, 1)?;
            let op = match op {
                BoolOp::Union => OpType::Add,
                BoolOp::Difference => OpType::Subtract,
                BoolOp::Intersection => OpType::Intersect,
            };
            let r = ma.boolean(&mb, op);
            if r.status() != Error::NoError {
                return Err(BooleanError::Engine(format!("{:?}", r.status())));
            }
            let out = r.get_mesh_gl64(-1);
            let np = out.num_prop as usize;
            let positions = out
                .vert_properties
                .chunks_exact(np)
                .map(|p| DVec3::new(p[0], p[1], p[2]))
                .collect();
            let triangles = out
                .tri_verts
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| t.map(|i| i as u32))
                .collect();
            let face_ids = out.face_id.iter().map(|&f| f as u32).collect();
            Ok(TriSolid {
                positions,
                triangles,
                face_ids,
            })
        }
    }
}

#[cfg(all(test, feature = "manifold"))]
mod tests {
    use super::*;

    /// Axis-aligned box with shared vertices, two triangles per side, face id per side.
    pub(crate) fn cube(min: DVec3, size: f64, face_base: u32) -> TriSolid {
        let p = |x: f64, y: f64, z: f64| min + DVec3::new(x, y, z) * size;
        let positions = vec![
            p(0., 0., 0.),
            p(1., 0., 0.),
            p(1., 1., 0.),
            p(0., 1., 0.),
            p(0., 0., 1.),
            p(1., 0., 1.),
            p(1., 1., 1.),
            p(0., 1., 1.),
        ];
        let quads: [[u32; 4]; 6] = [
            [0, 3, 2, 1], // bottom (-z)
            [4, 5, 6, 7], // top (+z)
            [0, 1, 5, 4], // front (-y)
            [2, 3, 7, 6], // back (+y)
            [1, 2, 6, 5], // right (+x)
            [3, 0, 4, 7], // left (-x)
        ];
        let mut triangles = Vec::new();
        let mut face_ids = Vec::new();
        for (i, q) in quads.iter().enumerate() {
            triangles.push([q[0], q[1], q[2]]);
            triangles.push([q[0], q[2], q[3]]);
            face_ids.extend([face_base + i as u32; 2]);
        }
        TriSolid {
            positions,
            triangles,
            face_ids,
        }
    }

    #[test]
    fn cube_volume() {
        assert!((cube(DVec3::ZERO, 2.0, 0).volume() - 8.0).abs() < 1e-12);
    }

    #[test]
    fn manifold_difference_and_union() {
        let a = cube(DVec3::ZERO, 2.0, 0);
        let b = cube(DVec3::splat(1.0), 2.0, 100);
        let d = ManifoldEngine.run(&a, &b, BoolOp::Difference).unwrap();
        assert!(
            (d.volume() - 7.0).abs() < 1e-9,
            "difference volume {}",
            d.volume()
        );
        let u = ManifoldEngine.run(&a, &b, BoolOp::Union).unwrap();
        assert!(
            (u.volume() - 15.0).abs() < 1e-9,
            "union volume {}",
            u.volume()
        );
        let i = ManifoldEngine.run(&a, &b, BoolOp::Intersection).unwrap();
        assert!(
            (i.volume() - 1.0).abs() < 1e-9,
            "intersection volume {}",
            i.volume()
        );
        // Face ids survive the engine: every output triangle traces to an operand face.
        assert!(d.face_ids.iter().all(|&f| f < 6 || (100..106).contains(&f)));
        assert_eq!(d.face_ids.len(), d.triangles.len());
    }

    #[test]
    fn open_operand_is_a_typed_error() {
        let mut a = cube(DVec3::ZERO, 1.0, 0);
        a.triangles.pop();
        a.face_ids.pop();
        let b = cube(DVec3::ZERO, 1.0, 0);
        assert!(matches!(
            ManifoldEngine.run(&a, &b, BoolOp::Union),
            Err(BooleanError::NotManifold(0, _))
        ));
    }
}
