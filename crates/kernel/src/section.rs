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
//!   section fill paints), and the outline segments around them. Since lane 19c it is built
//!   so that it closes the half it caps **by construction**, whatever the floating-point
//!   geometry: every vertex has one signed distance from the plane, zero within [`ON_FACE`]
//!   ([`distances`]; zero counts as kept: a plane laid on a face, facing into the solid, cuts
//!   nothing); the solid's own triangles (the ones `Object::tri_solid` measures it by) are
//!   clipped by their corners' distances, each edge's crossing worked out once for both
//!   triangles that share it; the chords those clipped triangles run along the plane are
//!   exactly the boundary their kept parts leave open, and the cap is laid on a planar
//!   arrangement from those chords ([`crate::arrangement`]). Other active sections that
//!   reach the same solids clip it. Deterministic and hashed.
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

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};
use slotmap::SecondaryMap;

use crate::brep::{Brep, Face};
use crate::error::{KernelError, SectionError, StructureError};
use crate::hash::StableHasher;
use crate::ids::{FaceId, NodeId, ObjectId, SectionId, VertexId, from_public, to_public};
use crate::model::{Model, OpOutcome};
use crate::nodes::NodeKind;
use crate::object::Object;
use crate::plane::Plane;
use crate::transform::Xform;

/// The longest name a section plane takes.
pub const MAX_NAME: usize = 64;

/// **The tie rule, the one tolerance of the cut** (lane 19c): a vertex whose signed distance
/// from a section plane is this or less in size is on the plane, distance exactly zero, and
/// zero counts as kept. It is the kernel's noise floor for lengths (`LENGTH_EPS`, 1 nm): far
/// below the merge distance (a vertex this close to the plane cannot be told from one on it),
/// far above f64 noise on coordinates within the model's 10 km. Nothing else decides a
/// vertex's side: no face, surface or bend ([`distances`]).
pub const ON_FACE: f64 = crate::tolerance::LENGTH_EPS;

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
    /// The surface's vector area, square metres: every kept face ring's own (Newell's sum
    /// over its 3D corners and crossings) plus the cap's (its area along -normal). Every
    /// edge of a closed surface is walked once each way, so the sum cancels. Since lane 19c
    /// the cut is built so that it does ([`clip_solid`]): along the normal to rounding, and
    /// across it to within the farthest a chord's end sits off the plane (a corner the tie
    /// rule puts on the plane is up to ON_FACE off it) times the cut's length. A cap
    /// missing, folded or facing the wrong way leaves its area here. It measures how far the
    /// cut is from watertight.
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

    /// Another solid's part, added.
    fn absorb(&mut self, o: KeptPart) {
        self.volume += o.volume;
        self.vector_area += o.vector_area;
        self.area += o.area;
        self.cap_length += o.cap_length;
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

// ---- the cut (lane 19c): one sign per vertex, the solid's own triangles, their chords ------

/// One signed distance per vertex of a solid ([`distances`]).
pub(crate) type Distances = SecondaryMap<VertexId, f64>;

/// **One sign per vertex** (lane 19c, ADR-0010): the signed distance of every vertex of `b`
/// from `plane`, worked out once, and exactly zero when it is within [`ON_FACE`] (zero counts
/// as kept). These are the only distances the cut, the kept halves and the oracles read.
///
/// History. Lanes 12 to 19b also decided a vertex's side by the faces around it: a face
/// with every corner within twice its own bend of the plane lay on it (lane 12d), decided
/// once per surface of faces sharing a stored plane (lane 19) or meeting within the merge
/// distance (lane 19b), a parallel surface at its corners' mean, and faces cut along the
/// line their stored plane gives or, when weak, along their triangles. Each pass fixed the
/// case a verifier's fuzz found and the hostile generator found the next sibling, 1 in 130k
/// to 700k cases, always faces nearly on the plane classified one way and cut another; the
/// last (the lane-19b verifier's) was a box top whose poked corner, 1.8 micrometres off the
/// plane, was beyond twice its 0.65 micrometre bend, two n-gons drawn over it, a plane at the
/// original top: the cut-away half kept 0.0251 m2 of sheet with no cap
/// (`tests/repro/2026-10-14-section-top-face-poked-beyond-its-bend-*.json`). Lane 19c stopped
/// patching rules: with one distance per vertex and the cut made of the solid's own
/// triangles ([`clip_solid`]), the chords the cap is laid from are exactly what the kept
/// triangles leave open, whatever the distances are.
pub(crate) fn distances(b: &Brep, plane: &Plane) -> Distances {
    let mut out = SecondaryMap::with_capacity(b.vertex_count());
    for (k, v) in b.vertices() {
        let d = plane.signed_distance(v.pos);
        out.insert(k, if d.abs() <= ON_FACE { 0.0 } else { d });
    }
    out
}

/// A point of the cut, named by where it is: a vertex of the solid (`(v, v)`; on the plane,
/// or a kept corner of a face), or where the plane crosses the segment between two vertices
/// whose distances have opposite signs (`(lo, hi)`, low first). A crossing is worked out once,
/// from its two vertices alone ([`node_point`]), so every triangle that has the segment, on
/// either side, sees the same point.
type Node = (VertexId, VertexId);

/// The crossing node on the segment between `p` and `q`.
fn edge_node(p: VertexId, q: VertexId) -> Node {
    if p < q { (p, q) } else { (q, p) }
}

/// Where node `n` is: the vertex, or the point between its two vertices where their
/// distances interpolate to zero (from the low one).
fn node_point(b: &Brep, dist: &Distances, n: Node) -> DVec3 {
    let (lo, hi) = n;
    let pl = b.vertices[lo].pos;
    if lo == hi {
        return pl;
    }
    let ph = b.vertices[hi].pos;
    let (dl, dh) = (dist[lo], dist[hi]);
    pl + (ph - pl) * (dl / (dl - dh))
}

/// A face's triangles, in the face's own turn: the tessellator's, in the face's own frame,
/// exactly the ones `Object::tri_solid` measures the solid by. For a face the tessellator
/// refuses (a committed solid has none: the validator refuses such a solid first), a fan from
/// the first corner of each loop. Either way they are a 2-chain whose boundary is the face's
/// loops (every diagonal is walked once each way), which is all the cut needs.
fn face_triangles(b: &Brep, f: &Face, loops: &[Vec<VertexId>]) -> Vec<[VertexId; 3]> {
    let flat: Vec<VertexId> = loops.iter().flatten().copied().collect();
    let rings: Vec<Vec<DVec2>> = loops
        .iter()
        .map(|l| {
            l.iter()
                .map(|v| f.plane.to_local(b.vertices[*v].pos))
                .collect()
        })
        .collect();
    match sawhorse_tessellate::triangulate(&rings[0], &rings[1..]) {
        Ok(ts) => ts.iter().map(|t| t.map(|i| flat[i as usize])).collect(),
        Err(_) => loops
            .iter()
            .flat_map(|l| (1..l.len().saturating_sub(1)).map(move |i| [l[0], l[i], l[i + 1]]))
            .collect(),
    }
}

/// What one triangle keeps ([`clip_triangle`]).
struct TriClip {
    /// The kept polygon, in the triangle's own turn (empty: nothing is kept).
    kept: Vec<Node>,
    /// Where the plane splits it: from where its boundary leaves the kept side to where it
    /// comes back, the kept part on its left.
    chord: Option<(Node, Node)>,
}

/// **The classic table** for one triangle, its corners in its own turn, under one distance
/// per corner, zero counting as kept:
///
/// - no corner below the plane (`+ + +`, `+ + 0`, `+ 0 0`, `0 0 0`): the whole triangle;
/// - no corner above it and one below (`- - -`, `- - 0`, `- 0 0`): nothing;
/// - otherwise (a corner above and one below) the plane splits it: the kept part is the
///   polygon of its kept corners and the crossings of the edges running from a corner above
///   to one below, and its chord runs from where its boundary leaves the kept side to where
///   it comes back, the crossing on that edge or, on an edge from a corner on the plane to
///   one below, that corner. Every such triangle has exactly one chord (its kept and cut
///   corners change over twice going round it).
fn clip_triangle(t: [VertexId; 3], dist: &Distances) -> TriClip {
    let d = t.map(|v| dist[v]);
    if d.iter().all(|&x| x >= 0.0) {
        return TriClip {
            kept: t.iter().map(|&v| (v, v)).collect(),
            chord: None,
        };
    }
    if d.iter().all(|&x| x <= 0.0) {
        return TriClip {
            kept: Vec::new(),
            chord: None,
        };
    }
    let mut kept = Vec::with_capacity(4);
    let (mut out, mut back) = (None, None);
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (p, q) = (t[i], t[j]);
        let (kp, kq) = (d[i] >= 0.0, d[j] >= 0.0);
        if kp {
            kept.push((p, p));
        }
        if kp != kq {
            // One end is below the plane; the other is on it (that corner is where the
            // boundary crosses) or above it (the edge's own crossing, a corner of the part).
            let node = if d[i] == 0.0 {
                (p, p)
            } else if d[j] == 0.0 {
                (q, q)
            } else {
                edge_node(p, q)
            };
            if node.0 != node.1 {
                kept.push(node);
            }
            if kp {
                out = Some(node);
            } else {
                back = Some(node);
            }
        }
    }
    TriClip {
        kept,
        chord: out.zip(back),
    }
}

