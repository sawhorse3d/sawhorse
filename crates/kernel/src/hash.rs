//! Deterministic model hashing for golden replay tests.
//!
//! FNV-1a 64 over a canonical byte stream: positions quantised to [`HASH_QUANTUM`] and
//! topology written as ordinals (slotmap insertion order), never as raw keys or pointer
//! values. The same op sequence therefore hashes identically on every platform, natively
//! and in WASM. Stored face planes are deliberately excluded: they are derived data.
//! Persistent face ids (and each object's id counter) are included: ops address faces by
//! id, so two states that differ only in ids would replay differently (v3).
//!
//! Sketches hash canonically (vertices ordered by quantised position, rings and edges in
//! that numbering, sorted), so the same strokes drawn in any order hash the same. Region
//! ids are left out: they record history, not shape.

use std::collections::BTreeMap;

use slotmap::Key;

use crate::ids::SVertexId;
use crate::model::Model;
use crate::sketch::Sketch;
use crate::tolerance::HASH_QUANTUM;

/// Bump when the hashed representation changes (and regenerate goldens on purpose).
pub const HASH_FORMAT: &str = "sawhorse-model-hash-v3";

#[derive(Debug, Clone, Copy)]
pub struct StableHasher(u64);

impl Default for StableHasher {
    fn default() -> Self {
        StableHasher(0xcbf2_9ce4_8422_2325)
    }
}

impl StableHasher {
    pub fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn usize(&mut self, v: usize) {
        self.u64(v as u64);
    }
    pub fn tag(&mut self, s: &str) {
        self.usize(s.len());
        self.bytes(s.as_bytes());
    }
    /// A length in metres, quantised to HASH_QUANTUM (1 nm).
    pub fn len(&mut self, x: f64) {
        let q = (x / HASH_QUANTUM).round();
        self.u64(if q == 0.0 { 0 } else { q as i64 as u64 });
    }
    pub fn finish(&self) -> u64 {
        self.0
    }
}

fn ordinals<K: Key, V>(map: &slotmap::SlotMap<K, V>) -> BTreeMap<K, usize> {
    map.keys().enumerate().map(|(i, k)| (k, i)).collect()
}

/// Quantised position key (1 nm grid).
fn q(x: f64) -> i64 {
    let v = (x / HASH_QUANTUM).round();
    if v == 0.0 { 0 } else { v as i64 }
}

/// A region in canonical vertex numbering: outer ring, holes, pinched, consumed.
type CanonRegion = (Vec<usize>, Vec<Vec<usize>>, bool, bool);

/// Canonical sketch hash: independent of the order strokes were drawn in. Vertices are
/// ordered by quantised position, edges and region rings are written in that ordering and
/// sorted. Region ids are deliberately left out: they record history, not shape.
fn hash_sketch(h: &mut StableHasher, s: &Sketch) {
    h.tag("sketch");
    let p = s.plane();
    for v in [p.origin, p.normal, p.u] {
        h.len(v.x);
        h.len(v.y);
        h.len(v.z);
    }
    let mut verts: Vec<((i64, i64), SVertexId)> =
        s.vertices().map(|(k, p)| ((q(p.x), q(p.y)), k)).collect();
    verts.sort();
    let ord: BTreeMap<SVertexId, usize> = verts
        .iter()
        .enumerate()
        .map(|(i, (_, k))| (*k, i))
        .collect();
    h.usize(verts.len());
    for ((x, y), _) in &verts {
        h.u64(*x as u64);
        h.u64(*y as u64);
    }
    let mut edges: Vec<(usize, usize)> = s
        .edges()
        .map(|(_, [a, b])| (ord[&a].min(ord[&b]), ord[&a].max(ord[&b])))
        .collect();
    edges.sort();
    h.usize(edges.len());
    for (a, b) in edges {
        h.usize(a);
        h.usize(b);
    }
    let ring = |r: &[SVertexId]| -> Vec<usize> {
        let idx: Vec<usize> = r.iter().map(|v| ord[v]).collect();
        let start = (0..idx.len()).min_by_key(|&i| idx[i]).unwrap_or(0);
        idx[start..]
            .iter()
            .chain(idx[..start].iter())
            .copied()
            .collect()
    };
    let mut regions: Vec<CanonRegion> = s
        .all_regions()
        .map(|(_, r)| {
            let mut holes: Vec<Vec<usize>> = r.holes.iter().map(|h| ring(h)).collect();
            holes.sort();
            (ring(&r.outer), holes, r.pinched, r.consumed)
        })
        .collect();
    regions.sort();
    h.usize(regions.len());
    for (outer, holes, pinched, consumed) in regions {
        h.usize(outer.len());
        outer.iter().for_each(|&i| h.usize(i));
        h.usize(holes.len());
        for hole in holes {
            h.usize(hole.len());
            hole.iter().for_each(|&i| h.usize(i));
        }
        h.u64(u64::from(pinched));
        h.u64(u64::from(consumed));
    }
}

