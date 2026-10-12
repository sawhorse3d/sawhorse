//! Plane snapping before (and after) a boolean (lane 4).
//!
//! Booleans are where nearly-coincident geometry hurts: two faces that should be coplanar
//! but differ by 1e-16 m on a tilted plane make the engine produce slivers, and the result
//! is refused as "not a valid solid". This pass makes nearly-coplanar faces **exactly**
//! coplanar first:
//!
//! 1. every face plane's normal is quantised: components within [`NORMAL_SNAP`] of 0 or
//!    +-1 become exact, and the normal is renormalised;
//! 2. faces (across all the given solids) whose vertices all lie within [`PLANARITY`] of
//!    one another's plane, with parallel normals (either facing), form a cluster that
//!    shares ONE canonical plane: the first member's quantised normal through the first
//!    member's centroid. Each face stores the canonical plane (its own facing, its old
//!    in-plane frame direction);
//! 3. every vertex is re-solved onto the canonical planes of the faces around it: the
//!    least-squares point of those planes nearest its old position (exact at a corner of
//!    three planes, a projection onto a line or a plane otherwise). A vertex moves only if
//!    it is off one of those planes by more than [`SNAP_FLOOR`] (rounding noise is left
//!    alone) and the move is within [`POINT_MERGE`] (inconsistent planes leave it alone);
//! 4. a vertex of a later solid within [`POINT_MERGE`] of a vertex of an earlier one takes
//!    the earlier one's exact coordinates.
//!
//! Every tolerance comes from [`crate::tolerance`]. [`set_plane_snapping`] turns the pass
//! off (the fuzzer measures the refusal rate both ways; it is on by default).

use std::cell::Cell;

use glam::{DMat3, DVec3};

use crate::brep::Polys;
use crate::plane::Plane;
use crate::tolerance::{PLANARITY, POINT_MERGE, SNAP_FLOOR};

/// Normal components this close to 0 or +-1 are snapped exactly.
pub const NORMAL_SNAP: f64 = 1e-9;

/// Two unit normals are parallel for clustering when |n1 . n2| >= this (about 0.45 urad:
/// over the 1 km working size that is under the planarity tolerance per metre of face).
pub const PARALLEL_DOT: f64 = 1.0 - 1e-13;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(true) };
}

/// Turn plane snapping on or off for this thread (default on). For measurement only: the
/// fuzzer runs the same seeds with and without it.
pub fn set_plane_snapping(on: bool) {
    ENABLED.with(|e| e.set(on));
}

pub fn plane_snapping() -> bool {
    ENABLED.with(|e| e.get())
}

/// What a pass changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SnapStats {
    /// Faces whose plane changed.
    pub planes: usize,
    /// Vertices that moved (re-solved or welded).
    pub vertices: usize,
}

fn quantise(n: DVec3) -> DVec3 {
    let q = |c: f64| {
        if c.abs() < NORMAL_SNAP {
            0.0
        } else if (c.abs() - 1.0).abs() < NORMAL_SNAP {
            c.signum()
        } else {
            c
        }
    };
    let m = DVec3::new(q(n.x), q(n.y), q(n.z));
    let len = m.length();
    if len > 0.0 { m / len } else { n }
}

struct Cluster {
    normal: DVec3,
    offset: f64,
}