/// A directed segment of a face's kept part.
#[derive(Debug, Clone, Copy)]
struct Seg {
    a: Node,
    b: Node,
    /// A triangle's chord (every other segment runs along an edge of a triangle).
    chord: bool,
}

/// One solid under one distance per vertex ([`clip_solid`]).
struct Clipped {
    /// Every face with anything kept, in face order: the rings of its kept part (nodes, in
    /// the face's turn), its kept triangles merged back into one face.
    faces: Vec<(FaceId, Vec<Vec<Node>>)>,
    /// The chords, in order: every segment the kept faces leave open, the kept part on its
    /// left, so the material is on its left seen from the plane's normal.
    chords: Vec<(Node, Node)>,
}

/// A run of chords found together ([`clip_solid`]).
struct Run {
    /// Where it was found: the face's place in face order, then the place of its first
    /// segment in that face.
    order: (usize, usize),
    segs: Vec<(Node, Node)>,
    /// A kept edge of the solid with both ends on the plane, waiting for the face across: if
    /// that face keeps its side too, the two run opposite ways and close each other.
    edge: bool,
}

/// **The cut of one solid** (lane 19c, ADR-0010): every face clipped along its own triangles
/// under the distances `dist` (one per vertex, [`distances`]), the triangles each face keeps
/// merged back into its kept rings, and the chords those rings leave open, which are what the
/// cap is laid from. `tol` (metres) is how far a chain of chords across one face may bend
/// and still be laid as one straight chord: the cap's arrangement tolerance.
///
/// **Why the cap closes the half, by construction.** The triangles of the faces are a closed
/// oriented surface: every edge of a triangle is an edge of exactly one other, walked the
/// other way (a diagonal of a face by the face's next triangle, an edge of the solid by the
/// triangle of the face across). What a triangle keeps of an edge depends on the edge's two
/// distances alone: all of it when neither end is below the plane, the part from the end
/// above to the crossing (one point, [`node_point`]) when one end is above and one below,
/// nothing else of any length; so both triangles of an edge keep the same part of it and
/// walk it opposite ways, and it closes, except along an edge with both ends on the plane,
/// which a triangle keeps whole when its third corner is not below the plane and not at all
/// when it is: such an edge stays open when one of its two triangles is kept and the other
/// is not. So what the kept triangles leave open is exactly (1) the chord of every triangle
/// the plane splits ([`clip_triangle`]) and (2) every edge with both ends on the plane kept by
/// one of its triangles only, walked as that triangle walks it; these are the chords, one
/// to one (`tests::the_kept_triangles_leave_open_exactly_the_chords` walks the kept triangles
/// of hostile solids under the tie rule's distances and under random ones and matches every
/// open segment to a chord). They close up into a cycle, being the boundary of a surface, and
/// the cap laid on them in the plane (the arrangement keeps them a cycle, and takes for
/// material every face its winding number puts inside, [`crate::arrangement`]) has them as
/// its boundary, walked the other way: kept faces and cap close each other edge for edge
/// (where the chords wind round no point of the plane twice: an embedded solid cut by its
/// own distances never makes them, and corners forced onto the plane could only where two
/// sheets of its surface lie within about a nanometre of each other and of the plane; there
/// the cap would be the union, and `testing::check_sections` would show the overlap).
/// Along the normal that is exact (the cap's boundary is the chords seen in the plane); across
/// it, a chord's end is off the plane only by how far the tie rule moved its corner (at most
/// ON_FACE). No face's stored plane, bend or neighbours enter it, so it holds for any
/// floating-point geometry.
///
/// **Faces, not triangles.** A face with no corner below the plane is kept whole (its loops;
/// its triangles' diagonals would close each other), a face with every corner below it keeps
/// nothing, and only a face the plane splits is triangulated. Its kept triangles are merged
/// back: the segments its triangles walk both ways close (its diagonals), the chords that
/// meet at a crossing on a diagonal are joined into one run from where the run enters the
/// face to where it leaves, and a run that lies within `tol` of a straight line is laid as
/// that line (Douglas and Peucker): a planar face cut by the plane gives exactly the chord
/// between the two crossings on its edges, as it did before lane 19c, so the cap's corners are
/// the crossings on the solid's own edges and every golden hashes as before. The merged rings
/// and the chords take the same straightened runs, so they still close each other.
fn clip_solid(b: &Brep, plane: &Plane, dist: &Distances, tol: f64) -> Clipped {
    let mut out = Clipped {
        faces: Vec::new(),
        chords: Vec::new(),
    };
    let below = dist.values().any(|&d| d < 0.0);
    let kept = dist.values().any(|&d| d >= 0.0);
    if !kept {
        return out;
    }
    let mut runs: Vec<Run> = Vec::new();
    for (place, (fk, f)) in b.faces().enumerate() {
        let loops: Vec<Vec<VertexId>> = f.loops().map(|l| b.loop_vertices(l)).collect();
        let any_below = below && loops.iter().flatten().any(|v| dist[*v] < 0.0);
        if !any_below {
            // Kept whole: its loops, and its edges with both ends on the plane.
            for (k, (p, q)) in loops
                .iter()
                .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
                .enumerate()
            {
                if dist[p] == 0.0 && dist[q] == 0.0 {
                    runs.push(Run {
                        order: (place, k),
                        segs: vec![((p, p), (q, q))],
                        edge: true,
                    });
                }
            }
            let rings = loops
                .iter()
                .map(|l| l.iter().map(|&v| (v, v)).collect())
                .collect();
            out.faces.push((fk, rings));
            continue;
        }
        if loops.iter().flatten().all(|v| dist[*v] < 0.0) {
            continue;
        }
        let rings = clip_face(b, plane, dist, tol, f, &loops, place, &mut runs);
        if !rings.is_empty() {
            out.faces.push((fk, rings));
        }
    }
    // The solid's edges with both ends on the plane: one kept from both sides closes.
    let mut alive = vec![true; runs.len()];
    let mut waiting: BTreeMap<(Node, Node), Vec<usize>> = BTreeMap::new();
    for i in 0..runs.len() {
        if !runs[i].edge {
            continue;
        }
        let (a, e) = runs[i].segs[0];
        if let Some(js) = waiting.get_mut(&(e, a))
            && let Some(j) = js.pop()
        {
            alive[i] = false;
            alive[j] = false;
            continue;
        }
        waiting.entry((a, e)).or_default().push(i);
    }
    let mut order: Vec<usize> = (0..runs.len()).filter(|&i| alive[i]).collect();
    order.sort_by_key(|&i| runs[i].order);
    out.chords = order
        .into_iter()
        .flat_map(|i| runs[i].segs.iter().copied())
        .collect();
    out
}

