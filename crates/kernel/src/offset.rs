//! Offset (lane 4): a face, a sketch region, or a chain of edges on a plane, offset by a
//! distance, through `cavalier_contours` (ADR-0001).
//!
//! Sign: a positive distance grows a face or region (outward), a negative one shrinks it
//! (inward). For an open chain of edges, positive is to the left of the chain's direction
//! seen from the plane's normal. Faces with holes are offset as a whole shape (the outer
//! boundary and the holes together), so an inward offset of a frame-shaped face stays a
//! frame and pieces that pinch apart come out as separate loops.
//!
//! `cavalier_contours` rounds convex corners with arcs; woodworkers expect sharp corners
//! (an offset rectangle is a rectangle), so every arc joining two straight segments is
//! replaced by the miter point where those segments' lines meet. Coordinates go to the
//! library in millimetres, so its fixed comparison epsilons (1e-5) mean 10 nm here.
//!
//! Where the result goes is decided the way Draw decides (lane 3): when every offset point
//! lies on the solid's faces on that plane, the loops are drawn on the solid (imprints, or
//! cuts where they cross edges); otherwise they become a new sketch on the plane (the
//! sketch already on it, or a new one). A region's offset goes into its own sketch.

use cavalier_contours::polyline::{PlineSource, PlineSourceMut, PlineVertex, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};
use glam::{DVec2, DVec3};

use sawhorse_tessellate::signed_area;

use crate::error::OffsetError;
use crate::plane::Plane;
use crate::tolerance::{MIN_EXTRUDE, PLANARITY, POINT_MERGE, in_bounds};

/// Millimetres per metre: the library works in mm.
const MM: f64 = 1000.0;

/// What is offset.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "of", rename_all = "snake_case")]
pub enum OffsetSource {
    /// A solid's face (persistent face id).
    Face { object: u64, face: u64 },
    /// A sketch region.
    Region { sketch: u64, region: u64 },
    /// A chain of edges (world points in order; `closed` joins the last to the first) on
    /// the plane through them across `normal`. With `object`, the result is drawn on that
    /// solid's faces on the plane when it lies on them.
    Edges {
        points: Vec<[f64; 3]>,
        closed: bool,
        normal: [f64; 3],
        object: Option<u64>,
    },
}

/// Offset closed rings (outer first counter-clockwise, holes clockwise, in a plane's 2D
/// frame) by `d` (positive grows). Returns the resulting rings with the same winding rule;
/// empty when the shape vanishes.
pub fn offset_rings(rings: &[Vec<DVec2>], d: f64) -> Vec<Vec<DVec2>> {
    let plines: Vec<Polyline<f64>> = rings
        .iter()
        .filter(|r| r.len() >= 3)
        .map(|r| to_pline(r, true))
        .collect();
    if plines.is_empty() {
        return Vec::new();
    }
    let shape = Shape::from_plines(plines);
    let out = shape.parallel_offset(-d * MM, &ShapeOffsetOptions::new());
    let mut rings: Vec<Vec<DVec2>> = Vec::new();
    for ip in out.ccw_plines.iter().chain(out.cw_plines.iter()) {
        let r = from_pline(&ip.polyline);
        if r.len() >= 3 && signed_area(&r).abs() > POINT_MERGE * POINT_MERGE {
            rings.push(r);
        }
    }
    // Outer loops (counter-clockwise) first, each followed by nothing in particular: the
    // order within each kind is the library's, which is deterministic.
    rings.sort_by_key(|r| signed_area(r) < 0.0);
    rings
}

/// Offset an open chain (2D) by `d` (positive: to the left of its direction). One or more
/// open polylines (an offset can break where the chain doubles back).
pub fn offset_chain(points: &[DVec2], d: f64) -> Vec<Vec<DVec2>> {
    if points.len() < 2 {
        return Vec::new();
    }
    let pl = to_pline(points, false);
    pl.parallel_offset(d * MM)
        .iter()
        .map(from_pline)
        .filter(|r| r.len() >= 2)
        .collect()
}

fn to_pline(points: &[DVec2], closed: bool) -> Polyline<f64> {
    let mut pl = if closed {
        Polyline::new_closed()
    } else {
        Polyline::new()
    };
    for p in points {
        pl.add(p.x * MM, p.y * MM, 0.0);
    }
    pl
}

