//! Section planes (lane 12, ADR-0010).
//!
//! A section plane is an entity of the model's tree: it sits in a context (the model, or a
//! group or component instance, like a sketch drawn there) and has a point, a direction, a
//! name, an active flag and a hidden flag. Its **normal points at what stays**: the side
//! the arrows point to is kept and the other side is cut away, so a plane placed on the
//! front face of a cabinet (normal into the cabinet) cuts nothing until it is moved in.
//!
//! - At most one section is **active** per context. An active section cuts everything
//!   inside its context (all the way down), and the model's active section cuts everything.
//!   A solid inside a group is cut by every active section of the contexts around it.
//! - Like strokes drawn inside a component instance, a section plane inside an instance
//!   belongs to that copy (it is not part of the definition), and it moves with the copy.
//! - The **cut** ([`Model::section_cut`]) is exact: for every visible solid the plane cuts,
//!   the plane's intersection with the solid as polygons with holes (the *caps*, what the
//!   section fill paints), and the outline segments around them. It is computed on the
//!   B-rep, in the plane's frame, with a consistent tie rule (a vertex exactly on the plane
//!   counts as kept: a plane laid on a face, facing into the solid, cuts nothing), so it
//!   never depends on floating-point luck at the boundary;
//!   other active sections that reach the same solids clip it. Deterministic and hashed.
//! - **Create Group from Slice** ([`Model::slice_to_group`]) draws the outline into a
//!   sketch on the plane inside a new group: closed loops become faces, ready to push.
//!
//! Hashing: everything here is hashed only when a section exists or a scene stores
//! section state, so every older document keeps its hash.

use std::collections::BTreeMap;

use glam::{DVec2, DVec3};

use crate::error::{KernelError, SectionError, StructureError};
use crate::hash::StableHasher;
use crate::ids::{NodeId, ObjectId, SectionId, VertexId, from_public, to_public};
use crate::model::{Model, OpOutcome};
use crate::nodes::NodeKind;
use crate::object::Object;
use crate::plane::Plane;
use crate::transform::Xform;

/// The longest name a section plane takes.
pub const MAX_NAME: usize = 64;

/// A signed distance this close to zero is zero: the vertex is on the plane (and, by the
/// tie rule, on the side that is kept). Far below the merge distance, far above f64
/// noise on coordinates of a few metres.
const ON_PLANE: f64 = 1e-12;
/// A face with every corner this close to a section plane lies on it (the tie rule): it is
/// never split, whatever its tilt ([`vertex_distances`]).
const ON_FACE: f64 = crate::tolerance::LENGTH_EPS;

/// A section plane (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct SectionPlane {
    pub name: String,
    /// A point on the plane, metres.
    pub origin: DVec3,
    /// Unit normal pointing at the side that is kept.
    pub normal: DVec3,
    /// The group or instance it is in (`None`: the model).
    pub parent: Option<NodeId>,
    pub active: bool,
    /// Hidden (the Outliner's eye): not drawn; an active hidden plane still cuts.
    pub hidden: bool,
}

impl SectionPlane {
    /// The plane with its deterministic in-plane frame (`u x v = normal`).
    pub fn plane(&self) -> Plane {
        Plane::new(self.origin, self.normal).unwrap_or(Plane::GROUND)
    }

    /// Signed distance of `p`: positive on the side that is kept.
    pub fn distance(&self, p: DVec3) -> f64 {
        (p - self.origin).dot(self.normal)
    }

    /// This plane under `x` (the normal by the inverse transpose).
    pub(crate) fn mapped(&self, x: &Xform) -> Result<SectionPlane, KernelError> {
        let p = crate::transform::map_plane(&x.to_affine(), &self.plane())
            .ok_or(SectionError::BadPlane)?;
        let mut out = self.clone();
        out.origin = p.origin;
        out.normal = p.normal;
        out.check()?;
        Ok(out)
    }

    pub(crate) fn check(&self) -> Result<(), SectionError> {
        let ok = self.origin.is_finite()
            && self.origin.abs().max_element() <= crate::tolerance::MAX_COORD
            && self.normal.is_finite()
            && (self.normal.length() - 1.0).abs() < 1e-9;
        if !ok {
            return Err(SectionError::BadPlane);
        }
        Ok(())
    }

    pub(crate) fn hash_into(&self, h: &mut StableHasher) {
        h.tag(&self.name);
        for v in [self.origin, self.normal] {
            h.len(v.x);
            h.len(v.y);
            h.len(v.z);
        }
        h.u64(self.parent.map_or(0, to_public));
        h.u64(u64::from(self.active) | u64::from(self.hidden) << 1);
    }
}

/// A unit normal and a point, checked (the ops' input).
pub(crate) fn plane_from(
    origin: &[f64; 3],
    normal: &[f64; 3],
) -> Result<(DVec3, DVec3), SectionError> {
    let o = DVec3::from_array(*origin);
    let n = DVec3::from_array(*normal);
    let len = n.length();
    if !o.is_finite()
        || o.abs().max_element() > crate::tolerance::MAX_COORD
        || !len.is_finite()
        || len < 1e-9
    {
        return Err(SectionError::BadPlane);
    }
    Ok((o, n / len))
}

fn check_name(n: &str) -> Result<String, KernelError> {
    let t = n.trim();
    if t.is_empty() || t.chars().count() > MAX_NAME {
        return Err(crate::error::DocumentError::BadName.into());
    }
    Ok(t.to_string())
}

/// What a scene remembers about sections (lane 12): which planes are active and which are
/// hidden (public ids). Activating the scene brings both back.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SceneSections {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<u64>,
}

