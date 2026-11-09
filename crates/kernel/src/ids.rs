//! Generational ids (slotmap keys). Stale ids fail lookups instead of aliasing new data.
//!
//! Ids that cross the kernel boundary (ops, the UI, golden files) are written as plain
//! `u64` "public ids" by [`to_public`] / [`from_public`]: the slot index in the low 32 bits
//! and the slot's reuse count in the high bits. The first key of a fresh map is `1`, the
//! second `2`, and so on until a slot is reused, so JSON stays readable and every value is
//! exact as a JavaScript number (below 2^53) for the first two million reuses of a slot.

use slotmap::{Key, KeyData, SlotMap};

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
    /// A group or a component instance: a node of the model's tree (see [`crate::nodes`]).
    pub struct NodeId;
    /// A component definition: the geometry and structure its instances share.
    pub struct DefId;
    /// A guide line or guide point (Tape measure).
    pub struct GuideId;
    /// A linear dimension annotation.
    pub struct DimensionId;
    /// An imported mesh that is not a valid solid (shown, never edited).
    pub struct MeshId;
    /// A section plane (lane 12): cuts what is in its group, component or the model.
    pub struct SectionId;
    /// A drawing sheet (lane 12): paper with viewports, dimensions, labels and tables.
    pub struct SheetId;
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

/// Why [`with_ids`] refused a list of ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdError {
    /// Zero, or a slot index past what the list could ever have used.
    OutOfRange(u64),
    /// Two entries name the same slot.
    Twice(u64),
    /// Restoring the ids would take more than the work left (see `limits::MAX_ID_WORK`).
    TooMuchWork,
}

