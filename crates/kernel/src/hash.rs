//! Deterministic model hashing for golden replay tests.
//!
//! FNV-1a 64 over a canonical byte stream: positions quantised to [`HASH_QUANTUM`] and
//! topology written as ordinals (slotmap insertion order), never as raw keys or pointer
//! values. The same op sequence therefore hashes identically on every platform, natively
//! and in WASM. Stored face planes are deliberately excluded: they are derived data.

use std::collections::BTreeMap;

use slotmap::Key;

use crate::model::Model;
use crate::tolerance::HASH_QUANTUM;

/// Bump when the hashed representation changes (and regenerate goldens on purpose).
pub const HASH_FORMAT: &str = "sawhorse-model-hash-v1";

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

pub fn model_hash(m: &Model) -> u64 {
    let mut h = StableHasher::default();
    h.tag(HASH_FORMAT);
    let sketches = m.sketches();
    h.usize(sketches.len());
    for s in sketches {
        h.tag("sketch");
        let p = s.plane();
        for v in [p.origin, p.normal, p.u] {
            h.len(v.x);
            h.len(v.y);
            h.len(v.z);
        }
        let verts: Vec<_> = s.vertices().collect();
        let ord: BTreeMap<_, _> = verts
            .iter()
            .enumerate()
            .map(|(i, (k, _))| (*k, i))
            .collect();
        h.usize(verts.len());
        for (_, q) in &verts {
            h.len(q.x);
            h.len(q.y);
        }
        let edges: Vec<_> = s.edges().collect();
        h.usize(edges.len());
        for (_, [a, b]) in edges {
            h.usize(ord[&a]);
            h.usize(ord[&b]);
        }
        h.usize(s.consumed().len());
        for key in s.consumed().keys() {
            h.usize(key.len());
            for v in key {
                h.usize(ord[v]);
            }
        }
    }
    let objects = m.objects();
    h.usize(objects.len());
    for o in objects {
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