/// One solid's cap: the plane's intersection with it, in the plane's frame (metres),
/// outer rings counter-clockwise, holes clockwise.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Cap {
    pub object: u64,
    pub outer: Vec<[f64; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<[f64; 2]>>,
    /// Area, square metres (holes taken off).
    pub area: f64,
}

/// A section's cut (see [`Model::section_cut`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SectionCut {
    pub section: u64,
    /// The plane's frame: 2D points `(x, y)` are `origin + u x + v y`.
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    pub caps: Vec<Cap>,
    /// The cut's outline (the caps' edges), 2D in the frame.
    pub outline: Vec<[[f64; 2]; 2]>,
    /// Total cap area, square metres.
    pub area: f64,
    /// FNV-1a over the plane, every cap ring and every outline segment at 1 nm.
    pub hash: String,
}

impl SectionCut {
    fn frame(&self) -> Plane {
        Plane {
            origin: DVec3::from_array(self.origin),
            normal: DVec3::from_array(self.normal),
            u: DVec3::from_array(self.u),
            v: DVec3::from_array(self.v),
        }
    }

    /// A 2D point of the cut in the world.
    pub fn to_world(&self, p: [f64; 2]) -> DVec3 {
        self.frame().to_world(DVec2::from_array(p))
    }

    /// The caps as world triangles (for the renderer's section fill): positions and
    /// counter-clockwise triangles seen from the side that is cut away (facing the viewer
    /// who looks along the normal at what stays). Caps that do not triangulate are skipped.
    pub fn triangles(&self) -> (Vec<DVec3>, Vec<[u32; 3]>) {
        let mut pos = Vec::new();
        let mut tris = Vec::new();
        for c in &self.caps {
            let outer: Vec<DVec2> = c.outer.iter().map(|p| DVec2::from_array(*p)).collect();
            let holes: Vec<Vec<DVec2>> = c
                .holes
                .iter()
                .map(|h| h.iter().map(|p| DVec2::from_array(*p)).collect())
                .collect();
            let Ok(ts) = sawhorse_tessellate::triangulate(&outer, &holes) else {
                continue;
            };
            let base = pos.len() as u32;
            for p in outer.iter().chain(holes.iter().flatten()) {
                pos.push(self.frame().to_world(*p));
            }
            // Triangles come counter-clockwise in the frame (seen from +normal); the cap
            // faces -normal, so flip them.
            tris.extend(ts.iter().map(|t| [base + t[0], base + t[2], base + t[1]]));
        }
        (pos, tris)
    }
}

/// What a section keeps of the solids it reaches ([`Model::kept_part`]): the kept part of
/// every face and the cap, summed as a surface.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KeptPart {
    /// Enclosed volume, cubic metres (the divergence theorem about the world origin).
    pub volume: f64,
    /// The surface's vector area, square metres: every clipped face ring's own (Newell's
    /// sum over its 3D corners and crossings) plus the cap's (its area along -normal).
    /// Every edge of a closed surface is walked once each way, so the sum cancels: to
    /// rounding for planar faces, and to about a face's bend times the cut's length for
    /// faces planar only to within PLANARITY (where a face meets the plane in a slightly
    /// bent line, its kept part and the cap join along chords up to that bend apart). A
    /// cap missing, folded or facing the wrong way leaves its area here. It measures how
    /// far the cut is from watertight.
    pub vector_area: DVec3,
    /// The surface's total (unsigned) area, square metres: the scale to compare against.
    pub area: f64,
    /// The length of the cap's rings, metres (this plane's cut alone, before other planes
    /// clip it): how long the seam between the faces and the cap is.
    pub cap_length: f64,
}

impl KeptPart {
    fn add(&mut self, normal: DVec3, origin: DVec3, area: f64) {
        self.volume += normal.dot(origin) * area / 3.0;
        self.area += area.abs();
    }

    /// Add a 3D ring's vector area (Newell).
    fn newell(&mut self, ring: &[DVec3]) {
        let n = ring.len();
        for i in 0..n {
            self.vector_area += ring[i].cross(ring[(i + 1) % n]) * 0.5;
        }
    }

    /// How open the surface is: its vector area over its area (0 when watertight).
    pub fn gap(&self) -> f64 {
        if self.area > 0.0 {
            self.vector_area.length() / self.area
        } else {
            0.0
        }
    }
}

/// A crossing of the plane on a B-rep edge, named by the edge's two vertices (low first).
type Node = (VertexId, VertexId);

/// One directed piece of the cut inside one face: from crossing `a` to crossing `b`, with
/// the solid's material on its left (seen from the plane's normal).
struct Piece {
    a: Node,
    b: Node,
}

/// A face this close to parallel with the plane (the cross product of the normals) is
/// parallel: the cut never crosses it (the tie rule).
const PARALLEL: f64 = 1e-12;

fn parallel(plane: &Plane, face: &crate::brep::Face) -> bool {
    plane.normal.cross(face.plane.normal).length() < PARALLEL
}

