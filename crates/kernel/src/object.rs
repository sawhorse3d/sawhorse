//! The solid tier: a manifold, watertight half-edge solid (ADR-0001).
//!
//! Born by extruding a sketch region (push/pull of a flat region). Lane 1 also implements
//! the "extend" branch of push/pull on an existing face: when every face touching the
//! pushed face runs parallel to the push direction (a prism cap, the common woodworking
//! case), the face's vertices slide along its normal and no new topology is needed.
//! Pushing a cap exactly onto the opposite cap collapses the solid (the model then hands
//! the region back to its sketch). Pushing further, or pushing a face whose neighbours are
//! not parallel to the push, needs the boolean / side-wall path of the push/pull lane and
//! is refused with a typed error today.

use std::collections::BTreeSet;

use glam::{DVec2, DVec3};
use slotmap::Key;

use sawhorse_tessellate::{TessellateError, signed_area, triangulate};

use crate::boolean::TriSolid;
use crate::brep::{Brep, PolyFace};
use crate::error::PushError;
use crate::ids::{FaceId, SketchId, VertexId};
use crate::plane::Plane;
use crate::tolerance::{MIN_EXTRUDE, PARALLEL_EPS, POINT_MERGE, in_bounds};
use crate::validate::{
    TopologyError, TopologyReport, Violation, holes_contained, rings_cross, validate,
};

/// Where a solid came from, so collapsing it can return its footprint to that sketch.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    pub sketch: SketchId,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub(crate) brep: Brep,
    pub(crate) watertight: bool,
    pub(crate) source: Option<Provenance>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PushOutcome {
    /// The face moved.
    Moved,
    /// The face met the opposite face: the solid has zero volume and should be removed.
    /// `footprint`: the flat rings (outer, then holes) it collapsed onto, world coordinates.
    Collapsed { footprint: Vec<Vec<DVec3>> },
}

impl Object {
    /// Extrude a planar region (outer ring + hole rings, in `plane`'s 2D frame) by
    /// `distance` along the plane normal (negative goes the other way).
    ///
    /// Face order is deterministic: base (on the plane), cap (offset), then side walls ring
    /// by ring, edge by edge.
    pub fn extrude(
        plane: &Plane,
        outer: &[DVec2],
        holes: &[Vec<DVec2>],
        distance: f64,
    ) -> Result<Object, PushError> {
        if !in_bounds(distance) {
            return Err(PushError::OutOfBounds);
        }
        if distance.abs() < MIN_EXTRUDE {
            return Err(PushError::ZeroDistance);
        }
        // Normalise winding: outer counter-clockwise, holes clockwise.
        let mut rings: Vec<Vec<DVec2>> = Vec::with_capacity(1 + holes.len());
        let mut o = outer.to_vec();
        if signed_area(&o) < 0.0 {
            o.reverse();
        }
        rings.push(o);
        for h in holes {
            let mut h = h.clone();
            if signed_area(&h) > 0.0 {
                h.reverse();
            }
            rings.push(h);
        }
        let offset = plane.normal * distance;
        let mut points: Vec<DVec3> = Vec::new();
        let mut base_idx: Vec<Vec<usize>> = Vec::new();
        for r in &rings {
            base_idx.push((points.len()..points.len() + r.len()).collect());
            points.extend(r.iter().map(|&q| plane.to_world(q)));
        }
        let n_base = points.len();
        let cap_idx: Vec<Vec<usize>> = base_idx
            .iter()
            .map(|r| r.iter().map(|&i| i + n_base).collect())
            .collect();
        for i in 0..n_base {
            points.push(points[i] + offset);
        }
        if points
            .iter()
            .any(|p| !in_bounds(p.x) || !in_bounds(p.y) || !in_bounds(p.z))
        {
            return Err(PushError::OutOfBounds);
        }

        let rev = |r: &Vec<usize>| r.iter().rev().copied().collect::<Vec<_>>();
        let mut faces = Vec::new();
        // Built for distance > 0; mirrored below when negative.
        faces.push(PolyFace {
            outer: rev(&base_idx[0]),
            holes: base_idx[1..].iter().map(rev).collect(),
        });
        faces.push(PolyFace {
            outer: cap_idx[0].clone(),
            holes: cap_idx[1..].to_vec(),
        });
        for (b, c) in base_idx.iter().zip(&cap_idx) {
            let n = b.len();
            for k in 0..n {
                let j = (k + 1) % n;
                faces.push(PolyFace {
                    outer: vec![b[k], b[j], c[j], c[k]],
                    holes: vec![],
                });
            }
        }
        if distance < 0.0 {
            for f in &mut faces {
                f.outer.reverse();
                for h in &mut f.holes {
                    h.reverse();
                }
            }
        }
        let brep = Brep::from_polygons(&points, &faces)?;
        let mut obj = Object {
            brep,
            watertight: false,
            source: None,
        };
        obj.watertight = validate(&obj.brep).map(|r| r.watertight).unwrap_or(false);
        Ok(obj)
    }

