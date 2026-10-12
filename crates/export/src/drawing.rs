//! Line drawings of the model for drawing sheets (lane 12, ADR-0010): exact vector
//! hidden-line removal, section cuts honoured, with every visible line tied back to the
//! model points it came from (so dimensions placed on a drawing measure the model).
//!
//! **How a drawing is made.** Every edge of every visible solid, imported mesh and sketch
//! is clipped to the kept side of the active section planes that reach it (when the style's
//! section cuts are on), and so are the triangles of the faces, which hide things; the cuts'
//! caps join the triangles that hide, their outlines join the lines (as cut lines, drawn
//! heavier). The view maps every point to drawing coordinates `(x, y)` and a depth:
//!
//! - parallel (the orthographic standard views, and a scene's parallel camera): `x` and `y`
//!   are metres along the view's right and up directions, depth along the view direction;
//! - perspective: the projective map `(x, y, depth) = (s r.d / z, s u.d / z, -1 / z)` with
//!   `d` from the eye and `z = f.d`, which keeps lines straight and planes flat, so the same
//!   exact test works (the drawing is not to scale: sheets mark it NTS).
//!
//! In those coordinates an edge is hidden by a triangle exactly where it lies inside the
//! triangle's outline (a closed test: an edge on the outline counts as inside) and behind
//! its plane (by more than a depth tolerance, so a face never hides its own edges). Both are
//! linear along the edge, so each triangle hides at most one interval of it; the union of
//! those intervals, found through a 2D BVH, is the hidden part, the rest is visible. No
//! sampling, no raster: a box drawn face-on is its four front edges, and in a three-quarter
//! view its nine visible edges, exactly.
//!
//! Profiles (silhouettes and the boundaries of open meshes) are marked so a drawing can
//! weigh them heavier; hidden parts can be kept as dashed lines. Deterministic: the lines
//! come in model order, and the drawing is hashed.

use glam::{DVec2, DVec3};
use sawhorse_inference::bvh::Bvh;
use sawhorse_inference::geom::Aabb;
use sawhorse_kernel::Model;
use sawhorse_kernel::hash::StableHasher;
use sawhorse_kernel::plane::Plane;

use crate::ExportError;

/// The standard views of a sheet viewport (Z up; the model's front is the -Y side).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StdView {
    /// Plan: looking down.
    Top,
    Bottom,
    /// Looking at the -Y side (towards +Y), like the app's Front view.
    Front,
    Back,
    /// Looking at the -X side (towards +X).
    Left,
    /// Looking at the +X side (towards -X), like the app's Right view.
    Right,
    /// The app's three-quarter view, parallel (not to scale on paper).
    Iso,
}

impl StdView {
    /// (view direction, up) for the view.
    pub fn axes(self) -> (DVec3, DVec3) {
        match self {
            StdView::Top => (-DVec3::Z, DVec3::Y),
            StdView::Bottom => (DVec3::Z, -DVec3::Y),
            StdView::Front => (DVec3::Y, DVec3::Z),
            StdView::Back => (-DVec3::Y, DVec3::Z),
            StdView::Left => (DVec3::X, DVec3::Z),
            StdView::Right => (-DVec3::X, DVec3::Z),
            StdView::Iso => (DVec3::new(1.0, 1.0, -1.0).normalize(), DVec3::Z),
        }
    }

    /// Orthographic and square to the model's axes: a drawing to scale.
    pub fn to_scale(self) -> bool {
        self != StdView::Iso
    }
}

/// A camera, as scenes store it (metres, Z up).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CameraSpec {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    /// Parallel projection.
    #[serde(default)]
    pub parallel: bool,
    /// Vertical field of view, degrees (perspective).
    #[serde(default = "default_fov")]
    pub fov: f64,
}

fn default_fov() -> f64 {
    35.0
}

/// What a drawing looks at.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewSource {
    Standard { view: StdView },
    Camera { camera: CameraSpec },
}