/// The signed distance of every vertex of `b` from `plane`, as the cut and the kept volume
/// both read it: within ON_PLANE of zero is zero, and every vertex of a face the cut treats
/// as parallel ([`parallel`]) takes the face's mean distance, so a face that is parallel to
/// the plane is wholly on one side or wholly on it. Without that, a face tilted 1e-13 off a
/// plane through it (a plane copied with a squashing scale, 2026-10-12 fuzz) had two corners
/// at 0 and two at -1e-12: the cut skipped it as parallel while the kept volume split it,
/// and the halves no longer made the whole. A face whose every corner is within
/// [`ON_FACE`] of the plane lies on it whatever its tilt and is treated the same way: a face
/// tilted 5e-11 off the plane (a Follow Me sweep along an edge 4e-6 off square, 2026-10-12
/// fuzz) had three corners exactly on it and three 1.2e-11 off, so one half kept the notch
/// between the three as a sliver of negative area with no cap to close it.
fn vertex_distances(b: &crate::brep::Brep, plane: &Plane) -> BTreeMap<VertexId, f64> {
    let mut dist: BTreeMap<VertexId, f64> = b
        .vertices()
        .map(|(k, v)| (k, plane.signed_distance(v.pos)))
        .collect();
    let raw = dist.clone();
    for (_, f) in b.faces() {
        let vs: Vec<VertexId> = f.loops().flat_map(|l| b.loop_vertices(l)).collect();
        if !(parallel(plane, f) || vs.iter().all(|v| raw[v].abs() <= ON_FACE)) {
            continue;
        }
        let first = dist[&vs[0]];
        if vs.iter().all(|v| dist[v] == first) {
            continue;
        }
        let mean = vs.iter().map(|v| dist[v]).sum::<f64>() / vs.len() as f64;
        for v in vs {
            dist.insert(v, mean);
        }
    }
    for d in dist.values_mut() {
        if d.abs() < ON_PLANE {
            *d = 0.0;
        }
    }
    dist
}

/// The rings where `plane` cuts a solid, in the plane's frame: material on the left of
/// every ring (outer rings counter-clockwise, holes clockwise). Exact up to the crossing
/// interpolation; see the module docs for the tie rule.
pub(crate) fn solid_rings(o: &Object, plane: &Plane) -> Vec<Vec<DVec2>> {
    let b = o.brep();
    let n = plane.normal;
    let dist = vertex_distances(b, plane);
    let kept = |v: VertexId| dist[&v] >= 0.0;
    let mut points: BTreeMap<Node, DVec3> = BTreeMap::new();
    let mut pieces: Vec<Piece> = Vec::new();
    for (_, f) in b.faces() {
        if parallel(plane, f) {
            continue; // parallel to the plane: never crossed (the tie rule)
        }
        let t = n.cross(f.plane.normal).normalize();
        let mut xs: Vec<(f64, Node)> = Vec::new();
        for l in f.loops() {
            let vs = b.loop_vertices(l);
            for i in 0..vs.len() {
                let (p, q) = (vs[i], vs[(i + 1) % vs.len()]);
                if kept(p) == kept(q) {
                    continue;
                }
                // A crossing at a vertex on the plane is that vertex, whichever edge
                // reaches it (node (v, v)): faces that touch the plane at a vertex must
                // chain through one node, or coincident crossings pair up differently on
                // two faces and the ring folds over itself.
                let on = [p, q].into_iter().find(|v| dist[v] == 0.0);
                let node = match on {
                    Some(v) => (v, v),
                    None if p < q => (p, q),
                    None => (q, p),
                };
                let x = *points.entry(node).or_insert_with(|| match on {
                    Some(v) => b.vertices[v].pos,
                    None => {
                        let (lo, hi) = node;
                        let (pl, ph) = (b.vertices[lo].pos, b.vertices[hi].pos);
                        let (dl, dh) = (dist[&lo], dist[&hi]);
                        pl + (ph - pl) * (dl / (dl - dh))
                    }
                });
                xs.push(((x - plane.origin).dot(t), node));
            }
        }
        if xs.len() % 2 == 1 {
            continue; // cannot happen on a valid solid; never pair a stray crossing
        }
        xs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for [a, b] in xs.as_chunks::<2>().0 {
            pieces.push(Piece { a: a.1, b: b.1 });
        }
    }
    // Chain the pieces head to tail. At every node as many pieces start as end: one per
    // face at an edge crossing, and around a vertex on the plane as many going in as coming
    // out (the faces around it alternate). A vertex where the cut passes more than once
    // (two lobes of the section touching there, or a hole touching its outer ring: a
    // folded solid, 2026-10-12 fuzz with plane snapping off) has several ways on. Before
    // 2026-10-12 the chain took any unused piece at a node, in either direction, so there
    // it could run a piece backwards: the ring folded over itself, its area came out
    // wrong or nearly zero, the cap was dropped, and the kept halves no longer made the
    // whole. Now a ring only ever follows a piece forwards, takes the leftmost way on (it
    // traces one patch of material at a time), and is split where it comes back to a node
    // it already passed, so every ring is simple and keeps the material on its left.
    pieces.retain(|p| p.a != p.b); // a face touching the plane at one vertex: nothing to cut
    let at2d = |k: &Node| plane.to_local(points[k]);
    let mut starts: BTreeMap<Node, Vec<usize>> = BTreeMap::new();
    for (i, p) in pieces.iter().enumerate() {
        starts.entry(p.a).or_default().push(i);
    }
    let mut used = vec![false; pieces.len()];
    let mut rings = Vec::new();
    for start in 0..pieces.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let first = pieces[start].a;
        let mut ring = vec![first];
        let (mut prev, mut cur) = (first, pieces[start].b);
        let mut closed = false;
        for _ in 0..=pieces.len() {
            if cur == first {
                closed = true;
                break;
            }
            ring.push(cur);
            let din = at2d(&cur) - at2d(&prev);
            let turn = |k: usize| {
                let dout = at2d(&pieces[k].b) - at2d(&cur);
                din.perp_dot(dout).atan2(din.dot(dout))
            };
            let next = starts
                .get(&cur)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&k| !used[k])
                .max_by(|&x, &y| turn(x).total_cmp(&turn(y)).then(y.cmp(&x)));
            let Some(k) = next else {
                break;
            };
            used[k] = true;
            (prev, cur) = (cur, pieces[k].b);
        }
        if !closed {
            continue;
        }
        for loop_nodes in split_pinches(ring) {
            let pts: Vec<DVec2> = loop_nodes.iter().map(at2d).collect();
            let pts = tidy_ring(pts);
            if pts.len() >= 3
                && sawhorse_tessellate::signed_area(&pts).abs() > crate::tolerance::AREA_EPS * 1e-6
            {
                rings.push(pts);
            }
        }
    }
    rings
}

