//! Objects from imported polygon soups: STL, OBJ, glTF, 3MF, DXF `3DFACE`, `.skp` faces.
//!
//! This is the kernel's build path for imports. An [`ImportItem`] is welded, cleaned,
//! split into edge-connected shells and oriented; a shell's triangles may be merged into
//! coplanar n-gons; then the half-edge B-rep is built and validated exactly like any edited
//! solid ([`Object::validate`]: topology, watertight, no self-intersection, positive
//! volume). A shell that passes becomes an [`Object`].
//!
//! A shell that does not pass (open edges, edges shared by three or more faces,
//! inconsistent orientation, a self-intersection, no volume) is **not repaired**: it is
//! kept as a visible, non-solid [`LooseMesh`] whose note says why. The only changes ever
//! made to the input are the ones the item's notes list: welding coincident vertices
//! (within [`POINT_MERGE`]), dropping degenerate and exactly repeated faces, turning a
//! closed shell that is inside out the right way round, turning reversed faces of a closed
//! shell to match their neighbours, merging coplanar triangles, and separating closed
//! solids that touch along an edge or a face (each face pairs with the face across its own
//! material, going round the shared edge).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use glam::{DVec2, DVec3};
use sawhorse_tessellate::{signed_area, triangulate};

use crate::boolean::{TriSolid, repolygonise};
use crate::brep::{Brep, PolyFace, Polys};
use crate::error::DocumentError;
use crate::object::Object;
use crate::plane::Plane;
use crate::tolerance::{AREA_EPS, MAX_COORD, PLANARITY, POINT_MERGE};

/// Largest import the kernel accepts in one item (points or faces).
pub const MAX_IMPORT_ELEMENTS: usize = 4_000_000;

/// One imported piece: a polygon soup in metres, Z up, plus what to call it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct ImportItem {
    /// Display name of the objects it becomes.
    pub name: String,
    /// Put the objects it becomes in the group of this name (created by the import).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// A material defined by the same import (or already in the model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    pub positions: Vec<[f64; 3]>,
    /// Faces as rings of indices into `positions`: the outer ring, then any holes.
    pub faces: Vec<Vec<Vec<u32>>>,
    /// Edges with no face (index pairs). Shown with the mesh; never part of a solid.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<[u32; 2]>,
    /// Merge adjacent coplanar faces into one n-gon (right for triangle soups such as
    /// STL; wrong for `.skp` faces, whose dividing edges were drawn on purpose).
    #[serde(default)]
    pub merge_coplanar: bool,
    /// With `merge_coplanar`: a label per face; only faces with the same label merge (a
    /// GLB written by Sawhorse carries each triangle's face id, so drawn edges between
    /// coplanar faces survive the round trip). Empty: any coplanar neighbours merge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_labels: Vec<u32>,
    /// Lane 10: the material of each face (`None`: the item's `material`), for files that
    /// paint faces one by one (`.skp`). Empty: every face shows the item's material.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_materials: Vec<Option<String>>,
    /// Lane 10: how a texture lies on each face, as two affine rows in world metres (u and v
    /// in repeats: `u = a[0..3] . p + a[3]`), for files that position textures per face.
    /// The kernel keeps the rotation and scale and drops any skew.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_uvs: Vec<Option<[[f64; 4]; 2]>>,
    /// Lane 15: soft, smooth and hidden edges as (index, index, flags) with flags from
    /// [`crate::edges`] (1 soft, 2 smooth, 4 hidden), for files that store them (`.skp`).
    /// They land on the solids and meshes the item becomes, matched by end points.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_flags: Vec<[u32; 3]>,
}

/// Imported geometry that is not a valid solid: drawn, listed, exported to mesh formats,
/// never pushed or pulled. `note` says why it is not a solid.
///
/// Lane 16: a shared, copy-on-write handle to its [`LooseMeshData`], so copies of the
/// model (each edit, the undo ring) share a mesh's points until the mesh changes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LooseMesh(std::sync::Arc<LooseMeshData>);

impl std::ops::Deref for LooseMesh {
    type Target = LooseMeshData;
    fn deref(&self) -> &LooseMeshData {
        &self.0
    }
}

impl std::ops::DerefMut for LooseMesh {
    fn deref_mut(&mut self) -> &mut LooseMeshData {
        std::sync::Arc::make_mut(&mut self.0)
    }
}

impl From<LooseMeshData> for LooseMesh {
    fn from(d: LooseMeshData) -> LooseMesh {
        LooseMesh(std::sync::Arc::new(d))
    }
}

impl LooseMesh {
    /// Whether two handles share one mesh's data.
    pub fn shares_with(&self, other: &LooseMesh) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }

    /// Bytes its data takes (estimated): what the undo history is charged for it.
    pub fn approx_bytes(&self) -> usize {
        256 + self.name.len()
            + self.note.len()
            + self.positions.len() * 24
            + self
                .faces
                .iter()
                .map(|f| 24 + f.iter().map(|r| 24 + r.len() * 4).sum::<usize>())
                .sum::<usize>()
            + self.lines.len() * 8
            + self.edge_flags.len() * 48
    }
}

/// What a [`LooseMesh`] holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LooseMeshData {
    pub name: String,
    pub positions: Vec<DVec3>,
    pub faces: Vec<Vec<Vec<u32>>>,
    pub lines: Vec<[u32; 2]>,
    pub tag: Option<String>,
    pub material: Option<String>,
    pub note: String,
    /// Its group or component instance, slot and hidden / locked flags.
    pub member: crate::nodes::Membership,
    /// Soft, smooth and hidden edges (lane 15), by vertex pair (smaller index first).
    pub edge_flags: std::collections::BTreeMap<(u32, u32), u8>,
}

impl LooseMesh {
    /// Plane of face `i` (best fit through its outer ring), if it has one.
    pub fn plane(&self, i: usize) -> Option<Plane> {
        let pts: Vec<DVec3> = self.faces[i][0]
            .iter()
            .map(|&k| self.positions[k as usize])
            .collect();
        Plane::fit(&pts).ok()
    }

    /// Triangles of every face that triangulates, and the face index of each.
    pub fn triangles(&self) -> (Vec<[u32; 3]>, Vec<u32>) {
        let mut tris = Vec::new();
        let mut owner = Vec::new();
        for (fi, f) in self.faces.iter().enumerate() {
            let Some(plane) = self.plane(fi) else {
                continue;
            };
            let flat: Vec<u32> = f.iter().flatten().copied().collect();
            let ring = |r: &Vec<u32>| -> Vec<DVec2> {
                r.iter()
                    .map(|&k| plane.to_local(self.positions[k as usize]))
                    .collect()
            };
            let holes: Vec<Vec<DVec2>> = f[1..].iter().map(ring).collect();
            if let Ok(ts) = triangulate(&ring(&f[0]), &holes) {
                for t in ts {
                    tris.push(t.map(|k| flat[k as usize]));
                    owner.push(fi as u32);
                }
            }
        }
        (tris, owner)
    }

