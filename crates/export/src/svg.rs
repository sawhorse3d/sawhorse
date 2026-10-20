//! SVG drawings.
//!
//! - [`view`]: a hidden-line drawing of the current view: every edge of every visible solid,
//!   imported mesh and sketch, projected through the camera, with the parts hidden behind
//!   faces removed (edges are sampled and each sample is ray-cast back to the eye through
//!   a BVH of the model's triangles). A parallel view is drawn **to scale** in millimetres
//!   (`width`/`height` in `mm`, 1 mm on paper = 1 mm in the model), so it prints or imports
//!   at true size; a perspective view is drawn in pixels.
//!
//!   The drawing follows the document's edge style (lane 10): no edges when they are off;
//!   every edge, front and back, for the wireframe and X-ray face styles; profiles
//!   (silhouettes and open boundaries) and endpoints in a heavier pen; extensions running
//!   each edge a little past its ends, like pencil lines.
//! - [`outline`]: the 2D outline of one face (or sketch region) for laser cutters and CNC,
//!   in millimetres, lower-left corner at the origin, one closed sub-path per ring with
//!   `fill-rule="evenodd"` so holes stay holes.

use std::fmt::Write as _;

use glam::{DVec2, DVec3};
use sawhorse_inference::bvh::Bvh;
use sawhorse_inference::geom::Aabb;
use sawhorse_kernel::Model;

use crate::{ExportError, MM_PER_M, Scope, dxf::normalised_rings};

/// An edge to draw, with the outward normals of the faces that meet at it (zero: none), and
/// whether it is soft (drawn only where it is the outline, lane 23b).
type DrawEdge = (DVec3, DVec3, [DVec3; 2], bool);

/// Triangles that hide things, and the edges to draw.
type Picture = (Vec<[DVec3; 3]>, Vec<DrawEdge>);

/// The camera of a drawing, as the viewport has it. Metres, Z up.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct ViewSpec {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    /// Parallel (orthographic) projection.
    pub ortho: bool,
    /// Vertical field of view in degrees (perspective).
    #[serde(default = "default_fov")]
    pub fov: f64,
    /// Half the visible height in metres (parallel).
    #[serde(default)]
    pub half_height: f64,
    /// Canvas size in pixels (sets the aspect ratio and the sampling density).
    pub width: f64,
    pub height: f64,
}

fn default_fov() -> f64 {
    35.0
}

struct Camera {
    eye: DVec3,
    f: DVec3,
    r: DVec3,
    u: DVec3,
    spec: ViewSpec,
    /// Output units per metre at the target (parallel: millimetres; perspective: pixels).
    scale: f64,
    w: f64,
    h: f64,
}

impl Camera {
    fn new(s: ViewSpec) -> Result<Camera, ExportError> {
        let eye = DVec3::from_array(s.eye);
        let target = DVec3::from_array(s.target);
        let f = (target - eye).normalize_or_zero();
        let r = f.cross(DVec3::from_array(s.up)).normalize_or_zero();
        if f == DVec3::ZERO || r == DVec3::ZERO || !(s.width > 0.0 && s.height > 0.0) {
            return Err(ExportError::BadView(
                "the camera needs an eye, a target and an up direction that are not parallel"
                    .into(),
            ));
        }
        let u = r.cross(f);
        let aspect = s.width / s.height;
        let (scale, w, h) = if s.ortho {
            if !(s.half_height > 0.0 && s.half_height.is_finite()) {
                return Err(ExportError::BadView(
                    "a parallel view needs a visible height".into(),
                ));
            }
            let h = 2.0 * s.half_height * MM_PER_M;
            (MM_PER_M, h * aspect, h)
        } else {
            if !(s.fov > 1.0 && s.fov < 179.0) {
                return Err(ExportError::BadView("field of view out of range".into()));
            }
            let t = (s.fov.to_radians() / 2.0).tan();
            (s.height / 2.0 / t, s.width, s.height)
        };
        Ok(Camera {
            eye,
            f,
            r,
            u,
            spec: s,
            scale,
            w,
            h,
        })
    }