/// A closed ring of nodes that passes a node more than once, as the simple rings it is
/// made of (each from one visit of the node to the next).
fn split_pinches(ring: Vec<Node>) -> Vec<Vec<Node>> {
    let mut out = Vec::new();
    let mut stack: Vec<Node> = Vec::with_capacity(ring.len());
    for k in ring {
        if let Some(i) = stack.iter().position(|x| *x == k) {
            out.push(stack.split_off(i));
        }
        stack.push(k);
    }
    out.push(stack);
    out
}

/// Drop repeated points and points on one line with their neighbours: between them, or
/// the tip of a zero-width spike (a ring that runs out along a line and back, as one
/// through a vertex touching the plane can).
fn tidy_ring(mut pts: Vec<DVec2>) -> Vec<DVec2> {
    let mut changed = true;
    while changed && pts.len() >= 3 {
        changed = false;
        let n = pts.len();
        for i in 0..n {
            let (a, p, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let same = p.distance(a) <= 1e-12;
            let in_line =
                (p - a).perp_dot(c - p).abs() <= 1e-18 * (1.0 + a.distance(p) + p.distance(c));
            if same || in_line {
                pts.remove(i);
                changed = true;
                break;
            }
        }
    }
    pts
}

/// Outer rings with the holes inside them: positive rings are outers, negative ones holes
/// of the smallest outer that holds them.
pub(crate) fn assemble(rings: Vec<Vec<DVec2>>) -> Vec<(Vec<DVec2>, Vec<Vec<DVec2>>)> {
    let (mut outers, mut holes): (Vec<_>, Vec<_>) = (Vec::new(), Vec::new());
    for r in rings {
        if sawhorse_tessellate::signed_area(&r) > 0.0 {
            outers.push((r, Vec::new()));
        } else {
            holes.push(r);
        }
    }
    for h in holes {
        // A point just inside the hole's material side: the middle of its first edge.
        let probe = (h[0] + h[1]) * 0.5;
        let best = outers
            .iter()
            .enumerate()
            .filter(|(_, (o, _))| inside(probe, o))
            .min_by(|a, b| {
                sawhorse_tessellate::signed_area(&a.1.0)
                    .total_cmp(&sawhorse_tessellate::signed_area(&b.1.0))
            })
            .map(|(i, _)| i);
        if let Some(i) = best {
            outers[i].1.push(h);
        }
    }
    outers
}

/// Even-odd point in ring.
fn inside(p: DVec2, ring: &[DVec2]) -> bool {
    let mut c = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                c = !c;
            }
        }
    }
    c
}

/// An affine function of the frame's 2D points: `c0 + c1 x + c2 y` (another plane's signed
/// distance seen in this plane).
#[derive(Debug, Clone, Copy)]
struct HalfPlane([f64; 3]);

impl HalfPlane {
    fn of(frame: &Plane, other: &SectionPlane) -> HalfPlane {
        HalfPlane([
            other.distance(frame.origin),
            frame.u.dot(other.normal),
            frame.v.dot(other.normal),
        ])
    }
    fn at(&self, p: DVec2) -> f64 {
        self.0[0] + self.0[1] * p.x + self.0[2] * p.y
    }
}

/// Sutherland-Hodgman: the part of `ring` where `h` is not negative (keeps orientation).
fn clip(ring: &[DVec2], h: &HalfPlane) -> Vec<DVec2> {
    let mut out = Vec::with_capacity(ring.len() + 2);
    let n = ring.len();
    for i in 0..n {
        let (p, q) = (ring[i], ring[(i + 1) % n]);
        let (dp, dq) = (h.at(p), h.at(q));
        let (ip, iq) = (dp >= 0.0, dq >= 0.0);
        if ip {
            out.push(p);
        }
        if ip != iq {
            out.push(p + (q - p) * (dp / (dp - dq)));
        }
    }
    out
}

impl Model {
    /// Section planes with their public ids, in id order.
    pub fn sections(&self) -> Vec<(u64, &SectionPlane)> {
        self.sections
            .iter()
            .map(|(k, s)| (to_public(k), s))
            .collect()
    }

    pub fn section(&self, id: u64) -> Option<&SectionPlane> {
        self.sections.get(from_public::<SectionId>(id))
    }

    fn section_key(&self, id: u64) -> Result<SectionId, SectionError> {
        let k = from_public::<SectionId>(id);
        if self.sections.contains_key(k) {
            Ok(k)
        } else {
            Err(SectionError::NoSuchSection(id))
        }
    }

    /// Anything to hash: a section, or a scene that remembers section state.
    pub(crate) fn has_sections(&self) -> bool {
        !self.sections.is_empty() || self.scenes.iter().any(|s| s.sections.is_some())
    }

    pub(crate) fn hash_sections(&self, h: &mut StableHasher) {
        h.tag("sections");
        h.usize(self.sections.len());
        for (id, s) in self.sections() {
            h.u64(id);
            s.hash_into(h);
        }
        for sc in &self.scenes {
            match &sc.sections {
                None => h.u64(u64::MAX),
                Some(x) => {
                    h.usize(x.active.len());
                    x.active.iter().for_each(|&k| h.u64(k));
                    h.usize(x.hidden.len());
                    x.hidden.iter().for_each(|&k| h.u64(k));
                }
            }
        }
    }

    /// True when `n` is `ancestor` or below it (`None`: the model holds everything).
    fn within(&self, n: Option<NodeId>, ancestor: Option<NodeId>) -> bool {
        match ancestor {
            None => true,
            Some(a) => self.path_of(n).contains(&a),
        }
    }

