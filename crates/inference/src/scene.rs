//! The read-only picture of a model that picking and inference query: triangles of every
//! visible face and sketch region (f64, exact, not the f32 view mesh), every edge, the arcs
//! and circles found among them, and guide lines, with a BVH over triangles and one over
//! edges. Built from the model after each edit; never mutates anything.

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

/// A pickable surface: an object face (persistent id) or a sketch region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceRef {
    Face { object: u64, face: u64 },
    Region { sketch: u64, region: u64 },
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
    pub guides: Vec<GuideLine>,
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
        for (oid, o) in model.objects() {
            if !model.is_visible(oid) {
                continue;
            }
            let b = o.brep();
            let owner = Owner {
                kind: OwnerKind::Object,
                id: oid,
            };
            for (_, f) in b.faces() {
                let outer = b.loop_points(f.outer);
                let holes: Vec<Vec<DVec3>> = f.holes.iter().map(|&h| b.loop_points(h)).collect();
                s.add_surface(
                    SurfaceRef::Face {
                        object: oid,
                        face: f.id,
                    },
                    f.plane,
                    &outer,
                    &holes,
                );
                for ring in std::iter::once(&outer).chain(holes.iter()) {
                    s.find_arcs(ring, true, owner);
                }
            }
            for (e, edge) in b.edges() {
                let (a, bb) = b.edge_points(e);
                s.segs.push(Seg {
                    a,
                    b: bb,
                    owner,
                    drawn: edge.user,
                });
            }
        }
        for (sid, sk) in model.sketches() {
            let plane = *sk.plane();
            let owner = Owner {
                kind: OwnerKind::Sketch,
                id: sid,
            };
            for (rid, r) in sk.regions() {
                let (o, h) = sk.region_rings(r);
                let outer: Vec<DVec3> = o.iter().map(|&q| plane.to_world(q)).collect();
                let holes: Vec<Vec<DVec3>> = h
                    .iter()
                    .map(|ring| ring.iter().map(|&q| plane.to_world(q)).collect())
                    .collect();
                s.add_surface(
                    SurfaceRef::Region {
                        sketch: sid,
                        region: to_public(rid),
                    },
                    plane,
                    &outer,
                    &holes,
                );
            }
            // Arcs among sketch strokes: chains through vertices with exactly two edges.
            let mut adj: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
            for (_, [a, b]) in sk.edges() {
                adj.entry(a).or_default().push(b);
                adj.entry(b).or_default().push(a);
                if let (Some(p), Some(q)) = (sk.vertex(a), sk.vertex(b)) {
                    s.segs.push(Seg {
                        a: plane.to_world(p),
                        b: plane.to_world(q),
                        owner,
                        drawn: true,
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
        s.dedup_arcs();
        s.index();
        s
    }

    /// Add guide lines (and points, with a zero direction) after building.
    pub fn with_guides(mut self, guides: Vec<GuideLine>) -> Scene {
        self.guides = guides;
        self
    }

    fn add_surface(&mut self, at: SurfaceRef, plane: Plane, outer: &[DVec3], holes: &[Vec<DVec3>]) {
        let to2 = |ring: &[DVec3]| {
            ring.iter()
                .map(|&p| plane.to_local(p))
                .collect::<Vec<DVec2>>()
        };
        let h2: Vec<Vec<DVec2>> = holes.iter().map(|h| to2(h)).collect();
        let Ok(tris) = triangulate(&to2(outer), &h2) else {
            return;
        };
        let flat: Vec<DVec3> = outer
            .iter()
            .chain(holes.iter().flatten())
            .copied()
            .collect();
        let surface = self.surfaces.len();
        self.surfaces.push(Surface { at, plane });
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