/// The view's frame: how model points become drawing coordinates.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Frame {
    /// Parallel projection (drawing units are model metres).
    pub ortho: bool,
    pub right: [f64; 3],
    pub up: [f64; 3],
    /// The view direction (into the drawing).
    pub forward: [f64; 3],
    /// Perspective: the eye and the scale `s` (the eye's distance to its target, so things
    /// at the target come out at about their size).
    pub eye: [f64; 3],
    pub scale: f64,
}

/// Perspective: nothing nearer the eye than this is drawn (metres).
const NEAR: f64 = 1e-3;

impl Frame {
    /// The frame for a view source.
    pub fn of(source: &ViewSource) -> Result<Frame, ExportError> {
        let (forward, up, ortho, eye, scale) = match source {
            ViewSource::Standard { view } => {
                let (f, u) = view.axes();
                (f, u, true, DVec3::ZERO, 1.0)
            }
            ViewSource::Camera { camera } => {
                let eye = DVec3::from_array(camera.eye);
                let target = DVec3::from_array(camera.target);
                let f = (target - eye).normalize_or_zero();
                if f == DVec3::ZERO || !f.is_finite() {
                    return Err(ExportError::BadView(
                        "the camera's eye and target are the same point".into(),
                    ));
                }
                (
                    f,
                    DVec3::from_array(camera.up),
                    camera.parallel,
                    eye,
                    eye.distance(target),
                )
            }
        };
        let right = forward.cross(up).normalize_or_zero();
        if right == DVec3::ZERO || !right.is_finite() {
            return Err(ExportError::BadView(
                "the camera's up is along its view".into(),
            ));
        }
        let up = right.cross(forward);
        Ok(Frame {
            ortho,
            right: right.to_array(),
            up: up.to_array(),
            forward: forward.to_array(),
            eye: eye.to_array(),
            scale,
        })
    }

    fn r(&self) -> DVec3 {
        DVec3::from_array(self.right)
    }
    fn u(&self) -> DVec3 {
        DVec3::from_array(self.up)
    }
    fn f(&self) -> DVec3 {
        DVec3::from_array(self.forward)
    }
    fn e(&self) -> DVec3 {
        DVec3::from_array(self.eye)
    }

    /// Drawing coordinates and depth of a model point (`None`: behind the eye).
    pub fn project(&self, p: DVec3) -> Option<DVec3> {
        if self.ortho {
            return Some(DVec3::new(
                p.dot(self.r()),
                p.dot(self.u()),
                p.dot(self.f()),
            ));
        }
        let d = p - self.e();
        let z = d.dot(self.f());
        if z < NEAR * 0.5 {
            return None;
        }
        Some(DVec3::new(
            self.scale * d.dot(self.r()) / z,
            self.scale * d.dot(self.u()) / z,
            -1.0 / z,
        ))
    }

    /// The model point at drawing coordinates and depth (the inverse of [`Frame::project`]).
    pub fn unproject(&self, q: DVec3) -> DVec3 {
        if self.ortho {
            return self.r() * q.x + self.u() * q.y + self.f() * q.z;
        }
        let z = -1.0 / q.z;
        self.e()
            + self.f() * z
            + self.r() * (q.x * z / self.scale)
            + self.u() * (q.y * z / self.scale)
    }

    /// The view plane's coordinates of a model point, metres (what a dimension measures):
    /// along right and up, whatever the projection.
    pub fn plane_xy(&self, p: DVec3) -> DVec2 {
        DVec2::new(p.dot(self.r()), p.dot(self.u()))
    }
}

/// What a line of a drawing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Edge,
    /// A silhouette or an open boundary: drawn heavier.
    Profile,
    /// Where a section plane cuts a solid.
    Cut,
    /// Hidden by something in front (when asked for): drawn dashed.
    Hidden,
}

/// A line of a drawing: its ends in drawing coordinates and the model points they are.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Line {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub a3: [f64; 3],
    pub b3: [f64; 3],
    pub kind: LineKind,
}

