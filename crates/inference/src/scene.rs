//! The read-only picture of a model that picking and inference query: triangles of every
//! visible face and sketch region (f64, exact, not the f32 view mesh), every edge, the arcs
//! and circles found among them, and guide lines, with a BVH over triangles and one over
//! edges. Built from the model after each edit; never mutates anything.
//!
//! Section planes (lane 12): with the style's section cuts on, what an active plane cuts
//! away is left out (triangles and edges clipped to the kept side, as the viewport draws
//! them), and each cut adds its caps (surfaces that select their solid) and its outline
//! (edges of that solid), so the cut's corners and edges snap like any other.

use glam::{DVec2, DVec3};
use sawhorse_kernel::ids::to_public;
use sawhorse_kernel::model::Model;
use sawhorse_kernel::plane::Plane;
use sawhorse_tessellate::triangulate;

use crate::bvh::Bvh;
use crate::geom::Aabb;

/// What an edge, vertex or guide belongs to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OwnerKind {
    Object,
    Sketch,
    Guide,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Owner {
    pub kind: OwnerKind,
    pub id: u64,
}

/// A pickable surface: an object face (persistent id), a sketch region, or (lane 12) the
/// cap a section plane cuts through a solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceRef {
    Face { object: u64, face: u64 },
    Region { sketch: u64, region: u64 },
    Cap { object: u64, section: u64 },
}

/// The kept side of a plane: points with `(p - origin) . normal >= 0`.
type Keep = (DVec3, DVec3);

fn kept(p: DVec3, planes: &[Keep]) -> bool {
    planes.iter().all(|(o, n)| (p - *o).dot(*n) >= 0.0)
}

/// The part of polygon `ring` on the kept side of every plane (Sutherland-Hodgman).
fn clip_polygon(ring: &[DVec3], planes: &[Keep]) -> Vec<DVec3> {
    let mut out = ring.to_vec();
    for (o, n) in planes {
        let pts = std::mem::take(&mut out);
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            let (dp, dq) = ((p - *o).dot(*n), (q - *o).dot(*n));
            if dp >= 0.0 {
                out.push(p);
            }
            if (dp >= 0.0) != (dq >= 0.0) {
                out.push(p + (q - p) * (dp / (dp - dq)));
            }
        }
    }
    out
}

/// The part of segment `a`-`b` on the kept side of every plane.
fn clip_segment(a: DVec3, b: DVec3, planes: &[Keep]) -> Option<(DVec3, DVec3)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (o, n) in planes {
        let (da, db) = ((a - *o).dot(*n), (b - *o).dot(*n));
        if da < 0.0 && db < 0.0 {
            return None;
        }
        if da < 0.0 || db < 0.0 {
            let t = da / (da - db);
            if da < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t1 - t0 > 1e-12).then(|| (a.lerp(b, t0), a.lerp(b, t1)))
}

/// A surface with its plane frame (what a tool draws on).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    pub at: SurfaceRef,
    pub plane: Plane,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tri {
    pub p: [DVec3; 3],
    /// Index into [`Scene::surfaces`].
    pub surface: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg {
    pub a: DVec3,
    pub b: DVec3,
    pub owner: Owner,
    /// Drawn by the user (sketch strokes, lines drawn on faces): erasable.
    pub drawn: bool,
    /// The surfaces on either side (indices into [`Scene::surfaces`]; object edges only).
    pub faces: [Option<usize>; 2],
    /// Soft (1), smooth (2) and hidden (4) flags (lane 15): a soft or hidden edge is not
    /// picked unless the query asks for it; a hidden one is not inferred on either.
    pub flags: u8,
    /// Lane 23: the curve it is a segment of (index into [`Scene::curves`]; [`NO_CURVE`]
    /// for none: a plain `u32`, so the segment stays the size it was on the picking path).
    pub curve: u32,
}

/// [`Seg::curve`] of a segment that is in no curve.
pub const NO_CURVE: u32 = u32::MAX;

/// A curve in the scene (lane 23): a sketch's curve or a solid's rim, its id there, and its
/// circle (for a circle, an arc or a polygon that still reads as one).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneCurve {
    pub owner: Owner,
    pub id: u64,
    pub kind: sawhorse_kernel::curve::CurveKind,
    pub closed: bool,
    pub centre: Option<DVec3>,
    pub normal: DVec3,
    pub radius: Option<f64>,
}

