//! Follow Me (lane 4): sweep a profile (a face of a solid, or a sketch region) along a
//! path of straight segments (a polyline, open or closed). Round paths come later.
//!
//! The profile stays where it is and travels along the path's directions, SketchUp-style:
//! between two path vertices every profile point moves parallel to that segment, and at a
//! vertex it turns on the **miter plane** (the plane through the vertex halving the turn).
//! An open sweep starts with the profile itself and ends square to the last segment; a
//! closed path is a ring with no caps (the profile is first carried along the first
//! segment's line onto the miter plane at the path's start). Each segment's walls are flat
//! (two parallel edges), so the result is an exact polygon solid.
//!
//! - A sketch-region profile becomes a new solid (the region is consumed, as extruding it
//!   would).
//! - A face profile combines with its own solid: the sweep is added when the path leaves
//!   the face outward, cut away when it runs into the solid (a moulding routed along the
//!   top of a box). The boolean goes through the plane-snapping pass first
//!   ([`crate::snap`]), and a solid cut in two becomes two solids ([`crate::sever`]).
//!
//! Refusals are typed ([`SweepError`]): a path that folds back on itself, a profile lying
//! along the path, a turn so tight the sweep would pass through itself.

use glam::{DVec2, DVec3};

use crate::brep::{PolyFace, Polys};
use crate::error::SweepError;
use crate::plane::Plane;
use crate::tolerance::{POINT_MERGE, in_bounds};

/// |cos| below which the first segment counts as lying in the profile's plane.
const ALONG: f64 = 1e-6;

/// What is swept.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "of", rename_all = "snake_case")]
pub enum Profile {
    /// A solid's face (persistent face id): the sweep joins or cuts that solid.
    Face { object: u64, face: u64 },
    /// A sketch region: the sweep becomes a new solid.
    Region { sketch: u64, region: u64 },
}

/// A cleaned path: no repeated points, no points running straight through. For a closed
/// path the last point is not repeated.
pub fn clean_path(points: &[DVec3], closed: bool) -> Result<Vec<DVec3>, SweepError> {
    if points
        .iter()
        .any(|p| !(in_bounds(p.x) && in_bounds(p.y) && in_bounds(p.z)))
    {
        return Err(SweepError::OutOfBounds);
    }
    let mut pts: Vec<DVec3> = Vec::with_capacity(points.len());
    for &p in points {
        if pts
            .last()
            .is_none_or(|q: &DVec3| q.distance(p) > POINT_MERGE)
        {
            pts.push(p);
        }
    }
    if closed && pts.len() > 1 && pts[0].distance(pts[pts.len() - 1]) <= POINT_MERGE {
        pts.pop();
    }
    // Drop points running straight through (and refuse a path that doubles back).
    let mut changed = true;
    while changed {
        changed = false;
        let n = pts.len();
        if n < 3 {
            break;
        }
        for i in 0..n {
            if !closed && (i == 0 || i == n - 1) {
                continue;
            }
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (u, v) = ((b - a).normalize(), (c - b).normalize());
            if u.cross(v).length() <= 1e-12 {
                if u.dot(v) < 0.0 {
                    return Err(SweepError::PathFoldsBack);
                }
                pts.remove(i);
                changed = true;
                break;
            }
        }
    }
    let need = if closed { 3 } else { 2 };
    if pts.len() < need {
        return Err(SweepError::PathTooShort);
    }
    Ok(pts)
}

