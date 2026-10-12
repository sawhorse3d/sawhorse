//! Transforms (lane 4): move, rotate, scale and mirror whole solids, copy them, array them,
//! and move or scale part of a solid (a face, an edge, a vertex).
//!
//! An [`Xform`] is an affine map (a 3x3 linear part plus a translation) in plain data, so
//! ops carrying one serialise to the op log like every other op.
//!
//! - **Object-level** ([`Object::transformed`]): every vertex and every stored face plane is
//!   mapped; topology, face ids, user-drawn edges, the tag and the display number are kept.
//!   A mirror (negative determinant) reverses every loop so the solid stays outward-facing;
//!   a singular map (anything flattened) is refused. The result is validated like any edit
//!   (planarity, watertightness, positive volume, no self-intersection), so a non-uniform
//!   scale that would fold the solid into itself is refused, typed. Touching or overlapping
//!   solids never weld (ADR-0001): each object is validated on its own.
//! - **Geometry-level** ([`Object::transform_geometry`]): only the chosen vertices move. A
//!   face whose vertices all move maps like the object case; a face with some moving
//!   vertices must stay planar, or, with `autofold`, is folded along a triangulation of its
//!   old shape (the triangles that stay coplanar merge back). Without autofold a bend is
//!   refused ([`TransformError::WouldBend`]), naming the face.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DAffine3, DMat3, DVec2, DVec3};

use crate::brep::{PolyFace, Polys};
use crate::error::TransformError;
use crate::faceops::merge_coplanar;
use crate::object::Object;
use crate::plane::Plane;
use crate::tolerance::{MAX_COORD, PLANARITY, POINT_MERGE, in_bounds};
use crate::validate::Violation;

/// An affine map in plain data: `p -> linear * p + translation`, `linear` row-major.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Xform {
    pub linear: [[f64; 3]; 3],
    pub translation: [f64; 3],
}

/// Determinants below this (in absolute value) flatten a solid: refused.
pub const MIN_DETERMINANT: f64 = 1e-9;

impl Xform {
    pub const IDENTITY: Xform = Xform {
        linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        translation: [0.0; 3],
    };

    /// Move by `d`.
    pub fn translation(d: DVec3) -> Xform {
        Xform {
            translation: d.to_array(),
            ..Xform::IDENTITY
        }
    }

    /// Turn by `angle` radians about the line through `origin` along `axis` (right-handed:
    /// counter-clockwise seen from the tip of `axis`).
    pub fn rotation(origin: DVec3, axis: DVec3, angle: f64) -> Xform {
        let m = DMat3::from_axis_angle(axis.normalize_or_zero(), angle);
        Xform::from_affine(
            DAffine3::from_translation(origin)
                * DAffine3::from_mat3(m)
                * DAffine3::from_translation(-origin),
        )
    }

    /// Scale by `factors` along the world axes about `origin` (negative factors mirror).
    pub fn scaling(origin: DVec3, factors: DVec3) -> Xform {
        Xform::from_affine(
            DAffine3::from_translation(origin)
                * DAffine3::from_scale(factors)
                * DAffine3::from_translation(-origin),
        )
    }

    pub fn from_affine(a: DAffine3) -> Xform {
        let m = a.matrix3;
        Xform {
            linear: [
                [m.x_axis.x, m.y_axis.x, m.z_axis.x],
                [m.x_axis.y, m.y_axis.y, m.z_axis.y],
                [m.x_axis.z, m.y_axis.z, m.z_axis.z],
            ],
            translation: a.translation.to_array(),
        }
    }

    pub fn to_affine(&self) -> DAffine3 {
        let l = &self.linear;
        DAffine3::from_mat3_translation(
            DMat3::from_cols(
                DVec3::new(l[0][0], l[1][0], l[2][0]),
                DVec3::new(l[0][1], l[1][1], l[2][1]),
                DVec3::new(l[0][2], l[1][2], l[2][2]),
            ),
            DVec3::from_array(self.translation),
        )
    }

    /// `self` after `other` (apply `other` first).
    pub fn after(&self, other: &Xform) -> Xform {
        Xform::from_affine(self.to_affine() * other.to_affine())
    }