/// A run of equal segments turning by equal angles (an arc, a circle, a regular polygon):
/// its centre is an inference point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    pub centre: DVec3,
    pub normal: DVec3,
    pub radius: f64,
    pub owner: Owner,
    /// The full circle (closed ring), not just an arc.
    pub closed: bool,
}

/// An infinite guide line (Tape measure) or a guide point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuideLine {
    pub id: u64,
    pub point: DVec3,
    /// Unit direction; zero for a guide point.
    pub dir: DVec3,
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub tris: Vec<Tri>,
    pub surfaces: Vec<Surface>,
    pub segs: Vec<Seg>,
    pub arcs: Vec<Arc>,
    /// Lane 23: the curves (sketch curves and solids' rims) segments name.
    pub curves: Vec<SceneCurve>,
    pub guides: Vec<GuideLine>,
    /// Dimension lines (id, the two ends of the drawn line): pickable, never snapped to.
    pub dims: Vec<(u64, DVec3, DVec3)>,
    pub tri_bvh: Bvh,
    pub seg_bvh: Bvh,
    pub bounds: Option<Aabb>,
}

/// Turning angle (radians) at most this for a run of segments to count as an arc: six or
/// more segments per full turn, so squares and triangles are not circles.
const ARC_MAX_TURN: f64 = std::f64::consts::FRAC_PI_3 + 1e-9;
/// Equal lengths and angles within this relative tolerance.
const ARC_TOL: f64 = 1e-6;

