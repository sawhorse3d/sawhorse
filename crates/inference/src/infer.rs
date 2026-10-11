//! Inference: where the cursor snaps for a drawing tool (ADR-0001, lane 3).
//!
//! Every candidate is measured in screen pixels (its distance from the eye ray divided by
//! the world size of a pixel at its own depth), so tolerances feel the same at any zoom and
//! in either projection, and candidates hidden behind a nearer surface are skipped.
//!
//! Order of business:
//! 1. **Points** within `point_px`: endpoints, midpoints, intersections (of edges, guides and
//!    the gesture's axis lines), arc and circle centres, guide points, and "from point"
//!    alignments (where an axis line through the gesture start meets an axis line through a
//!    recently hovered point). The best point wins outright: rank first, then distance.
//! 2. Otherwise **lines** within `line_px`: on an edge, on a guide, along an axis through the
//!    gesture start, parallel or perpendicular to a reference edge, along an axis through a
//!    hovered point. An axis line that crosses a nearby edge snaps to the crossing.
//! 3. Otherwise **on face** (or the drawing plane), with the **ground grid** when the point
//!    lies on the ground and a grid intersection is close on screen.
//!
//! A **drawing plane** (the face a rectangle is being drawn on) keeps every result on that
//! plane: off-plane points are projected onto it along its normal and reported as "from
//! point". A **lock** (arrow keys, or Shift held on an inference) forces the result onto a
//! line through the gesture start; a point snap elsewhere is projected onto that line.

use glam::DVec3;

