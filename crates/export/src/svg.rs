//! SVG drawings.
//!
//! - [`view`]: a hidden-line drawing of the current view: every edge of every visible solid,
//!   imported mesh and sketch, projected through the camera, with the parts hidden behind
//!   faces removed (edges are sampled and each sample is ray-cast back to the eye through
//!   a BVH of the model's triangles). A parallel view is drawn **to scale** in millimetres
//!   (`width`/`height` in `mm`, 1 mm on paper = 1 mm in the model), so it prints or imports
//!   at true size; a perspective view is drawn in pixels.
//! - [`outline`]: the 2D outline of one face (or sketch region) for laser cutters and CNC,
//!   in millimetres, lower-left corner at the origin, one closed sub-path per ring with
//!   `fill-rule="evenodd"` so holes stay holes.

use std::fmt::Write as _;

use glam::{DVec2, DVec3};
use sawhorse_inference::bvh::Bvh;
use sawhorse_inference::geom::Aabb;
use sawhorse_kernel::Model;

use crate::{ExportError, MM_PER_M, Scope, dxf::normalised_rings};

/// Triangles that hide things, and the edges to draw.
type Picture = (Vec<[DVec3; 3]>, Vec<(DVec3, DVec3)>);

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
        for (e, _) in b.edges() {
            edges.push(b.edge_points(e));
        }
    }
    for (_, m) in scope.loose(model)? {
        let (ts, _) = m.triangles();
        tris.extend(ts.iter().map(|t| t.map(|i| m.positions[i as usize])));
        let mut seen = std::collections::BTreeSet::new();
        for r in m.faces.iter().flatten() {
            for k in 0..r.len() {
                let (a, b) = (r[k], r[(k + 1) % r.len()]);
                if seen.insert((a.min(b), a.max(b))) {
                    edges.push((m.positions[a as usize], m.positions[b as usize]));
                }
            }
        }
        for l in &m.lines {
            if seen.insert((l[0].min(l[1]), l[0].max(l[1]))) {
                edges.push((m.positions[l[0] as usize], m.positions[l[1] as usize]));
            }
        }
    }
    for (_, s) in model.sketches() {
        let plane = s.plane();
        for (_, [a, b]) in s.edges() {
            if let (Some(p), Some(q)) = (s.vertex(a), s.vertex(b)) {
                edges.push((plane.to_world(p), plane.to_world(q)));
            }
        }
    }
    Ok((tris, edges))
}

/// Hidden-line SVG of `scope` (plus sketches) seen through `spec`.
pub fn view(model: &Model, scope: &Scope, spec: &ViewSpec) -> Result<String, ExportError> {
    let cam = Camera::new(*spec)?;
    let (tris, edges) = gather(model, scope)?;
    if edges.is_empty() {
        return Err(ExportError::Empty);
    }
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
    let mut runs = 0usize;
    for (a, b) in edges {
        let (Some((pa, _)), Some((pb, _))) = (cam.project(a), cam.project(b)) else {
            continue;
        };
        // Samples every ~2 output units (at least both ends), capped.
        let n = ((pa.distance(pb) / 2.0).ceil() as usize).clamp(1, 400);
        let mut run: Option<DVec2> = None;
        let mut last = pa;
        for k in 0..=n {
            let t = k as f64 / n as f64;
            let p = a.lerp(b, t);
            let Some((q, _)) = cam.project(p) else {
                continue;
            };
            if visible(p) {
                if run.is_none() {
                    run = Some(q);
                }
            } else if let Some(s) = run.take()
                && s.distance(last) > 1e-9
            {
                let _ = write!(paths, "M{:.3} {:.3}L{:.3} {:.3}", s.x, s.y, last.x, last.y);
                runs += 1;
            }
            last = q;
        }
        if let Some(s) = run
            && s.distance(last) > 1e-9
        {
            let _ = write!(paths, "M{:.3} {:.3}L{:.3} {:.3}", s.x, s.y, last.x, last.y);
            runs += 1;
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