/// Canonical hash of one sketch (for order-independence tests and the UI).
pub fn sketch_hash(s: &Sketch) -> u64 {
    let mut h = StableHasher::default();
    hash_sketch(&mut h, s);
    h.finish()
}

/// A solid's geometry, topology, face ids and drawn edges (positions quantised).
fn hash_solid(h: &mut StableHasher, o: &crate::object::Object) {
    let b = o.brep();
    let vord = ordinals(&b.vertices);
    h.usize(b.vertices.len());
    for v in b.vertices.values() {
        h.len(v.pos.x);
        h.len(v.pos.y);
        h.len(v.pos.z);
    }
    h.usize(b.faces.len());
    h.u64(o.next_face_id());
    for f in b.faces.values() {
        h.u64(f.id);
        h.usize(1 + f.holes.len());
        for l in f.loops() {
            let vs = b.loop_vertices(l);
            h.usize(vs.len());
            for v in vs {
                h.usize(vord[&v]);
            }
        }
    }
    let mut user: Vec<(usize, usize)> = b
        .edges
        .values()
        .filter(|e| e.user)
        .map(|e| {
            let (x, y) = (vord[&b.half_edges[e.he].origin], vord[&b.dest(e.he)]);
            (x.min(y), x.max(y))
        })
        .collect();
    user.sort();
    h.usize(user.len());
    for (x, y) in user {
        h.usize(x);
        h.usize(y);
    }
    h.u64(u64::from(o.is_watertight()));
}

fn xform(h: &mut StableHasher, x: &crate::transform::Xform) {
    x.linear.iter().flatten().for_each(|&v| h.len(v));
    x.translation.iter().for_each(|&v| h.len(v));
}

fn opt(h: &mut StableHasher, s: Option<&str>) {
    match s {
        Some(t) => h.tag(t),
        None => h.u64(u64::MAX),
    }
}

fn membership(h: &mut StableHasher, m: &crate::nodes::Membership) {
    h.u64(m.parent.map_or(0, crate::ids::to_public));
    h.u64(m.slot.map_or(u64::MAX, u64::from));
    h.u64(u64::from(m.hidden) | (u64::from(m.locked) << 1));
}

fn hash_mesh_geometry(h: &mut StableHasher, mesh: &crate::meshbuild::LooseMesh) {
    h.usize(mesh.positions.len());
    for p in &mesh.positions {
        h.len(p.x);
        h.len(p.y);
        h.len(p.z);
    }
    h.usize(mesh.faces.len());
    for f in &mesh.faces {
        h.usize(f.len());
        for r in f {
            h.usize(r.len());
            r.iter().for_each(|&i| h.u64(u64::from(i)));
        }
    }
}

