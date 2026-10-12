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
    #[error("every edge of that region also bounds another region; nothing to erase")]
    NothingToErase,
    #[error("a point is off the sketch's plane")]
    OffPlane,
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
    NoSuchFace(u64),
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
    NoSuchFace(u64),
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
    #[error("there is no edge there")]
    NoSuchEdge,
    /// Erasing an edge or face of a solid is allowed only where it joins two coplanar
    /// faces (a line drawn on a face, an imprint): anything else would open the solid.
    #[error(
        "erasing that would open the solid (only lines drawn on a face can be erased; delete the whole solid instead)"
    )]
    WouldOpenSolid,
    #[error("erasing that would leave a face touching itself at a point")]
    WouldPinch,
    /// A stroke on a face ends inside it. Faces of a solid cannot carry loose lines; the
    /// draw tools keep such a stroke as a preview until it reaches an edge or closes.
    #[error("a line on a face must start and end on its edges, or close into a loop")]
    Dangling,
    #[error("could not rebuild the face: {0}")]
    Build(#[from] BuildError),
    /// The new lines would come within 0.01 mm of another face of the solid (on a sliver
    /// thinner than that, say), so the solid would touch itself.
    #[error("that would put a corner or line within 0.01 mm of another face of the solid")]
    TooClose,
}

/// Errors from moving, rotating, scaling, copying and arraying (lane 4). Refusals of bad
/// input or of a result that would not be a valid solid; nothing changed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TransformError {
    #[error("nothing is selected to transform")]
    NothingSelected,
    #[error("no object #{0}")]
    NoSuchObject(u64),
    #[error("the transform is not finite or is beyond 10 km")]
    BadTransform,
    #[error("that would flatten the solid (a scale of zero)")]
    Singular,
    #[error("that would move geometry beyond 10 km")]
    OutOfBounds,
    #[error("an array needs 1 to {max} copies", max = crate::model::MAX_ARRAY)]
    BadCount,
    #[error("there is no point of the solid there")]
    NoSuchVertex,
    #[error("nothing to move")]
    NothingToMove,
    /// Moving part of a solid would bend a face out of its plane (autofold is off).
    #[error("that would bend face #{face} out of its plane (hold Alt to fold it)")]
    WouldBend { face: u64 },
    #[error("that would make the solid pass through itself")]
    SelfIntersects,
    #[error("that would turn the solid inside out")]
    InsideOut,
    #[error("that would shrink an edge to nothing")]
    CollapsesEdge,
    #[error("that would bend a face out of its plane")]
    NotPlanar,
    #[error("that would fold the solid onto itself")]
    FoldsOntoItself,
    #[error("that would not leave a valid solid ({0})")]
    InvalidResult(String),
}

/// Errors from Follow Me (lane 4). Nothing changed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SweepError {
    #[error("a path point is not finite or is beyond 10 km")]
    OutOfBounds,
    #[error("the path needs at least one segment (or three corners to close)")]
    PathTooShort,
    #[error("the path doubles back on itself")]
    PathFoldsBack,
    #[error("the profile lies along the path; it must face along the path's first segment")]
    ProfileAlongPath,
    #[error("the sweep would have a flat or zero-size wall")]
    Degenerate,
    #[error("the path turns too tightly for this profile: the sweep would pass through itself")]
    SelfIntersects,
    #[error("the sweep would cut the whole solid away")]
    RemovesEverything,
    #[error("that region cannot be swept: {0}")]
    BadProfile(String),
}

/// Errors from the Offset tool (lane 4). Nothing changed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum OffsetError {
    #[error("offset distance must be at least 0.01 mm")]
    ZeroDistance,
    #[error("a distance or point is not finite or is beyond 10 km")]
    OutOfBounds,
    #[error("the offset shrinks the shape to nothing; try a smaller distance")]
    Vanishes,
    #[error("the edges are not all on one plane")]
    NotPlanar,
    #[error("an offset needs at least two points")]
    TooFewPoints,
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
    #[error("a batch needs at least one op")]
    EmptyBatch,
    #[error("a guide or dimension needs finite points (and a direction or two distinct points)")]
    BadAnnotation,
    #[error("no guide or dimension #{0}")]
    NoSuchAnnotation(u64),
    #[error("there are no guides to delete")]
    NoGuides,
    #[error("undo, redo and batches cannot be nested inside a batch")]
    NotInBatch,
    #[error("the import data is invalid: {0}")]
    BadImport(String),
    #[error("the file has nothing Sawhorse can import")]
    NothingImported,
    #[error("no imported mesh #{0}")]
    NoSuchMesh(u64),
    #[error("no material named {0:?}")]
    NoSuchMaterial(String),
    #[error("the file cannot be opened: {0}")]
    BadSnapshot(String),
}

/// Errors from groups, components, the editing context, tags and scenes (lane 5). Nothing
/// changed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StructureError {
    #[error("no group or component #{0}")]
    NoSuchNode(u64),
    #[error("no component definition #{0}")]
    NoSuchDefinition(u64),
    /// Only what is directly inside the open group or component can be edited.
    #[error(
        "that is inside a group or component that is not open: double-click it to edit inside it"
    )]
    OutsideContext,
    #[error("that is locked: unlock it first")]
    Locked,
    #[error("you are already editing the whole model")]
    NothingToLeave,
    #[error("select at least one solid, group or component")]
    NothingSelected,
    #[error("that is not a component")]
    NotAnInstance,
    #[error("this component is already unique: it is its definition's only copy")]
    AlreadyUnique,
    #[error("you are editing inside it: leave it first (Esc)")]
    OnContextPath,
    #[error("a tag named {0:?} already exists")]
    TagExists(String),
    #[error("no tag named {0:?}")]
    NoSuchTag(String),
    #[error("\"Untagged\" is the default; pick another name for a tag")]
    ReservedTag,
    #[error("a scene named {0:?} already exists")]
    SceneExists(String),
    #[error("no scene named {0:?}")]
    NoSuchScene(String),
    #[error("a scene needs a finite camera")]
    BadScene,
    #[error("a component cannot contain itself")]
    Cycle,
    /// An edit inside a component would leave one of its other copies an invalid solid
    /// (a copy scaled so that the change folds it, or moved past 10 km).
    #[error("that would leave another copy of this component invalid ({0})")]
    InstanceInvalid(String),
    #[error("the import's groups and components do not fit together: {0}")]
    BadStructure(String),
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
    Transform(#[from] TransformError),
    #[error(transparent)]
    Offset(#[from] OffsetError),
    #[error(transparent)]
    Sweep(#[from] SweepError),
    #[error(transparent)]
    Boolean(#[from] BooleanError),
    #[error(transparent)]
    Structure(#[from] StructureError),
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
