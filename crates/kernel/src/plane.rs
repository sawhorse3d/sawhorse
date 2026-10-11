//! Oriented planes with an orthonormal in-plane frame (u, v, normal), right-handed:
//! `u x v = normal`. Faces store one; sketches live on one.

use glam::{DVec2, DVec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub origin: DVec3,
    pub normal: DVec3,
    pub u: DVec3,
    pub v: DVec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlaneError {
    #[error("plane normal is zero or not finite")]
    BadNormal,
    #[error("plane origin is not finite or out of bounds")]
    BadOrigin,
}

impl Plane {
    /// The ground: origin at 0, normal +Z, u = +X, v = +Y.
    pub const GROUND: Plane = Plane {
        origin: DVec3::ZERO,
        normal: DVec3::Z,
        u: DVec3::X,
        v: DVec3::Y,
    };

    /// Plane through `origin` with the given (not necessarily unit) normal. The in-plane
    /// frame is deterministic: for a +Z normal it is exactly (X, Y).
    pub fn new(origin: DVec3, normal: DVec3) -> Result<Plane, PlaneError> {
        if !origin.is_finite() || origin.abs().max_element() > crate::tolerance::MAX_COORD {
            return Err(PlaneError::BadOrigin);
        }
        let len = normal.length();
        if !len.is_finite() || len < 1e-12 {
            return Err(PlaneError::BadNormal);
        }
        let n = normal / len;
        let helper = if n.z.abs() < 0.9 { DVec3::Z } else { DVec3::Y };
        let u = helper.cross(n).normalize();
        let v = n.cross(u);
        Ok(Plane {
            origin,
            normal: n,
            u,
            v,
        })
    }

    /// Best-fit plane through a closed polygon (Newell normal through the centroid).
    /// The normal follows the polygon's winding (counter-clockwise seen from the normal).
    pub fn fit(points: &[DVec3]) -> Result<Plane, PlaneError> {
        if points.is_empty() {
            return Err(PlaneError::BadNormal);
        }
        let c = points.iter().copied().sum::<DVec3>() / points.len() as f64;
        Plane::new(c, newell(points, c))
    }

    #[inline]
    pub fn signed_distance(&self, p: DVec3) -> f64 {
        (p - self.origin).dot(self.normal)
    }

    #[inline]
    pub fn to_local(&self, p: DVec3) -> DVec2 {
        let d = p - self.origin;
        DVec2::new(d.dot(self.u), d.dot(self.v))
    }

    #[inline]
    pub fn to_world(&self, q: DVec2) -> DVec3 {
        self.origin + self.u * q.x + self.v * q.y
    }

    /// The same frame moved `d` along the normal.
    pub fn offset(&self, d: f64) -> Plane {
        Plane {
            origin: self.origin + self.normal * d,
            ..*self
        }
    }
}

/// Newell's normal of a closed polygon, relative to `centre` for precision.
/// Length is twice the polygon's area; direction follows the winding.
pub fn newell(points: &[DVec3], centre: DVec3) -> DVec3 {
    let n = points.len();
    let mut acc = DVec3::ZERO;
    for i in 0..n {
        let p = points[i] - centre;
        let q = points[(i + 1) % n] - centre;
        acc.x += (p.y - q.y) * (p.z + q.z);
        acc.y += (p.z - q.z) * (p.x + q.x);
        acc.z += (p.x - q.x) * (p.y + q.y);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_frame() {
        let p = Plane::new(DVec3::ZERO, DVec3::Z * 3.0).unwrap();
        assert_eq!(p, Plane::GROUND);
    }

    #[test]
    fn frame_is_orthonormal_and_right_handed() {
        for n in [
            DVec3::X,
            -DVec3::Z,
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::new(0.0, -1.0, 1e-3),
        ] {
            let p = Plane::new(DVec3::ONE, n).unwrap();
            assert!((p.u.length() - 1.0).abs() < 1e-12);
            assert!(p.u.dot(p.v).abs() < 1e-12);
            assert!((p.u.cross(p.v) - p.normal).length() < 1e-12);
            let q = DVec2::new(0.3, -2.0);
            assert!((p.to_local(p.to_world(q)) - q).length() < 1e-12);
        }
    }

    #[test]
    fn fit_follows_winding() {
        let ccw = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
        assert!((Plane::fit(&ccw).unwrap().normal - DVec3::Z).length() < 1e-12);
        let mut cw = ccw;
        cw.reverse();
        assert!((Plane::fit(&cw).unwrap().normal + DVec3::Z).length() < 1e-12);
        assert_eq!(
            Plane::new(DVec3::ZERO, DVec3::ZERO),
            Err(PlaneError::BadNormal)
        );
    }
}