/// Groups, instances, definitions, memberships, sketch parents, the editing context, tag
/// colours and scene contexts (lane 5). Written only when any of it is present.
fn hash_structure(h: &mut StableHasher, m: &Model) {
    use crate::nodes::{MemberKind, NodeKind};
    h.tag("structure");
    h.u64(m.context().unwrap_or(0));
    let nodes = m.nodes();
    h.usize(nodes.len());
    for (id, n) in nodes {
        h.u64(id);
        match n.kind {
            NodeKind::Group => h.u64(0),
            NodeKind::Instance(d) => h.u64(m.definition_id(d)),
        }
        opt(h, n.name.as_deref());
        opt(h, n.tag.as_deref());
        membership(h, &n.member);
        xform(h, &n.xform);
        if !n.part.is_default() {
            n.part.hash_into(h);
        }
    }
    let defs = m.definitions();
    h.usize(defs.len());
    for (id, d) in defs {
        h.u64(id);
        h.tag(&d.name);
        h.u64(u64::from(d.next_slot));
        if !d.part.is_default() {
            d.part.hash_into(h);
        }
        h.usize(d.members.len());
        for (&slot, mem) in &d.members {
            h.u64(u64::from(slot));
            h.u64(mem.parent.map_or(u64::MAX, u64::from));
            opt(h, mem.name.as_deref());
            opt(h, mem.tag.as_deref());
            h.u64(u64::from(mem.hidden) | (u64::from(mem.locked) << 1));
            if !mem.part.is_default() {
                mem.part.hash_into(h);
            }
            match &mem.kind {
                MemberKind::Solid(o) => {
                    h.tag("solid");
                    hash_solid(h, o);
                    opt(h, o.material());
                    if !o.part().is_default() {
                        o.part().hash_into(h);
                    }
                }
                MemberKind::Mesh(x) => {
                    h.tag("mesh");
                    hash_mesh_geometry(h, x);
                    opt(h, x.material.as_deref());
                }
                MemberKind::Group(x) => {
                    h.tag("group");
                    xform(h, x);
                }
                MemberKind::Instance(e, x) => {
                    h.tag("instance");
                    h.u64(m.definition_id(*e));
                    xform(h, x);
                }
            }
        }
    }
    for (id, o) in m.objects() {
        if o.membership() != &crate::nodes::Membership::default() {
            h.u64(id);
            membership(h, o.membership());
        }
    }
    h.tag("meshes");
    for (id, x) in m.meshes() {
        if x.member != crate::nodes::Membership::default() {
            h.u64(id);
            membership(h, &x.member);
        }
    }
    h.tag("sketches");
    for (k, (_, s)) in m.sketches().into_iter().enumerate() {
        if let Some(p) = s.parent() {
            h.usize(k);
            h.u64(p);
        }
    }
    h.tag("colours");
    for (t, c) in m.tag_colors() {
        h.tag(t);
        h.bytes(c);
    }
    h.tag("scenes");
    for sc in m.scenes() {
        h.u64(sc.context.unwrap_or(0));
    }
}

/// Hash of one solid's shape (what [`hash_solid`] writes), for comparing a component
/// instance's solids with their definition.
pub fn object_hash(o: &crate::object::Object) -> u64 {
    let mut h = StableHasher::default();
    hash_solid(&mut h, o);
    h.finish()
}