/// Sweep `rings` (world points: the outer ring counter-clockwise about `normal`, holes
/// clockwise) along `path`. Returns the closed solid as plain data (outward-facing) and
/// whether the sweep leaves along `normal` (a face profile then adds material; else cuts).
pub fn sweep_with_direction(
    rings: &[Vec<DVec3>],
    normal: DVec3,
    path: &[DVec3],
    closed: bool,
) -> Result<(Polys, bool), SweepError> {
    let mut path = clean_path(path, closed)?;
    let n = path.len();
    let segs = if closed { n } else { n - 1 };
    let dir = |path: &[DVec3], k: usize| (path[(k + 1) % n] - path[k]).normalize();
    if closed {
        // Start on the segment the profile sits against: the one running square to it (most
        // nearly along its normal) whose line passes nearest the profile's centre. The
        // profile slides along that line onto the ring's first miter plane.
        let centre = rings[0].iter().copied().sum::<DVec3>() / rings[0].len() as f64;
        let line_distance = |k: usize| {
            let d = dir(&path, k);
            let v = centre - path[k];
            (v - d * v.dot(d)).length()
        };
        let best = (0..n)
            .filter(|&k| dir(&path, k).dot(normal).abs() > ALONG)
            .min_by(|&a, &b| {
                let sa = dir(&path, a).dot(normal).abs();
                let sb = dir(&path, b).dot(normal).abs();
                // Square-ness first (within rounding), then nearness.
                if (sb - sa).abs() > 1e-9 {
                    sb.total_cmp(&sa)
                } else {
                    line_distance(a).total_cmp(&line_distance(b))
                }
            })
            .unwrap_or(0);
        path.rotate_left(best);
    }
    let d0 = dir(&path, 0);
    let outward = d0.dot(normal) > 0.0;
    if d0.dot(normal).abs() <= ALONG {
        return Err(SweepError::ProfileAlongPath);
    }
    // The plane each section lies on.
    let miter = |k: usize| -> Result<Plane, SweepError> {
        let into = dir(&path, (k + n - 1) % n);
        let out = dir(&path, k % n);
        let m = into + out;
        if m.length() < 1e-9 {
            return Err(SweepError::PathFoldsBack);
        }
        Plane::new(path[k], m).map_err(|_| SweepError::Degenerate)
    };
    let mut planes: Vec<Plane> = Vec::with_capacity(n);
    for k in 0..n {
        let p = if closed {
            miter(k)?
        } else if k == 0 {
            Plane::new(rings[0][0], normal).map_err(|_| SweepError::Degenerate)?
        } else if k == n - 1 {
            Plane::new(path[k], dir(&path, k - 1)).map_err(|_| SweepError::Degenerate)?
        } else {
            miter(k)?
        };
        planes.push(p);
    }
    // Carry a point along `d` onto `plane`.
    let onto = |q: DVec3, d: DVec3, plane: &Plane| -> Result<DVec3, SweepError> {
        let den = d.dot(plane.normal);
        if den.abs() < 1e-9 {
            return Err(SweepError::Degenerate);
        }
        Ok(q + d * ((plane.origin - q).dot(plane.normal) / den))
    };
    let first: Vec<Vec<DVec3>> = if closed {
        rings
            .iter()
            .map(|r| r.iter().map(|&q| onto(q, d0, &planes[0])).collect())
            .collect::<Result<_, _>>()?
    } else {
        rings.to_vec()
    };
    let mut sections: Vec<Vec<Vec<DVec3>>> = vec![first];
    // Sections 1..n-1 (a closed ring's last segment runs back to section 0).
    for k in 0..n - 1 {
        let d = dir(&path, k);
        let next: Vec<Vec<DVec3>> = sections[k]
            .iter()
            .map(|r| r.iter().map(|&q| onto(q, d, &planes[k + 1])).collect())
            .collect::<Result<_, _>>()?;
        // Every point must move forward along the segment: a point that would stand still
        // or go backwards means the turn is tighter than the profile is deep.
        for (ra, rb) in sections[k].iter().zip(&next) {
            if ra
                .iter()
                .zip(rb)
                .any(|(a, b)| (*b - *a).dot(d) <= POINT_MERGE)
            {
                return Err(SweepError::SelfIntersects);
            }
        }
        sections.push(next);
    }
    if closed {
        // The ring's last segment runs back to the first section.
        let d = dir(&path, n - 1);
        for (ra, rb) in sections[n - 1].iter().zip(&sections[0]) {
            if ra
                .iter()
                .zip(rb)
                .any(|(a, b)| (*b - *a).dot(d) <= POINT_MERGE)
            {
                return Err(SweepError::SelfIntersects);
            }
        }
    }
    // Plain data: every section's points in ring order.
    let mut points: Vec<DVec3> = Vec::new();
    let mut index: Vec<Vec<Vec<usize>>> = Vec::with_capacity(sections.len());
    for s in &sections {
        let mut rings_idx = Vec::with_capacity(s.len());
        for r in s {
            rings_idx.push((points.len()..points.len() + r.len()).collect::<Vec<usize>>());
            points.extend(r.iter().copied());
        }
        index.push(rings_idx);
    }
    if points
        .iter()
        .any(|p| !(in_bounds(p.x) && in_bounds(p.y) && in_bounds(p.z)))
    {
        return Err(SweepError::OutOfBounds);
    }
    let mut faces: Vec<PolyFace> = Vec::new();
    for k in 0..segs {
        let (a, b) = (&index[k], &index[(k + 1) % index.len()]);
        for (ra, rb) in a.iter().zip(b) {
            let m = ra.len();
            for i in 0..m {
                let j = (i + 1) % m;
                faces.push(PolyFace::new(vec![ra[i], ra[j], rb[j], rb[i]], vec![]));
            }
        }
    }
    if !closed {
        let rev = |r: &Vec<usize>| r.iter().rev().copied().collect::<Vec<_>>();
        let start = &index[0];
        let end = &index[n - 1];
        faces.push(PolyFace::new(
            rev(&start[0]),
            start[1..].iter().map(rev).collect(),
        ));
        faces.push(PolyFace::new(end[0].clone(), end[1..].to_vec()));
    }
    let mut p = Polys {
        points,
        faces,
        user_edges: Default::default(),
    };
    // Built for a sweep along the profile's normal; the other way round, flip it.
    if d0.dot(normal) < 0.0 {
        for f in &mut p.faces {
            f.outer.reverse();
            for h in &mut f.holes {
                h.reverse();
            }
        }
    }
    // Zero-area walls (a profile edge parallel to a segment) cannot be built.
    for f in &p.faces {
        let pts: Vec<DVec3> = f.outer.iter().map(|&i| p.points[i]).collect();
        if crate::plane::newell(&pts, pts[0]).length() <= POINT_MERGE * POINT_MERGE {
            return Err(SweepError::Degenerate);
        }
    }
    crate::faceops::merge_coplanar(&mut p, false);
    Ok((p, outward))
}