/// A filled area (a section cap): rings in drawing coordinates, outer first.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Fill {
    pub rings: Vec<Vec<[f64; 2]>>,
}

/// A drawing (see the module docs).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Drawing {
    pub frame: Frame,
    pub lines: Vec<Line>,
    pub fills: Vec<Fill>,
    /// `[x0, y0, x1, y1]` of everything drawn (`None`: nothing).
    pub bounds: Option<[f64; 4]>,
    pub hash: String,
}

/// What to draw besides the visible lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct DrawOptions {
    /// Keep the hidden parts of edges, as [`LineKind::Hidden`].
    #[serde(default)]
    pub hidden: bool,
}

/// An edge to draw: model points, the outward normals of the faces at it (zero: none), and
/// its kind before visibility.
struct Edge3 {
    a: DVec3,
    b: DVec3,
    normals: [DVec3; 2],
    cut: bool,
}

/// The kept side of a plane: `(p - origin) . normal >= 0`.
type Keep = (DVec3, DVec3);

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

/// Everything a view of `model` draws: triangles that hide (model points), edges, and the
/// section caps facing the eye (model points, rings).
struct Picture {
    tris: Vec<[DVec3; 3]>,
    edges: Vec<Edge3>,
    caps: Vec<Vec<Vec<DVec3>>>,
}