    /// Drawing coordinates (y down) and depth along the view; `None` behind the eye.
    fn project(&self, p: DVec3) -> Option<(DVec2, f64)> {
        let d = p - self.eye;
        let depth = d.dot(self.f);
        let (x, y) = if self.spec.ortho {
            (d.dot(self.r), d.dot(self.u))
        } else {
            if depth <= 1e-6 {
                return None;
            }
            (d.dot(self.r) / depth, d.dot(self.u) / depth)
        };
        Some((
            DVec2::new(self.w / 2.0 + x * self.scale, self.h / 2.0 - y * self.scale),
            depth,
        ))
    }

    /// Unit direction from `p` back towards the eye.
    fn to_eye(&self, p: DVec3) -> DVec3 {
        if self.spec.ortho {
            -self.f
        } else {
            (self.eye - p).normalize_or_zero()
        }
    }
}

/// Möller-Trumbore: distance along `dir` to the triangle, if hit beyond `t_min`.
fn ray_tri(o: DVec3, dir: DVec3, [a, b, c]: [DVec3; 3], t_min: f64) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-18 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(-1e-12..=1.0 + 1e-12).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < -1e-12 || u + v > 1.0 + 1e-12 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > t_min).then_some(t)
}

fn ray_box(o: DVec3, inv: DVec3, b: &Aabb, t_max: f64) -> bool {
    let t1 = (b.min - o) * inv;
    let t2 = (b.max - o) * inv;
    let lo = t1.min(t2).max_element();
    let hi = t1.max(t2).min_element();
    hi >= lo.max(0.0) && lo <= t_max
}

/// Everything the drawing needs: triangles that hide, and edges to draw.
fn gather(model: &Model, scope: &Scope) -> Result<Picture, ExportError> {
    let mut tris = Vec::new();
    let mut edges = Vec::new();
    for (_, o) in scope.solids(model)? {
        let (pts, ts, _) = crate::solid_triangles(o)?;
        tris.extend(ts.iter().map(|t| t.map(|i| pts[i as usize])));
        let b = o.brep();
        let normal_of = |he: Option<sawhorse_kernel::ids::HalfEdgeId>| {
            he.and_then(|h| b.half_edge(h))
                .and_then(|h| b.lp(h.lp))
                .and_then(|l| b.face(l.face))
                .map_or(DVec3::ZERO, |f| f.plane.normal)
        };
        for (e, edge) in b.edges() {
            // Lane 15: hidden edges are not drawn; soft ones only where the surface turns
            // away from the eye (lane 23b: a softened cylinder is its outline, not 24 lines,
            // and not nothing).
            let flags = o.flags_of(e);
            if flags & sawhorse_kernel::edges::HIDDEN != 0 {
                continue;
            }
            let (p, q) = b.edge_points(e);
            let soft = flags & sawhorse_kernel::edges::SOFT != 0;
            edges.push((p, q, [normal_of(Some(edge.he)), normal_of(edge.twin)], soft));
        }
    }
    for (_, m) in scope.loose(model)? {
        let (ts, _) = m.triangles();
        tris.extend(ts.iter().map(|t| t.map(|i| m.positions[i as usize])));
        let mut seen = std::collections::BTreeSet::new();
        for r in m.faces.iter().flatten() {
            for k in 0..r.len() {
                let (a, b) = (r[k], r[(k + 1) % r.len()]);
                let soft = m.flags_between(a, b)
                    & (sawhorse_kernel::edges::SOFT | sawhorse_kernel::edges::HIDDEN)
                    != 0;
                if seen.insert((a.min(b), a.max(b))) && !soft {
                    edges.push((
                        m.positions[a as usize],
                        m.positions[b as usize],
                        [DVec3::ZERO; 2],
                        false,
                    ));
                }
            }
        }
        for l in &m.lines {
            if seen.insert((l[0].min(l[1]), l[0].max(l[1]))) {
                edges.push((
                    m.positions[l[0] as usize],
                    m.positions[l[1] as usize],
                    [DVec3::ZERO; 2],
                    false,
                ));
            }
        }
    }
    for (sid, s) in model.sketches() {
        if !model.is_sketch_visible(sid) {
            continue;
        }
        let plane = s.plane();
        for (_, [a, b]) in s.edges() {
            if let (Some(p), Some(q)) = (s.vertex(a), s.vertex(b)) {
                edges.push((
                    plane.to_world(p),
                    plane.to_world(q),
                    [DVec3::ZERO; 2],
                    false,
                ));
            }
        }
    }
    Ok((tris, edges))
}