    /// `self` applied `k` times (k = 0 is the identity).
    pub fn power(&self, k: u32) -> Xform {
        let mut out = DAffine3::IDENTITY;
        let a = self.to_affine();
        for _ in 0..k {
            out = a * out;
        }
        Xform::from_affine(out)
    }

    pub fn inverse(&self) -> Xform {
        Xform::from_affine(self.to_affine().inverse())
    }

    pub fn determinant(&self) -> f64 {
        self.to_affine().matrix3.determinant()
    }

    pub fn apply(&self, p: DVec3) -> DVec3 {
        self.to_affine().transform_point3(p)
    }

    /// Finite, within bounds, and not flattening anything.
    pub fn check(&self) -> Result<(), TransformError> {
        let finite = self
            .linear
            .iter()
            .flatten()
            .chain(self.translation.iter())
            .all(|v| v.is_finite());
        if !finite || !self.translation.iter().all(|&t| in_bounds(t)) {
            return Err(TransformError::BadTransform);
        }
        if self.linear.iter().flatten().any(|v| v.abs() > MAX_COORD) {
            return Err(TransformError::BadTransform);
        }
        let det = self.determinant();
        if !det.is_finite() || det.abs() < MIN_DETERMINANT {
            return Err(TransformError::Singular);
        }
        Ok(())
    }

    /// True when the map is the identity within rounding.
    pub fn is_identity(&self) -> bool {
        let a = self.to_affine();
        a.matrix3.abs_diff_eq(DMat3::IDENTITY, 1e-15) && a.translation.length() < 1e-15
    }

    /// The array step that, applied `parts` times, gives `self`: the "/4" of an array.
    /// Defined for rigid motions (a move, a turn, or a screw of both: the n-th of the angle
    /// about the same axis and the n-th of the slide along it) and for positive scales about
    /// a point (the n-th root of each factor). `None` for anything else (a mirror, a shear).
    pub fn divided(&self, parts: u32) -> Option<Xform> {
        if parts == 0 {
            return None;
        }
        let n = f64::from(parts);
        let a = self.to_affine();
        let r = a.matrix3;
        let t = a.translation;
        if r.abs_diff_eq(DMat3::IDENTITY, 1e-12) {
            return Some(Xform::translation(t / n));
        }
        // Rigid: orthonormal with determinant +1.
        let rigid = (r.transpose() * r).abs_diff_eq(DMat3::IDENTITY, 1e-9)
            && (r.determinant() - 1.0).abs() < 1e-9;
        if rigid {
            let cos = ((r.x_axis.x + r.y_axis.y + r.z_axis.z - 1.0) / 2.0).clamp(-1.0, 1.0);
            let angle = cos.acos();
            // Axis from the skew part (r32 - r23, r13 - r31, r21 - r12), or near a half turn
            // from the symmetric part.
            let skew = DVec3::new(
                r.y_axis.z - r.z_axis.y,
                r.z_axis.x - r.x_axis.z,
                r.x_axis.y - r.y_axis.x,
            );
            let axis = if skew.length() > 1e-9 {
                skew.normalize()
            } else {
                let s = (r + DMat3::IDENTITY) * 0.5;
                let cols = [s.x_axis, s.y_axis, s.z_axis];
                cols.into_iter()
                    .max_by(|p, q| p.length().total_cmp(&q.length()))?
                    .normalize_or_zero()
            };
            if axis == DVec3::ZERO {
                return None;
            }
            // t = (I - R) c + s * axis, with c on the plane through the origin across the axis.
            let along = axis * t.dot(axis);
            let across = t - along;
            let m =
                DMat3::IDENTITY - r + DMat3::from_cols(axis * axis.x, axis * axis.y, axis * axis.z);
            if m.determinant().abs() < 1e-12 {
                return None;
            }
            let c = m.inverse().mul_vec3(across);
            let step = Xform::translation(along / n).after(&Xform::rotation(c, axis, angle / n));
            return Some(step);
        }
        // A positive scale along the world axes about a fixed point.
        let diag = DVec3::new(r.x_axis.x, r.y_axis.y, r.z_axis.z);
        let off = r - DMat3::from_diagonal(diag);
        if off.abs_diff_eq(DMat3::ZERO, 1e-12) && diag.min_element() > 0.0 {
            // The fixed point, axis by axis (an axis scaled by 1 must not move at all).
            let axis = |s: f64, t: f64| {
                if (1.0 - s).abs() > 1e-12 {
                    t / (1.0 - s)
                } else {
                    0.0
                }
            };
            let c = DVec3::new(axis(diag.x, t.x), axis(diag.y, t.y), axis(diag.z, t.z));
            let f = DVec3::new(
                diag.x.powf(1.0 / n),
                diag.y.powf(1.0 / n),
                diag.z.powf(1.0 / n),
            );
            let step = Xform::scaling(c, f);
            // Only when the fixed point really is one (a scale about a point, no extra move).
            return step
                .power(parts)
                .to_affine()
                .abs_diff_eq(a, 1e-9)
                .then_some(step);
        }
        None
    }
}

