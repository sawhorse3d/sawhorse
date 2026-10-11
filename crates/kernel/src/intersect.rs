//! Global 3D self-intersection test for a solid (ADR-0001: the validator rejects a solid
//! whose boundary crosses or touches itself anywhere except where faces share edges and
//! vertices).
//!
//! The per-face checks in [`crate::validate`] see one face at a time; push/pull can drive
//! a face through, or onto, another part of the same solid, which only a global test
//! catches. Three checks, each over B-rep features (not tessellation artefacts):
//!
//! 1. **Edge through face** (exact): an edge must not meet a face it does not belong to.
//!    When the edge shares one end with the face, the first `POINT_MERGE` of it is
//!    exempt, so an edge leaving a shared vertex is fine but one folding back through the
//!    face is caught. Tests are exact (Shewchuk orient3d/orient2d via `robust`) against the
//!    face's triangles.
//! 2. **Vertex on face** (tolerance): a vertex must stay more than `POINT_MERGE` from every
//!    face it does not belong to.
//! 3. **Edge on edge** (tolerance): edges that share no vertex must stay more than
//!    `POINT_MERGE` apart.
//!
//! Exact predicates alone would flag the 1e-16 m wobble of a vertex inserted on an edge
//! (an f64 point is rarely exactly on a tilted line); working on B-rep features with the
//! merge distance for contact makes the test exact where it matters (penetration) and
//! tolerant where the kernel already is (coincidence).

use std::collections::BTreeSet;

use glam::{DVec2, DVec3};

use crate::brep::Brep;
use crate::sketch::point_segment_distance;
use crate::tolerance::POINT_MERGE;
use crate::validate::point_in_ring;

fn c3(p: DVec3) -> robust::Coord3D<f64> {
    robust::Coord3D {
        x: p.x,
        y: p.y,
        z: p.z,
    }
}