/// [`sweep_with_direction`] without the direction.
pub fn sweep(
    rings: &[Vec<DVec3>],
    normal: DVec3,
    path: &[DVec3],
    closed: bool,
) -> Result<Polys, SweepError> {
    sweep_with_direction(rings, normal, path, closed).map(|(p, _)| p)
}

/// The swept solid as an object, validated (a sweep that passes through itself is refused).
pub fn swept_object(p: &Polys) -> Result<crate::object::Object, SweepError> {
    let b = crate::brep::Brep::build(p).map_err(|_| SweepError::Degenerate)?;
    let o = crate::object::Object::from_brep(b);
    o.validate().map_err(|e| {
        if e.violations.iter().any(|v| {
            matches!(
                v,
                crate::validate::Violation::SelfIntersectingSolid(_)
                    | crate::validate::Violation::NonPositiveVolume(_)
            )
        }) {
            SweepError::SelfIntersects
        } else {
            SweepError::Degenerate
        }
    })?;
    Ok(o)
}

/// The rings of a profile in its plane's frame, wound: outer counter-clockwise, holes
/// clockwise (about the plane's normal).
pub fn wound(plane: &Plane, rings: &[Vec<DVec3>]) -> Vec<Vec<DVec3>> {
    rings
        .iter()
        .enumerate()
        .map(|(k, r)| {
            let q: Vec<DVec2> = r.iter().map(|&p| plane.to_local(p)).collect();
            let a = sawhorse_tessellate::signed_area(&q);
            let mut r = r.clone();
            if (k == 0) == (a < 0.0) {
                r.reverse();
            }
            r
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::Object;

    fn square_x0(y0: f64, z0: f64, s: f64) -> Vec<DVec3> {
        // In the plane x = 0, counter-clockwise about +x.
        vec![
            DVec3::new(0.0, y0, z0),
            DVec3::new(0.0, y0 + s, z0),
            DVec3::new(0.0, y0 + s, z0 + s),
            DVec3::new(0.0, y0, z0 + s),
        ]
    }

    fn solid(p: Polys) -> Object {
        let o = Object::from_brep(crate::brep::Brep::build(&p).unwrap());
        o.validate().unwrap();
        o
    }

    #[test]
    fn a_straight_sweep_is_an_extrusion() {
        let p = sweep(
            &[square_x0(0.0, 0.0, 0.1)],
            DVec3::X,
            &[DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0)],
            false,
        )
        .unwrap();
        let o = solid(p);
        assert!((o.volume() - 0.01).abs() < 1e-12);
        assert_eq!(o.brep().face_count(), 6);
    }

    #[test]
    fn an_l_path_is_mitered_and_keeps_the_centroid_length() {
        let p = sweep(
            &[square_x0(0.0, 0.0, 0.1)],
            DVec3::X,
            &[DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0)],
            false,
        )
        .unwrap();
        let o = solid(p);
        // Area x the centroid's path: 0.95 along red, 0.95 along green.
        assert!((o.volume() - 0.01 * 1.9).abs() < 1e-12, "{}", o.volume());
    }

    #[test]
    fn a_closed_square_path_makes_a_mitered_frame() {
        let path = [
            DVec3::ZERO,
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(1.0, 0.5, 0.0),
            DVec3::new(0.0, 0.5, 0.0),
        ];
        let p = sweep(&[square_x0(0.0, 0.0, 0.1)], DVec3::X, &path, true).unwrap();
        let o = solid(p);
        let r = o.validate().unwrap();
        assert_eq!(r.shells[0].genus, Some(1), "a ring");
        // Centroid path: the rectangle inset by 0.05, perimeter 2 (0.9 + 0.4).
        assert!((o.volume() - 0.01 * 2.6).abs() < 1e-12, "{}", o.volume());
    }

    #[test]
    fn sweeping_against_the_normal_still_faces_out() {
        let p = sweep(
            &[square_x0(0.0, 0.0, 0.1)],
            DVec3::X,
            &[DVec3::ZERO, DVec3::new(-1.0, 0.0, 0.0)],
            false,
        )
        .unwrap();
        assert!((solid(p).volume() - 0.01).abs() < 1e-12);
    }

    #[test]
    fn refusals_are_typed() {
        let sq = [square_x0(0.0, 0.0, 0.1)];
        assert_eq!(
            sweep(&sq, DVec3::X, &[DVec3::ZERO, DVec3::Y], false).unwrap_err(),
            SweepError::ProfileAlongPath
        );
        assert_eq!(
            sweep(
                &sq,
                DVec3::X,
                &[DVec3::ZERO, DVec3::X, DVec3::new(0.5, 0.0, 0.0)],
                false
            )
            .unwrap_err(),
            SweepError::PathFoldsBack
        );
        assert_eq!(
            sweep(&sq, DVec3::X, &[DVec3::ZERO], false).unwrap_err(),
            SweepError::PathTooShort
        );
        assert_eq!(
            sweep(
                &sq,
                DVec3::X,
                &[DVec3::ZERO, DVec3::new(f64::NAN, 0.0, 0.0)],
                false
            )
            .unwrap_err(),
            SweepError::OutOfBounds
        );
    }
}