/// The image of a stored plane: origin mapped, normal by the inverse transpose, the in-plane
/// frame re-orthonormalised from the mapped `u` (so a rigid move keeps the frame exactly).
pub fn map_plane(a: &DAffine3, p: &Plane) -> Option<Plane> {
    let m = a.matrix3;
    let normal = m
        .inverse()
        .transpose()
        .mul_vec3(p.normal)
        .normalize_or_zero();
    if normal == DVec3::ZERO || !normal.is_finite() {
        return None;
    }
    let origin = a.transform_point3(p.origin);
    let mu = m.mul_vec3(p.u);
    let mut u = (mu - normal * mu.dot(normal)).normalize_or_zero();
    if u == DVec3::ZERO {
        u = Plane::new(origin, normal).ok()?.u;
    }
    let v = normal.cross(u);
    Some(Plane {
        origin,
        normal,
        u,
        v,
    })
}

/// A transform refusal from the validator's verdict on the candidate solid.
pub(crate) fn typed(e: &crate::validate::TopologyError) -> TransformError {
    for v in &e.violations {
        match v {
            Violation::SelfIntersectingSolid(_) => return TransformError::SelfIntersects,
            Violation::NonPositiveVolume(_) => return TransformError::InsideOut,
            Violation::ZeroLengthEdge(_) => return TransformError::CollapsesEdge,
            Violation::NonPlanar { .. } => return TransformError::NotPlanar,
            _ => {}
        }
    }
    TransformError::InvalidResult(e.to_string())
}

fn reverse_rings(f: &mut PolyFace) {
    f.outer.reverse();
    for h in &mut f.holes {
        h.reverse();
    }
}

fn bounded(p: DVec3) -> bool {
    in_bounds(p.x) && in_bounds(p.y) && in_bounds(p.z)
}

impl Object {
    /// This solid under `x` (see the module docs). Face ids, user edges, the tag, the source
    /// and the display number are kept; the result is validated.
    pub fn transformed(&self, x: &Xform) -> Result<Object, TransformError> {
        x.check()?;
        let a = x.to_affine();
        let mut p = self.brep.to_polys();
        for q in &mut p.points {
            *q = a.transform_point3(*q);
            if !bounded(*q) {
                return Err(TransformError::OutOfBounds);
            }
        }
        let mirror = x.determinant() < 0.0;
        for fi in 0..p.faces.len() {
            let old = p.plane(fi).ok_or(TransformError::NotPlanar)?;
            let plane = map_plane(&a, &old).ok_or(TransformError::Singular)?;
            let f = &mut p.faces[fi];
            f.plane = Some(plane);
            if mirror {
                reverse_rings(f);
            }
        }
        self.finish(p)
    }

    /// Rebuild from edited plain data keeping ids, then validate (typed refusals).
    fn finish(&self, p: Polys) -> Result<Object, TransformError> {
        let o = self
            .rebuilt(p)
            .map_err(|e| TransformError::InvalidResult(e.to_string()))?;
        o.validate().map_err(|e| typed(&e))?;
        Ok(o)
    }