/// A library polyline back to metres, every arc joining two straight segments replaced by
/// their lines' meeting point (a sharp corner); any other arc becomes a chord path.
fn from_pline(pl: &Polyline<f64>) -> Vec<DVec2> {
    let n = pl.vertex_count();
    let closed = pl.is_closed();
    let v = |i: usize| -> PlineVertex<f64> { pl.at(i % n) };
    let pt = |x: &PlineVertex<f64>| DVec2::new(x.x / MM, x.y / MM);
    let straight = |i: usize| -> bool { (closed || i + 1 < n) && v(i).bulge.abs() < 1e-12 };
    let mut out: Vec<DVec2> = Vec::new();
    let mut i = 0;
    while i < n {
        let cur = v(i);
        let arc = cur.bulge.abs() >= 1e-12 && (closed || i + 1 < n);
        if !arc {
            out.push(pt(&cur));
            i += 1;
            continue;
        }
        let (prev, next) = ((i + n - 1) % n, i + 1);
        let can_miter = (closed || (i > 0 && i + 2 < n)) && straight(prev) && straight(next);
        if can_miter {
            // Lines prev -> cur and next -> next+1 meet at the miter point.
            let (a0, a1) = (pt(&v(prev)), pt(&cur));
            let (b0, b1) = (pt(&v(next)), pt(&v(next + 1)));
            if let Some(m) = line_meet(a0, a1, b0, b1) {
                out.push(m);
                // The arc's end vertex is replaced by the miter too. When the arc is the
                // closing segment, its end is the first vertex, already out: drop it.
                if next >= n && !out.is_empty() {
                    out.remove(0);
                }
                i += 2;
                continue;
            }
        }
        // A chord path along the arc (8 pieces).
        let (p0, p1) = (pt(&cur), pt(&v(next)));
        let theta = 4.0 * cur.bulge.atan();
        let chord = p1 - p0;
        let len = chord.length();
        if len > 0.0 && theta.abs() > 1e-12 {
            let r = len / (2.0 * (theta / 2.0).sin());
            let mid = 0.5 * (p0 + p1);
            let nrm = DVec2::new(-chord.y, chord.x) / len;
            let h = r * (theta / 2.0).cos();
            let c = mid + nrm * h * theta.signum() * r.signum();
            let a0 = (p0 - c).y.atan2((p0 - c).x);
            for k in 0..8 {
                let a = a0 + theta * k as f64 / 8.0;
                out.push(c + DVec2::new(a.cos(), a.sin()) * r.abs());
            }
        } else {
            out.push(p0);
        }
        i += 1;
    }
    // If the first vertex was an arc's end replaced by a miter that wrapped around, drop
    // the duplicate; then clean up.
    clean(out, closed)
}

/// Where the lines through (a0, a1) and (b0, b1) meet; `None` when they are parallel.
fn line_meet(a0: DVec2, a1: DVec2, b0: DVec2, b1: DVec2) -> Option<DVec2> {
    let r = a1 - a0;
    let s = b1 - b0;
    let den = r.perp_dot(s);
    if den.abs() < 1e-18 * (r.length() * s.length()).max(1e-300) {
        return None;
    }
    let t = (b0 - a0).perp_dot(s) / den;
    Some(a0 + r * t)
}

/// Drop repeated points and points running straight through.
fn clean(mut pts: Vec<DVec2>, closed: bool) -> Vec<DVec2> {
    let mut changed = true;
    while changed && pts.len() >= 3 {
        changed = false;
        let n = pts.len();
        let mut keep = vec![true; n];
        for i in 0..n {
            if !closed && (i == 0 || i == n - 1) {
                continue;
            }
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let dup = a.distance(b) <= POINT_MERGE;
            let straight = (b - a).perp_dot(c - b).abs() <= POINT_MERGE * (c - a).length()
                && (b - a).dot(c - b) > 0.0;
            if dup || straight {
                keep[i] = false;
                changed = true;
                break;
            }
        }
        pts = pts
            .into_iter()
            .zip(keep)
            .filter_map(|(p, k)| k.then_some(p))
            .collect();
    }
    pts
}

/// The plane of a chain of world points (through the first, across `normal`), and the chain
/// in that plane's frame. Refused when a point is off the plane.
pub fn chain_on_plane(points: &[DVec3], normal: DVec3) -> Result<(Plane, Vec<DVec2>), OffsetError> {
    let first = *points.first().ok_or(OffsetError::TooFewPoints)?;
    if points
        .iter()
        .any(|p| !p.is_finite() || !in_bounds(p.abs().max_element()))
    {
        return Err(OffsetError::OutOfBounds);
    }
    let plane = Plane::new(first, normal).map_err(|_| OffsetError::NotPlanar)?;
    if points
        .iter()
        .any(|&p| plane.signed_distance(p).abs() > PLANARITY)
    {
        return Err(OffsetError::NotPlanar);
    }
    Ok((plane, points.iter().map(|&p| plane.to_local(p)).collect()))
}