/// Hidden-line SVG of `scope` (plus sketches) seen through `spec`.
pub fn view(model: &Model, scope: &Scope, spec: &ViewSpec) -> Result<String, ExportError> {
    let cam = Camera::new(*spec)?;
    let (tris, edges) = gather(model, scope)?;
    let style = model.style().cloned().unwrap_or_default();
    if edges.is_empty() || !style.edges.edges {
        return Err(ExportError::Empty);
    }
    use sawhorse_kernel::looks::FaceStyle;
    let see_through = matches!(style.face, FaceStyle::Wireframe | FaceStyle::XRay);
    let boxes: Vec<Aabb> = tris.iter().map(|t| Aabb::of(t)).collect();
    let bvh = Bvh::build(&boxes);
    let size = boxes
        .iter()
        .fold(Aabb::EMPTY, |a, b| a.union(*b))
        .corners()
        .iter()
        .fold(0.0f64, |m, p| m.max(p.length()))
        .max(1e-3);
    let eps = size * 1e-7 + 1e-9;
    let visible = |p: DVec3| -> bool {
        if see_through {
            return true;
        }
        let dir = cam.to_eye(p);
        let o = p + dir * eps;
        let t_max = if spec.ortho {
            f64::INFINITY
        } else {
            cam.eye.distance(p)
        };
        let inv = DVec3::ONE / dir;
        let hit = std::cell::Cell::new(false);
        bvh.query(
            |b| !hit.get() && ray_box(o, inv, &b.inflate(eps), t_max),
            |i| {
                if !hit.get() && ray_tri(o, dir, tris[i], eps).is_some_and(|t| t < t_max) {
                    hit.set(true);
                }
            },
        );
        !hit.get()
    };
    let unit = if spec.ortho { "mm" } else { "px" };
    let stroke = if spec.ortho { 0.25 } else { 1.0 };
    let mut paths = String::new();
    let mut heavy = String::new();
    let mut runs = 0usize;
    // Extensions and endpoints in output units (millimetres on paper, or pixels).
    let ext = if style.edges.extensions {
        3.0 * stroke * 4.0
    } else {
        0.0
    };
    let tip = 4.0 * stroke * 4.0;
    for (a, b, n, soft) in edges {
        let (Some((pa, _)), Some((pb, _))) = (cam.project(a), cam.project(b)) else {
            continue;
        };
        if soft && !crate::soft_outline(n, cam.to_eye(a.lerp(b, 0.5))) {
            continue;
        }
        // A profile: one face turns towards the eye and the other away, or only one face.
        let profile = style.edges.profiles && {
            let (h1, h2) = (n[0] != DVec3::ZERO, n[1] != DVec3::ZERO);
            if soft || h1 != h2 {
                true
            } else if h1 {
                let v = cam.to_eye(a.lerp(b, 0.5));
                let (d1, d2) = (n[0].dot(v), n[1].dot(v));
                (d1 > 1e-9 && d2 < -1e-9) || (d1 < -1e-9 && d2 > 1e-9)
            } else {
                false
            }
        };
        let dir2 = (pb - pa).normalize_or_zero();
        // Samples every ~2 output units (at least both ends), capped.
        let steps = ((pa.distance(pb) / 2.0).ceil() as usize).clamp(1, 400);
        let mut run: Option<(DVec2, bool)> = None;
        let mut last = pa;
        let mut emit = |s: DVec2, from_start: bool, e: DVec2, to_end: bool, paths: &mut String| {
            if s.distance(e) <= 1e-9 {
                return;
            }
            let s2 = if from_start { s - dir2 * ext } else { s };
            let e2 = if to_end { e + dir2 * ext } else { e };
            let out = if profile { &mut heavy } else { &mut *paths };
            let _ = write!(out, "M{:.3} {:.3}L{:.3} {:.3}", s2.x, s2.y, e2.x, e2.y);
            if style.edges.endpoints {
                let l = tip.min(s.distance(e) / 3.0);
                if from_start {
                    let q = s + dir2 * l;
                    let _ = write!(heavy, "M{:.3} {:.3}L{:.3} {:.3}", s.x, s.y, q.x, q.y);
                }
                if to_end {
                    let q = e - dir2 * l;
                    let _ = write!(heavy, "M{:.3} {:.3}L{:.3} {:.3}", q.x, q.y, e.x, e.y);
                }
            }
        };
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let p = a.lerp(b, t);
            let Some((q, _)) = cam.project(p) else {
                continue;
            };
            if visible(p) {
                if run.is_none() {
                    run = Some((q, k == 0));
                }
            } else if let Some((s, from_start)) = run.take() {
                if s.distance(last) > 1e-9 {
                    runs += 1;
                }
                emit(s, from_start, last, false, &mut paths);
            }
            last = q;
        }
        if let Some((s, from_start)) = run {
            if s.distance(last) > 1e-9 {
                runs += 1;
            }
            emit(s, from_start, last, true, &mut paths);
        }
    }
    let mut out = String::new();
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.3}{unit}" height="{h:.3}{unit}" viewBox="0 0 {w:.3} {h:.3}">"#,
        w = cam.w,
        h = cam.h
    );
    let _ = writeln!(
        out,
        "  <title>Sawhorse view ({}; {})</title>",
        if spec.ortho {
            "parallel, 1:1 in millimetres"
        } else {
            "perspective, not to scale"
        },
        runs
    );
    let _ = writeln!(
        out,
        r##"  <path d="{paths}" fill="none" stroke="#000" stroke-width="{stroke}" stroke-linecap="round"/>"##
    );
    if !heavy.is_empty() {
        let _ = writeln!(
            out,
            r##"  <path class="profiles" d="{heavy}" fill="none" stroke="#000" stroke-width="{w}" stroke-linecap="round"/>"##,
            w = stroke * 2.5
        );
    }
    out.push_str("</svg>\n");
    Ok(out)
}