impl Scene {
    /// Everything visible in `model`.
    pub fn build(model: &Model) -> Scene {
        let mut s = Scene::default();
        // Active section planes cut (lane 12), unless the style turns the cuts off.
        let cuts = model.style().is_none_or(|st| st.section_cuts);
        let planes_over = |parent: Option<sawhorse_kernel::ids::NodeId>| -> Vec<Keep> {
            if !cuts {
                return Vec::new();
            }
            model
                .active_sections_over(parent)
                .into_iter()
                .filter_map(|id| model.section(id))
                .map(|x| (x.origin, x.normal))
                .collect()
        };
        for (oid, o) in model.objects() {
            if !model.is_visible(oid) {
                continue;
            }
            let clip = planes_over(o.membership().parent);
            let b = o.brep();
            let owner = Owner {
                kind: OwnerKind::Object,
                id: oid,
            };
            let mut surface_of = std::collections::BTreeMap::new();
            for (fid, f) in b.faces() {
                surface_of.insert(fid, s.surfaces.len());
                let outer = b.loop_points(f.outer);
                let holes: Vec<Vec<DVec3>> = f.holes.iter().map(|&h| b.loop_points(h)).collect();
                s.add_clipped_surface(
                    SurfaceRef::Face {
                        object: oid,
                        face: f.id,
                    },
                    f.plane,
                    &outer,
                    &holes,
                    &clip,
                );
                for ring in std::iter::once(&outer).chain(holes.iter()) {
                    s.find_arcs(ring, true, owner);
                }
            }
            // Lane 23: the solid's rims (their centres are inference points).
            let mut rim: std::collections::BTreeMap<u64, u32> = Default::default();
            let on_rim = if o.curves().is_empty() {
                Default::default()
            } else {
                for &cid in o.curves().keys() {
                    if let Some(info) = o.curve_info(cid) {
                        rim.insert(cid, s.curves.len() as u32);
                        s.add_curve(owner, cid, &info);
                    }
                }
                o.edge_curves()
            };
            for (e, edge) in b.edges() {
                let (a, bb) = b.edge_points(e);
                let Some((a, bb)) = clip_segment(a, bb, &clip) else {
                    continue;
                };
                let curve = on_rim
                    .get(&e)
                    .and_then(|c| rim.get(c))
                    .copied()
                    .unwrap_or(NO_CURVE);
                let side = |h: Option<sawhorse_kernel::ids::HalfEdgeId>| {
                    let he = b.half_edge(h?)?;
                    let face = b.lp(he.lp)?.face;
                    surface_of.get(&face).copied()
                };
                s.segs.push(Seg {
                    a,
                    b: bb,
                    owner,
                    drawn: edge.user,
                    faces: [side(Some(edge.he)), side(edge.twin)],
                    flags: o.flags_of(e),
                    curve,
                });
            }
        }
        for (sid, sk) in model.sketches() {
            if !model.is_sketch_visible(sid) {
                continue;
            }
            let plane = *sk.plane();
            let owner = Owner {
                kind: OwnerKind::Sketch,
                id: sid,
            };
            let clip = planes_over(
                model
                    .sketch_parent(sid)
                    .map(sawhorse_kernel::ids::from_public),
            );
            for (rid, r) in sk.regions() {
                let (o, h) = sk.region_rings(r);
                let outer: Vec<DVec3> = o.iter().map(|&q| plane.to_world(q)).collect();
                let holes: Vec<Vec<DVec3>> = h
                    .iter()
                    .map(|ring| ring.iter().map(|&q| plane.to_world(q)).collect())
                    .collect();
                s.add_clipped_surface(
                    SurfaceRef::Region {
                        sketch: sid,
                        region: to_public(rid),
                    },
                    plane,
                    &outer,
                    &holes,
                    &clip,
                );
            }
            // Lane 23: the sketch's curves (their centres are inference points).
            let mut index: std::collections::BTreeMap<u64, u32> = Default::default();
            for (cid, _) in sk.curves() {
                if let Some(info) = sk.curve_info(cid) {
                    index.insert(cid, s.curves.len() as u32);
                    s.add_curve(owner, cid, &info);
                }
            }
            let on_curve = sk.edge_curves();
            // Arcs among sketch strokes: chains through vertices with exactly two edges.
            let mut adj: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
            for (_, [a, b]) in sk.edges() {
                adj.entry(a).or_default().push(b);
                adj.entry(b).or_default().push(a);
                if let (Some(p), Some(q)) = (sk.vertex(a), sk.vertex(b))
                    && let Some((p, q)) = clip_segment(plane.to_world(p), plane.to_world(q), &clip)
                {
                    let curve = on_curve
                        .get(&(a.min(b), a.max(b)))
                        .and_then(|c| index.get(c))
                        .copied()
                        .unwrap_or(NO_CURVE);
                    s.segs.push(Seg {
                        a: p,
                        b: q,
                        owner,
                        drawn: true,
                        faces: [None, None],
                        flags: 0,
                        curve,
                    });
                }
            }
            let mut seen = std::collections::BTreeSet::new();
            for (&v, ns) in &adj {
                if seen.contains(&v) || ns.len() == 2 {
                    continue;
                }
                // Walk every chain starting at a junction or an end.
                for &n0 in ns {
                    let mut chain = vec![v, n0];
                    let (mut prev, mut cur) = (v, n0);
                    while adj.get(&cur).is_some_and(|x| x.len() == 2) && !seen.contains(&cur) {
                        seen.insert(cur);
                        let x = &adj[&cur];
                        let next = if x[0] == prev { x[1] } else { x[0] };
                        chain.push(next);
                        prev = cur;
                        cur = next;
                        if cur == v {
                            break;
                        }
                    }
                    let pts: Vec<DVec3> = chain
                        .iter()
                        .filter_map(|&k| sk.vertex(k).map(|q| plane.to_world(q)))
                        .collect();
                    s.find_arcs(&pts, false, owner);
                }
            }
            // Closed loops of degree-2 vertices (a lone circle).
            for (&v, ns) in &adj {
                if seen.contains(&v) || ns.len() != 2 {
                    continue;
                }
                let mut ring = vec![v];
                let (mut prev, mut cur) = (v, ns[0]);
                seen.insert(v);
                while cur != v && adj.get(&cur).is_some_and(|x| x.len() == 2) && seen.insert(cur) {
                    ring.push(cur);
                    let x = &adj[&cur];
                    let next = if x[0] == prev { x[1] } else { x[0] };
                    prev = cur;
                    cur = next;
                }
                let pts: Vec<DVec3> = ring
                    .iter()
                    .filter_map(|&k| sk.vertex(k).map(|q| plane.to_world(q)))
                    .collect();
                s.find_arcs(&pts, cur == v, owner);
            }
        }
        for (id, g) in model.guides() {
            s.guides.push(GuideLine {
                id,
                point: g.point,
                dir: g.dir,
            });
        }
        for (id, d) in model.dimensions() {
            s.dims.push((id, d.a + d.offset, d.b + d.offset));
        }
        // The caps and outlines of active cuts (lane 12).
        if cuts {
            for (id, x) in model.sections() {
                if !x.active {
                    continue;
                }
                let Ok(cut) = model.section_cut(id) else {
                    continue;
                };
                let frame = sawhorse_kernel::plane::Plane::new(x.origin, -x.normal)
                    .unwrap_or(sawhorse_kernel::plane::Plane::GROUND);
                for c in &cut.caps {
                    let outer: Vec<DVec3> = c.outer.iter().map(|p| cut.to_world(*p)).collect();
                    let holes: Vec<Vec<DVec3>> = c
                        .holes
                        .iter()
                        .map(|h| h.iter().map(|p| cut.to_world(*p)).collect())
                        .collect();
                    let surface = s.surfaces.len();
                    s.add_surface(
                        SurfaceRef::Cap {
                            object: c.object,
                            section: id,
                        },
                        frame,
                        &outer,
                        &holes,
                    );
                    for ring in std::iter::once(&outer).chain(holes.iter()) {
                        for k in 0..ring.len() {
                            s.segs.push(Seg {
                                a: ring[k],
                                b: ring[(k + 1) % ring.len()],
                                owner: Owner {
                                    kind: OwnerKind::Object,
                                    id: c.object,
                                },
                                drawn: false,
                                faces: [Some(surface), None],
                                flags: 0,
                                curve: NO_CURVE,
                            });
                        }
                    }
                }
            }
        }
        s.dedup_arcs();
        s.index();
        s
    }

