//! Small f64 geometry for picking and inference: rays, axis-aligned boxes, closest points,
//! ray/triangle hits, and the pixel model that turns screen tolerances into world ones.

use glam::DVec3;

/// An eye ray: `origin + t * dir`, `dir` unit length, `t >= 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: DVec3,
    pub dir: DVec3,
}

impl Ray {
    pub fn new(origin: DVec3, dir: DVec3) -> Option<Ray> {
        let l = dir.length();
        (origin.is_finite() && l.is_finite() && l > 1e-12).then(|| Ray {
            origin,
            dir: dir / l,
        })
    }

    pub fn at(&self, t: f64) -> DVec3 {
        self.origin + self.dir * t
    }

    /// Parameter of the point on the ray closest to `p` (may be negative).
    pub fn param(&self, p: DVec3) -> f64 {
        (p - self.origin).dot(self.dir)
    }

    /// Distance from `p` to the ray's line.
    pub fn distance(&self, p: DVec3) -> f64 {
        let t = self.param(p);
        p.distance(self.at(t))
    }
}

/// How big a screen pixel is in the world. Parallel projection: a constant. Perspective:
/// proportional to the depth along the view direction.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PixelModel {
    pub ortho: bool,
    /// Parallel: metres per pixel. Perspective: metres per pixel per metre of depth.
    pub scale: f64,
    pub eye: [f64; 3],
    /// Unit view direction.
    pub forward: [f64; 3],
}

impl PixelModel {
    /// Metres per pixel at world point `p`.
    pub fn at(&self, p: DVec3) -> f64 {
        if self.ortho {
            return self.scale.max(1e-12);
        }
        let d = (p - DVec3::from_array(self.eye)).dot(DVec3::from_array(self.forward));
        self.scale.max(1e-12) * d.max(1e-6)
    }

    /// Screen-pixel distance between the ray and `p` (an approximation that is exact for
    /// points at the ray's own depth, which is where it is used).
    pub fn ray_px(&self, ray: &Ray, p: DVec3) -> f64 {
        ray.distance(p) / self.at(p)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        min: DVec3::splat(f64::INFINITY),
        max: DVec3::splat(f64::NEG_INFINITY),
    };

    pub fn of(points: &[DVec3]) -> Aabb {
        points.iter().fold(Aabb::EMPTY, |b, &p| b.grow(p))
    }

    pub fn grow(self, p: DVec3) -> Aabb {
        Aabb {
            min: self.min.min(p),
            max: self.max.max(p),
        }
    }

    pub fn union(self, o: Aabb) -> Aabb {
        Aabb {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }

    pub fn centre(&self) -> DVec3 {
        (self.min + self.max) * 0.5
    }

    pub fn inflate(&self, m: f64) -> Aabb {
        Aabb {
            min: self.min - DVec3::splat(m),
            max: self.max + DVec3::splat(m),
        }
    }

    pub fn corners(&self) -> [DVec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            DVec3::new(a.x, a.y, a.z),
            DVec3::new(b.x, a.y, a.z),
            DVec3::new(a.x, b.y, a.z),
            DVec3::new(b.x, b.y, a.z),
            DVec3::new(a.x, a.y, b.z),
            DVec3::new(b.x, a.y, b.z),
            DVec3::new(a.x, b.y, b.z),
            DVec3::new(b.x, b.y, b.z),
        ]
    }

    /// The ray (t >= 0) passes through the box: slab test.
    pub fn hit_by(&self, ray: &Ray) -> bool {
        let mut t0 = 0.0f64;
        let mut t1 = f64::INFINITY;
        for k in 0..3 {
            let (o, d, lo, hi) = (ray.origin[k], ray.dir[k], self.min[k], self.max[k]);
            if d.abs() < 1e-300 {
                if o < lo || o > hi {
                    return false;
                }
                continue;
            }
            let (mut a, mut b) = ((lo - o) / d, (hi - o) / d);
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }
            t0 = t0.max(a);
            t1 = t1.min(b);
            if t0 > t1 {
                return false;
            }
        }
        true
    }

    /// The ray passes within `px_radius` screen pixels of the box (conservative: the box is
    /// inflated by the pixel size at its farthest corner).
    pub fn near_ray(&self, ray: &Ray, px: &PixelModel, px_radius: f64) -> bool {
        let m = self
            .corners()
            .iter()
            .map(|&c| px.at(c))
            .fold(0.0f64, f64::max)
            * px_radius;
        self.inflate(m).hit_by(ray)
    }
}

