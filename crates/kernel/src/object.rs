//! The solid tier: a manifold, watertight half-edge solid (ADR-0001).
//!
//! Born by extruding a sketch region (push/pull of a flat region); changed by general
//! push/pull ([`crate::push`]), drawing on faces ([`crate::faceops`]: split_face,
//! imprint) and booleans ([`crate::boolean`]). Every edit rebuilds the B-rep from plain
//! data and must validate (topology, watertight, no self-intersection) before commit.

use glam::{DVec2, DVec3};
use slotmap::Key;

use sawhorse_tessellate::{TessellateError, signed_area, triangulate};

use crate::boolean::{BoolOp, BooleanError, TriSolid};
use crate::brep::{Brep, PolyFace};
use crate::error::{FaceEditError, PushError};
use crate::ids::{FaceId, SketchId, VertexId};
use crate::plane::Plane;
use crate::tolerance::{MIN_EXTRUDE, in_bounds};
use crate::validate::{TopologyError, TopologyReport, Violation, validate};

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
    /// Tag (layer) name; `None` is untagged. Hidden tags hide the object in the view.
    pub(crate) tag: Option<String>,
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
        faces.push(PolyFace::new(
            rev(&base_idx[0]),
            base_idx[1..].iter().map(rev).collect(),
        ));
        faces.push(PolyFace::new(cap_idx[0].clone(), cap_idx[1..].to_vec()));
        for (b, c) in base_idx.iter().zip(&cap_idx) {
            let n = b.len();
            for k in 0..n {
                let j = (k + 1) % n;
                faces.push(PolyFace::new(vec![b[k], b[j], c[j], c[k]], vec![]));
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
            tag: None,
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

    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
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
        if let Some(hit) = crate::intersect::self_intersection(&self.brep) {
            v.push(Violation::SelfIntersectingSolid(hit));
        }
        // Every face can be locally consistent while the whole solid is inside out (a face
        // pushed backwards through itself): the enclosed volume must be positive.
        let volume = self.volume();
        if volume.is_nan() || volume <= 0.0 {
            v.push(Violation::NonPositiveVolume(volume));
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

    /// Push/pull face `face` (ordinal) by `distance` metres along its outward normal
    /// (negative pushes inward). Any planar face of a watertight solid; see [`crate::push`]
    /// for the collapse / topological / boolean paths. On `Moved` the solid is replaced;
    /// on `Collapsed` it is left as it was (the caller removes it).
    pub fn push_pull(&mut self, face: usize, distance: f64) -> Result<PushOutcome, PushError> {
        let (outcome, next) = crate::push::push_pull(self, face, distance)?;
        if let Some(mut next) = next {
            next.source = self.source.clone();
            next.tag = self.tag.clone();
            *self = next;
        }
        Ok(outcome)
    }

    /// A solid from an already-built B-rep (watertight flag from the validator).
    pub(crate) fn from_brep(brep: Brep) -> Object {
        let watertight = validate(&brep).map(|r| r.watertight).unwrap_or(false);
        Object {
            brep,
            watertight,
            source: None,
            tag: None,
        }
    }

    /// `self op other` through the boolean engine, re-polygonised (see
    /// [`crate::boolean::combine`]). `Ok(None)`: the result is empty.
    pub fn combine(&self, other: &Object, op: BoolOp) -> Result<Option<Object>, BooleanError> {
        let engine = crate::boolean::default_engine()?;
        let res = crate::boolean::combine(
            &self.brep.to_polys(),
            &other.brep.to_polys(),
            op,
            engine.as_ref(),
        )?;
        let Some(p) = res else { return Ok(None) };
        let brep = Brep::build(&p).map_err(|e| BooleanError::InvalidResult(e.to_string()))?;
        let o = Object::from_brep(brep);
        o.validate()
            .map_err(|e| BooleanError::InvalidResult(e.to_string()))?;
        Ok(Some(o))
    }

    /// Total face area in square metres.
    pub fn surface_area(&self) -> f64 {
        self.brep
            .faces()
            .map(|(_, f)| {
                f.loops()
                    .map(|l| {
                        let pts: Vec<DVec2> = self
                            .brep
                            .loop_points(l)
                            .iter()
                            .map(|&p| f.plane.to_local(p))
                            .collect();
                        signed_area(&pts)
                    })
                    .sum::<f64>()
            })
            .sum()
    }

    /// Split face `face` (ordinal) along `path` (world points on the face's plane): ends on
    /// the boundary, interior strictly inside. Returns the two faces' ordinals (the first
    /// keeps `face`). See [`crate::faceops::split_face`].
    pub fn split_face(&mut self, face: usize, path: &[DVec3]) -> Result<[u32; 2], FaceEditError> {
        let mut p = self.brep.to_polys();
        if face >= p.faces.len() {
            return Err(FaceEditError::NoSuchFace(face as u32));
        }
        let (a, b) = crate::faceops::split_face(&mut p, face, path)?;
        self.brep = Brep::build(&p)?;
        self.brep.debug_validate("split_face");
        Ok([a as u32, b as u32])
    }

    /// Imprint the closed loop `points` (strictly inside face `face`) as a coplanar
    /// sub-face; the face gains the twin hole. Returns the sub-face's ordinal. See
    /// [`crate::faceops::imprint`].
    pub fn imprint(&mut self, face: usize, points: &[DVec3]) -> Result<u32, FaceEditError> {
        let mut p = self.brep.to_polys();
        if face >= p.faces.len() {
            return Err(FaceEditError::NoSuchFace(face as u32));
        }
        let sub = crate::faceops::imprint(&mut p, face, points)?;
        self.brep = Brep::build(&p)?;
        self.brep.debug_validate("imprint");
        Ok(sub as u32)
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