/// Check an offset distance.
pub fn check_distance(d: f64) -> Result<(), OffsetError> {
    if !in_bounds(d) {
        return Err(OffsetError::OutOfBounds);
    }
    if d.abs() < MIN_EXTRUDE {
        return Err(OffsetError::ZeroDistance);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x, y),
            DVec2::new(x + s, y),
            DVec2::new(x + s, y + s),
            DVec2::new(x, y + s),
        ]
    }

    fn close(a: &[DVec2], b: &[DVec2]) -> bool {
        a.len() == b.len() && a.iter().all(|p| b.iter().any(|q| p.distance(*q) < 1e-9))
    }

    #[test]
    fn a_square_offsets_to_a_square_both_ways() {
        let s = square(0.0, 0.0, 1.0);
        let inward = offset_rings(std::slice::from_ref(&s), -0.25);
        assert_eq!(inward.len(), 1);
        assert!(close(&inward[0], &square(0.25, 0.25, 0.5)), "{inward:?}");
        let outward = offset_rings(&[s], 0.25);
        assert_eq!(outward.len(), 1);
        assert!(
            close(&outward[0], &square(-0.25, -0.25, 1.5)),
            "sharp (mitered) corners, not arcs: {outward:?}"
        );
    }

    #[test]
    fn a_frame_stays_a_frame_and_too_far_vanishes() {
        let mut hole = square(0.25, 0.25, 0.5);
        hole.reverse();
        let out = offset_rings(&[square(0.0, 0.0, 1.0), hole], -0.1);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(signed_area(&out[0]) > 0.0 && signed_area(&out[1]) < 0.0);
        assert!((signed_area(&out[0]) - 0.64).abs() < 1e-9);
        assert!((signed_area(&out[1]) + 0.49).abs() < 1e-9);
        assert!(offset_rings(&[square(0.0, 0.0, 1.0)], -0.6).is_empty());
    }

    #[test]
    fn an_l_shape_pinches_into_two_or_keeps_its_reflex_corner_sharp() {
        let l = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(2.0, 1.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(1.0, 2.0),
            DVec2::new(0.0, 2.0),
        ];
        let grown = offset_rings(std::slice::from_ref(&l), 0.1);
        assert_eq!(grown.len(), 1);
        // 6 sharp corners, area of the L grown by 0.1 all round.
        assert_eq!(grown[0].len(), 6, "{grown:?}");
        assert!((signed_area(&grown[0]) - (3.0 + 0.1 * 8.0 + 4.0 * 0.01)).abs() < 1e-9);
        let shrunk = offset_rings(&[l], -0.25);
        assert_eq!(shrunk.len(), 1);
        assert!((signed_area(&shrunk[0]) - 1.25).abs() < 1e-9);
    }

    #[test]
    fn a_grown_hole_cutting_the_shrunk_outline_keeps_a_clean_ring() {
        // A face with a small hole near one side, shrunk by more than the hole is from the
        // edge: the grown hole cuts the outline, the rest is one clean rectangle (lane-4
        // fuzzer find: the miter of the closing arc used to leave a spike).
        let outer = vec![
            DVec2::new(-0.974, -0.3607),
            DVec2::new(0.974, -0.3607),
            DVec2::new(0.974, 0.3607),
            DVec2::new(-0.974, 0.3607),
        ];
        let hole = vec![
            DVec2::new(0.2923, -0.0361),
            DVec2::new(0.4871, -0.0361),
            DVec2::new(0.4871, -0.1804),
            DVec2::new(0.2923, -0.1804),
        ];
        let out = offset_rings(&[outer, hole], -0.25);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].len(), 4, "a rectangle, no spike: {out:?}");
        assert!(!crate::validate::rings_cross(std::slice::from_ref(&out[0])));
        assert!(
            (signed_area(&out[0]) - (0.0423 + 0.724) * 0.2214).abs() < 1e-9,
            "{}",
            signed_area(&out[0])
        );
    }

    #[test]
    fn an_open_chain_offsets_to_its_left() {
        let chain = vec![DVec2::ZERO, DVec2::new(1.0, 0.0), DVec2::new(1.0, 1.0)];
        let left = offset_chain(&chain, 0.1);
        assert_eq!(left.len(), 1);
        assert!(
            close(
                &left[0],
                &[
                    DVec2::new(0.0, 0.1),
                    DVec2::new(0.9, 0.1),
                    DVec2::new(0.9, 1.0)
                ]
            ),
            "{left:?}"
        );
        let right = offset_chain(&chain, -0.1);
        assert!(
            close(
                &right[0],
                &[
                    DVec2::new(0.0, -0.1),
                    DVec2::new(1.1, -0.1),
                    DVec2::new(1.1, 1.0)
                ]
            ),
            "{right:?}"
        );
    }

    #[test]
    fn distances_and_planes_are_checked() {
        assert_eq!(check_distance(0.0), Err(OffsetError::ZeroDistance));
        assert_eq!(check_distance(f64::NAN), Err(OffsetError::OutOfBounds));
        let pts = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.1)];
        assert_eq!(
            chain_on_plane(&pts, DVec3::Z).unwrap_err(),
            OffsetError::NotPlanar
        );
        let (pl, q) = chain_on_plane(&pts[..2], DVec3::Z).unwrap();
        assert_eq!(pl.normal, DVec3::Z);
        assert_eq!(q.len(), 2);
    }
}