/// One face the plane splits ([`clip_solid`]): its triangles clipped, merged back into the
/// rings of its kept part (returned), and its runs of chords added to `runs`.
#[allow(clippy::too_many_arguments)]
fn clip_face(
    b: &Brep,
    plane: &Plane,
    dist: &Distances,
    tol: f64,
    f: &Face,
    loops: &[Vec<VertexId>],
    place: usize,
    runs: &mut Vec<Run>,
) -> Vec<Vec<Node>> {
    // The face's own edges; any other segment between two of its corners is a diagonal.
    let edges: BTreeSet<Node> = loops
        .iter()
        .flat_map(|l| (0..l.len()).map(move |i| edge_node(l[i], l[(i + 1) % l.len()])))
        .collect();
    let on_diagonal = |n: Node| n.0 != n.1 && !edges.contains(&n);
    let mut segs: Vec<Seg> = Vec::new();
    for t in face_triangles(b, f, loops) {
        let c = clip_triangle(t, dist);
        let k = c.kept.len();
        for i in 0..k {
            let (a, e) = (c.kept[i], c.kept[(i + 1) % k]);
            segs.push(Seg {
                a,
                b: e,
                chord: c.chord == Some((a, e)),
            });
        }
    }
    // A segment two kept triangles walk opposite ways (a diagonal, or part of one) closes.
    let mut alive = vec![true; segs.len()];
    let mut waiting: BTreeMap<(Node, Node), Vec<usize>> = BTreeMap::new();
    for i in 0..segs.len() {
        if segs[i].chord {
            continue;
        }
        if let Some(js) = waiting.get_mut(&(segs[i].b, segs[i].a))
            && let Some(j) = js.pop()
        {
            alive[i] = false;
            alive[j] = false;
            continue;
        }
        waiting.entry((segs[i].a, segs[i].b)).or_default().push(i);
    }
    // Runs: chords joined through the crossings on the face's diagonals (each has exactly one
    // chord in and one out: both triangles of the diagonal are split), from where a run
    // enters the face to where it leaves; what is left goes round in closed runs (a level of
    // a bent face round one of its holes, crossing only diagonals).
    let mut from: BTreeMap<Node, usize> = BTreeMap::new();
    for (i, s) in segs.iter().enumerate() {
        if s.chord && on_diagonal(s.a) {
            from.insert(s.a, i);
        }
    }
    let mut used = vec![false; segs.len()];
    let mut chains: Vec<Vec<usize>> = Vec::new();
    for open in [true, false] {
        for i in 0..segs.len() {
            if !segs[i].chord || used[i] || (open && on_diagonal(segs[i].a)) {
                continue;
            }
            let mut chain = vec![i];
            used[i] = true;
            let mut at = segs[i].b;
            while on_diagonal(at) {
                match from.get(&at) {
                    Some(&j) if !used[j] => {
                        used[j] = true;
                        chain.push(j);
                        at = segs[j].b;
                    }
                    _ => break,
                }
            }
            chains.push(chain);
        }
    }
    let mut ring_segs: Vec<(Node, Node)> = Vec::new();
    for chain in &chains {
        let mut nodes: Vec<Node> = std::iter::once(segs[chain[0]].a)
            .chain(chain.iter().map(|&i| segs[i].b))
            .collect();
        if nodes.len() > 2 && nodes.first() != nodes.last() {
            let pts: Vec<DVec2> = nodes
                .iter()
                .map(|n| plane.to_local(node_point(b, dist, *n)))
                .collect();
            nodes = straighten(&pts, tol)
                .into_iter()
                .map(|k| nodes[k])
                .collect();
        }
        let run: Vec<(Node, Node)> = nodes.windows(2).map(|w| (w[0], w[1])).collect();
        ring_segs.extend(&run);
        runs.push(Run {
            order: (place, chain.iter().copied().min().unwrap_or(0)),
            segs: run,
            edge: false,
        });
    }
    // What is left open of the triangles' edges: the face's own edges (kept whole or in
    // part; the faces across close them) and, among them or on diagonals, edges with both
    // ends on the plane kept on one side only.
    for i in 0..segs.len() {
        let s = segs[i];
        if s.chord || !alive[i] {
            continue;
        }
        ring_segs.push((s.a, s.b));
        let on = |n: Node| n.0 == n.1 && dist[n.0] == 0.0;
        if on(s.a) && on(s.b) {
            runs.push(Run {
                order: (place, i),
                segs: vec![(s.a, s.b)],
                // Along an edge of the solid the face across decides; along a diagonal the
                // triangle across is this face's own, and it did not keep it.
                edge: edges.contains(&edge_node(s.a.0, s.b.0)),
            });
        }
    }
    link(&ring_segs)
}

/// Segments that close up (as many leave every node as reach it), linked into rings: from
/// each unused segment in order, on along the first unused segment out of each node, until
/// the ring is back where it started.
fn link(segs: &[(Node, Node)]) -> Vec<Vec<Node>> {
    let mut out_of: BTreeMap<Node, std::collections::VecDeque<usize>> = BTreeMap::new();
    for (i, s) in segs.iter().enumerate() {
        out_of.entry(s.0).or_default().push_back(i);
    }
    let mut used = vec![false; segs.len()];
    let mut rings = Vec::new();
    for i in 0..segs.len() {
        if used[i] {
            continue;
        }
        let start = segs[i].0;
        let mut ring = Vec::new();
        let mut j = i;
        loop {
            used[j] = true;
            ring.push(segs[j].0);
            let at = segs[j].1;
            if at == start {
                break;
            }
            let mut next = None;
            if let Some(q) = out_of.get_mut(&at) {
                while let Some(k) = q.pop_front() {
                    if !used[k] {
                        next = Some(k);
                        break;
                    }
                }
            }
            match next {
                Some(k) => j = k,
                None => break, // cannot happen: the segments of a kept face close up
            }
        }
        rings.push(ring);
    }
    rings
}

