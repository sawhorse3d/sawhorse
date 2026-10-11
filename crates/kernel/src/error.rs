//! Typed kernel errors (ADR-0001). A refused operation leaves the model untouched.

use crate::boolean::BooleanError;
use crate::plane::PlaneError;
use crate::validate::TopologyError;
use sawhorse_tessellate::TessellateError;

/// Errors from the sketch tier. Only genuinely degenerate input is refused: intersecting,
/// touching and overlapping strokes are resolved by the insertion rules (see `sketch`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SketchError {
    #[error("no sketch #{0}")]
    NoSuchSketch(u64),
    #[error("no region #{0} in this sketch")]
    NoSuchRegion(u64),
    #[error("region #{0} is already a solid")]
    RegionConsumed(u64),
    #[error("that shape is too small (under 0.01 mm)")]
    Degenerate,
    #[error("a coordinate is not finite or is beyond 10 km")]
    OutOfBounds,
    #[error("region #{0} touches itself at a point; a solid made from it would not be manifold")]
    PinchedRegion(u64),
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
pub enum PushError {
    #[error("distance must be at least 0.01 mm")]
    ZeroDistance,
    #[error("distance is not finite or is beyond 10 km")]
    OutOfBounds,
    #[error("no object #{0}")]
    NoSuchObject(u64),
    #[error("no face #{0} on this object")]
    NoSuchFace(u32),
    #[error("could not build the solid: {0}")]
    Build(#[from] BuildError),
    /// The boolean path (push through, into or flush with another part) could not produce
    /// a valid solid; nothing changed.
    #[error("push/pull through the solid: {0}")]
    Boolean(#[from] BooleanError),
}

/// Errors from drawing on an object's face (split_face, imprint). Refusals of bad input;
/// the face is left as it was.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FaceEditError {
    #[error("no object #{0}")]
    NoSuchObject(u64),
    #[error("no face #{0} on this object")]
    NoSuchFace(u32),
    #[error("a path needs at least {0} points")]
    TooFewPoints(usize),
    #[error("a coordinate is not finite or is beyond 10 km")]
    OutOfBounds,
    #[error("a point is off the face's plane")]
    NotOnPlane,
    #[error("that shape is too small (under 0.01 mm)")]
    Degenerate,
    #[error("a cut must start and end on the face's boundary")]
    NotOnBoundary,
    #[error("the path must run strictly inside the face")]
    NotInside,
    #[error("the path crosses the face's boundary")]
    CrossesBoundary,
    #[error("the path crosses itself")]
    SelfIntersecting,
    #[error("that cut joins two different loops, so it does not split the face")]
    DoesNotSplit,
    #[error("could not rebuild the face: {0}")]
    Build(#[from] BuildError),
}

/// Errors from document-level commands (undo, redo, groups, tags, deleting objects).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DocumentError {
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
    #[error("undo and redo act on a document, not on a bare model")]
    NotAModelOp,
    #[error("no object #{0}")]
    NoSuchObject(u64),
    #[error("no group #{0}")]
    NoSuchGroup(u64),
    #[error("a group needs at least one object")]
    EmptyGroup,
    #[error("a name must be 1 to 64 characters")]
    BadName,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KernelError {
    #[error(transparent)]
    Sketch(#[from] SketchError),
    #[error(transparent)]
    Push(#[from] PushError),
    #[error(transparent)]
    FaceEdit(#[from] FaceEditError),
    #[error(transparent)]
    Document(#[from] DocumentError),
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