    /// Lane 23: a curve, and its centre as an arc centre when it is round.
    fn add_curve(&mut self, owner: Owner, id: u64, info: &sawhorse_kernel::curve::CurveInfo) {
        let centre = info.centre.map(DVec3::from_array);
        let normal = DVec3::from_array(info.normal);
        self.curves.push(SceneCurve {
            owner,
            id,
            kind: info.kind,
            closed: info.closed,
            centre,
            normal,
            radius: info.radius,
        });
        if let (Some(centre), Some(radius)) = (centre, info.radius) {
            self.arcs.push(Arc {
                centre,
                normal,
                radius,
                owner,
                closed: info.closed,
            });
        }
    }

    /// Lane 23: the curve segment `i` belongs to, if any.
    pub fn curve_of(&self, i: usize) -> Option<&SceneCurve> {
        self.segs
            .get(i)
            .and_then(|s| self.curves.get(s.curve as usize))
    }

    /// Add guide lines (and points, with a zero direction) after building.
    pub fn with_guides(mut self, guides: Vec<GuideLine>) -> Scene {
        self.guides = guides;
        self
    }

    /// A surface with its triangles clipped to the kept side of `clip` (section planes).
    #[allow(clippy::too_many_arguments)]
    fn add_clipped_surface(
        &mut self,
        at: SurfaceRef,
        plane: Plane,
        outer: &[DVec3],
        holes: &[Vec<DVec3>],
        clip: &[Keep],
    ) {
        if clip.is_empty() {
            return self.add_surface(at, plane, outer, holes);
        }
        let first = self.tris.len();
        self.add_surface(at, plane, outer, holes);
        let tris = self.tris.split_off(first);
        for t in tris {
            if t.p.iter().all(|p| kept(*p, clip)) {
                self.tris.push(t);
                continue;
            }
            let poly = clip_polygon(&t.p, clip);
            for k in 1..poly.len().saturating_sub(1) {
                self.tris.push(Tri {
                    p: [poly[0], poly[k], poly[k + 1]],
                    surface: t.surface,
                });
            }
        }
    }