/// The points of an open run to keep so that every point dropped is within `tol` of the line
/// kept (Douglas and Peucker: the farthest point from the line between the ends stays when it
/// is farther than `tol`, and each side is done again). The ends always stay.
fn straighten(pts: &[DVec2], tol: f64) -> Vec<usize> {
    let n = pts.len();
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut todo = vec![(0, n - 1)];
    while let Some((a, c)) = todo.pop() {
        if c <= a + 1 {
            continue;
        }
        let (mut far, mut at) = (-1.0, a);
        for k in a + 1..c {
            let d = crate::sketch::point_segment_distance(pts[k], pts[a], pts[c]);
            if d > far {
                far = d;
                at = k;
            }
        }
        if far > tol {
            keep[at] = true;
            todo.push((a, at));
            todo.push((at, c));
        }
    }
    (0..n).filter(|&k| keep[k]).collect()
}

/// The tolerance of the cap's arrangement, per metre of the solid's largest coordinate: a
/// few hundred rounding errors, far below anything the model can hold apart.
const ARRANGE_TOL: f64 = 2e-13;

/// The rings where `plane` cuts a solid, in the plane's frame: material on the left of
/// every ring (outer rings counter-clockwise, holes clockwise), every ring simple. Exact up
/// to the crossing interpolation; see the module docs for the tie rule.
///
/// The rings come out of a planar arrangement ([`crate::arrangement`], lane 12d) laid from the
/// chords of the solid's kept faces ([`clip_solid`], lane 19c), which close up by construction:
/// every distinct end of a chord is made a vertex once, every chord is laid between its two
/// vertices (split where it crosses or touches another), and the faces of the arrangement
/// whose winding number is 1 or more are the cap.
pub(crate) fn solid_rings(o: &Object, plane: &Plane) -> Vec<Vec<DVec2>> {
    rings_under(o, plane, &distances(o.brep(), plane))
}

/// [`solid_rings`] under any distances `dist` (one per vertex; [`distances`] gives the tie
/// rule's).
fn rings_under(o: &Object, plane: &Plane, dist: &Distances) -> Vec<Vec<DVec2>> {
    // A solid with no corner below the plane is kept whole, and one with every corner below
    // it is cut away whole: neither has a cap.
    if !dist.values().any(|&d| d < 0.0) || dist.values().all(|&d| d < 0.0) {
        return Vec::new();
    }
    let tol = ARRANGE_TOL * solid_scale(o);
    let cut = clip_solid(o.brep(), plane, dist, tol);
    cap_rings(o.brep(), plane, dist, &cut.chords, tol)
}

