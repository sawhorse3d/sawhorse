//! Triangulation and 2D polygon operations for Sawhorse.
//!
//! This crate knows nothing about the B-rep: it works on plain 2D rings (a face's loops
//! projected into the face's plane frame). The kernel depends on it, not the other way
//! round, so booleans, rendering and export all share one triangulator.
//!
//! - [`triangulate`]: `earcut` (georust) with hole rings, followed by an area-deviation
//!   check. A bad triangulation is a typed [`TessellateError`], never a silent repair
//!   (ADR-0001). The `i_triangle` fallback is a later-lane addition.
//! - [`overlay`]: thin wrappers over `i_overlay` for coplanar merge / imprint tests.

pub mod overlay;

use glam::DVec2;

/// Largest accepted relative difference between the polygon area and the summed
/// triangle area. earcut reports it via [`earcut::deviation`].
pub const MAX_AREA_DEVIATION: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TessellateError {
    #[error("a ring needs at least 3 points, got {0}")]
    TooFewPoints(usize),
    #[error("ring contains a non-finite coordinate")]
    NonFinite,
    #[error("polygon has (near) zero area")]
    ZeroArea,
    #[error("triangulation area deviates by {deviation:e} (limit {MAX_AREA_DEVIATION:e})")]
    AreaDeviation { deviation: f64 },
    #[error("too many vertices for u32 indices")]
    TooLarge,
}

/// Signed area of a ring (positive = counter-clockwise).
pub fn signed_area(ring: &[DVec2]) -> f64 {
    let n = ring.len();
    let mut a = 0.0;
    for i in 0..n {
        let p = ring[i];
        let q = ring[(i + 1) % n];
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

/// Triangulate a polygon with holes.
///
/// Indices refer to the concatenation `outer ++ holes[0] ++ holes[1] ++ ...`.
/// Output triangles are counter-clockwise in the 2D frame regardless of input winding.
pub fn triangulate(
    outer: &[DVec2],
    holes: &[Vec<DVec2>],
) -> Result<Vec<[u32; 3]>, TessellateError> {
    for ring in std::iter::once(outer).chain(holes.iter().map(Vec::as_slice)) {
        if ring.len() < 3 {
            return Err(TessellateError::TooFewPoints(ring.len()));
        }
        if ring.iter().any(|p| !p.is_finite()) {
            return Err(TessellateError::NonFinite);
        }
    }
    let total: usize = outer.len() + holes.iter().map(Vec::len).sum::<usize>();
    if total > u32::MAX as usize {
        return Err(TessellateError::TooLarge);
    }
    let outer_area = signed_area(outer).abs();
    let hole_area: f64 = holes.iter().map(|h| signed_area(h).abs()).sum();
    let scale = outer
        .iter()
        .fold(0.0f64, |m, p| m.max(p.abs().max_element()))
        .max(1e-300);
    if outer_area - hole_area <= scale * scale * 1e-15 {
        return Err(TessellateError::ZeroArea);
    }

    let mut data: Vec<[f64; 2]> = Vec::with_capacity(total);
    let mut hole_indices: Vec<u32> = Vec::with_capacity(holes.len());
    data.extend(outer.iter().map(|p| [p.x, p.y]));
    for h in holes {
        hole_indices.push(data.len() as u32);
        data.extend(h.iter().map(|p| [p.x, p.y]));
    }
    let mut out: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(data.iter().copied(), &hole_indices, &mut out);
    let deviation = earcut::deviation(data.iter().copied(), &hole_indices, &out);
    if deviation.is_nan() || deviation > MAX_AREA_DEVIATION || out.is_empty() {
        return Err(TessellateError::AreaDeviation { deviation });
    }
    let mut tris = Vec::with_capacity(out.len() / 3);
    for t in out.as_chunks::<3>().0 {
        let (a, b, c) = (
            data[t[0] as usize],
            data[t[1] as usize],
            data[t[2] as usize],
        );
        let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        tris.push(if cross >= 0.0 {
            [t[0], t[1], t[2]]
        } else {
            [t[0], t[2], t[1]]
        });
    }
    Ok(tris)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x, y),
            DVec2::new(x + w, y),
            DVec2::new(x + w, y + h),
            DVec2::new(x, y + h),
        ]
    }

    fn tri_area(pts: &[DVec2], t: &[u32; 3]) -> f64 {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        0.5 * (b - a).perp_dot(c - a)
    }

    #[test]
    fn square_is_two_ccw_triangles() {
        let r = rect(0.0, 0.0, 2.0, 1.0);
        let t = triangulate(&r, &[]).unwrap();
        assert_eq!(t.len(), 2);
        let area: f64 = t.iter().map(|t| tri_area(&r, t)).sum();
        assert!((area - 2.0).abs() < 1e-12);
        assert!(t.iter().all(|t| tri_area(&r, t) > 0.0));
    }

    #[test]
    fn square_with_hole() {
        let outer = rect(0.0, 0.0, 4.0, 4.0);
        let mut hole = rect(1.0, 1.0, 2.0, 2.0);
        hole.reverse();
        let t = triangulate(&outer, std::slice::from_ref(&hole)).unwrap();
        let all: Vec<DVec2> = outer.iter().chain(hole.iter()).copied().collect();
        let area: f64 = t.iter().map(|t| tri_area(&all, t)).sum();
        assert!((area - 12.0).abs() < 1e-12, "area {area}");
        assert_eq!(t.len(), 8);
    }

    #[test]
    fn typed_failures() {
        assert_eq!(
            triangulate(&[DVec2::ZERO, DVec2::X], &[]),
            Err(TessellateError::TooFewPoints(2))
        );
        assert_eq!(
            triangulate(&[DVec2::ZERO, DVec2::X, DVec2::new(2.0, 0.0)], &[]),
            Err(TessellateError::ZeroArea)
        );
        assert_eq!(
            triangulate(&[DVec2::ZERO, DVec2::X, DVec2::new(f64::NAN, 1.0)], &[]),
            Err(TessellateError::NonFinite)
        );
    }

    proptest! {
        #[test]
        fn rect_area_preserved(x in -100.0..100.0f64, y in -100.0..100.0f64, w in 0.001..50.0f64, h in 0.001..50.0f64) {
            let r = rect(x, y, w, h);
            let t = triangulate(&r, &[]).unwrap();
            let area: f64 = t.iter().map(|t| tri_area(&r, t)).sum();
            prop_assert!((area - w * h).abs() <= 1e-9 * (w * h).max(1.0));
        }
    }
}