    /// Basic sanity (indices in range, every face has an outer ring); the model validator
    /// calls this.
    pub fn check(&self) -> Result<(), &'static str> {
        let n = self.positions.len() as u32;
        if self
            .faces
            .iter()
            .flatten()
            .flatten()
            .chain(self.lines.iter().flatten())
            .any(|&i| i >= n)
        {
            return Err("loose mesh index out of range");
        }
        if self.faces.iter().any(|f| f.is_empty() || f[0].len() < 3) {
            return Err("loose mesh face without an outer ring");
        }
        Ok(())
    }
}

/// What one item became.
#[derive(Debug, Clone, Default)]
pub struct Built {
    pub solids: Vec<Object>,
    /// Per solid, per face (in face order): the item's face it came from, when the solid kept
    /// the item's faces as they were (`None` where faces were merged or rebuilt).
    pub face_sources: Vec<Vec<Option<usize>>>,
    pub loose: Option<LooseMesh>,
    /// Plain-language notes: what was healed, what stayed a mesh and why.
    pub notes: Vec<String>,
}

fn bad(msg: impl Into<String>) -> DocumentError {
    DocumentError::BadImport(msg.into())
}

/// Weld points closer than [`POINT_MERGE`] (the first occurrence wins). Returns the welded
/// points and the old -> new index map.
fn weld(points: &[DVec3]) -> (Vec<DVec3>, Vec<u32>) {
    weld_within(points, POINT_MERGE)
}

/// Weld points closer than `gap` (the first occurrence wins).
fn weld_within(points: &[DVec3], gap: f64) -> (Vec<DVec3>, Vec<u32>) {
    let cell = gap;
    let key = |p: DVec3| {
        (
            (p.x / cell).floor() as i64,
            (p.y / cell).floor() as i64,
            (p.z / cell).floor() as i64,
        )
    };
    // Only looked up, never iterated, so the output does not depend on hash order.
    let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    let mut out: Vec<DVec3> = Vec::new();
    let mut map = Vec::with_capacity(points.len());
    for &p in points {
        let (x, y, z) = key(p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&(x + dx, y + dy, z + dz)) {
                        for &q in list {
                            if out[q as usize].distance(p) <= gap {
                                found = Some(q);
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        let i = match found {
            Some(q) => q,
            None => {
                let q = out.len() as u32;
                out.push(p);
                grid.entry((x, y, z)).or_default().push(q);
                q
            }
        };
        map.push(i);
    }
    (out, map)
}

/// Map a ring through the weld and drop repeated consecutive points (wrapping).
fn clean_ring(ring: &[u32], map: &[u32]) -> Vec<u32> {
    let mut r: Vec<u32> = Vec::with_capacity(ring.len());
    for &i in ring {
        let j = map[i as usize];
        if r.last() != Some(&j) {
            r.push(j);
        }
    }
    while r.len() > 1 && r.first() == r.last() {
        r.pop();
    }
    r
}

fn ring_points(ring: &[u32], pts: &[DVec3]) -> Vec<DVec3> {
    ring.iter().map(|&i| pts[i as usize]).collect()
}

/// Area of a ring in its own best-fit plane (0 when it has none).
fn ring_area(ring: &[u32], pts: &[DVec3]) -> f64 {
    let p = ring_points(ring, pts);
    match Plane::fit(&p) {
        Ok(plane) => {
            let q: Vec<DVec2> = p.iter().map(|&x| plane.to_local(x)).collect();
            signed_area(&q).abs()
        }
        Err(_) => 0.0,
    }
}

/// Canonical form of a face for duplicate detection: rings rotated to start at their
/// smallest index, holes sorted. Same orientation only.
fn face_key(f: &[Vec<u32>]) -> Vec<Vec<u32>> {
    let rot = |r: &Vec<u32>| -> Vec<u32> {
        let k = (0..r.len()).min_by_key(|&i| r[i]).unwrap_or(0);
        r[k..].iter().chain(r[..k].iter()).copied().collect()
    };
    let mut holes: Vec<Vec<u32>> = f[1..].iter().map(rot).collect();
    holes.sort();
    std::iter::once(rot(&f[0])).chain(holes).collect()
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Triangles of a set of faces (global indices) and the face each came from. `None` when a
/// face will not triangulate.
fn triangulate_faces(
    faces: &[&Vec<Vec<u32>>],
    pts: &[DVec3],
) -> Option<(Vec<[u32; 3]>, Vec<usize>)> {
    let mut tris = Vec::new();
    let mut from = Vec::new();
    for (fi, f) in faces.iter().enumerate() {
        let plane = Plane::fit(&ring_points(&f[0], pts)).ok()?;
        let flat: Vec<u32> = f.iter().flatten().copied().collect();
        let ring = |r: &Vec<u32>| -> Vec<DVec2> {
            r.iter().map(|&i| plane.to_local(pts[i as usize])).collect()
        };
        let holes: Vec<Vec<DVec2>> = f[1..].iter().map(ring).collect();
        for t in triangulate(&ring(&f[0]), &holes).ok()? {
            tris.push(t.map(|k| flat[k as usize]));
            from.push(fi);
        }
    }
    Some((tris, from))
}

/// At an edge `lo`-`hi` used by more than two faces, pair each face with the one across its
/// solid's material: faces sorted by angle round the edge, each paired with its neighbour
/// on the side its inward normal points to. `None` unless every face finds exactly one
/// partner that uses the edge the other way and picks it back.
fn radial_pairs(
    lo: u32,
    hi: u32,
    list: &[(usize, bool)],
    faces: &[Vec<Vec<u32>>],
    pts: &[DVec3],
) -> Option<Vec<(usize, usize)>> {
    let fwd = list.iter().filter(|u| u.1).count();
    if fwd * 2 != list.len()
        || list.iter().map(|u| u.0).collect::<BTreeSet<_>>().len() != list.len()
    {
        return None;
    }
    let d = (pts[hi as usize] - pts[lo as usize]).normalize_or_zero();
    if d == DVec3::ZERO {
        return None;
    }
    // Per use: angle of the face's interior direction round d, and which way its material is.
    let mut around: Vec<(f64, usize, bool, f64)> = Vec::with_capacity(list.len());
    let mut reference: Option<DVec3> = None;
    for &(fi, forward) in list {
        let ring: Vec<DVec3> = faces[fi][0].iter().map(|&v| pts[v as usize]).collect();
        let c = ring.iter().copied().sum::<DVec3>() / ring.len() as f64;
        let n = crate::plane::newell(&ring, c).normalize_or_zero();
        if n == DVec3::ZERO {
            return None;
        }
        let df = if forward { d } else { -d };
        let w = n.cross(df);
        let r = *reference.get_or_insert(w);
        // Angle in [0, 2 pi), quantised so coincident faces (two solids touching face to
        // face) tie exactly; within a tie, a face whose material lies behind comes first.
        let mut theta = d
            .dot(r.cross(w))
            .atan2(r.dot(w))
            .rem_euclid(std::f64::consts::TAU);
        if theta > std::f64::consts::TAU - 1e-9 {
            theta = 0.0;
        }
        let theta = (theta * 1e9).round();
        let side = d.dot(w.cross(-n));
        if side.abs() < 1e-12 {
            return None;
        }
        around.push((theta, fi, forward, side.signum()));
    }
    around.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.3.total_cmp(&b.3))
            .then(a.1.cmp(&b.1))
    });
    let k = around.len();
    let partner = |i: usize| -> usize {
        if around[i].3 > 0.0 {
            (i + 1) % k
        } else {
            (i + k - 1) % k
        }
    };
    let mut pairs = Vec::new();
    for i in 0..k {
        let j = partner(i);
        if partner(j) != i || around[i].2 == around[j].2 {
            return None;
        }
        if i < j {
            pairs.push((around[i].1, around[j].1));
        }
    }
    Some(pairs)
}

/// Which faces of a closed manifold shell to turn over so every edge is used once in each
/// direction (the first face keeps its way). `None` for a one-sided surface.
fn consistent_orientation(faces: &[Vec<Vec<u32>>]) -> Option<Vec<bool>> {
    let mut by_edge: BTreeMap<(u32, u32), Vec<(usize, bool)>> = BTreeMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for r in f {
            for k in 0..r.len() {
                let (a, b) = (r[k], r[(k + 1) % r.len()]);
                by_edge
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push((fi, a < b));
            }
        }
    }
    let mut turn: Vec<Option<bool>> = vec![None; faces.len()];
    for seed in 0..faces.len() {
        if turn[seed].is_some() {
            continue;
        }
        turn[seed] = Some(false);
        let mut stack = vec![seed];
        while let Some(f) = stack.pop() {
            let tf = turn[f]?;
            for r in &faces[f] {
                for k in 0..r.len() {
                    let (a, b) = (r[k], r[(k + 1) % r.len()]);
                    for &(g, fwd) in &by_edge[&(a.min(b), a.max(b))] {
                        if g == f {
                            continue;
                        }
                        // f uses the edge in direction (a < b) xor tf; g must use it the other way.
                        let f_dir = (a < b) != tf;
                        let need = fwd == f_dir; // same direction: g must turn
                        match turn[g] {
                            None => {
                                turn[g] = Some(need);
                                stack.push(g);
                            }
                            Some(t) if t != need => return None,
                            Some(_) => {}
                        }
                    }
                }
            }
        }
    }
    turn.into_iter().collect()
}