    /// A section is drawn: not hidden and its context shown.
    pub fn section_shown(&self, id: u64) -> bool {
        self.section(id)
            .is_some_and(|s| !s.hidden && s.parent.is_none_or(|p| self.node_shown(p)))
    }

    /// The active sections that cut what sits in `parent` (its own context's and every
    /// enclosing context's), in id order.
    pub fn active_sections_over(&self, parent: Option<NodeId>) -> Vec<u64> {
        self.sections
            .iter()
            .filter(|(_, s)| s.active && self.within(parent, s.parent))
            .map(|(k, _)| to_public(k))
            .collect()
    }

    /// The active sections that cut solid `object`.
    pub fn sections_cutting(&self, object: u64) -> Vec<u64> {
        match self.object(object) {
            Some(o) => self.active_sections_over(o.member.parent),
            None => Vec::new(),
        }
    }

    /// The visible solids a section reaches: everything in its context, in creation order.
    fn section_solids(&self, s: &SectionPlane) -> Vec<ObjectId> {
        self.object_order
            .iter()
            .copied()
            .filter(|&k| {
                let o = &self.objects[k];
                self.within(o.member.parent, s.parent) && self.member_shown(&o.member, o.tag())
            })
            .collect()
    }

    /// The exact cut of section `id` (active or not): see the module docs. Other active
    /// sections that reach a solid clip its cap.
    pub fn section_cut(&self, id: u64) -> Result<SectionCut, KernelError> {
        let k = self.section_key(id)?;
        let s = &self.sections[k];
        let frame = s.plane();
        let mut caps = Vec::new();
        let mut outline = Vec::new();
        for o in self.section_solids(s) {
            let obj = &self.objects[o];
            let others: Vec<HalfPlane> = self
                .active_sections_over(obj.member.parent)
                .into_iter()
                .filter(|&x| x != id)
                .map(|x| HalfPlane::of(&frame, &self.sections[from_public::<SectionId>(x)]))
                .collect();
            for (outer, holes) in assemble(solid_rings(obj, &frame)) {
                let mut outer = outer;
                let mut holes = holes;
                for h in &others {
                    outer = clip(&outer, h);
                    holes = holes.iter().map(|r| clip(r, h)).collect();
                }
                let outer = tidy_ring(outer);
                let holes: Vec<Vec<DVec2>> = holes
                    .into_iter()
                    .map(tidy_ring)
                    .filter(|r| r.len() >= 3)
                    .collect();
                if outer.len() < 3 {
                    continue;
                }
                let area = sawhorse_tessellate::signed_area(&outer)
                    + holes
                        .iter()
                        .map(|r| sawhorse_tessellate::signed_area(r))
                        .sum::<f64>();
                if area <= crate::tolerance::AREA_EPS * 1e-6 {
                    continue;
                }
                // The outline, without edges that only lie along another plane's cut.
                for r in std::iter::once(&outer).chain(&holes) {
                    for i in 0..r.len() {
                        let (a, b) = (r[i], r[(i + 1) % r.len()]);
                        if others
                            .iter()
                            .any(|h| h.at(a).abs() <= 1e-12 && h.at(b).abs() <= 1e-12)
                        {
                            continue;
                        }
                        outline.push([a.to_array(), b.to_array()]);
                    }
                }
                caps.push(Cap {
                    object: to_public(o),
                    outer: outer.iter().map(|p| p.to_array()).collect(),
                    holes: holes
                        .iter()
                        .map(|r| r.iter().map(|p| p.to_array()).collect())
                        .collect(),
                    area,
                });
            }
        }
        let area = caps.iter().map(|c| c.area).sum();
        let mut h = StableHasher::default();
        h.tag("section-cut");
        h.u64(id);
        for v in [frame.origin, frame.normal, frame.u, frame.v] {
            h.len(v.x);
            h.len(v.y);
            h.len(v.z);
        }
        h.usize(caps.len());
        for c in &caps {
            h.u64(c.object);
            for r in std::iter::once(&c.outer).chain(&c.holes) {
                h.usize(r.len());
                for p in r {
                    h.len(p[0]);
                    h.len(p[1]);
                }
            }
        }
        h.usize(outline.len());
        for [a, b] in &outline {
            for v in a.iter().chain(b) {
                h.len(*v);
            }
        }
        Ok(SectionCut {
            section: id,
            origin: frame.origin.to_array(),
            normal: frame.normal.to_array(),
            u: frame.u.to_array(),
            v: frame.v.to_array(),
            caps,
            outline,
            area,
            hash: format!("{:016x}", h.finish()),
        })
    }

    /// Volume, cubic metres, of what section `id` keeps of the solids it reaches (ignoring
    /// other sections): the divergence theorem over every face clipped to the kept side
    /// plus the cap. Tests and the fuzzer check it against the whole volume.
    pub fn kept_volume(&self, id: u64) -> Result<f64, KernelError> {
        Ok(self.kept_part(id)?.volume)
    }

