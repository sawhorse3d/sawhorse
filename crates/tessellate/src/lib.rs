//! Triangulation and 2D polygon operations for Sawhorse.
//!
//! This crate knows nothing about the B-rep: it works on plain 2D rings (a face's loops
//! projected into the face's plane frame). The kernel depends on it, not the other way
//! round, so booleans, rendering and export all share one triangulator.
//!
//! - [`triangulate`]: `earcut` (georust) with hole rings, followed by an area-deviation
//!   check. When earcut's triangles do not cover the polygon (it can mis-bridge a hole
//!   into an outer ring that has collinear vertices), our own ear clipper
//!   ([`ear_clip`]) runs instead and must pass the same check. A triangulation that
//!   still fails is a typed [`TessellateError`], never a silent repair (ADR-0001).
//! - [`overlay`]: thin wrappers over `i_overlay` for coplanar merge / imprint tests.

pub mod overlay;

use glam::DVec2;

/// Largest accepted relative difference between the polygon area and the summed
/// triangle area, measured by `area_deviation` (robust triangle areas; lane 22).
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
///
/// Summed about the ring's first point (lane 22): the same value, but its rounding scales
/// with the ring's own size, not with how far it lies from the frame's origin. Summed about
/// the origin, a 0.0044 m2 triangle 7 km out lost 1.5e-9 m2 (3e-7 of itself) to products
/// of 5,000 m coordinates.
pub fn signed_area(ring: &[DVec2]) -> f64 {
    let n = ring.len();
    let Some(&o) = ring.first() else {
        return 0.0;
    };
    let mut a = 0.0;
    for i in 1..n.saturating_sub(1) {
        let (p, q) = (ring[i] - o, ring[i + 1] - o);
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
    let bad = |out: &[u32]| -> Option<f64> {
        let deviation = area_deviation(&data, &hole_indices, out);
        (deviation.is_nan() || deviation > MAX_AREA_DEVIATION || out.is_empty())
            .then_some(deviation)
    };
    if let Some(deviation) = bad(&out) {
        // Fallback: bridge the holes and clip ears ourselves. Only polygons earcut gets
        // wrong reach it, so every face earcut handles keeps its exact triangles (and the
        // golden hashes built on them).
        let pts: Vec<DVec2> = data.iter().map(|&p| DVec2::from(p)).collect();
        out = ear_clip(&pts, outer.len(), &hole_indices)
            .map(|t| t.into_iter().flatten().collect())
            .unwrap_or_default();
        if bad(&out).is_some() {
            return Err(TessellateError::AreaDeviation { deviation });
        }
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

/// Twice the signed area of triangle abc, from the robust orientation predicate: accurate
/// to its own size wherever the points are, however thin the triangle.
fn area2(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    robust::orient2d(
        robust::Coord { x: a[0], y: a[1] },
        robust::Coord { x: b[0], y: b[1] },
        robust::Coord { x: c[0], y: c[1] },
    )
}

/// Twice a ring's signed area, as a fan of robust triangle areas from its first point.
fn ring_area2(ring: &[[f64; 2]]) -> f64 {
    (1..ring.len().saturating_sub(1))
        .map(|i| area2(ring[0], ring[i], ring[i + 1]))
        .sum()
}

/// `earcut::deviation`'s measure, |polygon area - triangles' area| / polygon area, with
/// every area summed from robust triangle areas (lane 22). earcut's own sums products of
/// absolute coordinates: a sliver 10 km long, or a region 5 km from its sketch's origin,
/// lost more to rounding than the 1e-9 the check allows, and correct triangles failed it.
/// Here each term is accurate to its own size, so the check stays as strict and measures
/// the triangulation, not the arithmetic.
fn area_deviation(data: &[[f64; 2]], hole_indices: &[u32], tris: &[u32]) -> f64 {
    let bounds: Vec<usize> = std::iter::once(0)
        .chain(hole_indices.iter().map(|&h| h as usize))
        .chain(std::iter::once(data.len()))
        .collect();
    let mut polygon = 0.0;
    for (k, w) in bounds.windows(2).enumerate() {
        if w[1] - w[0] < 3 {
            continue;
        }
        let a = ring_area2(&data[w[0]..w[1]]).abs();
        polygon += if k == 0 { a } else { -a };
    }
    let triangles: f64 = tris
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            area2(
                data[t[0] as usize],
                data[t[1] as usize],
                data[t[2] as usize],
            )
            .abs()
        })
        .sum();
    if polygon == 0.0 && triangles == 0.0 {
        return 0.0;
    }
    (polygon - triangles).abs() / polygon.abs()
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

fn cross(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Ear clipping for a polygon with holes, the fallback behind [`triangulate`].
///
/// `pts` is `outer ++ holes...`; `hole_starts` gives where each hole begins. The outer ring
/// is taken counter-clockwise and the holes clockwise (whatever their input winding). Each
/// hole, rightmost first, is joined to the ring by a bridge from its rightmost vertex to a
/// visible ring vertex: cast a ray in +x, take the nearest edge it hits and that edge's
/// right end, and if a reflex vertex lies inside the triangle (hole vertex, hit point,
/// end) take the reflex vertex nearest the ray's direction instead. The bridged ring is a
/// single (weakly simple) polygon; ears are clipped from it, and a vertex collinear with
/// its neighbours is dropped without a triangle (the caller's `cover_all_vertices` puts
/// it back on the edge it lies on). Returns `None` when no ear can be found.
pub fn ear_clip(pts: &[DVec2], outer_len: usize, hole_starts: &[u32]) -> Option<Vec<[u32; 3]>> {
    let ring_area = |r: &[u32]| {
        let q: Vec<DVec2> = r.iter().map(|&i| pts[i as usize]).collect();
        signed_area(&q)
    };
    let mut poly: Vec<u32> = (0..outer_len as u32).collect();
    if ring_area(&poly) < 0.0 {
        poly.reverse();
    }
    let mut holes: Vec<Vec<u32>> = Vec::with_capacity(hole_starts.len());
    for (k, &start) in hole_starts.iter().enumerate() {
        let end = hole_starts.get(k + 1).map_or(pts.len() as u32, |&e| e);
        let mut h: Vec<u32> = (start..end).collect();
        if ring_area(&h) > 0.0 {
            h.reverse();
        }
        holes.push(h);
    }
    let rightmost = |h: &[u32]| -> usize {
        let mut best = 0;
        for (k, &i) in h.iter().enumerate() {
            let (p, b) = (pts[i as usize], pts[h[best] as usize]);
            if p.x > b.x || (p.x == b.x && p.y < b.y) {
                best = k;
            }
        }
        best
    };
    holes.sort_by(|a, b| {
        let (pa, pb) = (pts[a[rightmost(a)] as usize], pts[b[rightmost(b)] as usize]);
        pb.x.total_cmp(&pa.x).then(pa.y.total_cmp(&pb.y))
    });
    for h in &holes {
        let r = rightmost(h);
        let m = pts[h[r] as usize];
        let k = bridge_target(pts, &poly, m)?;
        let mut next = Vec::with_capacity(poly.len() + h.len() + 2);
        next.extend_from_slice(&poly[..=k]);
        next.extend(h[r..].iter().chain(&h[..r]).copied());
        next.push(h[r]);
        next.push(poly[k]);
        next.extend_from_slice(&poly[k + 1..]);
        poly = next;
    }

    let mut tris = Vec::with_capacity(poly.len());
    while poly.len() > 3 {
        let n = poly.len();
        let mut clipped = false;
        for i in 0..n {
            let (ia, ib, ic) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
            let (a, b, c) = (pts[ia as usize], pts[ib as usize], pts[ic as usize]);
            let turn = cross(b - a, c - b);
            if turn == 0.0 {
                poly.remove(i);
                clipped = true;
                break;
            }
            if turn < 0.0 {
                continue;
            }
            let blocked = poly.iter().enumerate().any(|(j, &iq)| {
                if j == i || j == (i + n - 1) % n || j == (i + 1) % n {
                    return false;
                }
                let q = pts[iq as usize];
                if q == a || q == b || q == c {
                    return false;
                }
                cross(b - a, q - a) >= 0.0
                    && cross(c - b, q - b) >= 0.0
                    && cross(a - c, q - c) >= 0.0
            });
            if !blocked {
                tris.push([ia, ib, ic]);
                poly.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            return None;
        }
    }
    if poly.len() == 3 {
        let (a, b, c) = (
            pts[poly[0] as usize],
            pts[poly[1] as usize],
            pts[poly[2] as usize],
        );
        if cross(b - a, c - b) != 0.0 {
            tris.push([poly[0], poly[1], poly[2]]);
        }
    }
    Some(tris)
}

/// Position in the counter-clockwise ring `poly` of a vertex visible from `m` (a hole's
/// rightmost vertex), for the bridge [`ear_clip`] cuts.
fn bridge_target(pts: &[DVec2], poly: &[u32], m: DVec2) -> Option<usize> {
    let n = poly.len();
    let at = |k: usize| pts[poly[k % n] as usize];
    // Nearest crossing of the ray y = m.y, x >= m.x with a ring edge.
    let mut hit: Option<(f64, usize)> = None;
    for k in 0..n {
        let (a, b) = (at(k), at(k + 1));
        if (a.y > m.y) == (b.y > m.y) && a.y != m.y && b.y != m.y {
            continue;
        }
        let x = if a.y == b.y {
            if a.y != m.y {
                continue;
            }
            a.x.min(b.x)
        } else {
            a.x + (m.y - a.y) / (b.y - a.y) * (b.x - a.x)
        };
        if x < m.x {
            continue;
        }
        if hit.is_none_or(|(hx, _)| x < hx) {
            hit = Some((x, k));
        }
    }
    let (hx, k) = hit?;
    let hp = DVec2::new(hx, m.y);
    // The hit edge's end with the larger x (or the vertex the ray hit exactly).
    let (a, b) = (at(k), at(k + 1));
    let mut target = if a == hp {
        k
    } else if b == hp {
        (k + 1) % n
    } else if a.x >= b.x {
        k
    } else {
        (k + 1) % n
    };
    let p = at(target);
    if p != hp {
        // A reflex vertex inside the triangle (m, hit, p) would block the bridge: take the
        // one closest in angle to the ray (then the nearest) instead.
        let (t0, t1, t2) = if cross(hp - m, p - m) >= 0.0 {
            (m, hp, p)
        } else {
            (m, p, hp)
        };
        let mut best: Option<(f64, f64, usize)> = None;
        for j in 0..n {
            let q = at(j);
            if j == target || q == p || q == m {
                continue;
            }
            let reflex = cross(q - at(j + n - 1), at(j + 1) - q) < 0.0;
            if !reflex {
                continue;
            }
            let inside = cross(t1 - t0, q - t0) >= 0.0
                && cross(t2 - t1, q - t1) >= 0.0
                && cross(t0 - t2, q - t2) >= 0.0;
            if !inside {
                continue;
            }
            let d = q - m;
            let len = d.length();
            if len == 0.0 {
                continue;
            }
            let cos = d.x / len;
            if best.is_none_or(|(bc, bl, _)| cos > bc || (cos == bc && len < bl)) {
                best = Some((cos, len, j));
            }
        }
        if let Some((_, _, j)) = best {
            target = j;
        }
    }
    // A vertex can appear twice (an earlier bridge): use the occurrence whose interior
    // wedge holds the direction back to `m`.
    let id = poly[target];
    let occurrences: Vec<usize> = (0..n).filter(|&j| poly[j] == id).collect();
    if occurrences.len() > 1 {
        let d = m - at(target);
        for &j in &occurrences {
            let (prev, here, next) = (at(j + n - 1), at(j), at(j + 1));
            let (e1, e2) = (next - here, prev - here);
            let inside = if cross(e1, e2) >= 0.0 {
                cross(e1, d) > 0.0 && cross(d, e2) > 0.0
            } else {
                !(cross(e2, d) >= 0.0 && cross(d, e1) >= 0.0)
            };
            if inside {
                return Some(j);
            }
        }
    }
    Some(target)
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

    /// Every vertex used, every triangle counter-clockwise, area equal to outer minus holes.
    fn assert_covers(outer: &[DVec2], holes: &[Vec<DVec2>], t: &[[u32; 3]]) {
        let all: Vec<DVec2> = outer
            .iter()
            .chain(holes.iter().flatten())
            .copied()
            .collect();
        for i in 0..all.len() as u32 {
            assert!(t.iter().any(|t| t.contains(&i)), "vertex {i} unused: {t:?}");
        }
        assert!(
            t.iter().all(|t| tri_area(&all, t) > 0.0),
            "a clockwise or flat triangle"
        );
        let want =
            signed_area(outer).abs() - holes.iter().map(|h| signed_area(h).abs()).sum::<f64>();
        let got: f64 = t.iter().map(|t| tri_area(&all, t)).sum();
        assert!(
            (got - want).abs() <= 1e-12 * want.abs().max(1.0),
            "area {got} != {want}"
        );
    }

    #[test]
    fn earcut_misbridge_falls_back_to_ear_clipping() {
        // Fuzz find (CI, 2026-10-11, tests/repro/imprint-near-tolerance-non-positive-volume):
        // a side face with a notch and three collinear vertices on its right edge, plus an
        // imprinted hole well inside it. earcut bridged the hole wrongly and its triangles
        // missed 1.9% of the area, so the solid measured a volume of 0 and the imprint was
        // rejected as a kernel bug.
        let outer = vec![
            DVec2::new(0.75, -0.022136401340476754),
            DVec2::new(0.75, 0.05),
            DVec2::new(-0.7499999999999999, 0.05),
            DVec2::new(-0.7499999999999999, -0.05),
            DVec2::new(0.75, -0.05),
            DVec2::new(0.75, -0.03218185117291716),
            DVec2::new(0.75, -0.02963640134047675),
            DVec2::new(0.48731172429706215, -0.02963640134047675),
            DVec2::new(0.48731172429706215, -0.02213640134047675),
        ];
        let hole = vec![
            DVec2::new(0.3750000000000002, 0.02500000000000001),
            DVec2::new(0.4500000000000002, 0.02500000000000001),
            DVec2::new(0.4500000000000002, 0.0050000000000000044),
            DVec2::new(0.3750000000000002, 0.0050000000000000044),
        ];
        let holes = vec![hole];
        let t = triangulate(&outer, &holes).unwrap();
        assert_covers(&outer, &holes, &t);
    }

    #[test]
    fn ear_clip_alone_handles_holes_notches_and_collinear_points() {
        // An L-shaped ring with collinear points on two edges and two holes.
        let outer = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(4.0, 0.0),
            DVec2::new(4.0, 1.0),
            DVec2::new(4.0, 2.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(2.0, 4.0),
            DVec2::new(0.0, 4.0),
        ];
        let mut h1 = rect(0.5, 0.5, 1.0, 1.0);
        h1.reverse();
        let h2 = rect(2.5, 0.5, 1.0, 1.0); // counter-clockwise on purpose: ear_clip rewinds it
        let holes = vec![h1, h2];
        let all: Vec<DVec2> = outer
            .iter()
            .chain(holes.iter().flatten())
            .copied()
            .collect();
        let starts = [outer.len() as u32, (outer.len() + 4) as u32];
        let t = ear_clip(&all, outer.len(), &starts).unwrap();
        let area: f64 = t.iter().map(|t| tri_area(&all, t).abs()).sum();
        assert!((area - 10.0).abs() < 1e-12, "area {area}");
        // Through the public entry point every vertex comes back.
        let t = triangulate(&outer, &holes).unwrap();
        assert_covers(&outer, &holes, &t);
    }

    proptest! {
        /// The fallback agrees with the exact area on a rectangle with a rectangular hole and
        /// extra collinear points on the outer ring's right edge (the shape earcut missed).
        #[test]
        fn ear_clip_area_with_hole_and_collinear_edge(
            w in 0.5..10.0f64, h in 0.5..10.0f64,
            fx in 0.1..0.6f64, fy in 0.1..0.6f64, fw in 0.05..0.3f64, fh in 0.05..0.3f64,
            splits in proptest::collection::vec(0.01..0.99f64, 0..5),
        ) {
            let mut ys: Vec<f64> = splits.iter().map(|s| s * h).collect();
            ys.sort_by(f64::total_cmp);
            ys.dedup();
            let mut outer = vec![DVec2::new(0.0, 0.0), DVec2::new(w, 0.0)];
            outer.extend(ys.iter().map(|&y| DVec2::new(w, y)));
            outer.push(DVec2::new(w, h));
            outer.push(DVec2::new(0.0, h));
            let mut hole = rect(fx * w, fy * h, fw * w, fh * h);
            hole.reverse();
            let all: Vec<DVec2> = outer.iter().chain(hole.iter()).copied().collect();
            let t = ear_clip(&all, outer.len(), &[outer.len() as u32]).unwrap();
            let area: f64 = t.iter().map(|t| tri_area(&all, t).abs()).sum();
            let want = w * h - fw * w * fh * h;
            prop_assert!((area - want).abs() <= 1e-9 * want, "area {} != {}", area, want);
        }
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