    /// Move (or scale, or turn) only the vertices at `points` (world positions, each within
    /// the merge distance of a vertex) by `x`. Returns the new solid and the persistent ids
    /// of the faces that changed shape (folded faces included). See the module docs.
    pub fn transform_geometry(
        &self,
        points: &[DVec3],
        x: &Xform,
        autofold: bool,
    ) -> Result<(Object, Vec<u64>), TransformError> {
        x.check()?;
        if points.is_empty() {
            return Err(TransformError::NothingToMove);
        }
        let a = x.to_affine();
        let mut p = self.brep.to_polys();
        let mut moving: BTreeSet<usize> = BTreeSet::new();
        for q in points {
            if !q.is_finite() {
                return Err(TransformError::BadTransform);
            }
            let found = (0..p.points.len())
                .filter(|&i| p.points[i].distance(*q) <= POINT_MERGE)
                .min_by(|&i, &j| {
                    p.points[i]
                        .distance(*q)
                        .total_cmp(&p.points[j].distance(*q))
                })
                .ok_or(TransformError::NoSuchVertex)?;
            moving.insert(found);
        }
        let old_points = p.points.clone();
        for &i in &moving {
            p.points[i] = a.transform_point3(p.points[i]);
            if !bounded(p.points[i]) {
                return Err(TransformError::OutOfBounds);
            }
        }
        let mirror = x.determinant() < 0.0;
        let mut changed: Vec<u64> = Vec::new();
        let mut folded: BTreeMap<usize, Vec<PolyFace>> = BTreeMap::new();
        for fi in 0..p.faces.len() {
            let verts: BTreeSet<usize> = p.faces[fi].rings().flatten().copied().collect();
            let n_moving = verts.iter().filter(|v| moving.contains(v)).count();
            if n_moving == 0 {
                continue;
            }
            if let Some(id) = p.faces[fi].id {
                changed.push(id);
            }
            let old = p.faces[fi].plane.ok_or(TransformError::NotPlanar)?;
            if n_moving == verts.len() {
                let plane = map_plane(&a, &old).ok_or(TransformError::Singular)?;
                p.faces[fi].plane = Some(plane);
                if mirror {
                    reverse_rings(&mut p.faces[fi]);
                }
                continue;
            }
            // Mixed: still planar (within the kernel's planarity), or fold, or refuse.
            let outer: Vec<DVec3> = p.faces[fi].outer.iter().map(|&i| p.points[i]).collect();
            let fit = Plane::fit(&outer).ok();
            let flat = fit.is_some_and(|pl| {
                pl.normal.dot(old.normal) > 0.0
                    && verts
                        .iter()
                        .all(|&v| pl.signed_distance(p.points[v]).abs() <= PLANARITY)
            });
            if let (true, Some(pl)) = (flat, fit) {
                // Keep the old frame's orientation in the new plane.
                let u = (old.u - pl.normal * old.u.dot(pl.normal)).normalize_or_zero();
                p.faces[fi].plane = Some(if u == DVec3::ZERO {
                    pl
                } else {
                    Plane {
                        origin: pl.origin,
                        normal: pl.normal,
                        u,
                        v: pl.normal.cross(u),
                    }
                });
                continue;
            }
            if !autofold {
                return Err(TransformError::WouldBend {
                    face: p.faces[fi].id.unwrap_or(0),
                });
            }
            folded.insert(fi, fold(&p.faces[fi], &old, &old_points)?);
        }
        if !folded.is_empty() {
            // Replace each folded face by its triangles (the first keeps the face's id), then
            // let the triangles that stayed coplanar merge back into one face.
            let mut faces = Vec::with_capacity(p.faces.len());
            for (fi, f) in p.faces.iter().enumerate() {
                match folded.remove(&fi) {
                    Some(tris) => faces.extend(tris),
                    None => faces.push(f.clone()),
                }
            }
            p.faces = faces;
            merge_coplanar(&mut p, true);
        }
        let o = self.finish(p)?;
        // Faces that folded: the new triangle faces too.
        let before: BTreeSet<u64> = self.brep.persistent_face_ids().into_iter().collect();
        changed.extend(
            o.brep
                .persistent_face_ids()
                .into_iter()
                .filter(|id| !before.contains(id)),
        );
        let alive: BTreeSet<u64> = o.brep.persistent_face_ids().into_iter().collect();
        changed.retain(|id| alive.contains(id));
        changed.sort_unstable();
        changed.dedup();
        Ok((o, changed))
    }
}