    /// What section `id` keeps of the solids it reaches, as a closed surface: every face
    /// clipped to the kept side, plus the cap facing `-normal` (see [`KeptPart`]).
    pub fn kept_part(&self, id: u64) -> Result<KeptPart, KernelError> {
        let k = self.section_key(id)?;
        let s = &self.sections[k];
        let frame = s.plane();
        let mut part = KeptPart::default();
        for o in self.section_solids(s) {
            let obj = &self.objects[o];
            let b = obj.brep();
            // Which side each vertex is on, with the same tie rule and the same crossing
            // points as the cap (`solid_rings`): a face lying on the plane to within
            // ON_PLANE is kept whole exactly when the cap leaves it out, so the two halves
            // always add up to the whole.
            let dist = vertex_distances(b, &frame);
            for (_, f) in b.faces() {
                let fp = &f.plane;
                let mut area = 0.0;
                for l in f.loops() {
                    let vs = b.loop_vertices(l);
                    let mut ring: Vec<DVec3> = Vec::with_capacity(vs.len() + 2);
                    for i in 0..vs.len() {
                        let (p, q) = (vs[i], vs[(i + 1) % vs.len()]);
                        let (kp, kq) = (dist[&p] >= 0.0, dist[&q] >= 0.0);
                        if kp {
                            ring.push(b.vertices[p].pos);
                        }
                        if kp != kq && dist[&p] != 0.0 && dist[&q] != 0.0 {
                            let (lo, hi) = if p < q { (p, q) } else { (q, p) };
                            let (pl, ph) = (b.vertices[lo].pos, b.vertices[hi].pos);
                            let (dl, dh) = (dist[&lo], dist[&hi]);
                            ring.push(pl + (ph - pl) * (dl / (dl - dh)));
                        }
                    }
                    let flat: Vec<DVec2> = ring.iter().map(|p| fp.to_local(*p)).collect();
                    area += sawhorse_tessellate::signed_area(&flat);
                    part.newell(&ring);
                }
                part.add(fp.normal, fp.origin, area);
            }
            let rings = solid_rings(obj, &frame);
            part.cap_length += rings
                .iter()
                .map(|r| {
                    (0..r.len())
                        .map(|i| r[i].distance(r[(i + 1) % r.len()]))
                        .sum::<f64>()
                })
                .sum::<f64>();
            let cap: f64 = assemble(rings)
                .iter()
                .map(|(outer, holes)| {
                    sawhorse_tessellate::signed_area(outer)
                        + holes
                            .iter()
                            .map(|r| sawhorse_tessellate::signed_area(r))
                            .sum::<f64>()
                })
                .sum();
            // The cap is a face of the kept part whose outward normal is -normal.
            part.add(-s.normal, frame.origin, cap);
            part.vector_area -= s.normal * cap;
        }
        Ok(part)
    }

    /// The rectangle a section plane is drawn as: the bounds of what it reaches, seen in
    /// its frame, a little larger (`[x0, y0, x1, y1]`, metres; one foot square around its
    /// point when it reaches nothing).
    pub fn section_extent(&self, id: u64) -> Option<[f64; 4]> {
        let s = self.section(id)?;
        let frame = s.plane();
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        for o in self.section_solids(s) {
            for (_, v) in self.objects[o].brep().vertices() {
                let q = frame.to_local(v.pos);
                lo = lo.min(q);
                hi = hi.max(q);
            }
        }
        if !lo.is_finite() {
            let r = 0.1524;
            return Some([-r, -r, r, r]);
        }
        let pad = (hi - lo).max_element().max(0.05) * 0.08;
        Some([lo.x - pad, lo.y - pad, hi.x + pad, hi.y + pad])
    }

    // ---- ops ---------------------------------------------------------------------------

    /// A section can be moved, turned, flipped or deleted: it is in the open context.
    fn editable_section(&self, k: SectionId) -> Result<(), StructureError> {
        if self.sections[k].parent != self.context {
            return Err(StructureError::OutsideContext);
        }
        Ok(())
    }

    /// Make `k` the only active section of its context.
    fn activate_only(&mut self, k: SectionId) {
        let parent = self.sections[k].parent;
        for (j, s) in self.sections.iter_mut() {
            if s.parent == parent {
                s.active = j == k;
            }
        }
    }

    /// The next free "Section N" name.
    fn next_section_name(&self) -> String {
        let taken: std::collections::BTreeSet<&str> =
            self.sections.values().map(|s| s.name.as_str()).collect();
        (1..)
            .map(|k| format!("Section {k}"))
            .find(|n| !taken.contains(n.as_str()))
            .expect("a free name")
    }

    /// [`crate::model::Op::AddSection`]: a new plane in the open context.
    pub(crate) fn add_section(
        &mut self,
        origin: &[f64; 3],
        normal: &[f64; 3],
        name: Option<&str>,
        active: bool,
        hidden: bool,
    ) -> Result<OpOutcome, KernelError> {
        let (o, n) = plane_from(origin, normal)?;
        let name = match name {
            Some(x) => check_name(x)?,
            None => self.next_section_name(),
        };
        let s = SectionPlane {
            name,
            origin: o,
            normal: n,
            parent: self.context,
            active,
            hidden,
        };
        s.check()?;
        let k = self.sections.insert(s);
        if active {
            self.activate_only(k);
        }
        Ok(OpOutcome::SectionAdded {
            section: to_public(k),
        })
    }

    /// [`crate::model::Op::PlaceSection`]: put a plane exactly here (in the open context).
    pub(crate) fn place_section(
        &mut self,
        id: u64,
        origin: &[f64; 3],
        normal: &[f64; 3],
    ) -> Result<OpOutcome, KernelError> {
        let k = self.section_key(id)?;
        self.editable_section(k)?;
        let (o, n) = plane_from(origin, normal)?;
        let s = &mut self.sections[k];
        s.origin = o;
        s.normal = n;
        s.check()?;
        Ok(OpOutcome::SectionSet)
    }

    /// [`crate::model::Op::FlipSection`]: keep the other side.
    pub(crate) fn flip_section(&mut self, id: u64) -> Result<OpOutcome, KernelError> {
        let k = self.section_key(id)?;
        self.editable_section(k)?;
        let s = &mut self.sections[k];
        s.normal = -s.normal;
        Ok(OpOutcome::SectionSet)
    }