/// The 2D outline of a face for cutting (see the module docs). `rings` are in metres in the
/// face's plane frame, outer first.
pub fn outline(rings: &[Vec<DVec2>]) -> Result<String, ExportError> {
    let (rings, size) = normalised_rings(rings)?;
    let mut d = String::new();
    for r in &rings {
        for (k, p) in r.iter().enumerate() {
            // SVG's y runs down: flip so the drawing is the face seen from its front.
            let _ = write!(
                d,
                "{}{:.4} {:.4}",
                if k == 0 { 'M' } else { 'L' },
                p.x,
                size.y - p.y
            );
        }
        d.push('Z');
    }
    let mut out = String::new();
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.4}mm" height="{h:.4}mm" viewBox="0 0 {w:.4} {h:.4}">"#,
        w = size.x,
        h = size.y
    );
    let _ = writeln!(
        out,
        "  <title>Sawhorse face outline, millimetres, 1:1</title>"
    );
    let _ = writeln!(
        out,
        r##"  <path d="{d}" fill="none" fill-rule="evenodd" stroke="#000" stroke-width="0.1"/>"##
    );
    out.push_str("</svg>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn top_view(half: f64) -> ViewSpec {
        ViewSpec {
            eye: [0.05, 0.1, 10.0],
            target: [0.05, 0.1, 0.0],
            up: [0.0, 1.0, 0.0],
            ortho: true,
            fov: 35.0,
            half_height: half,
            width: 800.0,
            height: 800.0,
        }
    }

    fn segments(svg: &str) -> usize {
        svg.matches('M').count()
    }

    #[test]
    fn parallel_view_is_to_scale_and_hides_the_bottom() {
        let m = fixtures::box_model();
        let svg = view(&m, &Scope::visible(&m), &top_view(0.15)).unwrap();
        assert!(
            svg.contains(r#"width="300.000mm" height="300.000mm""#),
            "{svg}"
        );
        // Seen from above: the four top edges (the vertical edges are end-on, zero length,
        // dropped; the bottom edges lie exactly under the top ones). The box is 100 x 200 mm
        // on paper, centred: every drawn point is on the rectangle x 100..200, y 50..250.
        let n = segments(&svg);
        assert!((4..=8).contains(&n), "{svg}");
        let d = svg.split("d=\"").nth(1).unwrap().split('"').next().unwrap();
        for p in d.split(['M', 'L']).filter(|s| !s.is_empty()) {
            let xy: Vec<f64> = p.split(' ').map(|v| v.parse().unwrap()).collect();
            let on = |v: f64, a: f64| (v - a).abs() < 1e-3;
            assert!(
                (on(xy[0], 100.0) || on(xy[0], 200.0)) && (50.0..=250.0).contains(&xy[1])
                    || (on(xy[1], 50.0) || on(xy[1], 250.0)) && (100.0..=200.0).contains(&xy[0]),
                "{p}"
            );
        }
    }

    #[test]
    fn a_softened_cylinder_from_the_front_is_its_two_sides() {
        // Lane 23b: soft edges were left out altogether, so a pulled circle's sides (soft
        // seams where the surface turns away) were not drawn at all; now exactly those two
        // are, and the other seams are not.
        use sawhorse_kernel::curve::{CurveKind, CurveSpec};
        let cylinder = |curve: bool| {
            let mut m = Model::with_ground_sketch();
            let points: Vec<[f64; 3]> = (0..24)
                .map(|k| {
                    let a = std::f64::consts::TAU * k as f64 / 24.0;
                    [0.3 * a.cos(), 0.3 * a.sin(), 0.0]
                })
                .collect();
            m.apply(&sawhorse_kernel::Op::Draw {
                target: sawhorse_kernel::model::DrawTarget::Sketch { sketch: 1 },
                points,
                closed: true,
                curve: curve.then_some(CurveSpec {
                    kind: CurveKind::Circle,
                    centre: Some([0.0; 3]),
                }),
            })
            .unwrap();
            m.apply(&sawhorse_kernel::Op::Extrude {
                sketch: 1,
                region: 1,
                distance: 0.4,
            })
            .unwrap();
            m
        };
        let front = ViewSpec {
            eye: [0.0, -5.0, 0.2],
            target: [0.0, 0.0, 0.2],
            up: [0.0, 0.0, 1.0],
            ortho: true,
            fov: 35.0,
            half_height: 0.5,
            width: 800.0,
            height: 800.0,
        };
        // Upright runs: "Mx yLx y" with the same x and a different y.
        let uprights = |svg: &str| {
            svg.split('M')
                .skip(1)
                .filter_map(|r| {
                    let (a, b) = r.split_once('L')?;
                    let p: Vec<f64> = a.split(' ').filter_map(|x| x.parse().ok()).collect();
                    let q: Vec<f64> = b
                        .trim_end_matches(|c: char| !c.is_ascii_digit())
                        .split(' ')
                        .filter_map(|x| x.parse().ok())
                        .collect();
                    (p.len() == 2 && q.len() == 2).then(|| (p[0], q[0], (p[1] - q[1]).abs()))
                })
                .filter(|(x0, x1, dy)| (x0 - x1).abs() < 1e-3 && *dy > 1.0)
                .map(|(x, _, _)| x)
                .collect::<Vec<f64>>()
        };
        let m = cylinder(true);
        let soft = uprights(&view(&m, &Scope::visible(&m), &front).unwrap());
        assert_eq!(soft.len(), 2, "{soft:?}");
        assert!(((soft[0] - soft[1]).abs() - 600.0).abs() < 1e-3, "{soft:?}");
        let m = cylinder(false);
        let hard = uprights(&view(&m, &Scope::visible(&m), &front).unwrap());
        assert!(hard.len() > 2, "{hard:?}");
    }

    #[test]
    fn iso_view_hides_the_three_back_edges() {
        let m = fixtures::box_model();
        let spec = ViewSpec {
            eye: [1.0, -1.0, 1.0],
            target: [0.05, 0.1, 0.025],
            up: [0.0, 0.0, 1.0],
            ortho: true,
            fov: 35.0,
            half_height: 0.2,
            width: 640.0,
            height: 480.0,
        };
        let svg = view(&m, &Scope::visible(&m), &spec).unwrap();
        assert_eq!(
            segments(&svg),
            9,
            "12 edges, the 3 at the hidden corner removed: {svg}"
        );
        let persp = ViewSpec {
            ortho: false,
            eye: [0.6, -0.6, 0.5],
            ..spec
        };
        let p = view(&m, &Scope::visible(&m), &persp).unwrap();
        assert!(p.contains(r#"width="640.000px""#));
        assert_eq!(segments(&p), 9);
    }

    #[test]
    fn outline_is_millimetres_with_holes() {
        let sq = |o: f64, s: f64| {
            vec![
                DVec2::new(o, o),
                DVec2::new(o + s, o),
                DVec2::new(o + s, o + s),
                DVec2::new(o, o + s),
            ]
        };
        let svg = outline(&[sq(0.0, 0.3), sq(0.1, 0.1)]).unwrap();
        assert!(svg.contains(r#"width="300.0000mm" height="300.0000mm""#));
        assert!(svg.contains(r#"fill-rule="evenodd""#));
        assert_eq!(svg.matches('Z').count(), 2);
        assert!(
            svg.contains("M100.0000 200.0000"),
            "hole at 100 mm, y flipped: {svg}"
        );
        assert!(outline(&[]).is_err());
    }

    #[test]
    fn the_edge_style_shapes_the_drawing() {
        use sawhorse_kernel::looks::{EdgeStyle, FaceStyle, Style};
        let mut m = fixtures::box_model();
        let iso = ViewSpec {
            eye: [1.0, -1.0, 1.0],
            target: [0.05, 0.1, 0.025],
            up: [0.0, 0.0, 1.0],
            ortho: true,
            fov: 35.0,
            half_height: 0.2,
            width: 800.0,
            height: 600.0,
        };
        let plain = view(&m, &Scope::visible(&m), &iso).unwrap();
        assert!(!plain.contains("profiles"));
        let set = |m: &mut Model, s: Style| {
            m.apply(&sawhorse_kernel::Op::SetStyle { style: Some(s) })
                .unwrap();
        };
        // Profiles: the silhouette goes in the heavy pen; the three back edges stay hidden.
        set(
            &mut m,
            Style {
                edges: EdgeStyle {
                    profiles: true,
                    ..EdgeStyle::default()
                },
                ..Style::default()
            },
        );
        let prof = view(&m, &Scope::visible(&m), &iso).unwrap();
        let heavy = prof.lines().find(|l| l.contains("profiles")).unwrap();
        assert_eq!(
            heavy.matches('M').count(),
            6,
            "the six silhouette edges: {heavy}"
        );
        // X-ray: the back edges come through too.
        set(
            &mut m,
            Style {
                face: FaceStyle::XRay,
                ..Style::default()
            },
        );
        let xray = view(&m, &Scope::visible(&m), &iso).unwrap();
        assert_eq!(segments(&xray), 12, "every edge of the box");
        assert_eq!(segments(&plain), 9);
        // Extensions run past the ends; endpoints add heavy ticks; edges off draw nothing.
        set(
            &mut m,
            Style {
                edges: EdgeStyle {
                    extensions: true,
                    endpoints: true,
                    ..EdgeStyle::default()
                },
                ..Style::default()
            },
        );
        let pencil = view(&m, &Scope::visible(&m), &iso).unwrap();
        assert!(pencil.contains("profiles"), "endpoints in the heavy pen");
        assert_ne!(pencil, plain);
        set(
            &mut m,
            Style {
                edges: EdgeStyle {
                    edges: false,
                    ..EdgeStyle::default()
                },
                ..Style::default()
            },
        );
        assert_eq!(view(&m, &Scope::visible(&m), &iso), Err(ExportError::Empty));
    }

    #[test]
    fn bad_views_are_refused() {
        let m = fixtures::box_model();
        let mut s = top_view(0.1);
        s.up = [0.0, 0.0, 1.0];
        assert!(matches!(
            view(&m, &Scope::visible(&m), &s),
            Err(ExportError::BadView(_))
        ));
        assert!(matches!(
            view(&m, &Scope::visible(&m), &top_view(0.0)),
            Err(ExportError::BadView(_))
        ));
    }
}
