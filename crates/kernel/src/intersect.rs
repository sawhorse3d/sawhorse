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
//! 4. **Face on face**: two faces in one plane must not overlap in area (a zero-thickness
//!    fin can share every vertex and edge with its twin, so 1 to 3 cannot see it).
//!
//! An edge with both ends on a face that is not one of the face's edges (a chord) is tested
//! along its interior in check 1.
//!
//! Exact predicates alone would flag the 1e-16 m wobble of a vertex inserted on an edge
//! (an f64 point is rarely exactly on a tilted line); working on B-rep features with the
//! merge distance for contact makes the test exact where it matters (penetration) and
//! tolerant where the kernel already is (coincidence).
//!
//! Lane 16: checks 1, 2 and 4 find the faces near each edge, vertex or face through a grid
//! of face boxes ([`FaceGrid`]) instead of trying every face, once a solid has more than a
//! few dozen faces; candidates are tried in face order, so the first contact found (and
//! reported) is the same as trying every face. Check 3 was already a sweep.

use std::collections::BTreeSet;

use glam::{DVec2, DVec3};

use crate::brep::Brep;
use crate::sketch::point_segment_distance;
use crate::tolerance::{PLANARITY, POINT_MERGE};
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
    /// Two faces lie in one plane and overlap (a zero-thickness fin, or a double cover).
    FacesOverlap {
        a: usize,
        b: usize,
    },
}

struct FaceData {
    verts: BTreeSet<usize>,
    /// The face's own edges (sorted vertex pairs).
    edges: BTreeSet<(usize, usize)>,
    tris: Vec<[DVec3; 3]>,
    rings2: Vec<Vec<DVec2>>,
    /// Rings as vertex ordinals.
    rings3: Vec<Vec<usize>>,
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

/// Face boxes binned in a uniform grid: the faces whose boxes may meet a query box, in face
/// order. Faces spanning many cells are kept in a list every query tries.
struct FaceGrid {
    cell: f64,
    origin: DVec3,
    bins: std::collections::HashMap<(i64, i64, i64), Vec<u32>>,
    big: Vec<u32>,
    n: usize,
}

/// Below this many faces every face is tried (the grid costs more than it saves).
const GRID_MIN_FACES: usize = 64;
/// A face covering more cells than this goes in the always-tried list.
const GRID_MAX_CELLS: i64 = 64;

impl FaceGrid {
    fn new(boxes: &[(DVec3, DVec3)]) -> Option<FaceGrid> {
        if boxes.len() < GRID_MIN_FACES {
            return None;
        }
        let lo = boxes
            .iter()
            .fold(DVec3::splat(f64::INFINITY), |m, b| m.min(b.0));
        // A cell about the size of a typical face: the median of their largest sides.
        let mut sides: Vec<f64> = boxes
            .iter()
            .map(|b| (b.1 - b.0).max_element())
            .filter(|x| x.is_finite())
            .collect();
        if sides.len() != boxes.len() || !lo.is_finite() {
            return None;
        }
        let mid = sides.len() / 2;
        sides.select_nth_unstable_by(mid, f64::total_cmp);
        let cell = (sides[mid] * 2.0).max(POINT_MERGE * 1e3);
        let mut g = FaceGrid {
            cell,
            origin: lo - DVec3::splat(cell),
            bins: std::collections::HashMap::new(),
            big: Vec::new(),
            n: boxes.len(),
        };
        for (i, b) in boxes.iter().enumerate() {
            let (a, z) = (g.at(b.0), g.at(b.1));
            let cells = (z.0 - a.0 + 1) * (z.1 - a.1 + 1) * (z.2 - a.2 + 1);
            if cells > GRID_MAX_CELLS {
                g.big.push(i as u32);
                continue;
            }
            for x in a.0..=z.0 {
                for y in a.1..=z.1 {
                    for w in a.2..=z.2 {
                        g.bins.entry((x, y, w)).or_default().push(i as u32);
                    }
                }
            }
        }
        Some(g)
    }

    fn at(&self, p: DVec3) -> (i64, i64, i64) {
        let q = (p - self.origin) / self.cell;
        (q.x.floor() as i64, q.y.floor() as i64, q.z.floor() as i64)
    }