/// Snap the planes and vertices of every solid in `solids` together (see the module docs).
pub fn snap_planes(solids: &mut [&mut Polys]) -> SnapStats {
    let mut stats = SnapStats::default();
    if !plane_snapping() {
        return stats;
    }
    // 1-2: clusters of nearly-coplanar faces, across every solid.
    let mut clusters: Vec<Cluster> = Vec::new();
    // (solid, face) -> (cluster, facing sign)
    let mut member: Vec<Vec<Option<(usize, f64)>>> = Vec::with_capacity(solids.len());
    for p in solids.iter() {
        let mut row = Vec::with_capacity(p.faces.len());
        for fi in 0..p.faces.len() {
            let Some(pl) = p.plane(fi) else {
                row.push(None);
                continue;
            };
            let n = quantise(pl.normal);
            let verts: Vec<DVec3> = p.faces[fi]
                .rings()
                .flatten()
                .map(|&i| p.points[i])
                .collect();
            let found = clusters.iter().position(|c| {
                c.normal.dot(n).abs() >= PARALLEL_DOT
                    && verts
                        .iter()
                        .all(|v| (c.normal.dot(*v) - c.offset).abs() <= PLANARITY)
            });
            let k = match found {
                Some(k) => k,
                None => {
                    let centroid = verts.iter().copied().sum::<DVec3>() / verts.len() as f64;
                    clusters.push(Cluster {
                        normal: n,
                        offset: n.dot(centroid),
                    });
                    clusters.len() - 1
                }
            };
            let sign = if clusters[k].normal.dot(pl.normal) >= 0.0 {
                1.0
            } else {
                -1.0
            };
            row.push(Some((k, sign)));
        }
        member.push(row);
    }
    // Store the canonical planes.
    for (s, p) in solids.iter_mut().enumerate() {
        for (fi, m) in member[s].iter().enumerate() {
            let (Some((k, sign)), Some(old)) = (*m, p.plane(fi)) else {
                continue;
            };
            let c = &clusters[k];
            let normal = c.normal * sign;
            let origin = old.origin - c.normal * (c.normal.dot(old.origin) - c.offset);
            let u = (old.u - normal * old.u.dot(normal)).normalize_or_zero();
            let plane = if u == DVec3::ZERO {
                match Plane::new(origin, normal) {
                    Ok(pl) => pl,
                    Err(_) => continue,
                }
            } else {
                Plane {
                    origin,
                    normal,
                    u,
                    v: normal.cross(u),
                }
            };
            if Some(plane) != p.faces[fi].plane {
                stats.planes += 1;
                p.faces[fi].plane = Some(plane);
            }
        }
    }
    // 3: re-solve every vertex onto the canonical planes around it.
    for (s, p) in solids.iter_mut().enumerate() {
        let mut around: Vec<Vec<usize>> = vec![Vec::new(); p.points.len()];
        for (fi, f) in p.faces.iter().enumerate() {
            if let Some((k, _)) = member[s][fi] {
                for &v in f.rings().flatten() {
                    if !around[v].contains(&k) {
                        around[v].push(k);
                    }
                }
            }
        }
        for (v, ks) in around.iter().enumerate() {
            if ks.is_empty() {
                continue;
            }
            let x0 = p.points[v];
            let planes: Vec<(DVec3, f64)> = ks
                .iter()
                .map(|&k| (clusters[k].normal, clusters[k].offset))
                .collect();
            // Already on its planes up to rounding: leave it (re-solving would only trade one
            // rounding for another and break exact constructions such as a prism built on a
            // face's own vertices).
            let residual = planes
                .iter()
                .map(|(n, d)| (n.dot(x0) - d).abs())
                .fold(0.0, f64::max);
            if residual <= SNAP_FLOOR {
                continue;
            }
            let Some(x) = solve(&planes, x0) else {
                continue;
            };
            if x.is_finite() && x != x0 && x.distance(x0) <= POINT_MERGE {
                p.points[v] = x;
                stats.vertices += 1;
            }
        }
    }
    // 4: weld later solids' vertices onto earlier solids' coincident ones.
    for later in 1..solids.len() {
        let (head, tail) = solids.split_at_mut(later);
        let q = &mut tail[0];
        for v in 0..q.points.len() {
            let x = q.points[v];
            let mut best: Option<(f64, DVec3)> = None;
            for p in head.iter() {
                for &y in &p.points {
                    let d = x.distance(y);
                    if d <= POINT_MERGE && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, y));
                    }
                }
            }
            if let Some((d, y)) = best
                && d > 0.0
            {
                q.points[v] = y;
                stats.vertices += 1;
            }
        }
    }
    stats
}

