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
//! - **Create Group from Slice** ([`Model::slice_to_group`]) draws the caps into a sketch
//!   on the plane inside a new group: closed loops become faces, ready to push.
//! - **The cut keeps every cap exactly** (lane 18); what the slice can draw of one is one
//!   rule, [`representable`]: its rings with points closer than POINT_MERGE merged, as the
//!   sketch merges them, keeping three corners and some area. Every cap carries that as
//!   [`Cap::representable`]; the slice draws every representable cap and refuses none, and a
//!   section whose caps are all thinner than that says so in its own words.
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
    /// Whether Create Group from Slice can draw this cap (lane 18, [`representable`]): with its
    /// points closer than POINT_MERGE merged, its outer ring keeps three corners and an area
    /// above AREA_EPS per metre of the solid's size. A cap that is not is thinner than the
    /// model can draw (a sliver under 10 micrometres wide), and is still part of the cut.
    pub representable: bool,
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
/// the solid's material on its left (seen from the plane's normal) when `vote` is set.
struct Piece {
    a: Node,
    b: Node,
    /// The direction comes from a face that clearly crosses the plane: it counts as
    /// evidence of which side the material is on.
    vote: bool,
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
/// between the three as a sliver of negative area with no cap to close it. Since lane 12d
/// "within" is within twice the face's own bend when that is more (a face is planar only to
/// within PLANARITY, so where it lies relative to a plane closer than that cannot be told):
/// a box face bent by moving one corner 0.3 micrometres up off a plane through the other
/// three dented the top face, its corners classified one by one clipped it to a triangle
/// over the dent, and no cap could close what the side faces cut (lane 12d property tests;
/// the chain failed it the same way).
fn vertex_distances(b: &crate::brep::Brep, plane: &Plane) -> BTreeMap<VertexId, f64> {
    let mut dist: BTreeMap<VertexId, f64> = b
        .vertices()
        .map(|(k, v)| (k, plane.signed_distance(v.pos)))
        .collect();
    let raw = dist.clone();
    for (_, f) in b.faces() {
        let vs: Vec<VertexId> = f.loops().flat_map(|l| b.loop_vertices(l)).collect();
        let bend = vs
            .iter()
            .map(|v| f.plane.signed_distance(b.vertices[*v].pos).abs())
            .fold(0.0, f64::max);
        let near = ON_FACE.max(2.0 * bend);
        if !(parallel(plane, f) || vs.iter().all(|v| raw[v].abs() <= near)) {
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

/// The cut's pieces: what [`cut_pieces`] finds in each face.
struct Cut {
    pieces: Vec<Piece>,
    /// Where every crossing is.
    points: BTreeMap<Node, DVec3>,
}

/// How far below its bend times this a face's tilt off the plane (the sine of the angle
/// between them times the face's size) makes the direction of its cut untrustworthy: a
/// face that is planar only to within PLANARITY and lies within about that of the plane
/// meets it along a line its stored normal no longer points across.
const WEAK: f64 = 16.0;

/// The tolerance of the cap's arrangement, per metre of the solid's largest coordinate: a
/// few hundred rounding errors, far below anything the model can hold apart.
const ARRANGE_TOL: f64 = 2e-13;

/// Every piece where `plane` cuts a face of `b`, in face order, with the tie rule of
/// [`vertex_distances`]. Within a face the crossings of its boundary are sorted along the
/// line where it meets the plane and paired up; each pair is a piece, directed with the
/// material on its left. Such a direction comes from the face's normal, which says nothing
/// for a face lying within its own bend of the plane (2026-10-12, lane 12c verification:
/// two such faces of a box split a micrometre off a plane had a piece whose two ends sat at
/// the same place along that line, ordered by vertex id, pointing the wrong way): those
/// pieces are sorted along their own spread and laid without a vote ([`Piece::vote`]).
fn cut_pieces(b: &crate::brep::Brep, plane: &Plane, dist: &BTreeMap<VertexId, f64>) -> Cut {
    let n = plane.normal;
    let kept = |v: VertexId| dist[&v] >= 0.0;
    let mut points: BTreeMap<Node, DVec3> = BTreeMap::new();
    let mut pieces: Vec<Piece> = Vec::new();
    for (_, f) in b.faces() {
        if parallel(plane, f) {
            continue; // parallel to the plane: never crossed (the tie rule)
        }
        let cross = n.cross(f.plane.normal);
        let t = cross.normalize();
        let mut xs: Vec<(DVec3, Node)> = Vec::new();
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
                xs.push((x, node));
            }
        }
        if xs.len() % 2 == 1 {
            continue; // cannot happen on a valid solid; never pair a stray crossing
        }
        let corners: Vec<DVec3> = f.loops().flat_map(|l| b.loop_points(l)).collect();
        let bend = corners
            .iter()
            .map(|p| f.plane.signed_distance(*p).abs())
            .fold(0.0, f64::max);
        let size = corners
            .iter()
            .map(|p| p.distance(corners[0]))
            .fold(0.0, f64::max);
        let weak = cross.length() * size <= WEAK * bend;
        // Sort along the line: the plane's line through the face, or for a weak face the
        // crossings' own spread (its farthest pair), turned the same way.
        let along = if weak {
            let mut d = DVec3::ZERO;
            for (i, (p, _)) in xs.iter().enumerate() {
                for (q, _) in &xs[i + 1..] {
                    if (*q - *p).length_squared() > d.length_squared() {
                        d = *q - *p;
                    }
                }
            }
            if d.dot(t) < 0.0 { -d } else { d }
        } else {
            t
        };
        let mut keyed: Vec<(f64, Node)> = xs
            .iter()
            .map(|(x, k)| ((*x - plane.origin).dot(along), *k))
            .collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for [a, b] in keyed.as_chunks::<2>().0 {
            if a.1 != b.1 {
                pieces.push(Piece {
                    a: a.1,
                    b: b.1,
                    vote: !weak,
                });
            }
        }
    }
    Cut { pieces, points }
}

/// The rings where `plane` cuts a solid, in the plane's frame: material on the left of
/// every ring (outer rings counter-clockwise, holes clockwise), every ring simple. Exact up
/// to the crossing interpolation; see the module docs for the tie rule.
///
/// Since lane 12d the rings come out of a planar arrangement ([`crate::arrangement`]): every
/// piece of every face ([`cut_pieces`]) is laid in it with its material side, split where it
/// crosses or touches another and merged where pieces coincide; the arrangement's faces are
/// material when the pieces around them say so, or else when the solid holds a point inside
/// them ([`inside_below`]); the cap is the union of the material faces. The pieces used to be
/// chained into rings directly, which went wrong two ways: a hole whose edge lies along the
/// cap's outline (a plane along the top edge of the hole in a frame) never met the outline
/// at a node, so the hole was lost and the cap was a plain rectangle; and a piece pointing
/// the wrong way (a weak face, above) left a ring that could not close, so the whole cap
/// was dropped.
pub(crate) fn solid_rings(o: &Object, plane: &Plane) -> Vec<Vec<DVec2>> {
    let b = o.brep();
    let dist = vertex_distances(b, plane);
    let cut = cut_pieces(b, plane, &dist);
    if cut.pieces.is_empty() {
        return Vec::new();
    }
    let scale = solid_scale(o);
    let mut arr = crate::arrangement::Arrangement::new(ARRANGE_TOL * scale);
    let at2d = |k: &Node| plane.to_local(cut.points[k]);
    for (i, p) in cut.pieces.iter().enumerate() {
        arr.insert(at2d(&p.a), at2d(&p.b), i32::from(p.vote), i);
    }
    arr.settle();
    let mut rings = Vec::new();
    for r in arr.material_rings(|q| inside_below(b, plane, &dist, q, scale)) {
        let pts = tidy_ring(r);
        if pts.len() >= 3 && sawhorse_tessellate::signed_area(&pts).abs() > RING_FLOOR {
            rings.push(pts);
        }
    }
    rings
}

/// The size a solid's coordinates reach, at least 1 (metres): what the cut's tolerances are
/// scaled by (the arrangement's [`ARRANGE_TOL`], the caps' [`cap_area_floor`]).
fn solid_scale(o: &Object) -> f64 {
    o.brep()
        .vertices()
        .map(|(_, v)| v.pos.abs().max_element())
        .fold(1.0, f64::max)
}

/// Below this a ring of the cut has no area at all (`AREA_EPS * 1e-6`, 1e-16 m2): it is the
/// arrangement's rounding, not material. [`solid_rings`] leaves such rings out and
/// [`Model::section_cut`] drops a cap whose area is no more; nothing else is ever dropped, so
/// the cut, the kept halves and the fill all read the same exact caps.
const RING_FLOOR: f64 = crate::tolerance::AREA_EPS * 1e-6;

/// The smallest area a merged loop may have for the slice to draw it, for a solid of `scale`
/// ([`solid_scale`]): the kernel's area epsilon (`AREA_EPS`, POINT_MERGE squared) per metre of
/// the solid's size.
pub(crate) fn cap_area_floor(scale: f64) -> f64 {
    crate::tolerance::AREA_EPS * scale.max(1.0)
}

/// Points this close are one point to the slice ([`weld`]). The sketch merges points
/// within POINT_MERGE (10 micrometres) of a point or an edge, measured with its own rounding;
/// merging a part per million further keeps every corner the slice draws clear of the
/// sketch's merging, so the sketch takes each loop exactly as merged here (2026-10-13, lane
/// 18: merging only neighbours closer than POINT_MERGE left a cap's tip corner 9.9 micrometres
/// from the next edge, which the sketch folded into the corner before it, so it refused the
/// edge between them as "too small").
const WELD: f64 = crate::tolerance::POINT_MERGE * (1.0 + 1e-6);

/// Loops with their points closer than [`WELD`] merged, as the sketch would merge them (lane
/// 18): the closest merge of the most local kind first, again until none is left.
///
/// 1. Two neighbours on a loop within WELD become one point, their midpoint.
/// 2. A corner so sharp that one of its neighbours lies within WELD of its other edge is a
///    spike the sketch folds flat: it goes.
/// 3. A corner within WELD of a corner of another loop moves onto it (the later loop's onto
///    the earlier's): the sketch makes them one point.
/// 4. A corner within WELD of an edge of another loop is put into that edge: the sketch
///    splits the edge there, so the loops share the point.
/// 5. A corner within WELD of an edge of its own loop that is neither its own nor next to it
///    pinches the loop: it is split into the loops either side, which share that corner.
///
/// Then every point is more than WELD from every other point and from every edge it is not
/// an end of (points at exactly the same place are one point, as in the sketch), so the
/// sketch, whose snapping and repair look only within POINT_MERGE, merges and splits nothing
/// but proper crossings: it takes each loop as drawn. Which merge comes first is decided by
/// the distances and the loops' order alone, never by where a loop starts or which way round
/// it runs, so a plane and its flip merge a cap alike (2026-10-13: walking round and dropping
/// a point near the last one kept welded the same cap's tip two ways, and the slice drew it on
/// one side only). A loop left with fewer than three points is gone. One ring alone is
/// [`representable`]'s merge; the slice welds all the loops it draws together (lane 18's fuzz,
/// seed 20263608: two boxes a micrometre apart, each cap fine alone, and the sketch merged
/// one's corner into the other's edge and refused what was left of an edge).
fn weld(mut loops: Vec<Vec<DVec2>>) -> Vec<Vec<DVec2>> {
    let budget = 64 * (16 + loops.iter().map(Vec::len).sum::<usize>());
    for _ in 0..budget {
        loops.retain(|l| l.len() >= 3);
        let Some(fix) = worst(&loops) else {
            return loops;
        };
        match fix {
            Fix::Merge(l, i) => {
                let r = &mut loops[l];
                let j = (i + 1) % r.len();
                r[i] = (r[i] + r[j]) * 0.5;
                r.remove(j);
            }
            Fix::Spike(l, t) => {
                loops[l].remove(t);
            }
            Fix::Move(l, i, to) => loops[l][i] = to,
            Fix::Insert(l, i, p) => loops[l].insert(i + 1, p),
            Fix::Pinch(l, i, j) => {
                let r = std::mem::take(&mut loops[l]);
                let n = r.len();
                loops[l] = (i + 1..=i + (j + n - i) % n).map(|k| r[k % n]).collect();
                loops.insert(l + 1, (j..=j + (i + n - j) % n).map(|k| r[k % n]).collect());
            }
        }
    }
    // Never reached in practice (every merge removes a point, makes two the same, or splits
    // a loop in two smaller ones); what is left is still drawn, and the sketch has its say.
    loops.retain(|l| l.len() >= 3);
    loops
}

/// One merge of [`weld`]: loop, index, and what to do there.
#[derive(Debug, Clone, Copy)]
enum Fix {
    /// Points `i` and `i + 1` become their midpoint.
    Merge(usize, usize),
    /// Point `t`, a spike's tip, goes.
    Spike(usize, usize),
    /// Point `i` moves onto another loop's point.
    Move(usize, usize, DVec2),
    /// Another loop's point goes into edge `i -> i + 1`.
    Insert(usize, usize, DVec2),
    /// Point `j` pinches edge `i -> i + 1` of its own loop.
    Pinch(usize, usize, usize),
}

/// A merge [`worst`] may pick: (kind, distance, loop, two indices, the merge).
type Candidate = (u8, f64, usize, usize, usize, Fix);

/// Keep `fix` in `best` when it is within [`WELD`] and comes before it: the lowest kind, then
/// the closest, then the first in the loops' order.
fn offer(best: &mut Option<Candidate>, kind: u8, d: f64, l: usize, a: usize, b: usize, fix: Fix) {
    if d <= WELD
        && best.is_none_or(|(k, bd, bl, ba, bb, _)| {
            (kind.cmp(&k))
                .then(d.total_cmp(&bd))
                .then(l.cmp(&bl))
                .then(a.cmp(&ba))
                .then(b.cmp(&bb))
                .is_lt()
        })
    {
        *best = Some((kind, d, l, a, b, fix));
    }
}

/// The merge [`weld`] makes next, or `None` when every point is clear: the lowest kind, then
/// the closest, then the first in the loops' order.
fn worst(loops: &[Vec<DVec2>]) -> Option<Fix> {
    use crate::sketch::point_segment_distance as seg;
    let mut best: Option<Candidate> = None;
    for (l, r) in loops.iter().enumerate() {
        let n = r.len();
        for i in 0..n {
            offer(
                &mut best,
                0,
                r[i].distance(r[(i + 1) % n]),
                l,
                i,
                0,
                Fix::Merge(l, i),
            );
        }
        for t in 0..n {
            let (p, c, q) = (r[(t + n - 1) % n], r[t], r[(t + 1) % n]);
            offer(
                &mut best,
                1,
                seg(p, c, q).min(seg(q, p, c)),
                l,
                t,
                0,
                Fix::Spike(l, t),
            );
        }
    }
    if best.is_some() {
        return best.map(|b| b.5);
    }
    // Every point near every edge, swept along x: each edge looks only at the points near it.
    let mut pts: Vec<(DVec2, usize, usize)> = loops
        .iter()
        .enumerate()
        .flat_map(|(l, r)| r.iter().enumerate().map(move |(i, p)| (*p, l, i)))
        .collect();
    pts.sort_by(|a, b| {
        a.0.x
            .total_cmp(&b.0.x)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
    for (l, r) in loops.iter().enumerate() {
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + 1) % n]);
            let (lo, hi) = (a.min(b) - DVec2::splat(WELD), a.max(b) + DVec2::splat(WELD));
            let from = pts.partition_point(|q| q.0.x < lo.x);
            for &(p, m, j) in &pts[from..] {
                if p.x > hi.x {
                    break;
                }
                if p.y < lo.y || p.y > hi.y || p == a || p == b {
                    continue;
                }
                let d = seg(p, a, b);
                if m == l {
                    let off = (j + n - i) % n;
                    if off > 2 && off != n - 1 {
                        offer(&mut best, 4, d, l, i, j, Fix::Pinch(l, i, j));
                    }
                    continue;
                }
                // Another loop's point near this edge: near one of its ends, the later loop's
                // point moves onto the earlier's; otherwise it goes into the edge.
                let (end, k) = if p.distance(a) <= p.distance(b) {
                    (a, i)
                } else {
                    (b, (i + 1) % n)
                };
                let near = p.distance(end);
                if near <= WELD {
                    let fix = if m > l {
                        Fix::Move(m, j, end)
                    } else {
                        Fix::Move(l, k, p)
                    };
                    offer(&mut best, 2, near, l.min(m), l.max(m), 0, fix);
                } else {
                    offer(&mut best, 3, d, l, i, m, Fix::Insert(l, i, p));
                }
            }
        }
    }
    best.map(|b| b.5)
}