    fn add_surface(&mut self, at: SurfaceRef, plane: Plane, outer: &[DVec3], holes: &[Vec<DVec3>]) {
        let to2 = |ring: &[DVec3]| {
            ring.iter()
                .map(|&p| plane.to_local(p))
                .collect::<Vec<DVec2>>()
        };
        let h2: Vec<Vec<DVec2>> = holes.iter().map(|h| to2(h)).collect();
        // Every face gets a surface (edges refer to them by index), triangles when it
        // tessellates.
        let surface = self.surfaces.len();
        self.surfaces.push(Surface { at, plane });
        let Ok(tris) = triangulate(&to2(outer), &h2) else {
            return;
        };
        let flat: Vec<DVec3> = outer
            .iter()
            .chain(holes.iter().flatten())
            .copied()
            .collect();
        for t in tris {
            self.tris.push(Tri {
                p: t.map(|i| flat[i as usize]),
                surface,
            });
        }
    }

    /// Find runs of at least three equal segments turning by equal small angles in `pts`
    /// (a ring when `closed`), and record their centres.
    fn find_arcs(&mut self, pts: &[DVec3], closed: bool, owner: Owner) {
        let n = pts.len();
        if n < 4 {
            return;
        }
        let m = if closed { n } else { n - 1 }; // segments
        let seg = |i: usize| pts[(i + 1) % n] - pts[i % n];
        // Turning at the vertex between segment i and i + 1.
        let turn = |i: usize| -> Option<(f64, DVec3)> {
            let (u, v) = (seg(i), seg(i + 1));
            let c = u.cross(v);
            let ang = c.length().atan2(u.dot(v));
            (c.length() > 1e-15).then(|| (ang, c.normalize()))
        };
        let same = |i: usize, j: usize| -> bool {
            let (li, lj) = (seg(i).length(), seg(j).length());
            if (li - lj).abs() > ARC_TOL * li.max(lj) {
                return false;
            }
            match (turn(i), turn(j)) {
                (Some((a, na)), Some((b, nb))) => {
                    a <= ARC_MAX_TURN
                        && (a - b).abs() <= ARC_TOL * a.max(b).max(1e-3)
                        && na.dot(nb) > 1.0 - 1e-9
                }
                _ => false,
            }
        };
        let turns = if closed { m } else { m - 1 };
        let mut i = 0;
        while i < turns {
            let mut j = i;
            while j + 1 < turns && same(j, j + 1) {
                j += 1;
            }
            // Turns i..=j are equal: segments i..=j+1 (j - i + 2 of them).
            let segs = j - i + 2;
            if segs >= 3
                && turn(i).is_some_and(|(a, _)| a <= ARC_MAX_TURN)
                && let Some(c) = circumcentre(pts[i % n], pts[(i + 1) % n], pts[(i + 2) % n])
            {
                let normal = turn(i).map(|(_, nn)| nn).unwrap_or(DVec3::Z);
                self.arcs.push(Arc {
                    centre: c,
                    normal,
                    radius: c.distance(pts[i % n]),
                    owner,
                    closed: closed && segs >= m,
                });
            }
            i = j + 1;
        }
    }