use crate::InferenceKind;
use crate::geom::{PixelModel, Ray, line_line, ray_plane, ray_segment};
use crate::pick::{
    Entity, PickQuery, Picked, SurfaceHit, near_segments, pick, surface_hit, visible,
};
use crate::scene::{OwnerKind, Scene, SurfaceRef};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlaneQ {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LockQ {
    pub dir: [f64; 3],
    pub kind: InferenceKind,
}

fn yes() -> bool {
    true
}
fn point_px() -> f64 {
    10.0
}
fn line_px() -> f64 {
    7.0
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InferQuery {
    pub origin: [f64; 3],
    pub dir: [f64; 3],
    pub px: PixelModel,
    /// The gesture's start point (axis, parallel and perpendicular lines run from it).
    #[serde(default)]
    pub start: Option<[f64; 3]>,
    /// Keep the result on this plane.
    #[serde(default)]
    pub plane: Option<PlaneQ>,
    /// Force the result onto the line `start + s * dir`.
    #[serde(default)]
    pub lock: Option<LockQ>,
    /// Reference edges for parallel / perpendicular inference.
    #[serde(default)]
    pub refs: Vec<[[f64; 3]; 2]>,
    /// Recently hovered points for "from point" alignment.
    #[serde(default)]
    pub anchors: Vec<[f64; 3]>,
    /// Snap to the ground grid when nothing else is near.
    #[serde(default = "yes")]
    pub grid: bool,
    /// Minor and major grid spacing, metres (1 in / 12 in, or 10 mm / 100 mm).
    #[serde(default)]
    pub grid_step: Option<[f64; 2]>,
    #[serde(default = "point_px")]
    pub point_px: f64,
    #[serde(default = "line_px")]
    pub line_px: f64,
}

/// A line the UI draws for an inference (the axis from the start, the dotted alignment
/// from an anchor point).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InfLine {
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub kind: InferenceKind,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Inference {
    pub kind: InferenceKind,
    pub point: [f64; 3],
    pub tip: String,
    /// The entity the snap came from (an edge, a vertex, a face), if any.
    pub entity: Option<Entity>,
    /// The surface under the cursor, if any.
    pub surface: Option<SurfaceHit>,
    pub lines: Vec<InfLine>,
    /// The nearest edge under the cursor (the UI keeps it as a parallel reference).
    pub edge: Option<[[f64; 3]; 2]>,
    /// What a plain pick returns here (hover highlight, selection).
    pub picked: Option<Picked>,
    /// For a snap on a solid's edge or corner: the face beside it that faces the eye most
    /// (where a shape started there is drawn when the ray itself misses the solid).
    pub adjacent: Option<SurfaceHit>,
}

#[derive(Debug, Clone)]
struct Cand {
    kind: InferenceKind,
    /// Rank (lower wins): the kind's priority, except that an alignment *line* through a
    /// hovered point ranks with the axis lines, below edges.
    rank: u8,
    point: DVec3,
    /// Screen distance from the cursor.
    px: f64,
    /// Ray parameter (depth along the eye ray).
    t: f64,
    entity: Option<Entity>,
    lines: Vec<InfLine>,
}

const AXES: [DVec3; 3] = [DVec3::X, DVec3::Y, DVec3::Z];

fn axis_kind(d: DVec3) -> InferenceKind {
    InferenceKind::axis(d).unwrap_or(InferenceKind::Parallel)
}

fn line(from: DVec3, to: DVec3, kind: InferenceKind) -> InfLine {
    InfLine {
        from: from.to_array(),
        to: to.to_array(),
        kind,
    }
}

struct Ctx {
    ray: Ray,
    px: PixelModel,
    hit: Option<SurfaceHit>,
    plane: Option<(DVec3, DVec3)>,
}

impl Ctx {
    fn on_plane(&self, p: DVec3) -> bool {
        self.plane.is_none_or(|(o, n)| (p - o).dot(n).abs() <= 1e-7)
    }

    fn in_plane(&self, d: DVec3) -> bool {
        self.plane.is_none_or(|(_, n)| d.dot(n).abs() <= 1e-9)
    }

    /// A point candidate: measured on screen, occlusion-tested; off the drawing plane it is
    /// projected onto the plane and becomes a "from point" with a dotted line.
    fn point(&self, kind: InferenceKind, p: DVec3, entity: Option<Entity>) -> Option<Cand> {
        let t = self.ray.param(p);
        if t < 0.0 || !visible(&self.px, self.hit.as_ref(), p, t) {
            return None;
        }
        let (q, kind, lines) = match self.plane {
            Some((o, n)) if (p - o).dot(n).abs() > 1e-7 => {
                let q = p - n * (p - o).dot(n);
                (
                    q,
                    InferenceKind::FromPoint,
                    vec![line(p, q, InferenceKind::FromPoint)],
                )
            }
            _ => (p, kind, Vec::new()),
        };
        Some(Cand {
            kind,
            rank: kind.priority(),
            point: q,
            px: self.px.ray_px(&self.ray, q),
            t: self.ray.param(q),
            entity,
            lines,
        })
    }

    /// The point on the line `a + s d` nearest the eye ray (in front of the eye).
    fn on_line(&self, a: DVec3, d: DVec3) -> Option<(DVec3, f64)> {
        let (s, t) = line_line(a, d, self.ray.origin, self.ray.dir)?;
        if t < 0.0 {
            return None;
        }
        let p = a + d * s;
        Some((p, self.px.ray_px(&self.ray, p)))
    }
}

/// Pixels a candidate gives away per rank step: an endpoint beats a midpoint unless the
/// midpoint is more than two pixels nearer the cursor.
const RANK_PX: f64 = 2.0;

/// Better candidate: lower `priority * RANK_PX + px`; ties within a quarter pixel go to
/// the one nearer the eye.
fn better(a: &Cand, b: &Cand) -> bool {
    let sa = f64::from(a.rank) * RANK_PX + a.px;
    let sb = f64::from(b.rank) * RANK_PX + b.px;
    if (sa - sb).abs() <= 0.25 {
        a.t < b.t
    } else {
        sa < sb
    }
}

fn best(cands: Vec<Cand>) -> Option<Cand> {
    cands
        .into_iter()
        .fold(None, |acc: Option<Cand>, c| match acc {
            Some(b) if !better(&c, &b) => Some(b),
            _ => Some(c),
        })
}

/// Unit directions for the constraint lines through the gesture start.
fn start_directions(q: &InferQuery, ctx: &Ctx) -> Vec<(DVec3, InferenceKind)> {
    let mut dirs: Vec<(DVec3, InferenceKind)> = AXES.iter().map(|&a| (a, axis_kind(a))).collect();
    for r in &q.refs {
        let e = DVec3::from_array(r[1]) - DVec3::from_array(r[0]);
        if e.length() < 1e-9 {
            continue;
        }
        let e = e.normalize();
        if InferenceKind::axis(e).is_none() {
            dirs.push((e, InferenceKind::Parallel));
        }
        let mut perps = Vec::new();
        if let Some((_, n)) = ctx.plane {
            perps.push(n.cross(e));
        }
        perps.push(DVec3::Z.cross(e));
        perps.push(e.cross(DVec3::Z.cross(e)));
        for p in perps {
            if p.length() < 1e-9 {
                continue;
            }
            let p = p.normalize();
            if InferenceKind::axis(p).is_none()
                && !dirs.iter().any(|(d, _)| d.cross(p).length() < 1e-9)
            {
                dirs.push((p, InferenceKind::Perpendicular));
            }
        }
    }
    dirs.retain(|(d, _)| ctx.in_plane(*d));
    dirs
}

pub fn infer(scene: &Scene, q: &InferQuery) -> Option<Inference> {
    let ray = Ray::new(DVec3::from_array(q.origin), DVec3::from_array(q.dir))?;
    let px = q.px;
    let hit = surface_hit(scene, &ray);
    let picked = pick(
        scene,
        &PickQuery {
            origin: q.origin,
            dir: q.dir,
            px,
            point_px: 8.0,
            edge_px: 5.0,
        },
    );
    let plane = q
        .plane
        .map(|p| {
            (
                DVec3::from_array(p.origin),
                DVec3::from_array(p.normal).normalize_or_zero(),
            )
        })
        .filter(|(_, n)| n.length() > 0.5);
    let ctx = Ctx {
        ray,
        px,
        hit,
        plane,
    };
    let start = q.start.map(DVec3::from_array);
    let reach = q.point_px.max(q.line_px);
    let near = near_segments(scene, &ray, &px, reach, hit.as_ref());

    // ---- points ------------------------------------------------------------------------
    let mut points: Vec<Cand> = Vec::new();
    let edge_entity = |i: usize| {
        let s = &scene.segs[i];
        if s.owner.kind == OwnerKind::Guide {
            Entity::Guide { guide: s.owner.id }
        } else {
            Entity::Edge {
                owner: s.owner,
                a: s.a.to_array(),
                b: s.b.to_array(),
                drawn: s.drawn,
            }
        }
    };
    for &(i, _, _, _) in &near {
        let s = &scene.segs[i];
        for p in [s.a, s.b] {
            let v = Entity::Vertex {
                owner: s.owner,
                at: p.to_array(),
            };
            points.extend(ctx.point(InferenceKind::Endpoint, p, Some(v)));
        }
        points.extend(ctx.point(
            InferenceKind::Midpoint,
            (s.a + s.b) * 0.5,
            Some(edge_entity(i)),
        ));
    }
    for a in &scene.arcs {
        points.extend(ctx.point(InferenceKind::Centre, a.centre, None));
    }
    for g in &scene.guides {
        if g.dir == DVec3::ZERO {
            points.extend(ctx.point(
                InferenceKind::GuidePoint,
                g.point,
                Some(Entity::Guide { guide: g.id }),
            ));
        }
    }
    // Intersections of nearby edges with each other and with guide lines.
    let line_near: Vec<usize> = near
        .iter()
        .filter(|c| c.3 <= q.line_px)
        .map(|c| c.0)
        .collect();
    for (k, &i) in line_near.iter().enumerate() {
        let si = &scene.segs[i];
        for &j in &line_near[k + 1..] {
            let sj = &scene.segs[j];
            if let Some(p) = segment_crossing(si.a, si.b, sj.a, sj.b) {
                points.extend(ctx.point(InferenceKind::Intersection, p, None));
            }
        }
        for g in scene.guides.iter().filter(|g| g.dir != DVec3::ZERO) {
            if let Some(p) = segment_line_crossing(si.a, si.b, g.point, g.dir) {
                points.extend(ctx.point(InferenceKind::Intersection, p, None));
            }
        }
    }
    // Guide / guide crossings near the cursor.
    let lines_g: Vec<_> = scene
        .guides
        .iter()
        .filter(|g| g.dir != DVec3::ZERO)
        .collect();
    for (k, g) in lines_g.iter().enumerate() {
        for h in &lines_g[k + 1..] {
            if let Some((s, u)) = line_line(g.point, g.dir, h.point, h.dir) {
                let (p, r) = (g.point + g.dir * s, h.point + h.dir * u);
                if p.distance(r) <= 1e-9 * (1.0 + p.length()) {
                    points.extend(ctx.point(InferenceKind::Intersection, p, None));
                }
            }
        }
    }
    // From point: an axis line through the start meets an axis line through an anchor.
    let anchors: Vec<DVec3> = q
        .anchors
        .iter()
        .map(|&a| DVec3::from_array(a))
        .filter(|a| start.is_none_or(|s| s.distance(*a) > 1e-9))
        .map(|a| match ctx.plane {
            Some((o, n)) => a - n * (a - o).dot(n),
            None => a,
        })
        .collect();
    if let Some(s) = start {
        for (ds, ks) in start_directions(q, &ctx) {
            for &a in &anchors {
                for &da in &AXES {
                    if !ctx.in_plane(da) || ds.cross(da).length() < 1e-9 {
                        continue;
                    }
                    let Some((u, v)) = line_line(s, ds, a, da) else {
                        continue;
                    };
                    let (p, r) = (s + ds * u, a + da * v);
                    if p.distance(r) > 1e-9 * (1.0 + p.length()) || u.abs() < 1e-9 {
                        continue;
                    }
                    if let Some(mut c) = ctx.point(InferenceKind::FromPoint, p, None) {
                        c.kind = InferenceKind::FromPoint;
                        c.lines = vec![line(s, p, ks), line(a, p, InferenceKind::FromPoint)];
                        points.push(c);
                    }
                }
            }
        }
    }
    let points: Vec<Cand> = points.into_iter().filter(|c| c.px <= q.point_px).collect();
    let best_point = best(points);

    // ---- lines -------------------------------------------------------------------------
    let mut lines: Vec<Cand> = Vec::new();
    let mut near_edge: Option<(f64, [[f64; 3]; 2])> = None;
    for &(i, t, u, d) in &near {
        let s = &scene.segs[i];
        if d > q.line_px {
            continue;
        }
        if s.owner.kind != OwnerKind::Guide && near_edge.is_none_or(|(bd, _)| d < bd) {
            near_edge = Some((d, [s.a.to_array(), s.b.to_array()]));
        }
        if !(ctx.on_plane(s.a) && ctx.on_plane(s.b)) {
            continue;
        }
        let p = s.a.lerp(s.b, u);
        lines.push(Cand {
            kind: if s.owner.kind == OwnerKind::Guide {
                InferenceKind::OnGuide
            } else {
                InferenceKind::OnEdge
            },
            rank: InferenceKind::OnEdge.priority(),
            point: p,
            px: d,
            t,
            entity: Some(edge_entity(i)),
            lines: Vec::new(),
        });
    }
    for g in scene.guides.iter().filter(|g| g.dir != DVec3::ZERO) {
        if !(ctx.on_plane(g.point) && ctx.in_plane(g.dir)) {
            continue;
        }
        if let Some((p, d)) = ctx.on_line(g.point, g.dir)
            && d <= q.line_px
            && visible(&px, hit.as_ref(), p, ray.param(p))
        {
            lines.push(Cand {
                kind: InferenceKind::OnGuide,
                rank: InferenceKind::OnGuide.priority(),
                point: p,
                px: d,
                t: ray.param(p),
                entity: Some(Entity::Guide { guide: g.id }),
                lines: Vec::new(),
            });
        }
    }
    let mut constraint: Vec<Cand> = Vec::new();
    if let Some(s) = start {
        for (d, k) in start_directions(q, &ctx) {
            if let Some((p, dpx)) = ctx.on_line(s, d)
                && dpx <= q.line_px
                && p.distance(s) > 1e-9
            {
                let (p, dpx) = snap_length(q, &ctx, s, d, p).unwrap_or((p, dpx));
                constraint.push(Cand {
                    kind: k,
                    rank: k.priority(),
                    point: p,
                    px: dpx,
                    t: ray.param(p),
                    entity: None,
                    lines: vec![line(s, p, k)],
                });
            }
        }
    }
    for &a in &anchors {
        for &d in &AXES {
            if !ctx.in_plane(d) {
                continue;
            }
            if let Some((p, dpx)) = ctx.on_line(a, d)
                && dpx <= q.line_px
                && p.distance(a) > 1e-9
            {
                constraint.push(Cand {
                    kind: InferenceKind::FromPoint,
                    // A line, not a point: ranks with the axis lines, below edges.
                    rank: InferenceKind::AxisX.priority(),
                    point: p,
                    px: dpx,
                    t: ray.param(p),
                    entity: None,
                    lines: vec![line(a, p, InferenceKind::FromPoint)],
                });
            }
        }
    }
    // An axis line that crosses a nearby edge snaps to the crossing (still on the axis).
    let mut crossings: Vec<Cand> = Vec::new();
    for c in &constraint {
        let Some(l) = c.lines.first() else { continue };
        let (a, d) = (
            DVec3::from_array(l.from),
            DVec3::from_array(l.to) - DVec3::from_array(l.from),
        );
        if d.length() < 1e-12 {
            continue;
        }
        for e in lines
            .iter()
            .filter(|e| e.kind == InferenceKind::OnEdge || e.kind == InferenceKind::OnGuide)
        {
            let Some(Entity::Edge { a: ea, b: eb, .. }) = e.entity else {
                continue;
            };
            if let Some(p) = segment_line_crossing(
                DVec3::from_array(ea),
                DVec3::from_array(eb),
                a,
                d.normalize(),
            ) && let Some(mut x) = ctx.point(e.kind, p, e.entity)
                && x.px <= q.point_px
            {
                x.lines = vec![line(a, p, c.kind)];
                crossings.push(x);
            }
        }
    }
    let line_best = best(crossings).or_else(|| best(lines.into_iter().chain(constraint).collect()));

    // ---- surface, plane, grid ------------------------------------------------------------
    let surface_cand = || -> Option<Cand> {
        let (p, kind) = match ctx.plane {
            Some((o, n)) => {
                let t = ray_plane(&ray, o, n).or_else(|| {
                    ray_plane(
                        &Ray {
                            origin: ray.origin,
                            dir: -ray.dir,
                        },
                        o,
                        n,
                    )
                    .map(|t| -t)
                })?;
                let p = ray.at(t);
                let on_hit = hit.is_some_and(|h| {
                    let hp = DVec3::from_array(h.point);
                    (hp - o).dot(n).abs() <= 1e-7 && hp.distance(p) <= 2.0 * px.at(p)
                });
                (
                    p,
                    if on_hit {
                        InferenceKind::OnFace
                    } else {
                        InferenceKind::Free
                    },
                )
            }
            None => match hit {
                Some(h) => (DVec3::from_array(h.point), InferenceKind::OnFace),
                None => {
                    let t = ray_plane(&ray, DVec3::ZERO, DVec3::Z)?;
                    (ray.at(t), InferenceKind::Free)
                }
            },
        };
        Some(Cand {
            kind,
            rank: kind.priority(),
            point: p,
            px: 0.0,
            t: ray.param(p),
            entity: hit.map(|h| match h.at {
                SurfaceRef::Face { object, face } => Entity::Face { object, face },
                SurfaceRef::Region { sketch, region } => Entity::Region { sketch, region },
            }),
            lines: Vec::new(),
        })
    };
    let grid_cand = |c: &Cand| -> Option<Cand> {
        if !q.grid
            || c.point.z.abs() > 1e-9
            || ctx.plane.is_some_and(|(_, n)| n.z.abs() < 1.0 - 1e-9)
        {
            return None;
        }
        // Only on the ground itself or a sketch region lying on it, not on a solid's face.
        if matches!(c.entity, Some(Entity::Face { .. })) {
            return None;
        }
        let [minor, major] = q.grid_step.unwrap_or([0.0254, 0.3048]);
        for step in [minor, major] {
            if step <= 0.0 {
                continue;
            }
            let spacing_px = step / px.at(c.point);
            if spacing_px < 8.0 {
                continue;
            }
            let g = DVec3::new(
                (c.point.x / step).round() * step,
                (c.point.y / step).round() * step,
                0.0,
            );
            let d = px.ray_px(&ray, g);
            if d <= q.point_px.min(spacing_px * 0.4) {
                return Some(Cand {
                    kind: InferenceKind::Grid,
                    rank: InferenceKind::Grid.priority(),
                    point: g,
                    px: d,
                    t: ray.param(g),
                    entity: c.entity,
                    lines: Vec::new(),
                });
            }
            return None;
        }
        None
    };

    let mut chosen = match (best_point, line_best) {
        (Some(p), _) => p,
        (None, Some(l)) => l,
        (None, None) => {
            let s = surface_cand()?;
            grid_cand(&s).unwrap_or(s)
        }
    };

    // ---- lock --------------------------------------------------------------------------
    if let (Some(lock), Some(s)) = (q.lock, start) {
        let d = DVec3::from_array(lock.dir);
        if d.length() > 1e-9 && ctx.in_plane(d.normalize()) {
            let d = d.normalize();
            let on_lock =
                (chosen.point - s).cross(d).length() <= 1e-9 * (1.0 + chosen.point.distance(s));
            if chosen.kind.is_point() && !on_lock {
                let p = s + d * (chosen.point - s).dot(d);
                let from = chosen.point;
                chosen = Cand {
                    kind: InferenceKind::FromPoint,
                    rank: InferenceKind::FromPoint.priority(),
                    point: p,
                    px: px.ray_px(&ray, p),
                    t: ray.param(p),
                    entity: chosen.entity,
                    lines: vec![
                        line(s, p, lock.kind),
                        line(from, p, InferenceKind::FromPoint),
                    ],
                };
            } else if !on_lock {
                let p = ctx.on_line(s, d).map(|(p, _)| p).unwrap_or(s);
                let p = snap_length(q, &ctx, s, d, p).map(|(g, _)| g).unwrap_or(p);
                chosen = Cand {
                    kind: lock.kind,
                    rank: lock.kind.priority(),
                    point: p,
                    px: px.ray_px(&ray, p),
                    t: ray.param(p),
                    entity: None,
                    lines: vec![line(s, p, lock.kind)],
                };
            } else if chosen.lines.is_empty() {
                chosen.lines.push(line(s, chosen.point, lock.kind));
            }
        }
    }

    Some(Inference {
        kind: chosen.kind,
        point: chosen.point.to_array(),
        tip: chosen.kind.label().to_string(),
        entity: chosen.entity,
        surface: hit,
        lines: chosen.lines,
        edge: near_edge.map(|(_, e)| e),
        picked,
        adjacent: adjacent_surface(scene, &ray, chosen.point),
    })
}

/// The face beside a snapped solid edge or corner that faces the eye most, if the point
/// lies on it.
fn adjacent_surface(scene: &Scene, ray: &Ray, p: DVec3) -> Option<SurfaceHit> {
    let mut best: Option<(f64, usize)> = None;
    for s in scene
        .segs
        .iter()
        .filter(|s| s.owner.kind == OwnerKind::Object && crate::pick::seg_contains(s.a, s.b, p))
    {
        for &f in s.faces.iter().flatten() {
            let pl = &scene.surfaces[f].plane;
            if pl.signed_distance(p).abs() > 1e-7 {
                continue;
            }
            let facing = -pl.normal.dot(ray.dir);
            if facing > 0.0 && best.is_none_or(|(b, _)| facing > b) {
                best = Some((facing, f));
            }
        }
    }
    best.map(|(_, f)| {
        let sf = &scene.surfaces[f];
        SurfaceHit {
            at: sf.at,
            frame: crate::pick::Frame::from(&sf.plane),
            point: p.to_array(),
            t: ray.param(p),
        }
    })
}

/// Length snapping along a line from the gesture start: when grid snapping is on, round the
/// length to a whole grid step (1 in, or 1 ft when zoomed out; 10 mm / 100 mm metric) if
/// that point is still within the line tolerance on screen, so lengths drawn by mouse
/// along an axis come out exact.
fn snap_length(q: &InferQuery, ctx: &Ctx, s: DVec3, d: DVec3, p: DVec3) -> Option<(DVec3, f64)> {
    if !q.grid {
        return None;
    }
    let [minor, major] = q.grid_step.unwrap_or([0.0254, 0.3048]);
    let len = (p - s).dot(d);
    for step in [minor, major] {
        if step <= 0.0 || step / ctx.px.at(p) < 8.0 {
            continue;
        }
        let r = (len / step).round() * step;
        if r.abs() < 1e-12 {
            return None;
        }
        let g = s + d * r;
        let gpx = ctx.px.ray_px(&ctx.ray, g);
        return (gpx <= q.line_px).then_some((g, gpx));
    }
    None
}

/// Where two segments cross in 3D (not at a shared endpoint), if they do.
pub fn segment_crossing(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> Option<DVec3> {
    let (u, v) = (b - a, d - c);
    let (s, t) = line_line(a, u, c, v)?;
    let eps = 1e-9;
    if !(-eps..=1.0 + eps).contains(&s) || !(-eps..=1.0 + eps).contains(&t) {
        return None;
    }
    let (p, q) = (a + u * s, c + v * t);
    if p.distance(q) > 1e-9 * (1.0 + u.length() + v.length()) {
        return None;
    }
    let shared = [a, b].iter().any(|x| x.distance(p) <= 1e-9)
        && [c, d].iter().any(|x| x.distance(p) <= 1e-9);
    (!shared).then_some(p)
}

/// Where segment `a`-`b` crosses the infinite line `o + s d`, if it does.
pub fn segment_line_crossing(a: DVec3, b: DVec3, o: DVec3, d: DVec3) -> Option<DVec3> {
    let u = b - a;
    let (s, t) = line_line(a, u, o, d)?;
    if !(-1e-9..=1.0 + 1e-9).contains(&s) {
        return None;
    }
    let (p, q) = (a + u * s, o + d * t);
    (p.distance(q) <= 1e-9 * (1.0 + u.length() + p.length())).then_some(p)
}

/// Closest point of segment `a`-`b` to the ray (helper for tests and tools).
pub fn closest_on_segment(ray: &Ray, a: DVec3, b: DVec3) -> DVec3 {
    let (_, u) = ray_segment(ray, a, b);
    a.lerp(b, u)
}