    /// Faces whose cells meet the box `lo..hi` (grown by `pad`), ascending, each once.
    fn near(&self, lo: DVec3, hi: DVec3, pad: f64) -> Vec<usize> {
        let (a, z) = (
            self.at(lo - DVec3::splat(pad)),
            self.at(hi + DVec3::splat(pad)),
        );
        let cells = (z.0 - a.0 + 1)
            .saturating_mul(z.1 - a.1 + 1)
            .saturating_mul(z.2 - a.2 + 1);
        // A query box bigger than the grid is worth: every face.
        if cells > 4096 {
            return (0..self.n).collect();
        }
        let mut out: Vec<u32> = self.big.clone();
        for x in a.0..=z.0 {
            for y in a.1..=z.1 {
                for w in a.2..=z.2 {
                    if let Some(v) = self.bins.get(&(x, y, w)) {
                        out.extend_from_slice(v);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out.into_iter().map(|i| i as usize).collect()
    }
}

/// The first self-contact of the solid's boundary, or `None`.
pub fn self_intersection_detail(b: &Brep) -> Option<SelfHit> {
    detail(b, true)
}

/// [`self_intersection_detail`], with the face grid or (`grid` false, tests) every face.
fn detail(b: &Brep, grid: bool) -> Option<SelfHit> {
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
        let own_edges = loops
            .iter()
            .flat_map(|l| {
                (0..l.len()).map(move |k| {
                    (
                        l[k].min(l[(k + 1) % l.len()]),
                        l[k].max(l[(k + 1) % l.len()]),
                    )
                })
            })
            .collect();
        faces.push(FaceData {
            verts: flat.iter().copied().collect(),
            edges: own_edges,
            rings3: loops.clone(),
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

    let boxes: Vec<(DVec3, DVec3)> = faces.iter().map(|f| (f.lo, f.hi)).collect();
    let grid = if grid { FaceGrid::new(&boxes) } else { None };
    let all: Vec<usize> = (0..faces.len()).collect();
    let near = |lo: DVec3, hi: DVec3, pad: f64| -> std::borrow::Cow<'_, [usize]> {
        match &grid {
            Some(g) => std::borrow::Cow::Owned(g.near(lo, hi, pad)),
            None => std::borrow::Cow::Borrowed(&all),
        }
    };

    // 1. Edges through faces they do not belong to.
    for &(i, j) in &edges {
        let (p, q) = (pos[i], pos[j]);
        let (lo, hi) = (p.min(q), p.max(q));
        for &fi in near(lo, hi, POINT_MERGE).iter() {
            let f = &faces[fi];
            if !overlap(lo, hi, f.lo, f.hi, 0.0) {
                continue;
            }
            let (si, sj) = (f.verts.contains(&i), f.verts.contains(&j));
            let (a, z) = match (si, sj) {
                // One of the face's own edges.
                (true, true) if f.edges.contains(&(i.min(j), i.max(j))) => continue,
                // Both ends on the face but not its edge: a chord. Only its interior counts.
                (true, true) => {
                    let len = p.distance(q);
                    if len <= 2.0 * POINT_MERGE {
                        continue;
                    }
                    let step = (q - p) * (POINT_MERGE / len);
                    (p + step, q - step)
                }
                (false, false) => (p, q),
                (true, false) | (false, true) => {
                    let (from, to) = if si { (p, q) } else { (q, p) };
                    let len = from.distance(to);
                    if len <= 2.0 * POINT_MERGE {
                        continue;
                    }
                    // A straight edge from a point of the face's plane meets that plane only
                    // there: unless it lies in the plane, it cannot pass through the face.
                    // (Lane 23: the old test alone, from a point POINT_MERGE along, failed
                    // valid solids. Two edges leaving a sliver's tip 1.3e-8 rad apart, 4 km
                    // out: that point lies 1.3e-13 m off the other edge's wall, under the
                    // 1e-12 m its coordinates round to, so it fell on the wrong side.)
                    if f.plane.signed_distance(to).abs() > PLANARITY {
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
        for &fi in near(v, v, POINT_MERGE * 2.0).iter() {
            let f = &faces[fi];
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

    // 4. Faces in one plane must not overlap (their own vertices and edges can all be
    //    shared, so checks 1 to 3 cannot see a zero-thickness fin).
    for a in 0..faces.len() {
        for &b in near(faces[a].lo, faces[a].hi, POINT_MERGE * 2.0).iter() {
            if b <= a {
                continue;
            }
            let (fa, fb) = (&faces[a], &faces[b]);
            if !overlap(fa.lo, fa.hi, fb.lo, fb.hi, POINT_MERGE)
                || fa.plane.normal.dot(fb.plane.normal).abs() < 1.0 - 1e-9
                || fb
                    .verts
                    .iter()
                    .any(|&v| fa.plane.signed_distance(pos[v]).abs() > POINT_MERGE)
            {
                continue;
            }
            let contours = |f: &FaceData| -> Vec<Vec<[f64; 2]>> {
                f.rings3
                    .iter()
                    .map(|r| {
                        r.iter()
                            .map(|&v| fa.plane.to_local(pos[v]).to_array())
                            .collect()
                    })
                    .collect()
            };
            let shared = sawhorse_tessellate::overlay::intersection(&contours(fa), &contours(fb));
            if sawhorse_tessellate::overlay::area(&shared) > crate::tolerance::AREA_EPS {
                return Some(SelfHit::FacesOverlap { a, b });
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

    #[test]
    fn a_wedge_whose_tip_is_a_hair_wide_far_out_is_no_contact() {
        // Lane 23's property-test find: a 9 km 67-gon and its copy turned 1e-7 degrees bound
        // slivers whose tips are 1.3e-8 rad wide, 4 km out. Pulled 0.5 m, the edge leaving
        // the tip lies 1.3e-13 m off the other side's wall a merge distance along, under the
        // rounding of its coordinates; the test from that point called it a crossing.
        let plane = crate::plane::Plane::GROUND;
        let ring = vec![
            DVec2::new(4621.267741633579, 7722.945322876298),
            DVec2::new(3877.773704468734, 8121.752957147295),
            DVec2::new(3877.7736960486855, 8121.752949299573),
        ];
        let ring = if sawhorse_tessellate::signed_area(&ring) > 0.0 {
            ring
        } else {
            ring.into_iter().rev().collect()
        };
        let o = crate::object::Object::extrude(&plane, &ring, &[], 0.5).unwrap();
        assert_eq!(self_intersection(o.brep()), None);
        o.validate().unwrap();
        // An edge folding back flat over a face it starts on is still caught: a cube with a
        // corner pulled across its top face's plane is refused elsewhere; here the plain
        // cube stays clean.
        let cube = Brep::from_polygons(
            &cube_polys(v(0.0, 0.0, 0.0), 1.0).points,
            &cube_polys(v(0.0, 0.0, 0.0), 1.0).faces,
        )
        .unwrap();
        assert_eq!(self_intersection(&cube), None);
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

    /// Many cubes in one B-rep (each its own shell).
    fn cubes(at: &[(DVec3, f64)]) -> Brep {
        let mut p = cube_polys(at[0].0, at[0].1);
        for &(o, s) in &at[1..] {
            let b = cube_polys(o, s);
            let n = p.points.len();
            p.points.extend(b.points);
            for mut f in b.faces {
                f.outer.iter_mut().for_each(|i| *i += n);
                p.faces.push(f);
            }
        }
        Brep::build(&p).unwrap()
    }

    #[test]
    fn lane_16_the_face_grid_finds_what_trying_every_face_finds() {
        // Twenty cubes in a row (120 faces: the grid is used): apart, then one pair overlapping,
        // then one pair touching; and random scatters. The grid and the full scan must report
        // the same first contact every time.
        let row: Vec<(DVec3, f64)> = (0..20)
            .map(|i| (v(i as f64 * 2.0, 0.0, 0.0), 1.0))
            .collect();
        let b = cubes(&row);
        assert_eq!(detail(&b, true), None);
        assert_eq!(detail(&b, false), None);
        let mut overlap = row.clone();
        overlap[13].0 = v(24.5, 0.3, 0.2);
        let b = cubes(&overlap);
        assert!(detail(&b, false).is_some());
        assert_eq!(detail(&b, true), detail(&b, false));
        let mut touch = row.clone();
        touch[7].0 = v(13.0, 0.0, 0.0);
        let b = cubes(&touch);
        assert!(detail(&b, false).is_some());
        assert_eq!(detail(&b, true), detail(&b, false));
        let mut seed = 0x2026_1013_u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as f64 / (1u64 << 31) as f64
        };
        for _ in 0..40 {
            let at: Vec<(DVec3, f64)> = (0..16)
                .map(|_| (v(rnd() * 12.0, rnd() * 12.0, rnd() * 3.0), 0.5 + rnd()))
                .collect();
            let b = cubes(&at);
            assert_eq!(detail(&b, true), detail(&b, false), "{at:?}");
        }
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
    fn a_zero_thickness_fin_is_caught() {
        // The wedge a near-coincident boolean once produced (fuzz, 2026-10-11): the +x wall
        // (face 2) still covers a triangle that face 5 covers again, facing -x. Every vertex
        // of face 5 is a vertex of face 2, so checks 1 to 3 as first written missed it; the
        // chord rule (edge 4-6 runs across face 2) catches it, and the coplanar-overlap
        // check is the backstop for fins whose edges are all shared.
        let p = |x: f64, y: f64, z: f64| v(x, y, z);
        let points = vec![
            p(0.25, 0.0, 0.0),
            p(0.25, 0.25, 0.0),
            p(0.35, 0.25, 0.0),
            p(0.35, 0.0, 0.0),
            p(0.35, 0.0, 0.0875),
            p(0.25, 0.0, 0.0375),
            p(0.35, 0.25, 0.0875),
            p(0.35, 0.25, 0.1),
            p(0.25, 0.25, 0.0375),
        ];
        let f = |r: &[usize]| PolyFace::new(r.to_vec(), vec![]);
        let polys = Polys {
            points,
            faces: vec![
                f(&[0, 1, 2, 3]),
                f(&[0, 3, 4, 5]),
                f(&[3, 2, 6, 7, 4]),
                f(&[1, 8, 6, 2]),
                f(&[0, 5, 8, 1]),
                f(&[4, 7, 6]),
                f(&[5, 4, 6, 8]),
            ],
            user_edges: Default::default(),
        };
        let b = Brep::build(&polys).expect("the fin is a closed half-edge structure");
        assert!(crate::validate::validate(&b).unwrap().watertight);
        assert!(matches!(
            self_intersection_detail(&b),
            Some(
                SelfHit::EdgeThroughFace {
                    edge: (4, 6),
                    face: 2
                } | SelfHit::FacesOverlap { .. }
            )
        ));
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