fn o3(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> f64 {
    robust::orient3d(c3(a), c3(b), c3(c), c3(d))
}

fn o2(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    robust::orient2d(
        robust::Coord { x: a.x, y: a.y },
        robust::Coord { x: b.x, y: b.y },
        robust::Coord { x: c.x, y: c.y },
    )
}

/// Exact projection to 2D by dropping the axis the normal points along most.
fn projector(a: DVec3, b: DVec3, c: DVec3) -> impl Fn(DVec3) -> DVec2 {
    let n = (b - a).cross(c - a).abs();
    let drop = if n.x >= n.y && n.x >= n.z {
        0
    } else if n.y >= n.z {
        1
    } else {
        2
    };
    move |p: DVec3| match drop {
        0 => DVec2::new(p.y, p.z),
        1 => DVec2::new(p.z, p.x),
        _ => DVec2::new(p.x, p.y),
    }
}

fn on_seg2(a: DVec2, b: DVec2, p: DVec2) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Closed 2D segments intersect.
fn seg_seg2(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let (d1, d2, d3, d4) = (o2(a, b, c), o2(a, b, d), o2(c, d, a), o2(c, d, b));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    (d1 == 0.0 && on_seg2(a, b, c))
        || (d2 == 0.0 && on_seg2(a, b, d))
        || (d3 == 0.0 && on_seg2(c, d, a))
        || (d4 == 0.0 && on_seg2(c, d, b))
}

/// Closed 2D point in closed triangle.
fn point_in_tri2(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
    let (s1, s2, s3) = (o2(a, b, p), o2(b, c, p), o2(c, a, p));
    let neg = s1 < 0.0 || s2 < 0.0 || s3 < 0.0;
    let pos = s1 > 0.0 || s2 > 0.0 || s3 > 0.0;
    !(neg && pos)
}

/// Closed segment pq meets closed triangle abc (exact). Degenerate triangles never match.
pub(crate) fn seg_tri(p: DVec3, q: DVec3, a: DVec3, b: DVec3, c: DVec3) -> bool {
    let sp = o3(a, b, c, p);
    let sq = o3(a, b, c, q);
    if (sp > 0.0 && sq > 0.0) || (sp < 0.0 && sq < 0.0) {
        return false;
    }
    if sp == 0.0 && sq == 0.0 {
        let f = projector(a, b, c);
        let (p2, q2, a2, b2, c2) = (f(p), f(q), f(a), f(b), f(c));
        if o2(a2, b2, c2) == 0.0 {
            return false;
        }
        return point_in_tri2(p2, a2, b2, c2)
            || point_in_tri2(q2, a2, b2, c2)
            || seg_seg2(p2, q2, a2, b2)
            || seg_seg2(p2, q2, b2, c2)
            || seg_seg2(p2, q2, c2, a2);
    }
    let (s1, s2, s3) = (o3(p, q, a, b), o3(p, q, b, c), o3(p, q, c, a));
    if s1 == 0.0 && s2 == 0.0 && s3 == 0.0 {
        return false;
    }
    let neg = s1 < 0.0 || s2 < 0.0 || s3 < 0.0;
    let pos = s1 > 0.0 || s2 > 0.0 || s3 > 0.0;
    !(neg && pos)
}

/// Distance between segments ab and cd.
fn seg_seg_distance(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> f64 {
    let (u, v, w) = (b - a, d - c, a - c);
    let (uu, uv, vv, uw, vw) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w), v.dot(w));
    let den = uu * vv - uv * uv;
    let mut s = if den > 1e-300 {
        ((uv * vw - vv * uw) / den).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut t = if vv > 0.0 { (uv * s + vw) / vv } else { 0.0 };
    if t < 0.0 {
        t = 0.0;
        s = if uu > 0.0 {
            (-uw / uu).clamp(0.0, 1.0)
        } else {
            0.0
        };
    } else if t > 1.0 {
        t = 1.0;
        s = if uu > 0.0 {
            ((uv - uw) / uu).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    (a + u * s).distance(c + v * t)
}

/// What touched what (vertex / edge ends are vertex ordinals, faces are face ordinals).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfHit {
    EdgeThroughFace {
        edge: (usize, usize),
        face: usize,
    },
    VertexOnFace {
        vertex: usize,
        face: usize,
    },
    EdgesTouch {
        a: (usize, usize),
        b: (usize, usize),
    },
}

struct FaceData {
    verts: BTreeSet<usize>,
    tris: Vec<[DVec3; 3]>,
    rings2: Vec<Vec<DVec2>>,
    plane: crate::plane::Plane,
    lo: DVec3,
    hi: DVec3,
}

fn overlap(alo: DVec3, ahi: DVec3, blo: DVec3, bhi: DVec3, pad: f64) -> bool {
    alo.x <= bhi.x + pad
        && blo.x <= ahi.x + pad
        && alo.y <= bhi.y + pad
        && blo.y <= ahi.y + pad
        && alo.z <= bhi.z + pad
        && blo.z <= ahi.z + pad
}

/// The first self-contact of the solid's boundary, or `None`.
pub fn self_intersection_detail(b: &Brep) -> Option<SelfHit> {
    let vindex: std::collections::BTreeMap<_, usize> =
        b.vertices().enumerate().map(|(i, (k, _))| (k, i)).collect();
    let pos: Vec<DVec3> = b.vertices().map(|(_, v)| v.pos).collect();
    let mut faces: Vec<FaceData> = Vec::new();
    for (_, f) in b.faces() {
        let loops: Vec<Vec<usize>> = f
            .loops()
            .map(|l| b.loop_vertices(l).iter().map(|v| vindex[v]).collect())
            .collect();
        let flat: Vec<usize> = loops.iter().flatten().copied().collect();
        let rings2: Vec<Vec<DVec2>> = loops
            .iter()
            .map(|l| l.iter().map(|&i| f.plane.to_local(pos[i])).collect())
            .collect();
        let tris = sawhorse_tessellate::triangulate(&rings2[0], &rings2[1..])
            .map(|ts| {
                ts.into_iter()
                    .map(|t| t.map(|k| pos[flat[k as usize]]))
                    .collect()
            })
            .unwrap_or_default();
        let lo = flat
            .iter()
            .fold(DVec3::splat(f64::INFINITY), |m, &i| m.min(pos[i]));
        let hi = flat
            .iter()
            .fold(DVec3::splat(f64::NEG_INFINITY), |m, &i| m.max(pos[i]));
        faces.push(FaceData {
            verts: flat.iter().copied().collect(),
            tris,
            rings2,
            plane: f.plane,
            lo,
            hi,
        });
    }
    let edges: Vec<(usize, usize)> = b
        .edges()
        .map(|(e, _)| {
            let he = b.edge(e).expect("edge").he;
            let h = b.half_edge(he).expect("half-edge");
            (vindex[&h.origin], vindex[&b.dest(he)])
        })
        .collect();

    // 1. Edges through faces they do not belong to.
    for &(i, j) in &edges {
        let (p, q) = (pos[i], pos[j]);
        let (lo, hi) = (p.min(q), p.max(q));
        for (fi, f) in faces.iter().enumerate() {
            if !overlap(lo, hi, f.lo, f.hi, 0.0) {
                continue;
            }
            let (si, sj) = (f.verts.contains(&i), f.verts.contains(&j));
            let (a, z) = match (si, sj) {
                (true, true) => continue,
                (false, false) => (p, q),
                (true, false) | (false, true) => {
                    let (from, to) = if si { (p, q) } else { (q, p) };
                    let len = from.distance(to);
                    if len <= 2.0 * POINT_MERGE {
                        continue;
                    }
                    (from + (to - from) * (POINT_MERGE / len), to)
                }
            };
            if f.tris.iter().any(|t| seg_tri(a, z, t[0], t[1], t[2])) {
                return Some(SelfHit::EdgeThroughFace {
                    edge: (i, j),
                    face: fi,
                });
            }
        }
    }

    // 2. Vertices touching faces they do not belong to.
    for (vi, &v) in pos.iter().enumerate() {
        for (fi, f) in faces.iter().enumerate() {
            if f.verts.contains(&vi) || !overlap(v, v, f.lo, f.hi, POINT_MERGE) {
                continue;
            }
            let h = f.plane.signed_distance(v);
            if h.abs() > POINT_MERGE {
                continue;
            }
            let q = f.plane.to_local(v);
            let inside = point_in_ring(q, &f.rings2[0])
                && !f.rings2[1..].iter().any(|r| point_in_ring(q, r));
            let edge_d = f
                .rings2
                .iter()
                .flat_map(|r| (0..r.len()).map(move |k| (r[k], r[(k + 1) % r.len()])))
                .map(|(a, b)| point_segment_distance(q, a, b))
                .fold(f64::INFINITY, f64::min);
            let d = if inside {
                h.abs()
            } else {
                (h * h + edge_d * edge_d).sqrt()
            };
            if d <= POINT_MERGE {
                return Some(SelfHit::VertexOnFace {
                    vertex: vi,
                    face: fi,
                });
            }
        }
    }

    // 3. Edges sharing no vertex must keep their distance.
    let lo_x = |k: usize| pos[edges[k].0].x.min(pos[edges[k].1].x);
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by(|&a, &b| lo_x(a).total_cmp(&lo_x(b)));
    for (n, &x) in order.iter().enumerate() {
        let (a, b) = edges[x];
        let (pa, pb) = (pos[a], pos[b]);
        let hi_x = pa.x.max(pb.x) + POINT_MERGE;
        for &y in &order[n + 1..] {
            if lo_x(y) > hi_x {
                break;
            }
            let (c, d) = edges[y];
            if a == c || a == d || b == c || b == d {
                continue;
            }
            let (pc, pd) = (pos[c], pos[d]);
            if !overlap(pa.min(pb), pa.max(pb), pc.min(pd), pc.max(pd), POINT_MERGE) {
                continue;
            }
            if seg_seg_distance(pa, pb, pc, pd) <= POINT_MERGE {
                return Some(SelfHit::EdgesTouch {
                    a: (a, b),
                    b: (c, d),
                });
            }
        }
    }
    None
}

/// True when the solid's boundary touches or crosses itself (see the module docs).
pub fn self_intersection(b: &Brep) -> Option<SelfHit> {
    self_intersection_detail(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::{PolyFace, Polys};

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn cube_polys(o: DVec3, s: f64) -> Polys {
        let p = |x: f64, y: f64, z: f64| o + v(x, y, z) * s;
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
        let f = |r: [usize; 4]| PolyFace::new(r.to_vec(), vec![]);
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

    fn two(a: Polys, b: Polys) -> Brep {
        let n = a.points.len();
        let mut p = a;
        p.points.extend(b.points);
        for mut f in b.faces {
            f.outer.iter_mut().for_each(|i| *i += n);
            p.faces.push(f);
        }
        Brep::build(&p).unwrap()
    }

    #[test]
    fn a_cube_does_not_intersect_itself() {
        assert_eq!(
            self_intersection_detail(&Brep::build(&cube_polys(DVec3::ZERO, 1.0)).unwrap()),
            None
        );
    }

    #[test]
    fn overlapping_and_touching_shells_are_caught() {
        let overlap = two(cube_polys(DVec3::ZERO, 2.0), cube_polys(DVec3::ONE, 2.0));
        assert!(matches!(
            self_intersection_detail(&overlap),
            Some(SelfHit::EdgeThroughFace { .. })
        ));
        let touching_face = two(
            cube_polys(DVec3::ZERO, 1.0),
            cube_polys(v(1.0, 0.0, 0.0), 1.0),
        );
        assert!(self_intersection_detail(&touching_face).is_some());
        let touching_edge = two(
            cube_polys(DVec3::ZERO, 1.0),
            cube_polys(v(1.0, 1.0, 0.0), 1.0),
        );
        assert!(self_intersection_detail(&touching_edge).is_some());
        let nearly = two(
            cube_polys(DVec3::ZERO, 1.0),
            cube_polys(v(1.0 + 5e-6, 0.3, 0.3), 1.0),
        );
        assert!(
            self_intersection_detail(&nearly).is_some(),
            "closer than the merge distance counts as touching"
        );
        let apart = two(
            cube_polys(DVec3::ZERO, 1.0),
            cube_polys(v(1.5, 0.0, 0.0), 1.0),
        );
        assert_eq!(self_intersection_detail(&apart), None);
        let nested = two(cube_polys(DVec3::ZERO, 3.0), cube_polys(DVec3::ONE, 1.0));
        assert_eq!(
            self_intersection_detail(&nested),
            None,
            "containment is not a boundary contact"
        );
    }

    #[test]
    fn segment_triangle_cases() {
        let (o, x, y) = (DVec3::ZERO, v(1., 0., 0.), v(0., 1., 0.));
        assert!(seg_tri(v(0.2, 0.2, -1.), v(0.2, 0.2, 1.), o, x, y));
        assert!(!seg_tri(v(0.8, 0.8, -1.), v(0.8, 0.8, 1.), o, x, y));
        assert!(seg_tri(v(0.2, 0.2, 0.), v(2., 2., 0.), o, x, y), "coplanar");
        assert!(!seg_tri(v(2., 2., 0.), v(3., 2., 0.), o, x, y));
        assert!(
            seg_tri(v(0.5, 0.5, -1.), v(0.5, 0.5, 0.), o, x, y),
            "touches the edge"
        );
        assert!(!seg_tri(o, x, o, x, v(2., 0., 0.)), "degenerate triangle");
    }
}