/// Gaps the cut list's heal closes (0.1 mm): far above drawing noise, far below anything a
/// woodworker draws on purpose.
pub const HEAL_GAP: f64 = 1e-4;
/// An isolated face smaller than this share of a mesh's whole area is a sliver.
pub const SLIVER_SHARE: f64 = 1e-3;

/// What the cut list's heal found on a non-solid mesh ([`heal_check`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HealCheck {
    /// After the heal, every edge has exactly two faces and the faces can all face one
    /// way: a closed shell (or several), sized like a solid.
    pub closed: bool,
    /// Faces left after the heal.
    pub faces: usize,
}

/// The heal the cut list tries on a mesh before it sizes the part from its bounds: weld
/// vertices closer than [`HEAL_GAP`] (close tiny gaps), drop faces with no area and exact
/// repeats, drop isolated slivers (faces sharing no edge with any other face and smaller
/// than [`SLIVER_SHARE`] of the whole), then ask whether what is left is closed and can be
/// turned to face one way (inconsistent faces flipped). Nothing in the model changes: the
/// answer only decides whether a part is flagged "not solid (sizes from bounds)".
pub fn heal_check(positions: &[DVec3], faces: &[Vec<Vec<u32>>]) -> HealCheck {
    let (pts, map) = weld_within(positions, HEAL_GAP);
    let mut seen: BTreeSet<Vec<Vec<u32>>> = BTreeSet::new();
    let mut kept: Vec<Vec<Vec<u32>>> = Vec::new();
    let mut areas: Vec<f64> = Vec::new();
    let mut pinched = false;
    for f in faces {
        let Some(first) = f.first() else { continue };
        let outer = clean_ring(first, &map);
        let area = if outer.len() >= 3 {
            ring_area(&outer, &pts)
        } else {
            0.0
        };
        if area <= AREA_EPS {
            continue;
        }
        let mut rings = vec![outer];
        for h in &f[1..] {
            let h = clean_ring(h, &map);
            if h.len() >= 3 && ring_area(&h, &pts) > AREA_EPS {
                rings.push(h);
            }
        }
        let all: Vec<u32> = rings.iter().flatten().copied().collect();
        if all.iter().copied().collect::<BTreeSet<u32>>().len() != all.len() {
            pinched = true;
        }
        if seen.insert(face_key(&rings)) {
            kept.push(rings);
            areas.push(area);
        }
    }
    let edge_uses = |faces: &[Vec<Vec<u32>>]| {
        let mut uses: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
        for (fi, f) in faces.iter().enumerate() {
            for r in f {
                for k in 0..r.len() {
                    let (a, b) = (r[k], r[(k + 1) % r.len()]);
                    uses.entry((a.min(b), a.max(b))).or_default().push(fi);
                }
            }
        }
        uses
    };
    // Isolated slivers: no edge shared with another face, and tiny next to the whole.
    let total: f64 = areas.iter().sum();
    let uses = edge_uses(&kept);
    let mut shared = vec![false; kept.len()];
    for list in uses.values() {
        if list.len() > 1 {
            for &f in list {
                shared[f] = true;
            }
        }
    }
    let mut i = 0;
    kept.retain(|_| {
        let keep = shared[i] || areas[i] >= SLIVER_SHARE * total;
        i += 1;
        keep
    });
    let faces_left = kept.len();
    let closed = !pinched
        && faces_left >= 4
        && edge_uses(&kept).values().all(|l| l.len() == 2)
        && consistent_orientation(&kept).is_some();
    HealCheck {
        closed,
        faces: faces_left,
    }
}

/// The faces of a mesh in edge-connected pieces (faces sharing an edge are one piece), each
/// piece's face indices in order; pieces in order of their first face.
pub fn connected_pieces(faces: &[Vec<Vec<u32>>]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    let mut first_use: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for r in f {
            for k in 0..r.len() {
                let (a, b) = (r[k], r[(k + 1) % r.len()]);
                match first_use.entry((a.min(b), a.max(b))) {
                    std::collections::btree_map::Entry::Vacant(e) => {
                        e.insert(fi);
                    }
                    std::collections::btree_map::Entry::Occupied(e) => {
                        let (x, y) = (find(&mut parent, *e.get()), find(&mut parent, fi));
                        if x != y {
                            parent[x.max(y)] = x.min(y);
                        }
                    }
                }
            }
        }
    }
    let mut pieces: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for fi in 0..faces.len() {
        let r = find(&mut parent, fi);
        pieces.entry(r).or_default().push(fi);
    }
    pieces.into_values().collect()
}