/// Lane 18: **the one rule for what Create Group from Slice can draw** of a ring of the cut
/// (an outer ring or a hole): its loops after merging points closer than POINT_MERGE
/// ([`weld`], the ring alone) that keep three corners or more and an area above [`cap_area_floor`],
/// turning the ring's own way. The ring is *representable* when there is one; those loops are
/// what the slice draws, and the sketch takes every one of them. A cap is representable by its
/// outer ring ([`Cap::representable`]), and that flag is all the slice and every oracle read.
/// The cut never asks this to decide what it shows: a cap thinner than the sketch can hold is
/// still material, and the halves, the fill and the cross-section need it (2026-10-13, lane
/// 18: lane 17 dropped rings that welded to fewer than three corners, and with them slivers
/// 0.125 to 0.5 m long and a few micrometres wide, 5.5e-7 to 2.4e-6 m2 of cap).
pub(crate) fn representable(ring: &[DVec2], scale: f64) -> Vec<Vec<DVec2>> {
    let turn = sawhorse_tessellate::signed_area(ring).signum();
    let floor = cap_area_floor(scale);
    weld(vec![ring.to_vec()])
        .into_iter()
        .filter(|l| l.len() >= 3 && sawhorse_tessellate::signed_area(l) * turn > floor)
        .collect()
}