pub fn model_hash(m: &Model) -> u64 {
    let mut h = StableHasher::default();
    h.tag(HASH_FORMAT);
    let sketches = m.sketches();
    h.usize(sketches.len());
    for (_, s) in sketches {
        hash_sketch(&mut h, s);
    }
    let objects = m.objects();
    h.usize(objects.len());
    for (id, o) in objects {
        h.tag("object");
        h.u64(id);
        match o.tag() {
            Some(t) => h.tag(t),
            None => h.u64(u64::MAX),
        }
        hash_solid(&mut h, o);
        // Names and materials only when set, so older documents keep their hashes.
        if o.name().is_some() || o.material().is_some() {
            h.tag("meta");
            h.tag(o.name().unwrap_or(""));
            h.tag(o.material().unwrap_or(""));
        }
        // Cut-list attributes only when said (lane 7), so older documents keep their hashes.
        if !o.part().is_default() {
            o.part().hash_into(&mut h);
        }
    }
    // Annotations are hashed only when there are any, so documents without guides keep
    // their hashes.
    let (guides, dims) = (m.guides(), m.dimensions());
    if !guides.is_empty() || !dims.is_empty() {
        h.tag("annotations");
        h.usize(guides.len());
        for (id, g) in guides {
            h.u64(id);
            for v in [g.point, g.dir] {
                h.len(v.x);
                h.len(v.y);
                h.len(v.z);
            }
        }
        h.usize(dims.len());
        for (id, d) in dims {
            h.u64(id);
            for v in [d.a, d.b, d.offset] {
                h.len(v.x);
                h.len(v.y);
                h.len(v.z);
            }
        }
    }
    // Groups as lane 2 hashed them (name and solids directly inside), so documents with
    // plain groups keep their hashes; everything else about the tree is in "structure".
    let groups = m.groups();
    h.usize(groups.len());
    for (id, g) in groups {
        h.u64(id);
        h.tag(&g.name);
        h.usize(g.objects.len());
        g.objects.iter().for_each(|&o| h.u64(o));
    }
    if m.has_structure() {
        hash_structure(&mut h, m);
    }
    h.usize(m.tags().len());
    for (t, visible) in m.tags() {
        h.tag(t);
        h.u64(u64::from(*visible));
    }
    // Imported meshes, materials and scenes: hashed only when present (lane 6), so documents
    // without them keep their hashes.
    let meshes = m.meshes();
    if !meshes.is_empty() || !m.materials().is_empty() || !m.scenes().is_empty() {
        h.tag("imports");
        h.usize(meshes.len());
        for (id, mesh) in meshes {
            h.u64(id);
            h.tag(&mesh.name);
            h.tag(mesh.tag.as_deref().unwrap_or(""));
            h.tag(mesh.material.as_deref().unwrap_or(""));
            h.tag(&mesh.note);
            h.usize(mesh.positions.len());
            for p in &mesh.positions {
                h.len(p.x);
                h.len(p.y);
                h.len(p.z);
            }
            h.usize(mesh.faces.len());
            for f in &mesh.faces {
                h.usize(f.len());
                for r in f {
                    h.usize(r.len());
                    r.iter().for_each(|&i| h.u64(u64::from(i)));
                }
            }
            h.usize(mesh.lines.len());
            for l in &mesh.lines {
                h.u64(u64::from(l[0]));
                h.u64(u64::from(l[1]));
            }
        }
        h.usize(m.materials().len());
        for (name, mat) in m.materials() {
            h.tag(name);
            h.bytes(&mat.color);
            h.tag(mat.texture.as_deref().unwrap_or(""));
        }
        h.usize(m.scenes().len());
        for sc in m.scenes() {
            h.tag(&sc.name);
            for v in [sc.eye, sc.target, sc.up] {
                v.iter().for_each(|&x| h.len(x));
            }
            h.len(sc.fov * 1e-3);
            h.u64(u64::from(sc.parallel));
            h.usize(sc.hidden_tags.len());
            sc.hidden_tags.iter().for_each(|t| h.tag(t));
        }
    }
    // The stock library (lane 9): only when it is not the built-in one, so every older
    // document keeps its hash.
    if let Some(lib) = m.stored_stock_library() {
        lib.hash_into(&mut h);
    }
    // Materials beyond a colour, face paint, textures, styles and shadows (lane 10): hashed
    // only when present, so documents without them keep their hashes.
    if m.has_looks() {
        m.hash_looks(&mut h);
    }
    // Section planes and the section state of scenes (lane 12): only when present.
    if m.has_sections() {
        m.hash_sections(&mut h);
    }
    // Drawing sheets (lane 12): only when there is one.
    if !m.sheet_order.is_empty() {
        m.hash_sheets(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_reference_vectors() {
        let mut h = StableHasher::default();
        h.bytes(b"");
        assert_eq!(h.finish(), 0xcbf2_9ce4_8422_2325);
        let mut h = StableHasher::default();
        h.bytes(b"a");
        assert_eq!(h.finish(), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn quantisation_absorbs_noise() {
        let mut a = StableHasher::default();
        a.len(0.1 + 0.2);
        let mut b = StableHasher::default();
        b.len(0.3);
        assert_eq!(a.finish(), b.finish());
        let mut z = StableHasher::default();
        z.len(-0.0);
        let mut z2 = StableHasher::default();
        z2.len(1e-12);
        assert_eq!(z.finish(), z2.finish());
    }
}