    /// [`crate::model::Op::SetSectionActive`]: activate (deactivating the others of its
    /// context) or deactivate. Allowed anywhere in the tree, like the Outliner's flags.
    pub(crate) fn set_section_active(
        &mut self,
        id: u64,
        active: bool,
    ) -> Result<OpOutcome, KernelError> {
        let k = self.section_key(id)?;
        if active {
            self.activate_only(k);
        } else {
            self.sections[k].active = false;
        }
        Ok(OpOutcome::SectionSet)
    }

    pub(crate) fn rename_section(&mut self, id: u64, name: &str) -> Result<OpOutcome, KernelError> {
        let k = self.section_key(id)?;
        self.sections[k].name = check_name(name)?;
        Ok(OpOutcome::SectionSet)
    }

    pub(crate) fn set_section_hidden(
        &mut self,
        id: u64,
        hidden: bool,
    ) -> Result<OpOutcome, KernelError> {
        let k = self.section_key(id)?;
        self.sections[k].hidden = hidden;
        Ok(OpOutcome::SectionSet)
    }

    pub(crate) fn delete_section(&mut self, id: u64) -> Result<OpOutcome, KernelError> {
        let k = self.section_key(id)?;
        self.editable_section(k)?;
        self.sections.remove(k);
        self.forget_sections();
        Ok(OpOutcome::SectionDeleted)
    }

    /// The sections to move with a Move / Rotate / Scale (checked: in the open context).
    pub(crate) fn section_targets(&self, ids: &[u64]) -> Result<Vec<SectionId>, KernelError> {
        let mut out = Vec::with_capacity(ids.len());
        for &id in ids {
            let k = self.section_key(id)?;
            self.editable_section(k)?;
            if !out.contains(&k) {
                out.push(k);
            }
        }
        Ok(out)
    }

    /// Move / turn sections by `x`.
    pub(crate) fn transform_sections(
        &mut self,
        keys: &[SectionId],
        x: &Xform,
    ) -> Result<(), KernelError> {
        for &k in keys {
            let next = self.sections[k].mapped(x)?;
            self.sections[k] = next;
        }
        Ok(())
    }

    /// Copies of sections under `x^1 .. x^count`, inactive, in the open context.
    pub(crate) fn copy_sections(
        &mut self,
        keys: &[SectionId],
        x: &Xform,
        count: u32,
    ) -> Result<Vec<u64>, KernelError> {
        let mut made = Vec::new();
        for k in 1..=count {
            let xk = x.power(k);
            for &s in keys {
                let mut c = self.sections[s].mapped(&xk)?;
                c.active = false;
                c.parent = self.context;
                made.push(to_public(self.sections.insert(c)));
            }
        }
        Ok(made)
    }

    /// Sections whose context is gone go with it; scenes forget sections that are gone.
    /// Called by tidying after every op.
    pub(crate) fn forget_sections(&mut self) {
        let nodes = &self.nodes;
        self.sections
            .retain(|_, s| s.parent.is_none_or(|p| nodes.contains_key(p)));
        let live: std::collections::BTreeSet<u64> = self.sections.keys().map(to_public).collect();
        for sc in &mut self.scenes {
            if let Some(x) = &mut sc.sections {
                x.active.retain(|k| live.contains(k));
                x.hidden.retain(|k| live.contains(k));
            }
        }
    }

    /// Sections in node `from` move to `to` (an exploded group's, a vanished nested
    /// group's); any that would make a second active section there are deactivated.
    pub(crate) fn rehome_sections(&mut self, from: NodeId, to: Option<NodeId>) {
        let had_active = self.sections.values().any(|s| s.parent == to && s.active);
        let mut keep_one = !had_active;
        let keys: Vec<SectionId> = self
            .sections
            .iter()
            .filter(|(_, s)| s.parent == Some(from))
            .map(|(k, _)| k)
            .collect();
        for k in keys {
            let s = &mut self.sections[k];
            s.parent = to;
            if s.active {
                if keep_one {
                    keep_one = false;
                } else {
                    s.active = false;
                }
            }
        }
    }

    /// Sections directly in node `n`, in id order.
    pub(crate) fn sections_in(&self, n: NodeId) -> Vec<SectionId> {
        self.sections
            .iter()
            .filter(|(_, s)| s.parent == Some(n))
            .map(|(k, _)| k)
            .collect()
    }

    /// Scenes may name sections: they must exist.
    pub(crate) fn check_scene_sections(&self, x: &SceneSections) -> Result<(), KernelError> {
        for &id in x.active.iter().chain(&x.hidden) {
            if self.section(id).is_none() {
                return Err(SectionError::SceneSection(id).into());
            }
        }
        Ok(())
    }

    /// A scene's section state comes back: exactly the listed planes are active (one per
    /// context: the first listed wins) and exactly the listed ones hidden.
    pub(crate) fn apply_scene_sections(&mut self, x: &SceneSections) {
        let keys: Vec<SectionId> = self.sections.keys().collect();
        for k in &keys {
            let s = &mut self.sections[*k];
            s.active = false;
            s.hidden = x.hidden.contains(&to_public(*k));
        }
        for &id in &x.active {
            let k = from_public::<SectionId>(id);
            if !self.sections.contains_key(k) {
                continue;
            }
            let parent = self.sections[k].parent;
            if !self
                .sections
                .values()
                .any(|s| s.parent == parent && s.active)
            {
                self.sections[k].active = true;
            }
        }
    }

    /// What a scene saved now would remember about sections.
    pub fn scene_sections_now(&self) -> SceneSections {
        SceneSections {
            active: self
                .sections
                .iter()
                .filter(|(_, s)| s.active)
                .map(|(k, _)| to_public(k))
                .collect(),
            hidden: self
                .sections
                .iter()
                .filter(|(_, s)| s.hidden)
                .map(|(k, _)| to_public(k))
                .collect(),
        }
    }