fn gather(model: &Model, frame: &Frame) -> Result<Picture, ExportError> {
    let cuts = model.style().is_none_or(|s| s.section_cuts);
    let fill = model.style().is_none_or(|s| s.section_fill);
    let planes_over = |parent: Option<sawhorse_kernel::ids::NodeId>| -> Vec<Keep> {
        if !cuts {
            return Vec::new();
        }
        model
            .active_sections_over(parent)
            .into_iter()
            .filter_map(|id| model.section(id))
            .map(|s| (s.origin, s.normal))
            .collect()
    };
    let mut pic = Picture {
        tris: Vec::new(),
        edges: Vec::new(),
        caps: Vec::new(),
    };
    let add_tri = |pic: &mut Picture, t: [DVec3; 3], clip: &[Keep]| {
        if clip.is_empty()
            || t.iter()
                .all(|p| clip.iter().all(|(o, n)| (*p - *o).dot(*n) >= 0.0))
        {
            pic.tris.push(t);
            return;
        }
        let poly = clip_polygon(&t, clip);
        for k in 1..poly.len().saturating_sub(1) {
            pic.tris.push([poly[0], poly[k], poly[k + 1]]);
        }
    };
    for (id, o) in model.objects() {
        if !model.is_visible(id) {
            continue;
        }
        let clip = planes_over(o.membership().parent);
        let (pts, ts, _) = crate::solid_triangles(o)?;
        for t in &ts {
            add_tri(&mut pic, t.map(|i| pts[i as usize]), &clip);
        }
        let b = o.brep();
        let normal_of = |he: Option<sawhorse_kernel::ids::HalfEdgeId>| {
            he.and_then(|h| b.half_edge(h))
                .and_then(|h| b.lp(h.lp))
                .and_then(|l| b.face(l.face))
                .map_or(DVec3::ZERO, |f| f.plane.normal)
        };
        for (e, edge) in b.edges() {
            let (p, q) = b.edge_points(e);
            if let Some((p, q)) = clip_segment(p, q, &clip) {
                pic.edges.push(Edge3 {
                    a: p,
                    b: q,
                    normals: [normal_of(Some(edge.he)), normal_of(edge.twin)],
                    cut: false,
                });
            }
        }
    }
    for (id, m) in model.meshes() {
        if !model.is_mesh_visible(id) {
            continue;
        }
        let clip = planes_over(m.member.parent);
        let (ts, _) = m.triangles();
        for t in &ts {
            add_tri(&mut pic, t.map(|i| m.positions[i as usize]), &clip);
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut push = |pic: &mut Picture, a: u32, b: u32| {
            if seen.insert((a.min(b), a.max(b)))
                && let Some((p, q)) =
                    clip_segment(m.positions[a as usize], m.positions[b as usize], &clip)
            {
                pic.edges.push(Edge3 {
                    a: p,
                    b: q,
                    normals: [DVec3::ZERO; 2],
                    cut: false,
                });
            }
        };
        for r in m.faces.iter().flatten() {
            for k in 0..r.len() {
                push(&mut pic, r[k], r[(k + 1) % r.len()]);
            }
        }
        for l in &m.lines {
            push(&mut pic, l[0], l[1]);
        }
    }
    for sid in model.sketch_ids() {
        if !model.is_sketch_visible(sid) {
            continue;
        }
        let Some(s) = model.sketch(sid) else {
            continue;
        };
        let clip = planes_over(
            model
                .sketch_parent(sid)
                .map(sawhorse_kernel::ids::from_public),
        );
        let plane: Plane = *s.plane();
        for (_, [a, b]) in s.edges() {
            if let (Some(p), Some(q)) = (s.vertex(a), s.vertex(b))
                && let Some((p, q)) = clip_segment(plane.to_world(p), plane.to_world(q), &clip)
            {
                pic.edges.push(Edge3 {
                    a: p,
                    b: q,
                    normals: [DVec3::ZERO; 2],
                    cut: false,
                });
            }
        }
    }
    if cuts {
        for (id, s) in model.sections() {
            if !s.active {
                continue;
            }
            let cut = model
                .section_cut(id)
                .map_err(|e| ExportError::BadView(e.to_string()))?;
            let (pos, tris) = cut.triangles();
            for t in &tris {
                pic.tris.push(t.map(|i| pos[i as usize]));
            }
            for [a, b] in &cut.outline {
                pic.edges.push(Edge3 {
                    a: cut.to_world(*a),
                    b: cut.to_world(*b),
                    normals: [DVec3::ZERO; 2],
                    cut: true,
                });
            }
            // A cap is seen when the eye looks at its face: from the side cut away.
            let facing = if frame.ortho {
                frame.f().dot(s.normal) > 1e-9
            } else {
                (s.origin - frame.e()).dot(s.normal) > 0.0
            };
            if fill && facing {
                for c in &cut.caps {
                    let ring =
                        |r: &Vec<[f64; 2]>| r.iter().map(|p| cut.to_world(*p)).collect::<Vec<_>>();
                    pic.caps.push(
                        std::iter::once(&c.outer)
                            .chain(&c.holes)
                            .map(ring)
                            .collect(),
                    );
                }
            }
        }
    }
    Ok(pic)
}

/// A triangle in drawing coordinates, counter-clockwise, with its depth plane.
#[derive(Clone, Copy)]
struct Tri2 {
    p: [DVec2; 3],
    /// depth = d0 + dx x + dy y.
    depth: [f64; 3],
}

impl Tri2 {
    fn of(q: [DVec3; 3]) -> Option<Tri2> {
        let mut p = [q[0].truncate(), q[1].truncate(), q[2].truncate()];
        let mut z = [q[0].z, q[1].z, q[2].z];
        let area = (p[1] - p[0]).perp_dot(p[2] - p[0]);
        if !area.is_finite() || area.abs() < 1e-24 {
            return None; // seen edge-on: hides nothing
        }
        if area < 0.0 {
            p.swap(1, 2);
            z.swap(1, 2);
        }
        // Solve the depth plane through the three points.
        let (e1, e2) = (p[1] - p[0], p[2] - p[0]);
        let det = e1.perp_dot(e2);
        let (dz1, dz2) = (z[1] - z[0], z[2] - z[0]);
        let dx = (dz1 * e2.y - dz2 * e1.y) / det;
        let dy = (dz2 * e1.x - dz1 * e2.x) / det;
        let d0 = z[0] - dx * p[0].x - dy * p[0].y;
        Some(Tri2 {
            p,
            depth: [d0, dx, dy],
        })
    }

    fn depth_at(&self, q: DVec2) -> f64 {
        self.depth[0] + self.depth[1] * q.x + self.depth[2] * q.y
    }

    /// The interval of `t` in [0, 1] where `a + t (b - a)` is inside the closed triangle
    /// (within `tol` of its outline counts as inside).
    fn inside(&self, a: DVec2, b: DVec2, tol: f64) -> Option<(f64, f64)> {
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        for k in 0..3 {
            let (v, w) = (self.p[k], self.p[(k + 1) % 3]);
            let e = w - v;
            let len = e.length();
            // g(t) = e x (P(t) - v) / |e| >= -tol, linear in t.
            let g0 = e.perp_dot(a - v) / len;
            let g1 = e.perp_dot(b - v) / len;
            let (lo2, hi2) = half_interval(g0 + tol, g1 + tol)?;
            lo = lo.max(lo2);
            hi = hi.min(hi2);
            if lo > hi {
                return None;
            }
        }
        Some((lo, hi))
    }
}

/// Where the linear function going from `g0` (t = 0) to `g1` (t = 1) is not negative.
fn half_interval(g0: f64, g1: f64) -> Option<(f64, f64)> {
    match (g0 >= 0.0, g1 >= 0.0) {
        (true, true) => Some((0.0, 1.0)),
        (false, false) => None,
        (true, false) => Some((0.0, g0 / (g0 - g1))),
        (false, true) => Some((g0 / (g0 - g1), 1.0)),
    }
}

/// Parameter intervals along an edge.
type Intervals = Vec<(f64, f64)>;

/// The intervals of [0, 1] not covered by `hidden` (visible), and `hidden` merged.
fn complement(mut hidden: Intervals) -> (Intervals, Intervals) {
    hidden.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut merged: Intervals = Vec::new();
    for (a, b) in hidden {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 1e-12 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut visible = Vec::new();
    let mut at = 0.0;
    for &(a, b) in &merged {
        if a > at {
            visible.push((at, a));
        }
        at = at.max(b);
    }
    if at < 1.0 {
        visible.push((at, 1.0));
    }
    (visible, merged)
}

/// The drawing of `model` seen through `source` (see the module docs).
pub fn draw(
    model: &Model,
    source: &ViewSource,
    opts: &DrawOptions,
) -> Result<Drawing, ExportError> {
    let frame = Frame::of(source)?;
    let mut pic = gather(model, &frame)?;
    // Perspective: what lies behind the eye's near plane is not drawn.
    if !frame.ortho {
        let near: Keep = (frame.e() + frame.f() * NEAR, frame.f());
        let tris = std::mem::take(&mut pic.tris);
        for t in tris {
            let poly = clip_polygon(&t, &[near]);
            for k in 1..poly.len().saturating_sub(1) {
                pic.tris.push([poly[0], poly[k], poly[k + 1]]);
            }
        }
        pic.edges
            .retain_mut(|e| match clip_segment(e.a, e.b, &[near]) {
                Some((a, b)) => {
                    e.a = a;
                    e.b = b;
                    true
                }
                None => false,
            });
    }
    let project = |p: DVec3| frame.project(p).expect("in front of the eye");
    let tris: Vec<Tri2> = pic
        .tris
        .iter()
        .filter_map(|t| Tri2::of(t.map(project)))
        .collect();
    let boxes: Vec<Aabb> = tris
        .iter()
        .map(|t| Aabb::of(&t.p.map(|q| q.extend(0.0))))
        .collect();
    let bvh = Bvh::build(&boxes);
    // Tolerances from the drawing's size.
    let size = boxes
        .iter()
        .fold(Aabb::EMPTY, |a, b| a.union(*b))
        .corners()
        .iter()
        .fold(1e-3f64, |m, p| m.max(p.abs().max_element()));
    let tol2 = size * 1e-9;
    // Pieces shorter than this are left out: no scale shows them, and where a hidden edge
    // meets the outline at a shared corner the depth tolerance leaves a sliver this short.
    let min_len = size * 1e-7;
    let depth_tol = if frame.ortho {
        size * 1e-9 + 1e-12
    } else {
        // depth = -1/z: a relative tolerance of the inverse distance.
        1e-9
    };
    let view_dir = |p: DVec3| {
        if frame.ortho {
            frame.f()
        } else {
            (p - frame.e()).normalize_or_zero()
        }
    };
    let mut lines = Vec::new();
    for e in &pic.edges {
        let (qa, qb) = (project(e.a), project(e.b));
        let (a2, b2) = (qa.truncate(), qb.truncate());
        if a2.distance(b2) <= tol2 {
            continue; // seen end-on
        }
        let lo = a2.min(b2) - DVec2::splat(tol2);
        let hi = a2.max(b2) + DVec2::splat(tol2);
        let mut hidden: Vec<(f64, f64)> = Vec::new();
        bvh.query(
            |bb| bb.min.x <= hi.x && bb.max.x >= lo.x && bb.min.y <= hi.y && bb.max.y >= lo.y,
            |i| {
                let t = &tris[i];
                let Some((t0, t1)) = t.inside(a2, b2, tol2) else {
                    return;
                };
                // Behind the triangle's plane by more than the tolerance: h(t) > 0.
                let h0 = qa.z - t.depth_at(a2) - depth_tol;
                let h1 = qb.z - t.depth_at(b2) - depth_tol;
                if let Some((s0, s1)) = half_interval_strict(h0, h1) {
                    let (u0, u1) = (t0.max(s0), t1.min(s1));
                    if u1 > u0 {
                        hidden.push((u0, u1));
                    }
                }
            },
        );
        let kind = if e.cut {
            LineKind::Cut
        } else {
            let (n0, n1) = (e.normals[0], e.normals[1]);
            let v = view_dir(e.a.lerp(e.b, 0.5));
            let (h0, h1) = (n0 != DVec3::ZERO, n1 != DVec3::ZERO);
            let profile = if h0 != h1 {
                true
            } else if h0 {
                let (d0, d1) = (n0.dot(v), n1.dot(v));
                (d0 < -1e-9 && d1 > 1e-9) || (d0 > 1e-9 && d1 < -1e-9)
            } else {
                false
            };
            if profile {
                LineKind::Profile
            } else {
                LineKind::Edge
            }
        };
        let (visible, hid) = complement(hidden);
        let piece = |(s, t): (f64, f64), kind: LineKind| -> Option<Line> {
            let (p, q) = (qa.lerp(qb, s), qa.lerp(qb, t));
            if p.truncate().distance(q.truncate()) <= min_len {
                return None;
            }
            Some(Line {
                a: p.truncate().to_array(),
                b: q.truncate().to_array(),
                a3: frame.unproject(p).to_array(),
                b3: frame.unproject(q).to_array(),
                kind,
            })
        };
        lines.extend(visible.into_iter().filter_map(|iv| piece(iv, kind)));
        if opts.hidden && !e.cut {
            lines.extend(hid.into_iter().filter_map(|iv| piece(iv, LineKind::Hidden)));
        }
    }
    let fills: Vec<Fill> = pic
        .caps
        .iter()
        .map(|rings| Fill {
            rings: rings
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|p| project(*p).truncate().to_array())
                        .collect()
                })
                .collect(),
        })
        .collect();
    let mut bounds: Option<[f64; 4]> = None;
    let mut grow = |p: [f64; 2]| {
        let b = bounds.get_or_insert([p[0], p[1], p[0], p[1]]);
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    };
    for l in &lines {
        grow(l.a);
        grow(l.b);
    }
    for f in &fills {
        f.rings.iter().flatten().for_each(|p| grow(*p));
    }
    let mut h = StableHasher::default();
    h.tag("drawing");
    for v in [frame.right, frame.up, frame.forward, frame.eye] {
        v.iter().for_each(|x| h.len(*x));
    }
    h.u64(u64::from(frame.ortho));
    h.usize(lines.len());
    for l in &lines {
        l.a.iter().chain(&l.b).for_each(|x| h.len(*x));
        h.u64(l.kind as u64);
    }
    h.usize(fills.len());
    for f in &fills {
        for r in &f.rings {
            h.usize(r.len());
            r.iter().flatten().for_each(|x| h.len(*x));
        }
    }
    Ok(Drawing {
        frame,
        lines,
        fills,
        bounds,
        hash: format!("{:016x}", h.finish()),
    })
}

