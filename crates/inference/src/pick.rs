//! Picking: what is under the cursor, as an entity the UI can highlight, select and erase.
//! Vertices beat edges beat surfaces (within pixel radii converted to world units at each
//! candidate's depth); anything hidden behind a nearer surface is skipped.

use glam::DVec3;

use crate::geom::{PixelModel, Ray, ray_segment, ray_triangle};
use crate::scene::{Owner, OwnerKind, Scene, SurfaceRef};

/// Something the user can point at.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entity {
    Vertex {
        owner: Owner,
        at: [f64; 3],
    },
    Edge {
        owner: Owner,
        a: [f64; 3],
        b: [f64; 3],
        /// Drawn by the user (sketch strokes, lines on faces): erasable.
        drawn: bool,
    },
    Face {
        object: u64,
        face: u64,
    },
    Region {
        sketch: u64,
        region: u64,
    },
    Guide {
        guide: u64,
    },
}

/// A plane frame, as the UI uses it to draw on a surface.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Frame {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
}

impl From<&sawhorse_kernel::plane::Plane> for Frame {
    fn from(p: &sawhorse_kernel::plane::Plane) -> Frame {
        Frame {
            origin: p.origin.to_array(),
            normal: p.normal.to_array(),
            u: p.u.to_array(),
            v: p.v.to_array(),
        }
    }
}

/// The surface under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SurfaceHit {
    pub at: SurfaceRef,
    pub frame: Frame,
    pub point: [f64; 3],
    /// Ray parameter of the hit.
    pub t: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PickQuery {
    pub origin: [f64; 3],
    pub dir: [f64; 3],
    pub px: PixelModel,
    /// Radius for vertices, CSS pixels.
    #[serde(default = "default_point_px")]
    pub point_px: f64,
    /// Radius for edges, CSS pixels.
    #[serde(default = "default_edge_px")]
    pub edge_px: f64,
}

