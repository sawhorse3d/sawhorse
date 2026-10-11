//! Typed kernel errors (ADR-0001). A refused operation leaves the model untouched.

use crate::boolean::BooleanError;
use crate::plane::PlaneError;
use crate::validate::TopologyError;
use sawhorse_tessellate::TessellateError;

/// Errors from the sticky (sketch) tier.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StickyError {
    #[error("no sketch #{0}")]
    NoSuchSketch(u32),
    #[error("no region #{0} in this sketch")]
    NoSuchRegion(u32),
    #[error("region #{0} is already a solid")]
    RegionConsumed(u32),
    #[error("that shape is too small (under 0.01 mm)")]
    Degenerate,
    #[error("a coordinate is not finite or is beyond 10 km")]
    OutOfBounds,
    #[error(
        "the new edges touch or cross existing edges; intersecting sketch edits arrive with the draw-tools lane"
    )]
    IntersectionUnsupported,
    #[error("that edit would change a region that is already a solid")]
    EditsConsumedRegion,
    #[error("bad sketch plane: {0}")]
    Plane(#[from] PlaneError),
}

/// Errors from building a B-rep out of polygons.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BuildError {
    #[error("face {face}: a loop needs at least 3 vertices")]
    LoopTooShort { face: usize },
    #[error("face {face}: vertex index out of range")]
    BadIndex { face: usize },
    #[error("face {face}: a loop repeats a vertex")]
    RepeatedVertex { face: usize },
    #[error("directed edge {from}->{to} appears twice (non-manifold or inconsistently oriented)")]
    DuplicateDirectedEdge { from: usize, to: usize },
    #[error("face {face} has zero area")]
    DegenerateFace { face: usize },
}

/// Errors from push/pull (which includes extruding a sketch region).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PushPullError {
    #[error("distance must be at least 0.01 mm")]
    ZeroDistance,
    #[error("distance is not finite or is beyond 10 km")]
    OutOfBounds,
    #[error("no object #{0}")]
    NoSuchObject(u32),
    #[error("no face #{0} on this object")]
    NoSuchFace(u32),
    #[error("push/pull of this face is not supported yet: {0}")]
    Unsupported(&'static str),
    #[error("pushing through the opposite face needs the boolean path (push/pull lane)")]
    ThroughOppositeFace,
    #[error("that distance would collapse some edges but not the whole solid")]
    WouldCollapseEdge,
    #[error("could not build the solid: {0}")]
    Build(#[from] BuildError),
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KernelError {
    #[error(transparent)]
    Sticky(#[from] StickyError),
    #[error(transparent)]
    PushPull(#[from] PushPullError),
    #[error(transparent)]
    Boolean(#[from] BooleanError),
    #[error(transparent)]
    Tessellate(#[from] TessellateError),
    /// The validator rejected the result of an operation. Always a kernel bug: the
    /// transaction was rolled back, and the op sequence belongs in tests/repro/.
    #[error("kernel bug, operation rolled back: {0}")]
    Topology(#[from] TopologyError),
}

impl KernelError {
    /// True for validator failures (bugs), false for refusals of bad input.
    pub fn is_bug(&self) -> bool {
        matches!(self, KernelError::Topology(_))
    }
}