    fn dedup_arcs(&mut self) {
        let mut out: Vec<Arc> = Vec::new();
        for a in self.arcs.drain(..) {
            if let Some(o) = out.iter_mut().find(|o| {
                o.centre.distance(a.centre) <= 1e-7 && (o.radius - a.radius).abs() <= 1e-7
            }) {
                o.closed |= a.closed;
            } else {
                out.push(a);
            }
        }
        self.arcs = out;
    }

    fn index(&mut self) {
        let tb: Vec<Aabb> = self.tris.iter().map(|t| Aabb::of(&t.p)).collect();
        let sb: Vec<Aabb> = self.segs.iter().map(|s| Aabb::of(&[s.a, s.b])).collect();
        self.tri_bvh = Bvh::build(&tb);
        self.seg_bvh = Bvh::build(&sb);
        self.bounds = tb.iter().chain(sb.iter()).copied().reduce(Aabb::union);
    }
}

/// Centre of the circle through three points (`None` when they are collinear).
pub fn circumcentre(a: DVec3, b: DVec3, c: DVec3) -> Option<DVec3> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let d = 2.0 * n.length_squared();
    if d <= 1e-30 {
        return None;
    }
    Some(a + (ac.length_squared() * n.cross(ab) + ab.length_squared() * ac.cross(n)) / d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_kernel::model::Op;

    #[test]
    fn circumcentre_of_a_right_triangle_is_the_hypotenuse_midpoint() {
        let c = circumcentre(
            DVec3::ZERO,
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
        )
        .unwrap();
        assert!((c - DVec3::new(1.0, 1.0, 0.0)).length() < 1e-12);
        assert!(circumcentre(DVec3::ZERO, DVec3::X, DVec3::X * 2.0).is_none());
    }

    #[test]
    fn a_box_scene_has_its_faces_edges_and_no_arcs() {
        let mut m = Model::with_ground_sketch();
        m.apply(&Op::AddRectangle {
            sketch: 1,
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        })
        .unwrap();
        m.apply(&Op::Extrude {
            sketch: 1,
            region: 1,
            distance: 1.0,
        })
        .unwrap();
        let s = Scene::build(&m);
        assert_eq!(s.surfaces.len(), 6);
        assert_eq!(s.tris.len(), 12);
        assert_eq!(s.segs.len(), 12);
        assert!(s.arcs.is_empty(), "a square is not a circle");
    }

    #[test]
    fn circles_in_sketches_and_on_solids_have_centres() {
        let mut m = Model::with_ground_sketch();
        let pts: Vec<[f64; 2]> = (0..24)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / 24.0;
                [2.0 + 0.5 * a.cos(), 1.0 + 0.5 * a.sin()]
            })
            .collect();
        m.apply(&Op::AddPolyline {
            sketch: 1,
            points: pts,
            closed: true,
        })
        .unwrap();
        let s = Scene::build(&m);
        assert_eq!(s.arcs.len(), 1);
        assert!((s.arcs[0].centre - DVec3::new(2.0, 1.0, 0.0)).length() < 1e-9);
        assert!((s.arcs[0].radius - 0.5).abs() < 1e-9);
        assert!(s.arcs[0].closed);
        // Extruded: the cap and base circles both have centres.
        m.apply(&Op::Extrude {
            sketch: 1,
            region: 1,
            distance: 0.3,
        })
        .unwrap();
        let s = Scene::build(&m);
        let mut zs: Vec<f64> = s.arcs.iter().map(|a| a.centre.z).collect();
        zs.sort_by(f64::total_cmp);
        assert_eq!(zs.len(), 2);
        assert!(zs[0].abs() < 1e-9 && (zs[1] - 0.3).abs() < 1e-9);
    }
}
