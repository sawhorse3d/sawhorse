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
//! shell to match their neighbours, and merging coplanar triangles.

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
}

/// Imported geometry that is not a valid solid: drawn, listed, exported to mesh formats,
/// never pushed or pulled. `note` says why it is not a solid.
#[derive(Debug, Clone, PartialEq)]
pub struct LooseMesh {
    pub name: String,
    pub positions: Vec<DVec3>,
    pub faces: Vec<Vec<Vec<u32>>>,
    pub lines: Vec<[u32; 2]>,
    pub tag: Option<String>,
    pub material: Option<String>,
    pub note: String,
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
    let cell = POINT_MERGE;
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
                            if out[q as usize].distance(p) <= POINT_MERGE {
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
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    for list in uses.values() {
        for w in list.windows(2) {
            let (a, b) = (find(&mut parent, w[0].0), find(&mut parent, w[1].0));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut shells: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for fi in 0..faces.len() {
        let r = find(&mut parent, fi);
        shells.entry(r).or_default().push(fi);
    }

    let mut solids = Vec::new();
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
    for members in shells.values() {
        let in_shell: BTreeSet<usize> = members.iter().copied().collect();
        let (mut open, mut over, mut orient) = (0usize, 0usize, 0usize);
        for list in uses.values() {
            if !in_shell.contains(&list[0].0) {
                continue;
            }
            match list.len() {
                1 => open += 1,
                2 if list[0].1 == list[1].1 => orient += 1,
                2 => {}
                _ => over += 1,
            }
        }
        let mut shell_faces: Vec<Vec<Vec<u32>>> =
            members.iter().map(|&fi| faces[fi].clone()).collect();
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
        let attempt = if item.merge_coplanar {
            match merged_polys(&ltris, &lp, tri_labels) {
                Some(p) => {
                    merged_from += ltris.len();
                    as_solid(&p)
                }
                None => as_solid(&shell_polys(&lfaces, lp.clone())),
            }
        } else {
            match as_solid(&shell_polys(&lfaces, lp.clone())) {
                Ok(o) => Ok(o),
                // Faces a little off planar (common in exported files) are rebuilt from
                // their triangles, merged only where they are truly coplanar.
                Err(first) => match merged_polys(&ltris, &lp, None).map(|p| as_solid(&p)) {
                    Some(Ok(o)) => {
                        rebuilt += 1;
                        Ok(o)
                    }
                    _ => Err(first),
                },
            }
        };
        match attempt {
            Ok(o) => solids.push(o),
            Err(e) => {
                fail(&mut reasons, format!("not a valid solid ({e})"));
                loose_faces.extend(shell_faces);
            }
        }
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
        Some(LooseMesh {
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
        })
    };
    Ok(Built {
        solids,
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
        let note = fin.loose.unwrap().note;
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
}
