//! Generational ids (slotmap keys). Stale ids fail lookups instead of aliasing new data.

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
}