    pub fn brep(&self) -> &Brep {
        &self.brep
    }

    pub fn is_watertight(&self) -> bool {
        self.watertight
    }

    pub fn source(&self) -> Option<&Provenance> {
        self.source.as_ref()
    }

    /// Faces in deterministic order (index = the face ordinal used by ops and the UI).
    pub fn face_ids(&self) -> Vec<FaceId> {
        self.brep.face_ids()
    }

    /// Validate topology plus the object rules: watertight, and the stored flag agrees.
    pub fn validate(&self) -> Result<TopologyReport, TopologyError> {
        let report = validate(&self.brep)?;
        let mut v = Vec::new();
        if !report.watertight || !self.watertight {
            v.push(Violation::DanglingKey("object is not watertight"));
        }
        if report.shells.iter().any(|s| !s.closed) {
            v.push(Violation::DanglingKey("object has an open shell"));
        }
        if v.is_empty() {
            Ok(report)
        } else {
            Err(TopologyError {
                violations: v,
                report,
            })
        }
    }

    /// Push/pull an existing face by `distance` along its outward normal ("extend" branch).
    pub fn push_pull(&mut self, face: FaceId, distance: f64) -> Result<PushOutcome, PushError> {
        if !in_bounds(distance) {
            return Err(PushError::OutOfBounds);
        }
        if distance.abs() < MIN_EXTRUDE {
            return Err(PushError::ZeroDistance);
        }
        let b = &self.brep;
        let f = b.face(face).ok_or(PushError::NoSuchFace(u32::MAX))?;
        let n = f.plane.normal;
        let moved: BTreeSet<VertexId> = b.face_vertices(face);

        // Every other face touching the moved vertices must contain the push direction.
        for &v in &moved {
            for g in b.faces_around_vertex(v) {
                if g != face && b.faces[g].plane.normal.dot(n).abs() > PARALLEL_EPS.sqrt() {
                    return Err(PushError::Unsupported(
                        "a neighbouring face is not parallel to the push",
                    ));
                }
            }
        }
        // Rails: edges with exactly one moved end. Each must run straight along the normal:
        // "behind" rails (into the material, along -n) lengthen as the face moves out;
        // "ahead" rails (e.g. a hole's ring edges seen from its wall) shorten.
        let mut rails: Vec<(f64, f64)> = Vec::new(); // (length, +1 behind / -1 ahead)
        for (eid, _) in b.edges() {
            let he = b.edges[eid].he;
            let (a, c) = (b.half_edges[he].origin, b.dest(he));
            let (ma, mc) = (moved.contains(&a), moved.contains(&c));
            if ma == mc {
                continue;
            }
            let (from, to) = if ma { (a, c) } else { (c, a) };
            let d = b.vertices[to].pos - b.vertices[from].pos;
            let len = d.length();
            let along = if len > 0.0 { (d / len).dot(n) } else { 0.0 };
            let sign = if along <= -(1.0 - PARALLEL_EPS.sqrt()) {
                1.0
            } else if along >= 1.0 - PARALLEL_EPS.sqrt() {
                -1.0
            } else {
                return Err(PushError::Unsupported(
                    "an edge leaving the face is not parallel to the push",
                ));
            };
            rails.push((len, sign));
        }
        if rails.is_empty() {
            return Err(PushError::Unsupported(
                "the face has no edges running back from it",
            ));
        }
        let new_lengths: Vec<f64> = rails.iter().map(|(l, s)| l + s * distance).collect();
        if new_lengths.iter().any(|&l| l < -POINT_MERGE) {
            return Err(PushError::ThroughOppositeFace);
        }
        let collapsing = new_lengths.iter().filter(|&&l| l <= POINT_MERGE).count();
        if collapsing == new_lengths.len() && rails.iter().all(|(_, s)| *s > 0.0) {
            let footprint = f
                .loops()
                .map(|l| {
                    b.loop_points(l)
                        .into_iter()
                        .map(|p| p + n * distance)
                        .collect()
                })
                .collect();
            return Ok(PushOutcome::Collapsed { footprint });
        }
        if collapsing > 0 {
            return Err(PushError::WouldCollapseEdge);
        }
        // The moved face must not pass through another part of this solid: every face that
        // changes shape keeps simple, properly nested rings (enough for prisms; general solids
        // get a 3D check with the push/pull lane).
        let delta = n * distance;
        let pos = |v: VertexId| {
            b.vertices[v].pos
                + if moved.contains(&v) {
                    delta
                } else {
                    DVec3::ZERO
                }
        };
        let touched: BTreeSet<FaceId> = moved
            .iter()
            .flat_map(|&v| b.faces_around_vertex(v))
            .collect();
        for g in touched {
            let fg = &b.faces[g];
            let pts: Vec<DVec3> = b.loop_vertices(fg.outer).into_iter().map(pos).collect();
            let Ok(plane) = Plane::fit(&pts) else {
                return Err(PushError::WouldCollapseEdge);
            };
            let rings: Vec<Vec<DVec2>> = fg
                .loops()
                .map(|l| {
                    b.loop_vertices(l)
                        .into_iter()
                        .map(|v| plane.to_local(pos(v)))
                        .collect()
                })
                .collect();
            if rings_cross(&rings) || holes_contained(&rings).is_err() {
                return Err(PushError::Unsupported(
                    "the face would pass through another part of the solid",
                ));
            }
        }
        self.brep.translate_vertices(&moved, n * distance);
        Ok(PushOutcome::Moved)
    }