/// Closest points between the ray's line and segment `a`-`b`: (ray parameter, segment
/// parameter in [0, 1]).
pub fn ray_segment(ray: &Ray, a: DVec3, b: DVec3) -> (f64, f64) {
    let d1 = ray.dir;
    let d2 = b - a;
    let r = ray.origin - a;
    let (aa, e) = (d1.dot(d1), d2.dot(d2));
    if e <= 1e-300 {
        return (ray.param(a), 0.0);
    }
    let f = d2.dot(r);
    let c = d1.dot(r);
    let bb = d1.dot(d2);
    let denom = aa * e - bb * bb;
    let mut s = if denom.abs() > 1e-300 {
        ((bb * f - c * e) / denom).max(0.0)
    } else {
        0.0
    };
    let mut t = (bb * s + f) / e;
    if t < 0.0 {
        t = 0.0;
        s = (-c / aa).max(0.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((bb - c) / aa).max(0.0);
    }
    (s, t)
}

/// Closest points of two infinite lines `p + s u` and `q + t v`; `None` when parallel.
pub fn line_line(p: DVec3, u: DVec3, q: DVec3, v: DVec3) -> Option<(f64, f64)> {
    let w = p - q;
    let (a, b, c) = (u.dot(u), u.dot(v), v.dot(v));
    let (d, e) = (u.dot(w), v.dot(w));
    let den = a * c - b * b;
    if den.abs() <= 1e-18 * a * c {
        return None;
    }
    Some(((b * e - c * d) / den, (a * e - b * d) / den))
}

/// Ray / triangle intersection (Moller-Trumbore, both sides): ray parameter.
pub fn ray_triangle(ray: &Ray, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let p = ray.dir.cross(e2);
    let det = e1.dot(p);
    let scale = e1.length() * e2.length();
    if det.abs() <= 1e-14 * scale {
        return None;
    }
    let inv = 1.0 / det;
    let s = ray.origin - a;
    let u = s.dot(p) * inv;
    let eps = 1e-12;
    if !(-eps..=1.0 + eps).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = ray.dir.dot(q) * inv;
    if v < -eps || u + v > 1.0 + eps {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t >= 0.0).then_some(t)
}

/// Where the ray meets the plane through `origin` with `normal` (t >= 0).
pub fn ray_plane(ray: &Ray, origin: DVec3, normal: DVec3) -> Option<f64> {
    let den = ray.dir.dot(normal);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (origin - ray.origin).dot(normal) / den;
    (t >= 0.0).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(x: f64, y: f64) -> Ray {
        Ray::new(DVec3::new(x, y, 10.0), DVec3::NEG_Z).unwrap()
    }

    #[test]
    fn segment_closest_points() {
        let r = down(0.5, 0.2);
        let (s, t) = ray_segment(&r, DVec3::ZERO, DVec3::X);
        assert!((t - 0.5).abs() < 1e-12);
        assert!((s - 10.0).abs() < 1e-12);
        let (_, t) = ray_segment(&r, DVec3::new(2.0, 0.0, 0.0), DVec3::new(3.0, 0.0, 0.0));
        assert_eq!(t, 0.0, "clamped to the near end");
    }

    #[test]
    fn triangle_hits_both_sides_and_misses_outside() {
        let (a, b, c) = (DVec3::ZERO, DVec3::X, DVec3::Y);
        assert!((ray_triangle(&down(0.2, 0.2), a, b, c).unwrap() - 10.0).abs() < 1e-12);
        assert!((ray_triangle(&down(0.2, 0.2), a, c, b).unwrap() - 10.0).abs() < 1e-12);
        assert!(ray_triangle(&down(0.8, 0.8), a, b, c).is_none());
    }

    #[test]
    fn boxes_near_rays_scale_with_pixels() {
        let b = Aabb::of(&[DVec3::new(1.0, 1.0, 0.0), DVec3::new(2.0, 2.0, 0.0)]);
        let px = PixelModel {
            ortho: true,
            scale: 0.01,
            eye: [0.0, 0.0, 10.0],
            forward: [0.0, 0.0, -1.0],
        };
        assert!(b.hit_by(&down(1.5, 1.5)));
        assert!(!b.hit_by(&down(0.95, 1.5)));
        assert!(
            b.near_ray(&down(0.95, 1.5), &px, 8.0),
            "5 px away, 8 px radius"
        );
        assert!(!b.near_ray(&down(0.85, 1.5), &px, 8.0));
    }

    #[test]
    fn perspective_pixels_grow_with_depth() {
        let px = PixelModel {
            ortho: false,
            scale: 0.001,
            eye: [0.0, 0.0, 10.0],
            forward: [0.0, 0.0, -1.0],
        };
        assert!((px.at(DVec3::ZERO) - 0.01).abs() < 1e-15);
        assert!((px.at(DVec3::new(0.0, 0.0, -10.0)) - 0.02).abs() < 1e-15);
    }

    #[test]
    fn skew_lines() {
        let (s, t) =
            line_line(DVec3::ZERO, DVec3::X, DVec3::new(0.5, -1.0, 1.0), DVec3::Y).unwrap();
        assert!((s - 0.5).abs() < 1e-12 && (t - 1.0).abs() < 1e-12);
        assert!(line_line(DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::X).is_none());
    }
}