pub fn default_point_px() -> f64 {
    8.0
}
pub fn default_edge_px() -> f64 {
    5.0
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Picked {
    pub entity: Entity,
    /// The picked point (vertex position, closest point on the edge, surface hit).
    pub point: [f64; 3],
    /// The surface under the cursor, if any (even when an edge or vertex won).
    pub surface: Option<SurfaceHit>,
}

/// Nearest surface hit along the ray.
pub fn surface_hit(scene: &Scene, ray: &Ray) -> Option<SurfaceHit> {
    let mut best: Option<(f64, usize)> = None;
    scene.tri_bvh.query(
        |b| b.hit_by(ray),
        |i| {
            let t = &scene.tris[i];
            if let Some(d) = ray_triangle(ray, t.p[0], t.p[1], t.p[2])
                && best.is_none_or(|(bd, _)| d < bd)
            {
                best = Some((d, t.surface));
            }
        },
    );
    best.map(|(t, s)| {
        let sf = &scene.surfaces[s];
        SurfaceHit {
            at: sf.at,
            frame: Frame::from(&sf.plane),
            point: ray.at(t).to_array(),
            t,
        }
    })
}

/// A candidate at ray parameter `t` is visible when no surface hit lies clearly in front of
/// it (a couple of pixels of depth slack, so points on the hit surface itself count).
pub fn visible(px: &PixelModel, hit: Option<&SurfaceHit>, p: DVec3, t: f64) -> bool {
    match hit {
        None => true,
        Some(h) => t <= h.t + 2.0 * px.at(p) + 1e-9,
    }
}

/// The edge or vertex candidates within `radius` pixels of the ray: (segment index, ray
/// parameter, segment parameter, pixel distance), visible ones only.
pub fn near_segments(
    scene: &Scene,
    ray: &Ray,
    px: &PixelModel,
    radius: f64,
    hit: Option<&SurfaceHit>,
) -> Vec<(usize, f64, f64, f64)> {
    let mut out = Vec::new();
    scene.seg_bvh.query(
        |b| b.near_ray(ray, px, radius),
        |i| {
            let s = &scene.segs[i];
            let (t, u) = ray_segment(ray, s.a, s.b);
            let p = s.a.lerp(s.b, u);
            let d = px.ray_px(ray, p);
            if d <= radius && visible(px, hit, p, t) {
                out.push((i, t, u, d));
            }
        },
    );
    out
}

/// Candidate (pixel distance `d`, ray parameter `t`) beats (`bd`, `bt`): clearly nearer the
/// cursor, or about as near and nearer the eye.
pub fn closer(d: f64, t: f64, bd: f64, bt: f64) -> bool {
    if (d - bd).abs() <= 0.25 {
        t < bt
    } else {
        d < bd
    }
}

pub fn pick(scene: &Scene, q: &PickQuery) -> Option<Picked> {
    let ray = Ray::new(DVec3::from_array(q.origin), DVec3::from_array(q.dir))?;
    let px = &q.px;
    let hit = surface_hit(scene, &ray);
    let near = near_segments(scene, &ray, px, q.point_px.max(q.edge_px), hit.as_ref());
    // Vertices first; on a tie (within a quarter pixel) the one nearer the eye wins.
    let mut best_v: Option<(f64, f64, DVec3, Owner)> = None;
    for &(i, _, _, _) in &near {
        let s = &scene.segs[i];
        for p in [s.a, s.b] {
            let d = px.ray_px(&ray, p);
            let t = ray.param(p);
            if d <= q.point_px
                && visible(px, hit.as_ref(), p, t)
                && best_v.is_none_or(|(bd, bt, _, _)| closer(d, t, bd, bt))
            {
                best_v = Some((d, t, p, s.owner));
            }
        }
    }
    if let Some((_, _, p, owner)) = best_v {
        return Some(Picked {
            entity: Entity::Vertex {
                owner,
                at: p.to_array(),
            },
            point: p.to_array(),
            surface: hit,
        });
    }
    let best_e = near.iter().filter(|(_, _, _, d)| *d <= q.edge_px).fold(
        None,
        |best: Option<&(usize, f64, f64, f64)>, c| match best {
            Some(b) if !closer(c.3, c.1, b.3, b.1) => Some(b),
            _ => Some(c),
        },
    );
    if let Some(&(i, _, u, _)) = best_e {
        let s = &scene.segs[i];
        let entity = if s.owner.kind == OwnerKind::Guide {
            Entity::Guide { guide: s.owner.id }
        } else {
            Entity::Edge {
                owner: s.owner,
                a: s.a.to_array(),
                b: s.b.to_array(),
                drawn: s.drawn,
            }
        };
        return Some(Picked {
            entity,
            point: s.a.lerp(s.b, u).to_array(),
            surface: hit,
        });
    }
    let h = hit?;
    let entity = match h.at {
        SurfaceRef::Face { object, face } => Entity::Face { object, face },
        SurfaceRef::Region { sketch, region } => Entity::Region { sketch, region },
    };
    Some(Picked {
        entity,
        point: h.point,
        surface: Some(h),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_kernel::model::{Model, Op};

    fn boxed() -> Scene {
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
        m.apply(&Op::AddRectangle {
            sketch: 1,
            x: 2.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        })
        .unwrap();
        Scene::build(&m)
    }

    /// Looking straight down from z = 10, 1 mm per pixel.
    fn down(x: f64, y: f64) -> PickQuery {
        PickQuery {
            origin: [x, y, 10.0],
            dir: [0.0, 0.0, -1.0],
            px: PixelModel {
                ortho: true,
                scale: 0.001,
                eye: [x, y, 10.0],
                forward: [0.0, 0.0, -1.0],
            },
            point_px: 8.0,
            edge_px: 5.0,
        }
    }

    #[test]
    fn vertices_beat_edges_beat_faces() {
        let s = boxed();
        let p = pick(&s, &down(1.005, 1.004)).unwrap();
        assert!(
            matches!(p.entity, Entity::Vertex { at, .. } if at == [1.0, 1.0, 1.0]),
            "{p:?}"
        );
        let p = pick(&s, &down(0.5, 1.003)).unwrap();
        assert!(
            matches!(
                p.entity,
                Entity::Edge {
                    owner: Owner {
                        kind: OwnerKind::Object,
                        id: 1
                    },
                    drawn: false,
                    ..
                }
            ),
            "{p:?}"
        );
        let p = pick(&s, &down(0.5, 0.5)).unwrap();
        assert_eq!(p.entity, Entity::Face { object: 1, face: 2 });
        assert_eq!(p.point, [0.5, 0.5, 1.0]);
        let f = p.surface.unwrap();
        assert_eq!(f.frame.normal, [0.0, 0.0, 1.0]);
        // The ground rectangle is a sketch region with sketch edges.
        let p = pick(&s, &down(2.5, 0.5)).unwrap();
        assert!(matches!(p.entity, Entity::Region { sketch: 1, .. }));
        let p = pick(&s, &down(2.5, 0.002)).unwrap();
        assert!(matches!(
            p.entity,
            Entity::Edge {
                owner: Owner {
                    kind: OwnerKind::Sketch,
                    id: 1
                },
                drawn: true,
                ..
            }
        ));
        assert!(pick(&s, &down(5.0, 5.0)).is_none());
    }

    #[test]
    fn hidden_edges_are_not_picked() {
        let s = boxed();
        // The bottom edge y = 0, z = 0 seen from straight above is behind the top face's
        // edge at z = 1 (same screen position): the top edge wins.
        let p = pick(&s, &down(0.5, 0.0005)).unwrap();
        match p.entity {
            Entity::Edge { a, b, .. } => assert!(a[2] == 1.0 && b[2] == 1.0, "{p:?}"),
            other => panic!("{other:?}"),
        }
        // From below, the bottom face hides the top.
        let mut q = down(0.5, 0.5);
        q.origin = [0.5, 0.5, -10.0];
        q.dir = [0.0, 0.0, 1.0];
        q.px.eye = q.origin;
        q.px.forward = q.dir;
        assert_eq!(
            pick(&s, &q).unwrap().entity,
            Entity::Face { object: 1, face: 1 }
        );
    }
}