    /// [`crate::model::Op::SliceToGroup`] (SketchUp's "Create Group from Slice"): the cut's
    /// outline drawn into a sketch on the plane, inside a new group in the open context.
    /// Closed loops become faces of the sketch, ready to push.
    pub(crate) fn slice_to_group(
        &mut self,
        id: u64,
        name: Option<&str>,
    ) -> Result<OpOutcome, KernelError> {
        let k = self.section_key(id)?;
        if self
            .path_of(self.context)
            .iter()
            .any(|&n| matches!(self.nodes[n].kind, NodeKind::Instance(_)))
        {
            return Err(SectionError::SliceInComponent.into());
        }
        let name = match name {
            Some(n) => check_name(n)?,
            None => format!(
                "{} slice",
                self.sections[k]
                    .name
                    .chars()
                    .take(MAX_NAME - 6)
                    .collect::<String>()
            ),
        };
        let cut = self.section_cut(id)?;
        if cut.outline.is_empty() {
            return Err(SectionError::NothingToSlice.into());
        }
        let frame = self.sections[k].plane();
        let mut sketch = crate::sketch::Sketch::new(frame);
        for [a, b] in &cut.outline {
            let (a, b) = (DVec2::from_array(*a), DVec2::from_array(*b));
            if a.distance(b) <= crate::tolerance::POINT_MERGE {
                continue;
            }
            sketch.insert_segment(a, b)?;
        }
        if sketch.edge_count() == 0 {
            return Err(SectionError::NothingToSlice.into());
        }
        let group = self.nodes.insert(crate::nodes::Node {
            name: Some(name),
            kind: NodeKind::Group,
            xform: Xform::IDENTITY,
            tag: None,
            member: crate::nodes::Membership {
                parent: self.context,
                slot: None,
                hidden: false,
                locked: false,
            },
            part: Default::default(),
            material: None,
        });
        sketch.parent = Some(group);
        let sid = self.sketches.insert(sketch);
        self.sketch_order.push(sid);
        self.mark(self.context);
        Ok(OpOutcome::Sliced {
            group: to_public(group),
            sketch: to_public(sid),
        })
    }

    /// The model's rules for sections: live parents, unit normals in bounds, names, at
    /// most one active section per context, scenes naming live sections.
    pub(crate) fn check_sections(&self) -> Result<(), String> {
        let mut active: std::collections::BTreeSet<Option<NodeId>> = Default::default();
        for (k, s) in &self.sections {
            if s.parent.is_some_and(|p| !self.nodes.contains_key(p)) {
                return Err(format!("section {} is in a missing group", to_public(k)));
            }
            s.check()
                .map_err(|e| format!("section {}: {e}", to_public(k)))?;
            if s.name.trim().is_empty() || s.name.chars().count() > MAX_NAME {
                return Err(format!("section {} has a bad name", to_public(k)));
            }
            if s.active && !active.insert(s.parent) {
                return Err("two active sections in one context".into());
            }
        }
        for sc in &self.scenes {
            if let Some(x) = &sc.sections
                && let Some(&bad) = x
                    .active
                    .iter()
                    .chain(&x.hidden)
                    .find(|&&id| self.section(id).is_none())
            {
                return Err(format!("scene {:?} names missing section {bad}", sc.name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ]
    }

    #[test]
    fn holes_find_their_outer() {
        let mut hole = square(1.0, 1.0, 2.0, 2.0);
        hole.reverse();
        let caps = assemble(vec![
            square(0.0, 0.0, 3.0, 3.0),
            hole,
            square(5.0, 0.0, 6.0, 1.0),
        ]);
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0].1.len(), 1);
        assert!(caps[1].1.is_empty());
    }

    #[test]
    fn clipping_keeps_the_positive_side() {
        let r = square(0.0, 0.0, 2.0, 2.0);
        let h = HalfPlane([-1.0, 1.0, 0.0]); // x > 1
        let c = clip(&r, &h);
        assert!((sawhorse_tessellate::signed_area(&c) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn collinear_and_repeated_points_go() {
        let r = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 0.0),
            DVec2::new(1.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(0.0, 2.0),
        ];
        assert_eq!(tidy_ring(r).len(), 4);
    }

    #[test]
    fn a_box_cut_through_its_middle() {
        let o = Object::extrude(
            &Plane::GROUND,
            &[
                DVec2::ZERO,
                DVec2::new(2.0, 0.0),
                DVec2::new(2.0, 1.0),
                DVec2::new(0.0, 1.0),
            ],
            &[],
            1.0,
        )
        .unwrap();
        let plane = Plane::new(DVec3::new(0.5, 0.0, 0.0), DVec3::X).unwrap();
        let caps = assemble(solid_rings(&o, &plane));
        assert_eq!(caps.len(), 1);
        assert!((sawhorse_tessellate::signed_area(&caps[0].0) - 1.0).abs() < 1e-12);
        // On a face exactly, facing into the solid: the tie rule keeps the face, so nothing
        // is cut. Facing out, everything but the face is cut away: the face is the cap.
        let into = Plane::new(DVec3::ZERO, DVec3::X).unwrap();
        assert!(solid_rings(&o, &into).is_empty());
        let out = Plane::new(DVec3::ZERO, -DVec3::X).unwrap();
        let caps = assemble(solid_rings(&o, &out));
        assert_eq!(caps.len(), 1);
        assert!((sawhorse_tessellate::signed_area(&caps[0].0) - 1.0).abs() < 1e-12);
        let top = Plane::new(DVec3::new(0.0, 0.0, 1.0), -DVec3::Z).unwrap();
        assert!(solid_rings(&o, &top).is_empty());
    }
}