/// Where the linear function from `h0` to `h1` is strictly positive.
fn half_interval_strict(h0: f64, h1: f64) -> Option<(f64, f64)> {
    match (h0 > 0.0, h1 > 0.0) {
        (true, true) => Some((0.0, 1.0)),
        (false, false) => None,
        (true, false) => Some((0.0, h0 / (h0 - h1))),
        (false, true) => Some((h0 / (h0 - h1), 1.0)),
    }
}

/// The drawing of scene `scene` (its tags, style and sections) or of the model as it is.
pub fn draw_scene(
    model: &Model,
    scene: Option<&str>,
    source: &ViewSource,
    opts: &DrawOptions,
) -> Result<Drawing, ExportError> {
    match scene {
        Some(name) => {
            let m = model
                .as_scene(name)
                .map_err(|e| ExportError::BadView(e.to_string()))?;
            draw(&m, source, opts)
        }
        None => draw(model, source, opts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_kernel::model::{Model, Op};

    fn cube() -> Model {
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
        m
    }

    fn std(view: StdView) -> ViewSource {
        ViewSource::Standard { view }
    }

    fn visible(d: &Drawing) -> usize {
        d.lines
            .iter()
            .filter(|l| l.kind != LineKind::Hidden)
            .count()
    }

    #[test]
    fn a_cube_face_on_is_its_four_front_edges() {
        let m = cube();
        for v in [
            StdView::Front,
            StdView::Top,
            StdView::Left,
            StdView::Right,
            StdView::Back,
            StdView::Bottom,
        ] {
            let d = draw(&m, &std(v), &DrawOptions::default()).unwrap();
            assert_eq!(visible(&d), 4, "{v:?}: {:?}", d.lines);
            // Each is a silhouette (its other face is seen edge-on, turned away).
            let b = d.bounds.unwrap();
            assert!((b[2] - b[0] - 1.0).abs() < 1e-12 && (b[3] - b[1] - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn a_cube_in_a_three_quarter_view_shows_nine_edges_and_hides_three() {
        let m = cube();
        // A generic parallel view from the -X, -Y, +Z side, not along a diagonal.
        let cam = ViewSource::Camera {
            camera: CameraSpec {
                eye: [-3.0, -4.0, 5.0],
                target: [0.5, 0.5, 0.5],
                up: [0.0, 0.0, 1.0],
                parallel: true,
                fov: 35.0,
            },
        };
        let d = draw(&m, &cam, &DrawOptions { hidden: true }).unwrap();
        assert_eq!(visible(&d), 9, "{:?}", d.lines);
        assert_eq!(
            d.lines
                .iter()
                .filter(|l| l.kind == LineKind::Hidden)
                .count(),
            3
        );
        // The six silhouette edges are profiles.
        assert_eq!(
            d.lines
                .iter()
                .filter(|l| l.kind == LineKind::Profile)
                .count(),
            6
        );
        // The same in perspective: the projective map keeps it exact.
        let persp = ViewSource::Camera {
            camera: CameraSpec {
                parallel: false,
                ..match cam {
                    ViewSource::Camera { camera } => camera,
                    _ => unreachable!(),
                }
            },
        };
        let p = draw(&m, &persp, &DrawOptions::default()).unwrap();
        assert_eq!(visible(&p), 9);
        // Every visible line knows its model points: the anchors project back onto it.
        for l in &d.lines {
            let q = d.frame.project(DVec3::from_array(l.a3)).unwrap();
            assert!((q.truncate() - DVec2::from_array(l.a)).length() < 1e-9);
        }
        // Deterministic.
        assert_eq!(draw(&m, &cam, &DrawOptions { hidden: true }).unwrap(), d);
    }

    #[test]
    fn an_edge_partly_behind_another_solid_is_split() {
        let mut m = cube();
        // A second, taller box in front of the cube's right half (seen from the front).
        let sawhorse_kernel::model::OpOutcome::SketchEdited { regions } = m
            .apply(&Op::AddRectangle {
                sketch: 1,
                x: 0.5,
                y: -2.0,
                w: 1.0,
                h: 0.5,
            })
            .unwrap()
        else {
            panic!()
        };
        let region = regions[0];
        m.apply(&Op::Extrude {
            sketch: 1,
            region,
            distance: 2.0,
        })
        .unwrap();
        let d = draw(&m, &std(StdView::Front), &DrawOptions::default()).unwrap();
        // The cube's top front edge (z = 1, x 0..1) shows only from x = 0 to 0.5.
        let top: Vec<&Line> = d
            .lines
            .iter()
            .filter(|l| (l.a[1] - 1.0).abs() < 1e-12 && (l.b[1] - 1.0).abs() < 1e-12)
            .collect();
        assert_eq!(top.len(), 1, "{top:?}");
        let (x0, x1) = (top[0].a[0].min(top[0].b[0]), top[0].a[0].max(top[0].b[0]));
        // (The closed inside test hides a few nanometres early.)
        assert!(x0.abs() < 1e-12 && (x1 - 0.5).abs() < 1e-8, "{x0} {x1}");
    }

    #[test]
    fn a_section_cut_draws_its_outline_and_fill_and_hides_what_is_behind() {
        let mut m = cube();
        m.apply(&Op::AddSection {
            origin: [0.0, 0.5, 0.0],
            normal: [0.0, 1.0, 0.0],
            name: None,
            active: true,
            hidden: false,
        })
        .unwrap();
        // From the front (looking +Y at the cut face): the cut's four lines, filled.
        let d = draw(&m, &std(StdView::Front), &DrawOptions::default()).unwrap();
        assert_eq!(
            d.lines.iter().filter(|l| l.kind == LineKind::Cut).count(),
            4
        );
        assert_eq!(d.fills.len(), 1);
        // The cube's front edges at y = 0 are cut away, and its back edges hide behind the cap.
        assert_eq!(visible(&d), 4, "{:?}", d.lines);
        // From the back: the uncut back face; the cap faces away, so no fill.
        let b = draw(&m, &std(StdView::Back), &DrawOptions::default()).unwrap();
        assert!(b.fills.is_empty());
        assert_eq!(visible(&b), 4);
    }

    #[test]
    fn frames_project_and_unproject() {
        let f = Frame::of(&std(StdView::Front)).unwrap();
        let p = DVec3::new(1.0, 2.0, 3.0);
        let q = f.project(p).unwrap();
        assert_eq!(q.truncate(), DVec2::new(1.0, 3.0));
        assert!(f.unproject(q).distance(p) < 1e-12);
        let c = Frame::of(&ViewSource::Camera {
            camera: CameraSpec {
                eye: [0.0, -10.0, 1.0],
                target: [0.0, 0.0, 1.0],
                up: [0.0, 0.0, 1.0],
                parallel: false,
                fov: 35.0,
            },
        })
        .unwrap();
        let q = c.project(p).unwrap();
        assert!(c.unproject(q).distance(p) < 1e-9);
        assert!(
            c.project(DVec3::new(0.0, -20.0, 0.0)).is_none(),
            "behind the eye"
        );
    }
}
