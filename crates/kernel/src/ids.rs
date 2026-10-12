//! Generational ids (slotmap keys). Stale ids fail lookups instead of aliasing new data.
//!
//! Ids that cross the kernel boundary (ops, the UI, golden files) are written as plain
//! `u64` "public ids" by [`to_public`] / [`from_public`]: the slot index in the low 32 bits
//! and the slot's reuse count in the high bits. The first key of a fresh map is `1`, the
//! second `2`, and so on until a slot is reused, so JSON stays readable and every value is
//! exact as a JavaScript number (below 2^53) for the first two million reuses of a slot.

use slotmap::{Key, KeyData};

slotmap::new_key_type! {
    /// B-rep vertex.
    pub struct VertexId;
    /// One directed side of an edge, owned by exactly one loop.
    pub struct HalfEdgeId;
    /// Undirected edge: one half-edge, or two twinned half-edges.
    pub struct EdgeId;
    /// Closed cycle of half-edges: a face's outer boundary or one of its holes.
    pub struct LoopId;
    /// Planar face: one outer loop plus any number of hole loops.
    pub struct FaceId;
    /// A solid (watertight half-edge B-rep).
    pub struct ObjectId;
    /// A planar sketch graph.
    pub struct SketchId;
    /// Sketch vertex (2D, in the sketch plane's frame).
    pub struct SVertexId;
    /// Sketch edge.
    pub struct SEdgeId;
    /// A sketch region (a face of the sketch graph). Stable across edits: re-detection keeps
    /// the id of the region it matches (same boundary, or containing the old anchor point).
    pub struct RegionId;
    /// A named group of objects.
    pub struct GroupId;
    /// A guide line or guide point (Tape measure).
    pub struct GuideId;
    /// A linear dimension annotation.
    pub struct DimensionId;
    /// An imported mesh that is not a valid solid (shown, never edited).
    pub struct MeshId;
}

/// Public (serialisable) form of a key. Null keys map to 0.
pub fn to_public<K: Key>(k: K) -> u64 {
    if k.is_null() {
        return 0;
    }
    let ffi = k.data().as_ffi();
    let idx = ffi & 0xffff_ffff;
    let version = ffi >> 32; // odd for occupied slots
    ((version >> 1) << 32) | idx
}

/// Key for a public id. Unknown ids produce keys that simply fail lookups.
pub fn from_public<K: Key>(id: u64) -> K {
    if id == 0 {
        return K::null();
    }
    let idx = id & 0xffff_ffff;
    let version = ((id >> 32) << 1) | 1;
    KeyData::from_ffi((version << 32) | idx).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    #[test]
    fn public_ids_are_small_and_round_trip() {
        let mut m: SlotMap<ObjectId, u8> = SlotMap::with_key();
        let a = m.insert(1);
        let b = m.insert(2);
        assert_eq!((to_public(a), to_public(b)), (1, 2));
        assert_eq!(from_public::<ObjectId>(2), b);
        m.remove(a);
        let c = m.insert(3); // reuses slot 1 with a new version
        assert_eq!(to_public(c), (1 << 32) | 1);
        assert_eq!(from_public::<ObjectId>(to_public(c)), c);
        assert!(
            m.get(from_public::<ObjectId>(1)).is_none(),
            "stale id fails"
        );
        assert!(m.get(from_public::<ObjectId>(0)).is_none());
        assert!(m.get(from_public::<ObjectId>(u64::MAX)).is_none());
    }
}