/// Directions (off `-normal`, along `u` and `v`) of the rays [`inside_below`] casts, tried in
/// turn until one meets nothing on an edge.
const RAYS: [(f64, f64); 6] = [
    (0.1371, 0.0583),
    (-0.0917, 0.1229),
    (0.0443, -0.1613),
    (-0.1789, -0.0371),
    (0.2113, 0.1931),
    (-0.0611, 0.2477),
];

/// Whether the solid holds the point `q` of the plane's frame, taken just below the plane
/// (on the side that is cut away, which is what the tie rule makes the cap: a face lying on
/// the plane is kept, so the cap is the cut a hair below it). A ray from `q` into the cut-away
/// side counts the faces it passes through; a face the tie rule keeps whole or puts on the
/// plane is never met. `None` when every ray runs within the tolerance of a face's edge or
/// starts on a face. The cap's arrangement asks this only about a face whose pieces disagree
/// or say nothing.
fn inside_below(
    b: &crate::brep::Brep,
    plane: &Plane,
    dist: &BTreeMap<VertexId, f64>,
    q: DVec2,
    scale: f64,
) -> Option<bool> {
    let start = plane.to_world(q);
    let faces: Vec<_> = b
        .faces()
        .filter(|(_, f)| {
            f.loops()
                .flat_map(|l| b.loop_vertices(l))
                .any(|v| dist[&v] < 0.0)
        })
        .map(|(_, f)| {
            let rings: Vec<Vec<DVec2>> = f
                .loops()
                .map(|l| {
                    b.loop_points(l)
                        .iter()
                        .map(|p| f.plane.to_local(*p))
                        .collect()
                })
                .collect();
            let bend = f
                .loops()
                .flat_map(|l| b.loop_points(l))
                .map(|p| f.plane.signed_distance(p).abs())
                .fold(0.0, f64::max);
            (&f.plane, rings, 1e-9 * scale + 2.0 * bend)
        })
        .collect();
    'rays: for (a, c) in RAYS {
        let r = (-plane.normal + plane.u * a + plane.v * c).normalize();
        let mut hits = 0u32;
        for (fp, rings, tol) in &faces {
            let h = fp.signed_distance(start);
            let denom = r.dot(fp.normal);
            let s = if denom.abs() < 1e-9 {
                f64::NAN
            } else {
                -h / denom
            };
            // The ray starts in the face's plane (or runs along it): if it starts on the
            // face, no ray can tell.
            let on_plane = s.is_nan() || s.abs() <= *tol;
            let at = if on_plane {
                if h.abs() > *tol {
                    continue;
                }
                start
            } else if s < 0.0 {
                continue;
            } else {
                start + r * s
            };
            let p = fp.to_local(at);
            let near = rings.iter().any(|ring| {
                (0..ring.len()).any(|i| {
                    crate::sketch::point_segment_distance(p, ring[i], ring[(i + 1) % ring.len()])
                        <= *tol
                })
            });
            if near {
                continue 'rays;
            }
            let inside = rings
                .iter()
                .filter(|ring| crate::validate::point_in_ring(p, ring))
                .count()
                % 2
                == 1;
            if inside {
                if on_plane {
                    continue 'rays;
                }
                hits += 1;
            }
        }
        return Some(hits % 2 == 1);
    }
    None
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
            .filter(|(_, (o, _))| crate::validate::point_in_ring(probe, o))
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
            let scale = solid_scale(obj);
            for (outer, holes) in assemble(solid_rings(obj, &frame)) {
                let mut outer = outer;
                let mut holes = holes;
                for h in &others {
                    outer = clip(&outer, h);
                    holes = holes.iter().map(|r| clip(r, h)).collect();
                }
                // Lane 18: every cap exactly as the plane cuts it, never thinned out for the
                // slice (the halves, the fill and the cross-section read these rings); only
                // what other planes clip away to nothing goes. Whether the slice can draw it
                // is the cap's flag (`representable`).
                let outer = tidy_ring(outer);
                if outer.len() < 3 {
                    continue;
                }
                let holes: Vec<Vec<DVec2>> = holes
                    .into_iter()
                    .map(tidy_ring)
                    .filter(|r| r.len() >= 3)
                    .collect();
                let area = sawhorse_tessellate::signed_area(&outer)
                    + holes
                        .iter()
                        .map(|r| sawhorse_tessellate::signed_area(r))
                        .sum::<f64>();
                if area <= RING_FLOOR {
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
                    representable: !representable(&outer, scale).is_empty(),
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
    /// caps drawn into a sketch on the plane, inside a new group in the open context. Closed
    /// loops become faces of the sketch, ready to push. Every representable cap is drawn
    /// ([`representable`]); a plane that cuts nothing, and one whose every cap is thinner than
    /// the model can draw, are refused, each in its own words.
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
        if cut.caps.is_empty() {
            return Err(SectionError::NothingToSlice.into());
        }
        if !cut.caps.iter().any(|c| c.representable) {
            return Err(SectionError::SliceTooThin.into());
        }
        let frame = self.sections[k].plane();
        let mut sketch = crate::sketch::Sketch::new(frame);
        // Lane 18: every representable cap, each of its rings as the one rule merges it
        // (`representable`), and then all those loops welded together (`weld`): a corner of
        // one cap within the merge distance of another cap's corner or edge is merged as the
        // sketch would, so every corner is clear of the sketch's merging and the sketch takes
        // each loop as drawn; each closes into a face. A hole thinner than that is not drawn
        // (the face covers the slit). (Lane 17 welded a ring by walking round it, which left a
        // tip corner within the merge distance of the next edge in one of the plane's two
        // orientations, and drew each ring alone, so a corner of one cap a micrometre from
        // another's edge was merged into it: the sketch refused both.)
        let mut loops = Vec::new();
        for c in cut.caps.iter().filter(|c| c.representable) {
            let scale = solid_scale(&self.objects[from_public::<ObjectId>(c.object)]);
            for r in std::iter::once(&c.outer).chain(&c.holes) {
                let ring: Vec<DVec2> = r.iter().map(|p| DVec2::from_array(*p)).collect();
                loops.extend(representable(&ring, scale));
            }
        }
        for l in weld(loops) {
            for i in 0..l.len() {
                sketch.insert_segment(l[i], l[(i + 1) % l.len()])?;
            }
        }
        debug_assert!(sketch.edge_count() > 0, "a representable cap drew nothing");
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

    /// CX2 in miniature: a frame cut by a plane through the top edge of its hole. One side's
    /// top face crosses the plane in a single piece straight over the hole while the hole's
    /// wall lays its edge along it the other way; the arrangement cancels the two, so both
    /// sides get the same notched cap (it used to be a plain rectangle on one side).
    #[test]
    fn a_frame_cut_along_its_hole_edge_keeps_the_notch() {
        let sq = |x0: f64, y0: f64, x1: f64, y1: f64| {
            vec![
                DVec2::new(x0, y0),
                DVec2::new(x1, y0),
                DVec2::new(x1, y1),
                DVec2::new(x0, y1),
            ]
        };
        let mut hole = sq(1.0, 1.5, 2.0, 2.5);
        hole.reverse();
        let o = Object::extrude(&Plane::GROUND, &sq(0.0, 0.0, 3.0, 3.0), &[hole], 1.0).unwrap();
        // Through the hole's top edge (y = 2.5, z = 1), rising 1 in 2: it leaves the slab at
        // y = 0.5 on the bottom, so the cut spans y 0.5..2.5 and the hole notches its top.
        let n = DVec3::new(0.0, 1.0, -2.0).normalize();
        let expect = 5.0 * 5f64.sqrt() / 2.0;
        for normal in [n, -n] {
            let plane = Plane::new(DVec3::new(0.0, 2.5, 1.0), normal).unwrap();
            let caps = assemble(solid_rings(&o, &plane));
            assert_eq!(caps.len(), 1, "{normal}: {caps:?}");
            assert!(caps[0].1.is_empty(), "{normal}: {caps:?}");
            assert_eq!(caps[0].0.len(), 8, "{normal}: {caps:?}");
            let a = sawhorse_tessellate::signed_area(&caps[0].0);
            assert!((a - expect).abs() < 1e-12, "{normal}: {a} {expect}");
        }
    }

    /// The ray that settles a face the pieces disagree on: inside a box, just below a plane
    /// through it, yes; outside, no; on a face the plane lies on, the side that is cut away.
    #[test]
    fn the_ray_reads_the_solid_just_below_the_plane() {
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
        let b = o.brep();
        let at = |plane: &Plane, p: DVec3| {
            let dist = vertex_distances(b, plane);
            inside_below(b, plane, &dist, plane.to_local(p), 2.0)
        };
        let mid = Plane::new(DVec3::new(0.0, 0.0, 0.5), DVec3::Z).unwrap();
        assert_eq!(at(&mid, DVec3::new(1.0, 0.5, 0.5)), Some(true));
        assert_eq!(at(&mid, DVec3::new(3.0, 0.5, 0.5)), Some(false));
        // On the top face: facing out (down into the box) the box is below; facing in, not.
        let out = Plane::new(DVec3::new(0.0, 0.0, 1.0), DVec3::Z).unwrap();
        assert_eq!(at(&out, DVec3::new(1.0, 0.5, 1.0)), Some(true));
        let into = Plane::new(DVec3::new(0.0, 0.0, 1.0), -DVec3::Z).unwrap();
        assert_eq!(at(&into, DVec3::new(1.0, 0.5, 1.0)), Some(false));
    }
}
