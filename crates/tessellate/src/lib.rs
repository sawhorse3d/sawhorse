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
    cover_all_vertices(&data, &mut tris);
    Ok(tris)
}

/// earcut drops points that are exactly collinear with their neighbours. A B-rep face keeps
/// such points (an edge split where a neighbouring face attaches), and the neighbour's
/// triangles use them, so dropping them would leave T-junctions: a mesh that is not
/// manifold (booleans refuse it) and an STL that is not watertight. Re-insert every unused
/// point by splitting the triangle whose edge it lies on.
fn cover_all_vertices(data: &[[f64; 2]], tris: &mut Vec<[u32; 3]>) {
    let mut used = vec![false; data.len()];
    for t in tris.iter() {
        for &i in t {
            used[i as usize] = true;
        }
    }
    for (v, _) in used.iter().enumerate().filter(|(_, u)| !**u) {
        let p = DVec2::from(data[v]);
        // The triangle edge (a, b) that `p` sits on: smallest distance, projecting inside.
        let mut best: Option<(f64, usize, usize)> = None;
        for (ti, t) in tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (
                    DVec2::from(data[t[k] as usize]),
                    DVec2::from(data[t[(k + 1) % 3] as usize]),
                );
                let ab = b - a;
                let len2 = ab.length_squared();
                if len2 == 0.0 {
                    continue;
                }
                let s = (p - a).dot(ab) / len2;
                if s <= 0.0 || s >= 1.0 {
                    continue;
                }
                let d = (a + ab * s).distance(p);
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, ti, k));
                }
            }
        }
        if let Some((_, ti, k)) = best {
            let t = tris[ti];
            let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            tris[ti] = [a, v as u32, c];
            tris.push([v as u32, b, c]);
        }
    }
}

/// A point strictly inside a polygon with holes (even-odd), as far from the boundary as a
/// cheap scanline search finds, or `None` for an empty or degenerate polygon.
///
/// Scanlines run halfway between consecutive distinct vertex heights (so no vertex lies on
/// one); each yields interior intervals between pairs of crossings. The midpoint of the
/// interval with the best clearance wins. The point sits at the golden-section fraction of
/// its interval and gap rather than the exact middle, so it rarely lands where a user later
/// draws through a midpoint (which would make it a boundary point). Deterministic.
pub fn interior_point(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Option<DVec2> {
    let rings: Vec<&[DVec2]> = std::iter::once(outer)
        .chain(holes.iter().map(Vec::as_slice))
        .collect();
    if outer.len() < 3 {
        return None;
    }
    let mut ys: Vec<f64> = rings.iter().flat_map(|r| r.iter().map(|p| p.y)).collect();
    if ys.iter().any(|y| !y.is_finite()) {
        return None;
    }
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let mut gaps: Vec<(f64, f64)> = ys.windows(2).map(|w| (w[1] - w[0], w[0])).collect();
    // Widest gaps first; a handful of scanlines is plenty.
    gaps.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.total_cmp(&b.1)));
    gaps.truncate(16);
    let mut best: Option<(f64, DVec2)> = None;
    const GOLDEN: f64 = 0.381_966_011_250_105_1;
    for (gap, lo) in gaps {
        if gap <= 0.0 {
            continue;
        }
        let y = lo + gap * GOLDEN;
        let mut xs: Vec<f64> = Vec::new();
        for r in &rings {
            let n = r.len();
            for i in 0..n {
                let (a, b) = (r[i], r[(i + 1) % n]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let w = pair[1] - pair[0];
            if w <= 0.0 {
                continue;
            }
            let score = (GOLDEN * w).min(GOLDEN * gap);
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, DVec2::new(pair[0] + w * GOLDEN, y)));
            }
        }
    }
    best.map(|(_, p)| p)
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

    #[test]
    fn collinear_points_are_kept() {
        // A face edge split where a neighbour attaches: (1, 0) and (2, 0) sit on the bottom edge.
        let r = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(3.0, 0.0),
            DVec2::new(3.0, 1.0),
            DVec2::new(0.0, 1.0),
        ];
        let t = triangulate(&r, &[]).unwrap();
        for i in 0..r.len() as u32 {
            assert!(t.iter().any(|t| t.contains(&i)), "vertex {i} unused: {t:?}");
        }
        let area: f64 = t.iter().map(|t| tri_area(&r, t)).sum();
        assert!((area - 3.0).abs() < 1e-12);
        assert!(t.iter().all(|t| tri_area(&r, t) > 0.0));
    }

    #[test]
    fn interior_point_avoids_holes_and_notches() {
        let outer = rect(0.0, 0.0, 4.0, 4.0);
        let mut hole = rect(1.0, 1.0, 2.0, 2.0);
        hole.reverse();
        let p = interior_point(&outer, std::slice::from_ref(&hole)).unwrap();
        let inside_outer = p.x > 0.0 && p.x < 4.0 && p.y > 0.0 && p.y < 4.0;
        let inside_hole = p.x > 1.0 && p.x < 3.0 && p.y > 1.0 && p.y < 3.0;
        assert!(inside_outer && !inside_hole, "{p}");
        // U shape: the notch is not inside.
        let u = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(3.0, 0.0),
            DVec2::new(3.0, 3.0),
            DVec2::new(2.0, 3.0),
            DVec2::new(2.0, 1.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(1.0, 3.0),
            DVec2::new(0.0, 3.0),
        ];
        let p = interior_point(&u, &[]).unwrap();
        assert!(
            !(p.x > 1.0 && p.x < 2.0 && p.y > 1.0),
            "{p} is in the notch"
        );
        assert!(interior_point(&[DVec2::ZERO, DVec2::X], &[]).is_none());
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