/// Lane 28: a slot map holding `items` at exactly the public ids given, so a model read
/// back from a file has the ids it was saved with, holes and reused slots included. Ops,
/// scenes, plugins' data and the model hash all name things by id; a reader that numbered
/// them 1, 2, 3 again gave a model whose hash differed from the one saved whenever anything
/// had been deleted, so Save refused every file of such a model.
///
/// Each id is built as the live map built it: the slots are taken in index order (a fresh
/// map appends while nothing is free), each slot is freed and taken again once per reuse
/// (a freed slot is the next one taken, so it comes straight back with its version moved
/// on), and the slots no item uses are filled by `fill` and freed again at the end. A slot
/// index above `max_index` is refused (a live map only grows past its count of items when
/// every slot is full, so its largest index is the most it ever held at once), and the
/// steps are taken from `work`. Returns the map and each item's key, in the order given.
pub fn with_ids<K: Key, V>(
    items: Vec<(u64, V)>,
    mut fill: impl FnMut() -> V,
    max_index: u64,
    work: &mut u64,
) -> Result<(SlotMap<K, V>, Vec<K>), IdError> {
    let n = items.len();
    let mut wanted: std::collections::BTreeMap<u64, (usize, u64, V)> =
        std::collections::BTreeMap::new();
    let mut steps: u64 = 0;
    for (pos, (id, v)) in items.into_iter().enumerate() {
        let (idx, reuse) = (id & 0xffff_ffff, id >> 32);
        // A slot's version is odd and 32 bits wide: 2 * reuse + 1 must fit.
        if idx == 0 || idx > max_index || reuse > (u64::from(u32::MAX) - 1) / 2 {
            return Err(IdError::OutOfRange(id));
        }
        if wanted.insert(idx, (pos, reuse, v)).is_some() {
            return Err(IdError::Twice(id));
        }
        steps = steps.saturating_add(reuse);
    }
    let top = wanted.keys().next_back().copied().unwrap_or(0);
    steps = steps.saturating_add(top);
    if steps > *work {
        return Err(IdError::TooMuchWork);
    }
    *work -= steps;
    let mut map: SlotMap<K, V> = SlotMap::with_capacity_and_key(top as usize);
    let mut keys: Vec<Option<K>> = (0..n).map(|_| None).collect();
    let mut holes = Vec::new();
    for idx in 1..=top {
        match wanted.remove(&idx) {
            Some((pos, reuse, v)) => {
                let mut k = map.insert(v);
                for _ in 0..reuse {
                    let v = map.remove(k).expect("the slot was just filled");
                    k = map.insert(v);
                }
                debug_assert_eq!(to_public(k), (reuse << 32) | idx);
                keys[pos] = Some(k);
            }
            None => holes.push(map.insert(fill())),
        }
    }
    for h in holes {
        map.remove(h);
    }
    Ok((
        map,
        keys.into_iter()
            .map(|k| k.expect("every item placed"))
            .collect(),
    ))
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

    /// Lane 28: a map with holes and reused slots is rebuilt with exactly its ids, in the
    /// same iteration order, and the stand-ins for the holes are gone.
    #[test]
    fn with_ids_rebuilds_holes_and_reused_slots_exactly() {
        let mut live: SlotMap<ObjectId, u32> = SlotMap::with_key();
        let k: Vec<ObjectId> = (0..6).map(|i| live.insert(i)).collect();
        live.remove(k[1]);
        live.remove(k[4]);
        live.remove(k[2]);
        let again = live.insert(10); // slot 3 again
        live.remove(again);
        live.insert(11); // slot 3, reused twice
        live.insert(12); // slot 5, reused once
        let ids: Vec<(u64, u32)> = live.iter().map(|(k, &v)| (to_public(k), v)).collect();
        assert!(ids.iter().any(|&(id, _)| id >> 32 == 2), "{ids:?}");
        let mut work = 1000;
        let (map, keys) = with_ids::<ObjectId, u32>(ids.clone(), || 0, 100, &mut work).unwrap();
        let back: Vec<(u64, u32)> = map.iter().map(|(k, &v)| (to_public(k), v)).collect();
        assert_eq!(back, ids, "same ids, same order");
        assert_eq!(
            keys.iter().map(|&k| to_public(k)).collect::<Vec<_>>(),
            ids.iter().map(|&(id, _)| id).collect::<Vec<_>>()
        );
        assert_eq!(map.len(), ids.len(), "the stand-ins are gone");
        // Spent: one step per slot up to the largest, one per reuse.
        let reuses: u64 = ids.iter().map(|&(id, _)| id >> 32).sum();
        assert_eq!(1000 - work, 6 + reuses);
        // Given out of order, the keys still come back in the order given.
        let mut rev = ids.clone();
        rev.reverse();
        let (_, keys) = with_ids::<ObjectId, u32>(rev.clone(), || 0, 100, &mut 1000).unwrap();
        assert_eq!(
            keys.iter().map(|&k| to_public(k)).collect::<Vec<_>>(),
            rev.iter().map(|&(id, _)| id).collect::<Vec<_>>()
        );
        // Compact ids are what a fresh map hands out.
        let (map, _) = with_ids::<ObjectId, u32>(vec![(1, 1), (2, 2)], || 0, 10, &mut 10).unwrap();
        let mut fresh: SlotMap<ObjectId, u32> = SlotMap::with_key();
        fresh.insert(1);
        fresh.insert(2);
        assert_eq!(
            map.keys().collect::<Vec<_>>(),
            fresh.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn with_ids_refuses_what_a_live_map_could_not_hold() {
        let w = |items: Vec<(u64, u8)>, max: u64, work: u64| {
            let mut work = work;
            with_ids::<ObjectId, u8>(items, || 0, max, &mut work).map(|_| ())
        };
        assert_eq!(w(vec![(0, 0)], 10, 100), Err(IdError::OutOfRange(0)));
        assert_eq!(w(vec![(11, 0)], 10, 100), Err(IdError::OutOfRange(11)));
        assert_eq!(
            w(vec![(1 << 32, 0)], 10, 100),
            Err(IdError::OutOfRange(1 << 32))
        );
        assert_eq!(
            w(vec![(u64::MAX, 0)], u64::MAX, u64::MAX),
            Err(IdError::OutOfRange(u64::MAX))
        );
        assert_eq!(
            w(vec![(3, 0), ((1 << 32) | 3, 0)], 10, 100),
            Err(IdError::Twice((1 << 32) | 3))
        );
        // A slot reused a billion times is a billion steps: refused by the work left.
        assert_eq!(
            w(vec![((1_000_000_000u64 << 32) | 1, 0)], 10, 1 << 24),
            Err(IdError::TooMuchWork)
        );
        assert_eq!(w(vec![(9, 0)], 10, 8), Err(IdError::TooMuchWork));
        assert_eq!(w(vec![(9, 0)], 10, 9), Ok(()));
        assert_eq!(w(vec![], 10, 0), Ok(()));
    }
}