/// A face's triangulation in its OLD plane (so the triangles are valid), as triangle faces
/// (planes fitted from the moved points). The first triangle carries the face's id.
fn fold(f: &PolyFace, old: &Plane, old_points: &[DVec3]) -> Result<Vec<PolyFace>, TransformError> {
    let flat: Vec<usize> = f.rings().flatten().copied().collect();
    let ring2 =
        |r: &Vec<usize>| -> Vec<DVec2> { r.iter().map(|&i| old.to_local(old_points[i])).collect() };
    let holes: Vec<Vec<DVec2>> = f.holes.iter().map(ring2).collect();
    let tris = sawhorse_tessellate::triangulate(&ring2(&f.outer), &holes)
        .map_err(|e| TransformError::InvalidResult(e.to_string()))?;
    Ok(tris
        .into_iter()
        .enumerate()
        .map(|(k, t)| PolyFace {
            outer: t.iter().map(|&i| flat[i as usize]).collect(),
            holes: Vec::new(),
            plane: None,
            id: if k == 0 { f.id } else { None },
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn xform_algebra() {
        let t = Xform::translation(DVec3::new(1.0, 2.0, 3.0));
        assert_eq!(t.apply(DVec3::ZERO), DVec3::new(1.0, 2.0, 3.0));
        let r = Xform::rotation(DVec3::new(1.0, 0.0, 0.0), DVec3::Z, FRAC_PI_2);
        assert!(
            r.apply(DVec3::new(2.0, 0.0, 0.0))
                .distance(DVec3::new(1.0, 1.0, 0.0))
                < 1e-15
        );
        let s = Xform::scaling(DVec3::ONE, DVec3::new(2.0, 1.0, -1.0));
        assert_eq!(
            s.apply(DVec3::new(2.0, 2.0, 2.0)),
            DVec3::new(3.0, 2.0, 0.0)
        );
        assert!((s.determinant() + 2.0).abs() < 1e-15);
        assert!(t.after(&t.inverse()).is_identity());
        assert!(
            t.power(3)
                .apply(DVec3::ZERO)
                .distance(DVec3::new(3.0, 6.0, 9.0))
                < 1e-12
        );
        assert!(r.power(4).apply(DVec3::X).distance(DVec3::X) < 1e-14);
        assert_eq!(
            Xform::scaling(DVec3::ZERO, DVec3::new(1.0, 0.0, 1.0)).check(),
            Err(TransformError::Singular)
        );
        let mut bad = Xform::IDENTITY;
        bad.translation[0] = f64::NAN;
        assert_eq!(bad.check(), Err(TransformError::BadTransform));
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Xform>(&json).unwrap(), s);
    }

    #[test]
    fn divided_steps_compose_back() {
        let cases = [
            Xform::translation(DVec3::new(0.9144, 0.0, -0.3)),
            Xform::rotation(DVec3::new(0.5, -1.0, 0.0), DVec3::Z, FRAC_PI_2),
            Xform::rotation(DVec3::new(0.1, 0.2, 0.3), DVec3::new(1.0, 2.0, -0.5), 2.5),
            Xform::rotation(DVec3::ZERO, DVec3::X, std::f64::consts::PI),
            Xform::translation(DVec3::new(0.0, 0.0, 0.4)).after(&Xform::rotation(
                DVec3::ONE,
                DVec3::Z,
                1.0,
            )),
            Xform::scaling(DVec3::new(1.0, 2.0, 3.0), DVec3::new(8.0, 1.0, 0.125)),
        ];
        for x in cases {
            for parts in [1, 2, 4, 7] {
                let step = x
                    .divided(parts)
                    .unwrap_or_else(|| panic!("{x:?} / {parts}"));
                let back = step.power(parts).to_affine();
                assert!(
                    back.abs_diff_eq(x.to_affine(), 1e-12),
                    "{x:?} / {parts}: {back:?}"
                );
            }
        }
        let mirror = Xform::scaling(DVec3::ZERO, DVec3::new(-1.0, 1.0, 1.0));
        assert!(mirror.divided(2).is_none());
        assert!(Xform::IDENTITY.divided(0).is_none());
    }
}