/// The point on every plane `(n, d)` (n.x = d) nearest `x0`: exact projections for one or
/// two planes, the least-squares corner for three or more independent ones; `None` when
/// the planes are degenerate (parallel pairs, a rank-deficient set).
fn solve(planes: &[(DVec3, f64)], x0: DVec3) -> Option<DVec3> {
    match planes {
        [] => None,
        [(n, d)] => Some(x0 - *n * (n.dot(x0) - d)),
        [(n1, d1), (n2, d2)] => {
            // x = x0 + a n1 + b n2 on both planes.
            let (g11, g12, g22) = (n1.dot(*n1), n1.dot(*n2), n2.dot(*n2));
            let det = g11 * g22 - g12 * g12;
            if det.abs() < 1e-12 {
                return None;
            }
            let (r1, r2) = (d1 - n1.dot(x0), d2 - n2.dot(x0));
            let a = (r1 * g22 - r2 * g12) / det;
            let b = (r2 * g11 - r1 * g12) / det;
            Some(x0 + *n1 * a + *n2 * b)
        }
        _ => {
            let mut m = DMat3::ZERO;
            let mut rhs = DVec3::ZERO;
            for (n, d) in planes {
                m += DMat3::from_cols(*n * n.x, *n * n.y, *n * n.z);
                rhs += *n * *d;
            }
            let det = m.determinant();
            if !det.is_finite() || det.abs() < 1e-9 {
                return None;
            }
            Some(m.inverse().mul_vec3(rhs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::PolyFace;

    fn unit_box(origin: DVec3, size: DVec3) -> Polys {
        let p = |x: f64, y: f64, z: f64| origin + DVec3::new(x, y, z) * size;
        let points = vec![
            p(0., 0., 0.),
            p(1., 0., 0.),
            p(1., 1., 0.),
            p(0., 1., 0.),
            p(0., 0., 1.),
            p(1., 0., 1.),
            p(1., 1., 1.),
            p(0., 1., 1.),
        ];
        let f = |v: [usize; 4]| PolyFace::new(v.to_vec(), vec![]);
        Polys {
            points,
            faces: vec![
                f([0, 3, 2, 1]),
                f([4, 5, 6, 7]),
                f([0, 1, 5, 4]),
                f([2, 3, 7, 6]),
                f([1, 2, 6, 5]),
                f([3, 0, 4, 7]),
            ],
            user_edges: Default::default(),
        }
    }

    #[test]
    fn nearly_coplanar_faces_become_exactly_coplanar() {
        let mut a = unit_box(DVec3::ZERO, DVec3::ONE);
        // b's top is 3e-10 m off a's top (drift, not rounding), one corner 2e-7 m off.
        let mut b = unit_box(
            DVec3::new(0.25, 0.25, 0.5),
            DVec3::new(0.5, 0.5, 0.5 + 6e-10),
        );
        b.points[6].z += 2e-7;
        let s = snap_planes(&mut [&mut a, &mut b]);
        assert!(s.planes > 0 && s.vertices > 0, "{s:?}");
        let top_a = a.plane(1).unwrap();
        let top_b = b.plane(1).unwrap();
        assert_eq!(top_a.normal, DVec3::Z);
        assert_eq!(top_b.normal, DVec3::Z);
        for &i in &b.faces[1].outer {
            assert_eq!(b.points[i].z, 1.0, "exactly on a's top");
        }
    }

    #[test]
    fn far_apart_planes_stay_put_and_the_switch_works() {
        let mut a = unit_box(DVec3::ZERO, DVec3::ONE);
        let before = a.clone();
        let s = snap_planes(&mut [&mut a]);
        assert_eq!(s.vertices, 0);
        assert_eq!(a.points, before.points);
        set_plane_snapping(false);
        let mut b = unit_box(DVec3::ZERO, DVec3::splat(1.0 + 1e-15));
        assert_eq!(snap_planes(&mut [&mut b]), SnapStats::default());
        set_plane_snapping(true);
    }
}
