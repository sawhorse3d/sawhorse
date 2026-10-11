//! Deterministic model hashing for golden replay tests.
//!
//! FNV-1a 64 over a canonical byte stream: positions quantised to [`HASH_QUANTUM`] and
//! topology written as ordinals (slotmap insertion order), never as raw keys or pointer
//! values. The same op sequence therefore hashes identically on every platform, natively
//! and in WASM. Stored face planes are deliberately excluded: they are derived data.
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
pub const HASH_FORMAT: &str = "sawhorse-model-hash-v2";

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
    for (_, o) in objects {
        h.tag("object");
        let b = o.brep();
        let vord = ordinals(&b.vertices);
        h.usize(b.vertices.len());
        for v in b.vertices.values() {
            h.len(v.pos.x);
            h.len(v.pos.y);
            h.len(v.pos.z);
        }
        h.usize(b.faces.len());
        for f in b.faces.values() {
            h.usize(1 + f.holes.len());
            for l in f.loops() {
                let vs = b.loop_vertices(l);
                h.usize(vs.len());
                for v in vs {
                    h.usize(vord[&v]);
                }
            }
        }
        h.u64(u64::from(o.is_watertight()));
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