fn signed_volume(tris: &[[u32; 3]], pts: &[DVec3]) -> f64 {
    tris.iter()
        .map(|t| {
            let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

/// Group triangles into coplanar, edge-connected clusters: a label per triangle and a plane
/// per label. A neighbour joins when all its vertices lie within [`PLANARITY`] of the
/// cluster's plane and it faces the same way.
fn coplanar_clusters(
    tris: &[[u32; 3]],
    pts: &[DVec3],
    labels: Option<&[u32]>,
) -> (Vec<u32>, Vec<Plane>) {
    let mut by_edge: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (ti, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push(ti);
        }
    }
    let normal = |t: &[u32; 3]| {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        (b - a).cross(c - a).normalize_or_zero()
    };
    let mut label = vec![u32::MAX; tris.len()];
    let mut planes = Vec::new();
    for seed in 0..tris.len() {
        if label[seed] != u32::MAX {
            continue;
        }
        let n0 = normal(&tris[seed]);
        let plane = Plane::new(pts[tris[seed][0] as usize], n0).unwrap_or(Plane::GROUND);
        let id = planes.len() as u32;
        planes.push(plane);
        label[seed] = id;
        let mut stack = vec![seed];
        while let Some(t) = stack.pop() {
            for k in 0..3 {
                let (a, b) = (tris[t][k], tris[t][(k + 1) % 3]);
                for &u in &by_edge[&(a.min(b), a.max(b))] {
                    if label[u] != u32::MAX {
                        continue;
                    }
                    let on_plane = tris[u]
                        .iter()
                        .all(|&v| plane.signed_distance(pts[v as usize]).abs() <= PLANARITY);
                    let same = labels.is_none_or(|l| l[u] == l[seed]);
                    if same && on_plane && normal(&tris[u]).dot(n0) > 1.0 - 1e-6 {
                        label[u] = id;
                        stack.push(u);
                    }
                }
            }
        }
    }
    (label, planes)
}

/// Plain-data B-rep of one closed shell (local indices), polygons as given. Edges between
/// two coplanar faces are flagged user-drawn: a solid only carries such an edge when
/// someone drew it, and SketchUp keeps exactly those.
fn shell_polys(faces: &[Vec<Vec<u32>>], pts: Vec<DVec3>) -> Polys {
    let mut p = Polys {
        points: pts,
        faces: faces
            .iter()
            .map(|f| {
                PolyFace::new(
                    f[0].iter().map(|&i| i as usize).collect(),
                    f[1..]
                        .iter()
                        .map(|h| h.iter().map(|&i| i as usize).collect())
                        .collect(),
                )
            })
            .collect(),
        user_edges: BTreeSet::new(),
    };
    let planes: Vec<Option<Plane>> = (0..p.faces.len()).map(|i| p.plane(i)).collect();
    let dir = p.directed_edges();
    let mut user = BTreeSet::new();
    for (&(a, b), &(fa, _)) in &dir {
        if a > b {
            continue;
        }
        if let Some(&(fb, _)) = dir.get(&(b, a))
            && let (Some(pa), Some(pb)) = (planes[fa], planes[fb])
            && pa.normal.dot(pb.normal) > 1.0 - 1e-9
            && p.faces[fb]
                .outer
                .iter()
                .all(|&v| pa.signed_distance(p.points[v]).abs() <= PLANARITY)
        {
            user.insert(Polys::key(a, b));
        }
    }
    p.user_edges = user;
    p
}

/// Merge coplanar triangles of one closed shell into n-gons (through the boolean path's
/// re-polygoniser, which also heals vertices left running straight through an edge).
fn merged_polys(tris: &[[u32; 3]], pts: &[DVec3], tri_labels: Option<&[u32]>) -> Option<Polys> {
    let (labels, planes) = coplanar_clusters(tris, pts, tri_labels);
    // With labels, an edge between two differently labelled coplanar faces is a drawn
    // edge: keep it (the re-polygoniser merges coplanar neighbours across anything else).
    let mut preserved = Vec::new();
    if tri_labels.is_some() {
        let mut by_edge: BTreeMap<(u32, u32), usize> = BTreeMap::new();
        for (t, tri) in tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                if let Some(&s) = by_edge.get(&(b, a)) {
                    let (pa, pb) = (planes[labels[t] as usize], planes[labels[s] as usize]);
                    if labels[t] != labels[s]
                        && pa.normal.dot(pb.normal) > 1.0 - 1e-9
                        && pa.signed_distance(pb.origin).abs() <= PLANARITY
                    {
                        preserved.push((pts[a as usize], pts[b as usize]));
                    }
                } else {
                    by_edge.insert((a, b), t);
                }
            }
        }
    }
    let solid = TriSolid {
        positions: pts.to_vec(),
        triangles: tris.to_vec(),
        face_ids: labels,
    };
    let ids = vec![None; planes.len()];
    let mut p = repolygonise(&solid, &planes, &ids, &preserved).ok()?;
    for f in &mut p.faces {
        f.id = None;
    }
    Some(p)
}

/// A validated solid from plain data, faces numbered 1.. in face order.
fn as_solid(p: &Polys) -> Result<Object, String> {
    let mut p = p.clone();
    for (k, f) in p.faces.iter_mut().enumerate() {
        f.id = Some(k as u64 + 1);
    }
    let brep = Brep::build(&p).map_err(|e| e.to_string())?;
    let o = Object::from_brep(brep);
    o.validate().map_err(|e| e.to_string())?;
    Ok(o)
}

/// Build what an item becomes. Refuses (typed) only malformed data: out-of-range indices,
/// non-finite or out-of-bounds coordinates, an item over [`MAX_IMPORT_ELEMENTS`].
pub fn build_item(item: &ImportItem) -> Result<Built, DocumentError> {
    let mut built = build_item_geometry(item)?;
    if !item.edge_flags.is_empty() {
        apply_edge_flags(item, &mut built)?;
    }
    Ok(built)
}