    /// Indexed triangle solid (shared vertices in slotmap order, face ordinal per triangle),
    /// the form booleans and exporters consume.
    pub fn tri_solid(&self) -> Result<TriSolid, TessellateError> {
        let b = &self.brep;
        let vindex: std::collections::BTreeMap<VertexId, u32> = b
            .vertices
            .keys()
            .enumerate()
            .map(|(i, k)| (k, i as u32))
            .collect();
        let positions: Vec<DVec3> = b.vertices.values().map(|v| v.pos).collect();
        let mut triangles = Vec::new();
        let mut face_ids = Vec::new();
        for (fi, (_, f)) in b.faces.iter().enumerate() {
            let loops: Vec<Vec<VertexId>> = f.loops().map(|l| b.loop_vertices(l)).collect();
            let flat: Vec<VertexId> = loops.iter().flatten().copied().collect();
            let ring2 = |l: &Vec<VertexId>| {
                l.iter()
                    .map(|v| f.plane.to_local(b.vertices[*v].pos))
                    .collect::<Vec<_>>()
            };
            let outer = ring2(&loops[0]);
            let holes: Vec<Vec<DVec2>> = loops[1..].iter().map(ring2).collect();
            for t in triangulate(&outer, &holes)? {
                triangles.push(t.map(|i| vindex[&flat[i as usize]]));
                face_ids.push(fi as u32);
            }
        }
        Ok(TriSolid {
            positions,
            triangles,
            face_ids,
        })
    }

    /// Enclosed volume in cubic metres.
    pub fn volume(&self) -> f64 {
        self.tri_solid().map(|s| s.volume()).unwrap_or(0.0)
    }