/// The cap laid from `chords` ([`solid_rings`]).
fn cap_rings(
    b: &Brep,
    plane: &Plane,
    dist: &Distances,
    chords: &[(Node, Node)],
    tol: f64,
) -> Vec<Vec<DVec2>> {
    if chords.is_empty() {
        return Vec::new();
    }
    let mut arr = crate::arrangement::Arrangement::new(tol);
    let mut ids: BTreeMap<Node, usize> = BTreeMap::new();
    for &(a, c) in chords {
        for n in [a, c] {
            if let std::collections::btree_map::Entry::Vacant(e) = ids.entry(n) {
                e.insert(arr.vertex(plane.to_local(node_point(b, dist, n))));
            }
        }
    }
    for (i, (a, c)) in chords.iter().enumerate() {
        arr.insert_between(ids[a], ids[c], 1, i);
    }
    arr.settle();
    tidy_rings(arr.material_rings())
        .into_iter()
        .filter(|r| r.len() >= 3 && sawhorse_tessellate::signed_area(r).abs() > RING_FLOOR)
        .collect()
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

/// Drop repeated points and points on one line with their neighbours: between them, or
/// the tip of a zero-width spike (a ring that runs out along a line and back, as one
/// through a vertex touching the plane can).
#[cfg(test)]
fn tidy_ring(pts: Vec<DVec2>) -> Vec<DVec2> {
    tidy_ring_keeping(pts, &BTreeSet::new())
}

/// [`tidy_ring`], leaving every point in `keep` (as bits) where it is, unless it repeats
/// its neighbour.
fn tidy_ring_keeping(mut pts: Vec<DVec2>, keep: &BTreeSet<[u64; 2]>) -> Vec<DVec2> {
    let mut changed = true;
    while changed && pts.len() >= 3 {
        changed = false;
        let n = pts.len();
        for i in 0..n {
            let (a, p, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let same = p.distance(a) <= 1e-12;
            let in_line =
                (p - a).perp_dot(c - p).abs() <= 1e-18 * (1.0 + a.distance(p) + p.distance(c));
            if same || (in_line && !keep.contains(&bits(p))) {
                pts.remove(i);
                changed = true;
                break;
            }
        }
    }
    pts
}

/// A point as its two coordinates' bits.
fn bits(p: DVec2) -> [u64; 2] {
    [p.x.to_bits(), p.y.to_bits()]
}

/// [`tidy_ring`] for rings that may touch each other (a cap's outer ring and a hole that
/// meet at a corner, two caps pinched apart at one): a point that more than one of them
/// passes stays in each, even where it lies in line with its neighbours in one of them.
/// Dropped from one ring only, that ring's edge would run straight past the other ring's
/// corner, a rounding error to one side of it or the other, and the rings would cross there
/// (lane 19c's property runs, seed 2026101953: a hole touching its cap's outer ring at a
/// crossing on its straight edge, the corner dropped from the outer ring and the hole's
/// first edge then crossing it by a rounding error).
fn tidy_rings(rings: Vec<Vec<DVec2>>) -> Vec<Vec<DVec2>> {
    let mut seen: BTreeMap<[u64; 2], usize> = BTreeMap::new();
    for r in &rings {
        let mine: BTreeSet<[u64; 2]> = r.iter().map(|p| bits(*p)).collect();
        for k in mine {
            *seen.entry(k).or_default() += 1;
        }
    }
    let shared: BTreeSet<[u64; 2]> = seen
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(k, _)| k)
        .collect();
    rings
        .into_iter()
        .map(|r| tidy_ring_keeping(r, &shared))
        .collect()
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

/// A cap (its outer ring and holes) clipped by other planes' half-planes `others`, as simple
/// rings (lane 19c). Each ring is clipped on its own (Sutherland and Hodgman) first, which is
/// exact for a convex ring, and that answer stands when its rings are simple ([`simple`]):
/// every cut clipped by another plane before lane 19c is clipped the same way, and every
/// golden hashes as before. Where it is not (the fuzzer's finds, seeds 20267302 and
/// 20267307: a hole clipped by the line of another plane's cut lay along the outer ring's
/// clipped edge, and a ring the line crossed four times ran back along it past its own
/// corners, its edges crossing by rounding errors), the cap is cut again on a planar
/// arrangement ([`clip_exactly`]), which gives the region itself.
fn clip_cap(
    outer: Vec<DVec2>,
    holes: Vec<Vec<DVec2>>,
    others: &[HalfPlane],
    tol: f64,
) -> Vec<(Vec<DVec2>, Vec<Vec<DVec2>>)> {
    if others.is_empty() {
        return vec![(outer, holes)];
    }
    let mut o = outer.clone();
    let mut hs = holes.clone();
    for h in others {
        o = clip(&o, h);
        hs = hs.iter().map(|r| clip(r, h)).collect();
    }
    // Judged tidied (a corner on the line comes out twice), returned as clipped: the cut
    // tidies all of a solid's rings together.
    let tidied = tidy_rings(
        std::iter::once(o.clone())
            .chain(hs.iter().cloned())
            .collect(),
    );
    if simple(&tidied) {
        return vec![(o, hs)];
    }
    let rings: Vec<Vec<DVec2>> = std::iter::once(outer).chain(holes).collect();
    assemble(clip_exactly(&rings, others, tol))
}

/// Whether `rings` (a cap's: an outer ring and its holes, or any rings) are simple and meet
/// only at corners: no ring passes a point twice, no edge crosses another, and no corner lies
/// on another ring's edge or on a non-adjacent edge of its own (exactly, by the robust
/// orientation). Rings of fewer than three points are left out (they are dropped anyway).
fn simple(rings: &[Vec<DVec2>]) -> bool {
    use crate::sketch::{orient, proper_cross};
    let rings: Vec<&Vec<DVec2>> = rings.iter().filter(|r| r.len() >= 3).collect();
    for r in &rings {
        let pts: BTreeSet<[u64; 2]> = r.iter().map(|p| bits(*p)).collect();
        if pts.len() != r.len() {
            return false;
        }
    }
    let edges: Vec<(usize, usize, DVec2, DVec2)> = rings
        .iter()
        .enumerate()
        .flat_map(|(k, r)| (0..r.len()).map(move |i| (k, i, r[i], r[(i + 1) % r.len()])))
        .collect();
    for (x, &(k1, i1, a, b)) in edges.iter().enumerate() {
        for &(k2, i2, p, q) in &edges[x + 1..] {
            let n = rings[k1].len();
            let next = k1 == k2 && ((i1 + 1) % n == i2 || (i2 + 1) % n == i1);
            if !next && proper_cross(a, b, p, q) {
                return false;
            }
        }
        // A corner lying exactly on this edge, other than its own two ends.
        for r in &rings {
            for &c in r.iter() {
                if c == a || c == b || orient(a, b, c) != 0.0 {
                    continue;
                }
                let t = (c - a).dot(b - a);
                if t > 0.0 && t < (b - a).length_squared() {
                    return false;
                }
            }
        }
    }
    true
}

/// `rings` (outer rings counter-clockwise, holes clockwise: material on their left) clipped
/// by `others`, exactly: the rings laid on a planar arrangement with their votes, each other
/// plane's line across them laid with none, and the material faces kept where every
/// half-plane keeps them (a face lies wholly on one side of each line; its corner farthest
/// from the line says which). Simple rings with the material on their left, as the cut's own.
fn clip_exactly(rings: &[Vec<DVec2>], others: &[HalfPlane], tol: f64) -> Vec<Vec<DVec2>> {
    let mut arr = crate::arrangement::Arrangement::new(tol);
    let mut first = 0;
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for r in rings.iter().filter(|r| r.len() >= 3) {
        let ids: Vec<usize> = r.iter().map(|p| arr.vertex(*p)).collect();
        for i in 0..ids.len() {
            arr.insert_between(ids[i], ids[(i + 1) % ids.len()], 1, first);
            first += 1;
        }
        for p in r {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    if !lo.is_finite() {
        return Vec::new();
    }
    // Each line across the rings' box, a margin beyond it: every face it passes through is
    // split by it.
    let centre = (lo + hi) * 0.5;
    let reach = (hi - lo).length() + 1.0;
    for h in others {
        let g = DVec2::new(h.0[1], h.0[2]);
        let len2 = g.length_squared();
        if len2 < 1e-24 {
            continue; // parallel: it keeps everything or nothing, which the faces' test says
        }
        let foot = centre - g * (h.at(centre) / len2);
        if foot.distance(centre) > reach {
            continue; // misses the rings
        }
        let along = g.perp() / len2.sqrt();
        let (a, b) = (
            arr.vertex(foot - along * reach),
            arr.vertex(foot + along * reach),
        );
        arr.insert_between(a, b, 0, first);
        first += 1;
    }
    arr.settle();
    arr.material_rings_where(|face| {
        others.iter().all(|h| {
            face.iter()
                .map(|p| h.at(*p))
                .max_by(|x, y| x.abs().total_cmp(&y.abs()))
                .is_none_or(|d| d >= 0.0)
        })
    })
    .into_iter()
    .filter(|r| r.len() >= 3 && sawhorse_tessellate::signed_area(r).abs() > RING_FLOOR)
    .collect()
}

/// What `frame` keeps of solid `o` under the distances `dist` (one per vertex; [`distances`]
/// gives the tie rule's), as a closed surface: the kept part of every face (its kept
/// triangles merged back into rings, [`clip_solid`]) plus the cap facing `-normal`, laid from
/// the chords those rings leave open, so the two close each other under any distances.
fn kept_under(o: &Object, frame: &Plane, dist: &Distances) -> KeptPart {
    let b = o.brep();
    let tol = ARRANGE_TOL * solid_scale(o);
    let cut = clip_solid(b, frame, dist, tol);
    let mut part = KeptPart::default();
    for (fk, rings) in &cut.faces {
        let fp = &b.faces[*fk].plane;
        let mut area = 0.0;
        for r in rings {
            let ring: Vec<DVec3> = r.iter().map(|n| node_point(b, dist, *n)).collect();
            let flat: Vec<DVec2> = ring.iter().map(|p| fp.to_local(*p)).collect();
            area += sawhorse_tessellate::signed_area(&flat);
            part.newell(&ring);
        }
        part.add(fp.normal, fp.origin, area);
    }
    let rings = cap_rings(b, frame, dist, &cut.chords, tol);
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
    part.add(-frame.normal, frame.origin, cap);
    part.vector_area -= frame.normal * cap;
    part
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
            let tol = ARRANGE_TOL * scale;
            let clipped: Vec<(Vec<DVec2>, Vec<Vec<DVec2>>)> = assemble(solid_rings(obj, &frame))
                .into_iter()
                .flat_map(|(outer, holes)| clip_cap(outer, holes, &others, tol))
                .collect();
            // Lane 18: every cap exactly as the plane cuts it, never thinned out for the
            // slice (the halves, the fill and the cross-section read these rings); only what
            // other planes clip away to nothing goes. Whether the slice can draw it is the
            // cap's flag (`representable`). Lane 19c: the rings of all the solid's caps are
            // tidied together, so a corner where two of them touch stays in both.
            let counts: Vec<usize> = clipped.iter().map(|(_, hs)| hs.len()).collect();
            let mut tidied = tidy_rings(
                clipped
                    .into_iter()
                    .flat_map(|(outer, holes)| std::iter::once(outer).chain(holes))
                    .collect(),
            )
            .into_iter();
            for n in counts {
                let outer = tidied.next().expect("every cap has its outer ring");
                let holes: Vec<Vec<DVec2>> =
                    tidied.by_ref().take(n).filter(|r| r.len() >= 3).collect();
                if outer.len() < 3 {
                    continue;
                }
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
            // One distance per vertex, the same as the cap's (`solid_rings`, both through
            // `distances`): a face lying on the plane is kept whole by both halves, and the
            // half whose material is not behind it has the face itself as its cap there, so
            // the two halves always add up to the whole.
            part.absorb(kept_under(obj, &frame, &distances(obj.brep(), &frame)));
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

    /// Lane 19c: a cap clipped by another plane's line comes out as simple rings. Convex, the
    /// ring-by-ring clip stands as it always did. A hole the line passes through opens into a
    /// notch of one ring (clipped ring by ring, the hole's clipped edge lay along the outer
    /// ring's, the fuzzer's seed 20267302); a U the line crosses four times comes out as its
    /// two arms (ring by ring, it ran back along the line, seed 20267307).
    #[test]
    fn another_plane_clips_a_cap_into_simple_rings() {
        let v = DVec2::new;
        let area = |caps: &[(Vec<DVec2>, Vec<Vec<DVec2>>)]| -> f64 {
            caps.iter()
                .map(|(o, hs)| {
                    sawhorse_tessellate::signed_area(o)
                        + hs.iter()
                            .map(|h| sawhorse_tessellate::signed_area(h))
                            .sum::<f64>()
                })
                .sum()
        };
        // y >= 1
        let above = [HalfPlane([-1.0, 0.0, 1.0])];
        let caps = clip_cap(square(0.0, 0.0, 4.0, 3.0), vec![], &above, 1e-12);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].0.len(), 4);
        assert!((area(&caps) - 8.0).abs() < 1e-12);
        // A hole from y = 0.5 to 2: opened by the line into a notch.
        let mut hole = square(1.0, 0.5, 2.0, 2.0);
        hole.reverse();
        let caps = clip_cap(square(0.0, 0.0, 4.0, 3.0), vec![hole], &above, 1e-12);
        assert_eq!(caps.len(), 1, "{caps:?}");
        assert!(caps[0].1.is_empty(), "{caps:?}");
        assert_eq!(caps[0].0.len(), 8, "{caps:?}");
        assert!((area(&caps) - (8.0 - 1.0)).abs() < 1e-12);
        assert!(simple(std::slice::from_ref(&caps[0].0)));
        // A U (arms x 0..1 and 3..4, joined below y = 1): the line y = 2 keeps the arms' tops.
        let u = vec![
            v(0.0, 0.0),
            v(4.0, 0.0),
            v(4.0, 3.0),
            v(3.0, 3.0),
            v(3.0, 1.0),
            v(1.0, 1.0),
            v(1.0, 3.0),
            v(0.0, 3.0),
        ];
        let line = [HalfPlane([-2.0, 0.0, 1.0])];
        let caps = clip_cap(u, vec![], &line, 1e-12);
        assert_eq!(caps.len(), 2, "{caps:?}");
        assert!((area(&caps) - 2.0).abs() < 1e-12);
        for (o, hs) in &caps {
            assert!(hs.is_empty());
            assert!(simple(std::slice::from_ref(o)), "{o:?}");
        }
    }

    /// Lane 19c: a hole that touches its outer ring at a point of the outer ring's straight
    /// edge keeps that corner in both rings (tidied alone, the outer ring would drop it as a
    /// point in line with its neighbours, and the hole's edges would then run past the outer
    /// ring's edge by a rounding error); a corner in line on one ring only still goes.
    #[test]
    fn touching_rings_keep_the_corner_they_share() {
        let v = DVec2::new;
        let outer = vec![
            v(0.0, 0.0),
            v(2.0, 0.0),
            v(2.0, 2.0),
            v(1.0, 2.0),
            v(0.0, 2.0),
        ];
        let hole = vec![v(1.0, 2.0), v(1.5, 1.0), v(0.5, 1.0)];
        let lone = vec![
            v(5.0, 0.0),
            v(6.0, 0.0),
            v(7.0, 0.0),
            v(7.0, 1.0),
            v(5.0, 1.0),
        ];
        let t = tidy_rings(vec![outer.clone(), hole.clone(), lone]);
        assert_eq!(t[0], outer);
        assert_eq!(t[1], hole);
        assert_eq!(
            t[2],
            vec![v(5.0, 0.0), v(7.0, 0.0), v(7.0, 1.0), v(5.0, 1.0)]
        );
        assert_eq!(tidy_ring(outer).len(), 4);
    }

    fn a_box() -> Object {
        Object::extrude(
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
        .unwrap()
    }

    #[test]
    fn a_box_cut_through_its_middle() {
        let o = a_box();
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

    /// A frame: a 3 x 3 square with a hole from (1, 1.5) to (2, 2.5), pulled up 1.
    fn a_frame() -> Object {
        let mut hole = square(1.0, 1.5, 2.0, 2.5);
        hole.reverse();
        Object::extrude(&Plane::GROUND, &square(0.0, 0.0, 3.0, 3.0), &[hole], 1.0).unwrap()
    }

    /// CX2 in miniature: a frame cut by a plane through the top edge of its hole. One side's
    /// top face crosses the plane in a single piece straight over the hole while the hole's
    /// wall lays its edge along it the other way; both sides get the same notched cap (it
    /// used to be a plain rectangle on one side). Since lane 19c the hole's top edge, with both
    /// ends on the plane and kept by the triangles on both sides of it, closes itself, and the
    /// top face's runs end at the hole's corners.
    #[test]
    fn a_frame_cut_along_its_hole_edge_keeps_the_notch() {
        let o = a_frame();
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

    /// Lane 19c: one sign per vertex. Every distance is the vertex's own, zero within ON_FACE
    /// and nothing else: a corner 0.9 nm off the plane is on it, one 2 nm off is not, whatever
    /// face or surface it is a corner of.
    #[test]
    fn one_sign_per_vertex() {
        let o = a_box();
        let b = o.brep();
        for (h, on) in [(0.0, true), (0.9e-9, true), (-0.9e-9, true), (2e-9, false)] {
            let plane = Plane::new(DVec3::new(0.0, 0.0, 1.0 + h), DVec3::Z).unwrap();
            let dist = distances(b, &plane);
            for (v, x) in b.vertices() {
                let raw = plane.signed_distance(x.pos);
                let want = if raw.abs() <= ON_FACE { 0.0 } else { raw };
                assert_eq!(dist[v], want);
                if x.pos.z == 1.0 {
                    assert_eq!(dist[v] == 0.0, on, "{h}");
                }
            }
        }
    }

    /// A repro of `tests/repro` played to its end, with its plane snapping.
    fn replay(json: &str) -> crate::document::Document {
        let r: crate::testing::Repro = serde_json::from_str(json).unwrap();
        let was = crate::snap::plane_snapping();
        crate::snap::set_plane_snapping(r.plane_snap.unwrap_or(true));
        let mut d = crate::document::Document::new();
        for op in &r.ops {
            let _ = d.apply(op);
        }
        crate::snap::set_plane_snapping(was);
        d
    }

    /// Hostile solids and planes through them: the lane-19b verifier's find (a box top poked
    /// beyond its bend under two n-gons, both snappings), the lane-19 verifier's (autofold
    /// facets round a poked corner), the lane-18 verifier's (a tilted face drawn on), folded
    /// boxes cut through two vertices, and a box and a frame; each with its own planes, both
    /// ways round, and planes through its middle at odd angles.
    fn hostile() -> Vec<(String, Object, Vec<Plane>)> {
        let repros = [
            (
                "top face poked beyond its bend, snapping off",
                include_str!(
                    "../tests/repro/2026-10-14-section-top-face-poked-beyond-its-bend-snap-off.json"
                ),
            ),
            (
                "top face poked beyond its bend, snapping on",
                include_str!(
                    "../tests/repro/2026-10-14-section-top-face-poked-beyond-its-bend-snap-on.json"
                ),
            ),
            (
                "poked corner facets",
                include_str!("../tests/repro/2026-10-14-section-poked-corner-facets.json"),
            ),
            (
                "tie rule on an imprinted tilted face",
                include_str!(
                    "../tests/repro/2026-10-13-section-tie-rule-on-an-imprinted-tilted-face.json"
                ),
            ),
            (
                "pinch at two vertices",
                include_str!("../tests/repro/2026-10-12-section-pinch-at-two-vertices.json"),
            ),
            (
                "pinch: a face on the plane meets a fold",
                include_str!(
                    "../tests/repro/2026-10-12-section-pinch-face-on-plane-meets-fold.json"
                ),
            ),
        ];
        let mut out = Vec::new();
        let odd = [
            DVec3::new(0.3, -0.2, 1.0),
            DVec3::new(1.0, 0.7, 0.1),
            DVec3::new(-0.4, 1.0, 0.25),
        ];
        let middle = |o: &Object| {
            let b = o.brep();
            let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
            for (_, v) in b.vertices() {
                lo = lo.min(v.pos);
                hi = hi.max(v.pos);
            }
            (lo + hi) * 0.5
        };
        for (name, json) in repros {
            let d = replay(json);
            let m = d.model();
            for (k, (_, o)) in m.objects().into_iter().enumerate() {
                let mut planes = Vec::new();
                for (_, s) in m.sections() {
                    planes.push(Plane::new(s.origin, s.normal).unwrap());
                    planes.push(Plane::new(s.origin, -s.normal).unwrap());
                }
                for n in odd {
                    planes.push(Plane::new(middle(o), n.normalize()).unwrap());
                }
                out.push((format!("{name}, solid {k}"), o.clone(), planes));
            }
        }
        for (name, o) in [("a box", a_box()), ("a frame", a_frame())] {
            let c = middle(&o);
            let mut planes = vec![
                Plane::new(
                    DVec3::new(0.0, 2.5, 1.0),
                    DVec3::new(0.0, 1.0, -2.0).normalize(),
                )
                .unwrap(),
                Plane::new(DVec3::new(1.0, 1.5, 0.5), DVec3::X).unwrap(),
                Plane::new(DVec3::ZERO, -DVec3::Z).unwrap(),
            ];
            planes.extend(odd.map(|n| Plane::new(c, n.normalize()).unwrap()));
            out.push((name.to_string(), o, planes));
        }
        out
    }

    /// A small deterministic generator (xorshift64*), for distances nobody chose.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// The distances to try on a solid: the tie rule's; every corner within `h` of the plane
    /// forced onto it, for `h` from a nanometre to five micrometres; and random ones, any
    /// size and either sign, a third of them exactly zero, which no geometry gives.
    fn tries(b: &Brep, plane: &Plane, seed: u64) -> Vec<(String, Distances, bool)> {
        let tie = distances(b, plane);
        let mut out = vec![("the tie rule".to_string(), tie.clone(), true)];
        for h in [1e-8, 1e-7, 2e-6, 5e-6] {
            let mut forced = tie.clone();
            for (v, x) in forced.iter_mut() {
                if plane.signed_distance(b.vertices[v].pos).abs() <= h {
                    *x = 0.0;
                }
            }
            out.push((format!("forced within {h}"), forced, true));
        }
        let mut rng = Rng(seed | 1);
        for k in 0..3 {
            let mut any = tie.clone();
            for (_, x) in any.iter_mut() {
                let r = rng.next();
                *x = if r < 0.33 {
                    0.0
                } else {
                    (rng.next() - 0.5) * 10f64.powf(-9.0 + 9.0 * rng.next())
                };
            }
            out.push((format!("random {k}"), any, false));
        }
        out
    }

    /// The directed segments that `rings` leave open: each ring walked in its own turn, and
    /// two segments that run opposite ways between the same nodes closing each other. Sorted.
    fn left_open(rings: impl IntoIterator<Item = Vec<Node>>) -> Vec<(Node, Node)> {
        let mut count: BTreeMap<(Node, Node), u32> = BTreeMap::new();
        for r in rings {
            for i in 0..r.len() {
                let (a, e) = (r[i], r[(i + 1) % r.len()]);
                if a == e {
                    continue;
                }
                match count.get_mut(&(e, a)) {
                    Some(c) if *c > 0 => *c -= 1,
                    _ => *count.entry((a, e)).or_default() += 1,
                }
            }
        }
        count
            .into_iter()
            .flat_map(|(k, c)| std::iter::repeat_n(k, c as usize))
            .collect()
    }

    /// **The proof, checked** (lane 19c, see [`clip_solid`]): the kept parts of a solid's
    /// triangles, walked edge by edge, leave open exactly the chords, one to one: every
    /// triangle's chord where the plane splits it, and every edge with both ends on the plane
    /// that one of its triangles keeps and the other does not. Then the same for the faces:
    /// the kept triangles merged back into face rings (runs straightened) leave open exactly
    /// the chords the cap is laid from. Under the tie rule's distances, under every corner
    /// near the plane forced onto it, and under random distances no geometry gives.
    #[test]
    fn the_kept_triangles_leave_open_exactly_the_chords() {
        let mut checked = 0;
        for (name, o, planes) in hostile() {
            let b = o.brep();
            for (k, plane) in planes.iter().enumerate() {
                for (what, dist, _) in tries(b, plane, 0x5eed + 977 * k as u64 + checked) {
                    // Triangles: every kept part, every chord, and every edge with both ends
                    // on the plane that a kept triangle walks; two of those that run opposite
                    // ways (the edge kept by both its triangles) close each other.
                    let mut pieces = Vec::new();
                    let mut chords = Vec::new();
                    let mut on_plane: BTreeMap<(Node, Node), u32> = BTreeMap::new();
                    for (_, f) in b.faces() {
                        let loops: Vec<Vec<VertexId>> =
                            f.loops().map(|l| b.loop_vertices(l)).collect();
                        for t in face_triangles(b, f, &loops) {
                            let c = clip_triangle(t, &dist);
                            chords.extend(c.chord);
                            if c.chord.is_none() && !c.kept.is_empty() {
                                for i in 0..3 {
                                    let (p, q) = (t[i], t[(i + 1) % 3]);
                                    if dist[p] == 0.0 && dist[q] == 0.0 {
                                        let (a, e) = ((p, p), (q, q));
                                        match on_plane.get_mut(&(e, a)) {
                                            Some(c) if *c > 0 => *c -= 1,
                                            _ => *on_plane.entry((a, e)).or_default() += 1,
                                        }
                                    }
                                }
                            }
                            if !c.kept.is_empty() {
                                pieces.push(c.kept);
                            }
                        }
                    }
                    let mut want: Vec<(Node, Node)> = on_plane
                        .into_iter()
                        .flat_map(|(k, c)| std::iter::repeat_n(k, c as usize))
                        .chain(chords)
                        .collect();
                    want.sort();
                    let open = left_open(pieces);
                    assert_eq!(open, want, "{name}, plane {k}, {what}: triangles");
                    // Faces: the merged rings leave open exactly the chords the cap is laid from.
                    let cut = clip_solid(b, plane, &dist, ARRANGE_TOL * solid_scale(&o));
                    let open = left_open(cut.faces.into_iter().flat_map(|(_, r)| r));
                    let mut chords = cut.chords.clone();
                    chords.sort();
                    assert_eq!(open, chords, "{name}, plane {k}, {what}: faces");
                    checked += 1;
                }
            }
        }
        assert!(checked > 300, "{checked}");
    }

    /// Lane 19c: the two halves of a plane share every point of the cut. A plane and its flip
    /// give every vertex the same distance with its sign turned (exactly: negating the normal
    /// negates every term of the dot product), so the same vertices are on the plane; every
    /// crossing is worked out from its segment's two vertices alone, so it is the same point,
    /// to the bit, for the half kept and the half cut away; and the crossings on the solid's own
    /// edges, where the caps' corners are, are the ends of both halves' chords, the same nodes.
    #[test]
    fn both_halves_share_every_point_of_the_cut() {
        let mut crossings = 0;
        for (name, o, planes) in hostile() {
            let b = o.brep();
            let tol = ARRANGE_TOL * solid_scale(&o);
            for (k, plane) in planes.iter().enumerate() {
                let flip = Plane::new(plane.origin, -plane.normal).unwrap();
                let (kept, away) = (distances(b, plane), distances(b, &flip));
                for (v, _) in b.vertices() {
                    assert_eq!(away[v], -kept[v], "{name}, plane {k}: vertex distances");
                }
                // The crossings on the solid's own edges.
                let want: BTreeSet<Node> = b
                    .edges()
                    .filter_map(|(e, _)| {
                        let he = b.edges[e].he;
                        let (p, q) = (b.half_edges[he].origin, b.dest(he));
                        (kept[p] * kept[q] < 0.0).then(|| edge_node(p, q))
                    })
                    .collect();
                let ends = |frame: &Plane, dist: &Distances| -> BTreeSet<Node> {
                    clip_solid(b, frame, dist, tol)
                        .chords
                        .iter()
                        .flat_map(|&(a, c)| [a, c])
                        .filter(|n| want.contains(n))
                        .collect()
                };
                assert_eq!(
                    ends(plane, &kept),
                    want,
                    "{name}, plane {k}: the kept half's corners"
                );
                assert_eq!(
                    ends(&flip, &away),
                    want,
                    "{name}, plane {k}: the cut-away half's corners"
                );
                for (a, c) in clip_solid(b, plane, &kept, tol)
                    .chords
                    .into_iter()
                    .chain(clip_solid(b, &flip, &away, tol).chords)
                {
                    for n in [a, c] {
                        let (x, y) = (node_point(b, &kept, n), node_point(b, &away, n));
                        assert_eq!(
                            x.to_array().map(f64::to_bits),
                            y.to_array().map(f64::to_bits),
                            "{name}, plane {k}: node {n:?}"
                        );
                        crossings += usize::from(n.0 != n.1);
                    }
                }
            }
        }
        assert!(crossings > 1000, "{crossings}");
    }

    /// Lane 19c: the half a plane keeps, laid as faces plus the cap from their chords, is
    /// closed: along the normal to rounding, across it to the farthest a corner sits off the
    /// plane at the distance it was given times the seam. Under the tie rule's distances and
    /// every corner near the plane forced onto it (the distances a geometry can give; random
    /// ones close at the chords, above, but the cap of a cycle that winds twice is the union,
    /// not the sum), on every hostile solid, both ways round.
    #[test]
    fn the_cap_closes_the_half_under_any_distances() {
        let mut tried = 0;
        for (name, o, planes) in hostile() {
            let b = o.brep();
            for (k, plane) in planes.iter().enumerate() {
                for (what, dist, geometric) in tries(b, plane, 7 + k as u64) {
                    if !geometric {
                        continue;
                    }
                    let part = kept_under(&o, plane, &dist);
                    let off = b
                        .vertices()
                        .map(|(v, p)| (plane.signed_distance(p.pos) - dist[v]).abs())
                        .fold(0.0, f64::max);
                    let along = part.vector_area.dot(plane.normal).abs();
                    assert!(
                        along < 1e-12 * (1.0 + part.area),
                        "{name}, plane {k}, {what}: {part:?}"
                    );
                    let across = part.vector_area.length();
                    assert!(
                        across <= 2.0 * off * part.cap_length + 1e-12 * (1.0 + part.area),
                        "{name}, plane {k}, {what}: {part:?} (corners up to {off} off)"
                    );
                    tried += 1;
                }
            }
        }
        assert!(tried > 200, "{tried}");
    }

    /// Lane 19c: a planar face cut by the plane gives one chord, from the crossing on one of
    /// its edges to the crossing on another, however many of its triangles' diagonals the
    /// plane crosses between them: the cap's corners are the crossings on the solid's own
    /// edges, as before the cut was made of triangles (so every golden hashes as before). A
    /// box cut by a plane through its middle at an angle: six crossings, six corners; a
    /// 24-sided prism (every face a fan of diagonals, its ends 22 triangles each): one corner
    /// for every side the plane crosses.
    #[test]
    fn a_planar_face_cut_through_gives_one_chord() {
        let o = a_box();
        let plane = Plane::new(
            DVec3::new(1.0, 0.5, 0.5),
            DVec3::new(0.3, 0.6, 0.75).normalize(),
        )
        .unwrap();
        let rings = solid_rings(&o, &plane);
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].len(), 6, "{rings:?}");
        let circle: Vec<DVec2> = (0..24)
            .map(|k| {
                let t = std::f64::consts::TAU * k as f64 / 24.0;
                DVec2::new(t.cos(), t.sin())
            })
            .collect();
        let prism = Object::extrude(&Plane::GROUND, &circle, &[], 2.0).unwrap();
        // Through the axis, tilted: it crosses both ends and the sides between.
        let plane = Plane::new(
            DVec3::new(0.0, 0.0, 1.0),
            DVec3::new(1.0, 0.2, 0.3).normalize(),
        )
        .unwrap();
        let rings = solid_rings(&prism, &plane);
        assert_eq!(rings.len(), 1);
        let dist = distances(prism.brep(), &plane);
        let crossed = prism
            .brep()
            .edges()
            .filter(|(e, _)| {
                let he = prism.brep().edges[*e].he;
                let (p, q) = (prism.brep().half_edges[he].origin, prism.brep().dest(he));
                (dist[p] < 0.0) != (dist[q] < 0.0)
            })
            .count();
        assert_eq!(rings[0].len(), crossed, "{rings:?}");
    }
}