/// Lane 15: put the item's soft, smooth and hidden edges onto what it became, matching each
/// flagged edge to an edge of a solid or of the mesh by its end points (welding moves a
/// point by at most 0.01 mm, so they are compared within twice that).
fn apply_edge_flags(item: &ImportItem, built: &mut Built) -> Result<(), DocumentError> {
    use crate::edges::{ALL, pair};
    let n = item.positions.len() as u32;
    let p = |i: u32| DVec3::from_array(item.positions[i as usize]);
    let mut want: Vec<(DVec3, DVec3, u8)> = Vec::new();
    for e in &item.edge_flags {
        if e[0] >= n || e[1] >= n {
            return Err(bad("an edge flag names a point that is not there"));
        }
        if e[2] > u32::from(ALL) {
            return Err(bad(
                "an edge flag is not 1 (soft), 2 (smooth), 4 (hidden) or a sum of them",
            ));
        }
        if e[2] != 0 && e[0] != e[1] {
            want.push((p(e[0]), p(e[1]), e[2] as u8));
        }
    }
    if want.is_empty() {
        return Ok(());
    }
    const CELL: f64 = 1e-4;
    let cell = |q: DVec3| {
        [
            (q.x / CELL).floor() as i64,
            (q.y / CELL).floor() as i64,
            (q.z / CELL).floor() as i64,
        ]
    };
    let mut grid: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
    for (k, w) in want.iter().enumerate() {
        grid.entry(cell(w.0)).or_default().push(k);
        grid.entry(cell(w.1)).or_default().push(k);
    }
    let tol = 2.0 * POINT_MERGE;
    let near = |a: DVec3, b: DVec3| a.distance(b) <= tol;
    let lookup = |a: DVec3, b: DVec3| -> u8 {
        let c = cell(a);
        let mut flags = 0;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(ks) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) {
                        for &k in ks {
                            let w = want[k];
                            if (near(w.0, a) && near(w.1, b)) || (near(w.0, b) && near(w.1, a)) {
                                flags |= w.2;
                            }
                        }
                    }
                }
            }
        }
        flags
    };
    for o in &mut built.solids {
        let edges: Vec<_> = o.brep.edges().map(|(e, _)| e).collect();
        for e in edges {
            let (a, b) = o.brep.edge_points(e);
            let f = lookup(a, b);
            if f != 0
                && let Some(key) = o.edge_key(e)
            {
                o.set_edge_flags(key, f, 0);
            }
        }
    }
    if let Some(m) = &mut built.loose {
        let mut keys: Vec<(u32, u32)> = m.edge_faces().into_keys().collect();
        keys.extend(m.lines.iter().map(|l| pair(l[0], l[1])));
        for (x, y) in keys {
            let f = lookup(m.positions[x as usize], m.positions[y as usize]);
            if f != 0 {
                m.set_edge_flags((x, y), f, 0);
            }
        }
    }
    Ok(())
}