    /// True when both objects have the same topology (vertex/face order) and every vertex
    /// is within `tol`. Used by the inverse-operation invariants.
    pub fn same_shape(&self, other: &Object, tol: f64) -> bool {
        let (a, b) = (&self.brep, &other.brep);
        if a.vertex_count() != b.vertex_count()
            || a.face_count() != b.face_count()
            || a.edge_count() != b.edge_count()
        {
            return false;
        }
        let close = a
            .vertices
            .values()
            .zip(b.vertices.values())
            .all(|(p, q)| p.pos.distance(q.pos) <= tol);
        let topo = a.faces.values().zip(b.faces.values()).all(|(f, g)| {
            let fl: Vec<Vec<VertexId>> = f.loops().map(|l| a.loop_vertices(l)).collect();
            let gl: Vec<Vec<VertexId>> = g.loops().map(|l| b.loop_vertices(l)).collect();
            fl.len() == gl.len()
                && fl.iter().zip(&gl).all(|(x, y)| {
                    x.len() == y.len()
                        && x.iter()
                            .zip(y)
                            .all(|(p, q)| p.data().as_ffi() == q.data().as_ffi())
                })
        });
        close && topo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Vec<DVec2> {
        vec![
            DVec2::ZERO,
            DVec2::new(w, 0.0),
            DVec2::new(w, h),
            DVec2::new(0.0, h),
        ]
    }

    #[test]
    fn extrude_makes_a_watertight_box() {
        let o = Object::extrude(&Plane::GROUND, &rect(2.0, 3.0), &[], 4.0).unwrap();
        let r = o.validate().unwrap();
        assert!(r.watertight);
        assert_eq!((r.vertices, r.edges, r.faces), (8, 12, 6));
        assert_eq!(r.shells[0].genus, Some(0));
        assert!((o.volume() - 24.0).abs() < 1e-9);
    }

    #[test]
    fn negative_extrude_points_down_and_stays_outward() {
        let o = Object::extrude(&Plane::GROUND, &rect(1.0, 1.0), &[], -0.5).unwrap();
        o.validate().unwrap();
        assert!(
            (o.volume() - 0.5).abs() < 1e-12,
            "outward orientation gives positive volume"
        );
        assert!(o.brep.vertices().all(|(_, v)| v.pos.z <= 1e-12));
    }

    #[test]
    fn extrude_with_hole_is_genus_one() {
        let mut hole = vec![
            DVec2::ONE,
            DVec2::new(3.0, 1.0),
            DVec2::new(3.0, 3.0),
            DVec2::new(1.0, 3.0),
        ];
        hole.reverse();
        let o = Object::extrude(&Plane::GROUND, &rect(4.0, 4.0), &[hole], 1.0).unwrap();
        let r = o.validate().unwrap();
        assert_eq!(r.holes, 2);
        assert_eq!(r.shells[0].euler, 0);
        assert_eq!(r.shells[0].genus, Some(1));
        assert!((o.volume() - 12.0).abs() < 1e-9);
    }

    #[test]
    fn push_pull_cap_extends_and_restores() {
        let mut o = Object::extrude(&Plane::GROUND, &rect(1.0, 1.0), &[], 1.0).unwrap();
        let before = o.clone();
        let cap = o.face_ids()[1];
        assert_eq!(o.push_pull(cap, 0.25).unwrap(), PushOutcome::Moved);
        assert!((o.volume() - 1.25).abs() < 1e-9);
        assert_eq!(o.push_pull(cap, -0.25).unwrap(), PushOutcome::Moved);
        assert!(o.same_shape(&before, 1e-12));
        assert!(matches!(
            o.push_pull(cap, -1.0).unwrap(),
            PushOutcome::Collapsed { .. }
        ));
        assert_eq!(o.push_pull(cap, -1.5), Err(PushError::ThroughOppositeFace));
        assert_eq!(o.push_pull(cap, 0.0), Err(PushError::ZeroDistance));
    }

    #[test]
    fn push_pull_side_wall_also_extends() {
        let mut o = Object::extrude(&Plane::GROUND, &rect(1.0, 1.0), &[], 1.0).unwrap();
        let side = o.face_ids()[2];
        assert_eq!(o.push_pull(side, 0.5).unwrap(), PushOutcome::Moved);
        assert!((o.volume() - 1.5).abs() < 1e-9);
        o.validate().unwrap();
    }

    #[test]
    fn push_pull_hole_wall_shrinks_the_hole() {
        let mut hole = vec![
            DVec2::ONE,
            DVec2::new(3.0, 1.0),
            DVec2::new(3.0, 3.0),
            DVec2::new(1.0, 3.0),
        ];
        hole.reverse();
        let mut o = Object::extrude(&Plane::GROUND, &rect(4.0, 4.0), &[hole], 1.0).unwrap();
        // Find the hole wall facing +x (at x = 1).
        let wall = o
            .face_ids()
            .into_iter()
            .find(|&f| {
                let p = o.brep.face(f).unwrap().plane;
                p.normal.x > 0.5 && (p.origin.x - 1.0).abs() < 1e-9
            })
            .unwrap();
        assert_eq!(o.push_pull(wall, 0.5).unwrap(), PushOutcome::Moved);
        assert!((o.volume() - 13.0).abs() < 1e-9);
        assert_eq!(o.push_pull(wall, 1.5), Err(PushError::WouldCollapseEdge));
        assert_eq!(o.push_pull(wall, 3.0), Err(PushError::ThroughOppositeFace));
        // Pushing the hole wall back through the outer wall would leave the hole outside.
        assert!(matches!(
            o.push_pull(wall, -2.0),
            Err(PushError::Unsupported(_))
        ));
        // Pushing the outer wall in through the hole, likewise.
        let outer_wall = o
            .face_ids()
            .into_iter()
            .find(|&f| o.brep.face(f).unwrap().plane.normal.x < -0.5)
            .unwrap();
        assert!(matches!(
            o.push_pull(outer_wall, -2.0),
            Err(PushError::Unsupported(_))
        ));
        o.validate().unwrap();
    }

    #[test]
    fn typed_refusals() {
        assert_eq!(
            Object::extrude(&Plane::GROUND, &rect(1.0, 1.0), &[], 1e-7).unwrap_err(),
            PushError::ZeroDistance
        );
        assert_eq!(
            Object::extrude(&Plane::GROUND, &rect(1.0, 1.0), &[], f64::INFINITY).unwrap_err(),
            PushError::OutOfBounds
        );
    }
}