fn build_item_geometry(item: &ImportItem) -> Result<Built, DocumentError> {
    let name = item.name.trim();
    let name = if name.is_empty() { "Imported" } else { name };
    if item.positions.len() > MAX_IMPORT_ELEMENTS || item.faces.len() > MAX_IMPORT_ELEMENTS {
        return Err(bad(format!(
            "{name}: more than {MAX_IMPORT_ELEMENTS} elements"
        )));
    }
    let n = item.positions.len() as u64;
    if item
        .faces
        .iter()
        .flatten()
        .flatten()
        .chain(item.lines.iter().flatten())
        .any(|&i| u64::from(i) >= n)
    {
        return Err(bad(format!(
            "{name}: a face or line names a missing vertex"
        )));
    }
    let raw: Vec<DVec3> = item
        .positions
        .iter()
        .map(|p| DVec3::from_array(*p))
        .collect();
    if raw
        .iter()
        .any(|p| !p.is_finite() || p.abs().max_element() > MAX_COORD)
    {
        return Err(bad(format!(
            "{name}: a coordinate is not finite or lies more than 10 km out"
        )));
    }
    let (pts, map) = weld(&raw);
    let mut notes = Vec::new();

    // Clean faces: weld, drop degenerate rings and exact repeats.
    let (mut degenerate, mut repeated) = (0usize, 0usize);
    let mut pinched_faces: Vec<Vec<Vec<u32>>> = Vec::new();
    let mut seen: BTreeSet<Vec<Vec<u32>>> = BTreeSet::new();
    let mut faces: Vec<Vec<Vec<u32>>> = Vec::new();
    let mut face_label: Vec<u32> = Vec::new();
    let mut face_src: Vec<usize> = Vec::new();
    let labelled = item.face_labels.len() == item.faces.len() && !item.faces.is_empty();
    if !item.face_labels.is_empty() && !labelled {
        return Err(bad(format!(
            "{name}: {} face labels for {} faces",
            item.face_labels.len(),
            item.faces.len()
        )));
    }
    for (src, f) in item.faces.iter().enumerate() {
        if f.is_empty() {
            degenerate += 1;
            continue;
        }
        let outer = clean_ring(&f[0], &map);
        if outer.len() < 3 || ring_area(&outer, &pts) <= AREA_EPS {
            degenerate += 1;
            continue;
        }
        let mut rings = vec![outer];
        for h in &f[1..] {
            let h = clean_ring(h, &map);
            if h.len() >= 3 && ring_area(&h, &pts) > AREA_EPS {
                rings.push(h);
            }
        }
        let all: Vec<u32> = rings.iter().flatten().copied().collect();
        let distinct: BTreeSet<u32> = all.iter().copied().collect();
        if distinct.len() != all.len() {
            pinched_faces.push(rings);
            continue;
        }
        if !seen.insert(face_key(&rings)) {
            repeated += 1;
            continue;
        }
        faces.push(rings);
        face_src.push(src);
        face_label.push(if labelled { item.face_labels[src] } else { 0 });
    }
    if degenerate > 0 {
        notes.push(format!(
            "{name}: dropped {degenerate} degenerate face{} (no area)",
            plural(degenerate)
        ));
    }
    if repeated > 0 {
        notes.push(format!(
            "{name}: dropped {repeated} exactly repeated face{}",
            plural(repeated)
        ));
    }

    // Edge uses and edge-connected shells.
    let mut uses: BTreeMap<(u32, u32), Vec<(usize, bool)>> = BTreeMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for r in f {
            for k in 0..r.len() {
                let (a, b) = (r[k], r[(k + 1) % r.len()]);
                uses.entry((a.min(b), a.max(b)))
                    .or_default()
                    .push((fi, a < b));
            }
        }
    }
    // Faces join a shell across each edge. Where solids touch along an edge (more than two
    // faces on it, as in an STL of parts that touch), each face joins only the face across
    // its own solid's material, found by going round the edge; if the uses do not pair up
    // that way, all of them join (and the shell is reported as non-manifold).
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    let join = |parent: &mut Vec<usize>, x: usize, y: usize| {
        let (a, b) = (find(parent, x), find(parent, y));
        if a != b {
            parent[a.max(b)] = a.min(b);
        }
    };
    let mut separated_edges = 0usize;
    for (&(lo, hi), list) in &uses {
        let pairs = if list.len() > 2 {
            radial_pairs(lo, hi, list, &faces, &pts)
        } else {
            None
        };
        match pairs {
            Some(pairs) => {
                separated_edges += 1;
                for (x, y) in pairs {
                    join(&mut parent, x, y);
                }
            }
            None => {
                for w in list.windows(2) {
                    join(&mut parent, w[0].0, w[1].0);
                }
            }
        }
    }
    let mut shells: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut root_of: Vec<usize> = Vec::with_capacity(faces.len());
    for fi in 0..faces.len() {
        let r = find(&mut parent, fi);
        root_of.push(r);
        shells.entry(r).or_default().push(fi);
    }
    // Each shell's open, over-shared and mis-oriented edges, in one pass over the edges
    // (lane 16: every shell used to scan every edge, quadratic in the shells: 20,000
    // disjoint triangles were a billion steps).
    let mut tally: BTreeMap<usize, (usize, usize, usize)> = BTreeMap::new();
    for list in uses.values() {
        // (shell, uses, first direction, second direction) for the few shells on this edge.
        let mut on: Vec<(usize, usize, bool, bool)> = Vec::with_capacity(2);
        for &(f, dir) in list {
            let r = root_of[f];
            match on.iter_mut().find(|x| x.0 == r) {
                Some(x) => {
                    if x.1 == 1 {
                        x.3 = dir;
                    }
                    x.1 += 1;
                }
                None => on.push((r, 1, dir, dir)),
            }
        }
        for (r, n, d0, d1) in on {
            let t = tally.entry(r).or_default();
            match n {
                1 => t.0 += 1,
                2 if d0 == d1 => t.2 += 1,
                2 => {}
                _ => t.1 += 1,
            }
        }
    }

    let mut solids = Vec::new();
    let mut face_sources: Vec<Vec<Option<usize>>> = Vec::new();
    let mut loose_faces: Vec<Vec<Vec<u32>>> = pinched_faces.clone();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    if !pinched_faces.is_empty() {
        *reasons
            .entry("faces that touch themselves at a vertex".into())
            .or_default() += pinched_faces.len();
    }
    let fail = |reasons: &mut BTreeMap<String, usize>, why: String| {
        *reasons.entry(why).or_default() += 1;
    };
    let (mut flipped, mut merged_from, mut rebuilt, mut turned) = (0usize, 0usize, 0usize, 0usize);
    for (root, members) in &shells {
        let (open, over, mut orient) = tally.get(root).copied().unwrap_or_default();
        let mut shell_faces: Vec<Vec<Vec<u32>>> =
            members.iter().map(|&fi| faces[fi].clone()).collect();
        // Lane 16: a solid is checked whole whenever it changes (self-intersection); a
        // shell with more faces than that check is built for stays a mesh.
        if members.len() > crate::limits::MAX_SOLID_FACES {
            fail(
                &mut reasons,
                format!(
                    "{} faces in one piece, more than the {} a solid may have",
                    members.len(),
                    crate::limits::MAX_SOLID_FACES
                ),
            );
            loose_faces.extend(shell_faces);
            continue;
        }
        let shell_labels: Vec<u32> = members.iter().map(|&fi| face_label[fi]).collect();
        // A closed manifold shell whose faces disagree about which way they face (common in
        // SketchUp models with reversed faces) has exactly one consistent orientation up to
        // turning the whole shell over: find it, and say how many faces were turned.
        if open == 0 && over == 0 && orient > 0 {
            match consistent_orientation(&shell_faces) {
                Some(turn) => {
                    let n = turn.iter().filter(|&&t| t).count();
                    for (f, t) in shell_faces.iter_mut().zip(&turn) {
                        if *t {
                            for r in f.iter_mut() {
                                r.reverse();
                            }
                        }
                    }
                    turned += n;
                    orient = 0;
                }
                None => {
                    fail(
                        &mut reasons,
                        "a one-sided surface (it cannot face one way)".into(),
                    );
                    loose_faces.extend(shell_faces);
                    continue;
                }
            }
        }
        if open + over + orient > 0 {
            let mut why = Vec::new();
            if open > 0 {
                why.push(format!("{open} open edge{}", plural(open)));
            }
            if over > 0 {
                why.push(format!(
                    "{over} edge{} shared by more than two faces",
                    plural(over)
                ));
            }
            if orient > 0 {
                why.push(format!(
                    "{orient} edge{} between faces facing opposite ways",
                    plural(orient)
                ));
            }
            fail(&mut reasons, why.join(", "));
            loose_faces.extend(shell_faces);
            continue;
        }
        let refs: Vec<&Vec<Vec<u32>>> = shell_faces.iter().collect();
        let Some((mut tris, tri_face)) = triangulate_faces(&refs, &pts) else {
            fail(&mut reasons, "a face that will not triangulate".into());
            loose_faces.extend(shell_faces);
            continue;
        };
        let vol = signed_volume(&tris, &pts);
        if vol.abs() <= AREA_EPS * POINT_MERGE {
            fail(&mut reasons, "encloses no volume".into());
            loose_faces.extend(shell_faces);
            continue;
        }
        if vol < 0.0 {
            flipped += 1;
            for f in &mut shell_faces {
                for r in f.iter_mut() {
                    r.reverse();
                }
            }
            for t in &mut tris {
                t.swap(1, 2);
            }
        }
        // Local numbering for this shell, in first-use order.
        let mut local: BTreeMap<u32, u32> = BTreeMap::new();
        let mut order: Vec<u32> = Vec::new();
        for &v in shell_faces.iter().flatten().flatten() {
            if let std::collections::btree_map::Entry::Vacant(e) = local.entry(v) {
                e.insert(order.len() as u32);
                order.push(v);
            }
        }
        let lp: Vec<DVec3> = order.iter().map(|&g| pts[g as usize]).collect();
        let lfaces: Vec<Vec<Vec<u32>>> = shell_faces
            .iter()
            .map(|f| {
                f.iter()
                    .map(|r| r.iter().map(|v| local[v]).collect())
                    .collect()
            })
            .collect();
        let ltris: Vec<[u32; 3]> = tris.iter().map(|t| t.map(|v| local[&v])).collect();
        let tri_labels: Vec<u32> = tri_face.iter().map(|&f| shell_labels[f]).collect();
        let tri_labels = labelled.then_some(tri_labels.as_slice());
        // Which item face each solid face came from, when the faces are kept as given.
        let kept: Vec<Option<usize>> = members.iter().map(|&fi| Some(face_src[fi])).collect();
        let attempt = if item.merge_coplanar {
            match merged_polys(&ltris, &lp, tri_labels) {
                Some(p) => {
                    merged_from += ltris.len();
                    as_solid(&p).map(|o| (o, false))
                }
                None => as_solid(&shell_polys(&lfaces, lp.clone())).map(|o| (o, true)),
            }
        } else {
            match as_solid(&shell_polys(&lfaces, lp.clone())) {
                Ok(o) => Ok((o, true)),
                // Faces a little off planar (common in exported files) are rebuilt from
                // their triangles, merged only where they are truly coplanar.
                Err(first) => match merged_polys(&ltris, &lp, None).map(|p| as_solid(&p)) {
                    Some(Ok(o)) => {
                        rebuilt += 1;
                        Ok((o, false))
                    }
                    _ => Err(first),
                },
            }
        };
        match attempt {
            Ok((o, as_given)) => {
                let n = o.brep().face_count();
                face_sources.push(if as_given && n == kept.len() {
                    kept
                } else {
                    vec![None; n]
                });
                solids.push(o);
            }
            Err(e) => {
                fail(&mut reasons, format!("not a valid solid ({e})"));
                loose_faces.extend(shell_faces);
            }
        }
    }
    if separated_edges > 0 && solids.len() > 1 {
        notes.push(format!(
            "{name}: separated solids that touch along {separated_edges} edge{}",
            plural(separated_edges)
        ));
    }
    if turned > 0 {
        notes.push(format!(
            "{name}: turned {turned} reversed face{} to match their neighbours",
            plural(turned)
        ));
    }
    if flipped > 0 {
        notes.push(format!(
            "{name}: turned {flipped} inside-out shell{} the right way round",
            plural(flipped)
        ));
    }
    if merged_from > 0 {
        notes.push(format!(
            "{name}: merged {merged_from} triangles into flat faces"
        ));
    }
    if rebuilt > 0 {
        notes.push(format!(
            "{name}: rebuilt {rebuilt} shell{} with slightly non-planar faces from triangles",
            plural(rebuilt)
        ));
    }

    // Loose lines (welded; no zero-length or repeated ones).
    let mut lines: Vec<[u32; 2]> = Vec::new();
    let mut seen_lines = BTreeSet::new();
    for l in &item.lines {
        let (a, b) = (map[l[0] as usize], map[l[1] as usize]);
        if a != b && seen_lines.insert((a.min(b), a.max(b))) {
            lines.push([a, b]);
        }
    }

    let loose = if loose_faces.is_empty() && lines.is_empty() {
        None
    } else {
        let mut why: Vec<String> = reasons
            .iter()
            .map(|(why, k)| {
                if *k == 1 {
                    why.clone()
                } else {
                    format!("{why} (x{k})")
                }
            })
            .collect();
        if loose_faces.is_empty() {
            why.push("edges only, no faces".into());
        }
        let note = format!("not a solid: {}", why.join("; "));
        notes.push(format!("{name}: kept as a mesh, {note}"));
        let mut local: BTreeMap<u32, u32> = BTreeMap::new();
        let mut order: Vec<u32> = Vec::new();
        for &v in loose_faces
            .iter()
            .flatten()
            .flatten()
            .chain(lines.iter().flatten())
        {
            if let std::collections::btree_map::Entry::Vacant(e) = local.entry(v) {
                e.insert(order.len() as u32);
                order.push(v);
            }
        }
        Some(LooseMesh::from(LooseMeshData {
            name: name.to_string(),
            positions: order.iter().map(|&g| pts[g as usize]).collect(),
            faces: loose_faces
                .iter()
                .map(|f| {
                    f.iter()
                        .map(|r| r.iter().map(|v| local[v]).collect())
                        .collect()
                })
                .collect(),
            lines: lines.iter().map(|l| l.map(|v| local[&v])).collect(),
            tag: item.tag.clone(),
            material: item.material.clone(),
            note,
            member: Default::default(),
            edge_flags: Default::default(),
        }))
    };
    Ok(Built {
        solids,
        face_sources,
        loose,
        notes,
    })
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Axis box as 12 triangles with unshared vertices, like a binary STL.
    pub(crate) fn stl_box(min: [f64; 3], size: [f64; 3]) -> ImportItem {
        let c = |i: usize| {
            [
                min[0] + if i & 1 != 0 { size[0] } else { 0.0 },
                min[1] + if i & 2 != 0 { size[1] } else { 0.0 },
                min[2] + if i & 4 != 0 { size[2] } else { 0.0 },
            ]
        };
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [1, 3, 7, 5],
            [0, 4, 6, 2],
        ];
        let mut positions = Vec::new();
        let mut faces = Vec::new();
        for q in quads {
            for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                let base = positions.len() as u32;
                positions.extend(t.map(c));
                faces.push(vec![vec![base, base + 1, base + 2]]);
            }
        }
        ImportItem {
            name: "box".into(),
            group: None,
            tag: None,
            material: None,
            positions,
            faces,
            lines: vec![],
            merge_coplanar: true,
            face_labels: vec![],
            face_materials: vec![],
            face_uvs: vec![],
            edge_flags: vec![],
        }
    }

    #[test]
    fn triangle_soup_box_becomes_a_six_face_solid() {
        let b = build_item(&stl_box([0.0; 3], [0.1, 0.2, 0.3])).unwrap();
        assert_eq!(b.solids.len(), 1);
        assert!(b.loose.is_none());
        let o = &b.solids[0];
        let r = o.validate().unwrap();
        assert_eq!((r.faces, r.edges, r.vertices), (6, 12, 8));
        assert!((o.volume() - 0.006).abs() < 1e-12);
        let mut ids: Vec<u64> = (0..6).map(|i| o.face_id_at(i).unwrap()).collect();
        ids.sort();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6]);
        assert!(b.notes.iter().any(|n| n.contains("merged 12 triangles")));
    }

    #[test]
    fn inside_out_box_is_turned_and_said_so() {
        let mut item = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        for f in &mut item.faces {
            f[0].reverse();
        }
        let b = build_item(&item).unwrap();
        assert_eq!(b.solids.len(), 1);
        assert!(b.solids[0].volume() > 0.0);
        assert!(
            b.notes.iter().any(|n| n.contains("inside-out")),
            "{:?}",
            b.notes
        );
    }

    #[test]
    fn open_box_stays_a_mesh_with_a_note() {
        let mut item = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        item.faces.remove(3); // one triangle of the top
        item.faces.remove(2);
        let b = build_item(&item).unwrap();
        assert!(b.solids.is_empty());
        let m = b.loose.unwrap();
        assert_eq!(m.faces.len(), 10, "nothing was repaired or dropped");
        assert!(m.note.contains("4 open edges"), "{}", m.note);
        m.check().unwrap();
    }

    #[test]
    fn two_boxes_are_two_solids_and_a_third_face_is_non_manifold() {
        let mut a = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        let b = stl_box([2.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let off = a.positions.len() as u32;
        a.positions.extend(b.positions);
        a.faces.extend(
            b.faces
                .into_iter()
                .map(|f| vec![f[0].iter().map(|i| i + off).collect()]),
        );
        let two = build_item(&a).unwrap();
        assert_eq!(two.solids.len(), 2);
        // A fin glued onto an edge of the first box: three faces on one edge.
        let base = a.positions.len() as u32;
        a.positions
            .extend([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.5, -1.0, 0.0]]);
        a.faces.push(vec![vec![base, base + 1, base + 2]]);
        let fin = build_item(&a).unwrap();
        assert_eq!(fin.solids.len(), 1, "the second box is untouched");
        let note = fin.loose.unwrap().note.clone();
        assert!(note.contains("more than two faces"), "{note}");
    }

    #[test]
    fn welds_drops_degenerates_and_repeats() {
        let mut item = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        item.faces.push(item.faces[0].clone());
        let base = item.positions.len() as u32;
        item.positions
            .extend([[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        item.faces.push(vec![vec![base, base + 1, base + 2]]);
        let b = build_item(&item).unwrap();
        assert_eq!(b.solids.len(), 1);
        assert!(b.notes.iter().any(|n| n.contains("1 degenerate")));
        assert!(b.notes.iter().any(|n| n.contains("1 exactly repeated")));
    }

    #[test]
    fn polygon_faces_keep_their_dividing_edges() {
        // A box whose top is two coplanar faces (a line drawn across it, SketchUp-style).
        let positions = vec![
            [0., 0., 0.],
            [2., 0., 0.],
            [2., 1., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [2., 0., 1.],
            [2., 1., 1.],
            [0., 1., 1.],
            [1., 0., 1.],
            [1., 1., 1.],
        ];
        let f = |r: &[u32]| vec![r.to_vec()];
        let faces = vec![
            f(&[0, 3, 2, 1]),    // bottom (down)
            f(&[4, 8, 9, 7]),    // top left
            f(&[8, 5, 6, 9]),    // top right
            f(&[0, 1, 5, 8, 4]), // front
            f(&[2, 3, 7, 9, 6]), // back
            f(&[1, 2, 6, 5]),    // right
            f(&[0, 4, 7, 3]),    // left
        ];
        let item = ImportItem {
            name: "slab".into(),
            group: None,
            tag: None,
            material: None,
            positions,
            faces,
            lines: vec![],
            merge_coplanar: false,
            face_labels: vec![],
            face_materials: vec![],
            face_uvs: vec![],
            edge_flags: vec![],
        };
        let b = build_item(&item).unwrap();
        assert_eq!(b.solids.len(), 1, "{:?}", b.notes);
        let p = b.solids[0].brep().to_polys();
        assert_eq!(p.faces.len(), 7);
        assert_eq!(
            p.user_edges.len(),
            1,
            "the line across the top is a drawn edge"
        );
    }

    #[test]
    fn face_labels_keep_coplanar_faces_apart() {
        // The box's top (two triangles) labelled as two different faces: a drawn diagonal.
        let mut item = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        item.face_labels = vec![1, 1, 2, 3, 4, 4, 5, 5, 6, 6, 7, 7];
        let b = build_item(&item).unwrap();
        let p = b.solids[0].brep().to_polys();
        assert_eq!(p.faces.len(), 7);
        assert_eq!(
            p.user_edges.len(),
            1,
            "the diagonal between the labels is a drawn edge"
        );
        // Without labels the top is one face again.
        item.face_labels.clear();
        let one = build_item(&item).unwrap().solids[0].brep().to_polys();
        assert_eq!((one.faces.len(), one.user_edges.len()), (6, 0));
        item.face_labels = vec![1; 12];
        item.face_labels.pop();
        assert!(build_item(&item).is_err(), "one label per face");
    }

    #[test]
    fn reversed_faces_are_turned_to_match() {
        let mut item = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        // Reverse both triangles of one side (a SketchUp "reversed face").
        item.faces[4][0].reverse();
        item.faces[5][0].reverse();
        let b = build_item(&item).unwrap();
        assert_eq!(b.solids.len(), 1, "{:?}", b.notes);
        assert!(
            b.notes
                .iter()
                .any(|n| n.contains("turned 2 reversed faces")),
            "{:?}",
            b.notes
        );
        assert!((b.solids[0].volume() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn solids_touching_along_an_edge_or_a_face_come_apart() {
        // Two unit boxes sharing one edge (diagonal neighbours), as one triangle soup.
        let join = |a: ImportItem, b: ImportItem| {
            let mut a = a;
            let off = a.positions.len() as u32;
            a.positions.extend(b.positions);
            a.faces.extend(
                b.faces
                    .into_iter()
                    .map(|f| vec![f[0].iter().map(|i| i + off).collect()]),
            );
            a
        };
        let edge = join(
            stl_box([0.0; 3], [1.0, 1.0, 1.0]),
            stl_box([1.0, 1.0, 0.0], [1.0, 1.0, 1.0]),
        );
        let b = build_item(&edge).unwrap();
        assert_eq!(b.solids.len(), 2, "{:?}", b.notes);
        assert!(b.loose.is_none());
        assert!(
            b.notes
                .iter()
                .any(|n| n.contains("separated solids that touch"))
        );
        // Two boxes face to face (a shelf sitting on a side), sharing the whole contact face.
        let face = join(
            stl_box([0.0; 3], [1.0, 1.0, 1.0]),
            stl_box([1.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
        );
        let b = build_item(&face).unwrap();
        assert_eq!(b.solids.len(), 2, "{:?}", b.notes);
        let v: f64 = b.solids.iter().map(|o| o.volume()).sum();
        assert!((v - 2.0).abs() < 1e-12);
    }

    #[test]
    fn malformed_data_is_refused() {
        let mut item = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        item.faces.push(vec![vec![0, 1, 999]]);
        assert!(matches!(
            build_item(&item),
            Err(DocumentError::BadImport(_))
        ));
        let mut item = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        item.positions[0][0] = f64::NAN;
        assert!(build_item(&item).is_err());
    }

    fn pts(item: &ImportItem) -> Vec<DVec3> {
        item.positions
            .iter()
            .map(|p| DVec3::from_array(*p))
            .collect()
    }

    #[test]
    fn the_cut_list_heal_closes_tiny_gaps_and_drops_isolated_slivers() {
        // A closed box: closed.
        let b = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        assert!(heal_check(&pts(&b), &b.faces).closed);
        // An open box (the top gone): not closed, whatever the heal does.
        let mut open = b.clone();
        open.faces.truncate(10);
        assert!(!heal_check(&pts(&open), &open.faces).closed);
        // A 50 micrometre gap on one corner: the import's 10 micrometre weld leaves it open,
        // the heal's 0.1 mm weld closes it.
        let mut gap = b.clone();
        gap.positions[0][0] += 5e-5;
        assert!(build_item(&gap).unwrap().loose.is_some());
        assert!(heal_check(&pts(&gap), &gap.faces).closed);
        // Reversed faces are turned.
        let mut rev = b.clone();
        rev.faces[0][0].reverse();
        assert!(heal_check(&pts(&rev), &rev.faces).closed);
        // A floating sliver beside the box is dropped; a floating panel the size of a face
        // is not (it is real geometry), so that mesh stays open.
        let mut sliver = b.clone();
        let o = sliver.positions.len() as u32;
        sliver
            .positions
            .extend([[3.0, 0.0, 0.0], [3.001, 0.0, 0.0], [3.0, 0.001, 0.0]]);
        sliver.faces.push(vec![vec![o, o + 1, o + 2]]);
        assert!(heal_check(&pts(&sliver), &sliver.faces).closed);
        let mut panel = b.clone();
        panel
            .positions
            .extend([[3.0, 0.0, 0.0], [4.0, 0.0, 0.0], [3.0, 1.0, 0.0]]);
        panel.faces.push(vec![vec![o, o + 1, o + 2]]);
        assert!(!heal_check(&pts(&panel), &panel.faces).closed);
    }

    #[test]
    fn faces_come_apart_into_edge_connected_pieces() {
        let a = stl_box([0.0; 3], [1.0, 1.0, 1.0]);
        let b = stl_box([2.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        // Welded indices, as a mesh holds them.
        let mut item = a.clone();
        let off = item.positions.len() as u32;
        item.positions.extend(b.positions.clone());
        item.faces.extend(
            b.faces
                .iter()
                .map(|f| vec![f[0].iter().map(|v| v + off).collect()]),
        );
        let (_, map) = weld(&pts(&item));
        let faces: Vec<Vec<Vec<u32>>> = item
            .faces
            .iter()
            .map(|f| vec![clean_ring(&f[0], &map)])
            .collect();
        let pieces = connected_pieces(&faces);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0], (0..12).collect::<Vec<_>>());
        assert_eq!(pieces[1], (12..24).collect::<Vec<_>>());
    }
}
