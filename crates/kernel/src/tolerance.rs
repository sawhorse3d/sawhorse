//! Every geometric tolerance in the kernel, named, in one place (ADR-0001).
//!
//! All lengths are f64 metres. Nothing else in the kernel may hard-code an epsilon;
//! add a constant here with a comment saying what it decides.

/// Numerical noise floor for lengths (1 nm). Differences below this are rounding.
pub const LENGTH_EPS: f64 = 1e-9;

/// Two points closer than this are the same point (0.01 mm). Drives sketch endpoint
/// merging, T-junction detection and the smallest edge the kernel will create.
pub const POINT_MERGE: f64 = 1e-5;

/// The sketch tier's merge distance (lane 22): POINT_MERGE plus the noise floor. Every
/// decision the sketch makes (merge two points, split an edge at a point, refuse a short
/// segment, the invariants `Sketch::check` holds) uses it, so whatever a sketch keeps is
/// longer, or clears another edge, by more than POINT_MERGE plus rounding. A region pushed
/// into a solid is measured again by the solid tier, in world coordinates and with the
/// plain POINT_MERGE (zero-length edges, a vertex on a face it does not bound); with one
/// shared number, a sketch edge a rounding error longer than POINT_MERGE in the sketch's
/// frame could measure a rounding error shorter in the world (lane 22's property tests
/// found edges 1.6e-17 m over the merge distance that the solid called zero-length, and
/// holes 1e-17 m clear of their outer ring that it called touching). The margin is far
/// above the rounding of any coordinate within MAX_COORD (about 2e-12 m).
pub const SKETCH_MERGE: f64 = POINT_MERGE + LENGTH_EPS;

/// How far a curve's point may sit off the circle its parameters describe (lane 23,
/// ADR-0013): four merge distances (40 micrometres), plus one part in 1e9 of the circle's
/// size for rounding. A circle's points are where the tool put them, except that inserting
/// them may merge one into a point already there (up to the merge distance each time, and
/// a repair can merge again); anything further off is a curve whose parameters went stale.
pub const CURVE_FIT: f64 = 4.0 * SKETCH_MERGE;

/// Smallest area a face or region may have (POINT_MERGE squared).
pub const AREA_EPS: f64 = POINT_MERGE * POINT_MERGE;

/// Largest distance a kernel-made face's vertices may sit off its stored plane (1 µm).
pub const PLANARITY: f64 = 1e-6;

/// Wider planarity for imported geometry (0.1 mm): SKP/DXF/STL files are rarely exact.
pub const IMPORT_PLANARITY: f64 = 1e-4;

/// |sin(angle)| below which two unit directions count as parallel.
pub const PARALLEL_EPS: f64 = 1e-9;

/// Shortest push/pull or extrude distance (same as the merge distance: anything
/// shorter would create edges the kernel then refuses).
pub const MIN_EXTRUDE: f64 = POINT_MERGE;

/// Largest coordinate magnitude accepted from outside (10 km). Keeps POINT_MERGE
/// meaningful: at 1e4 m an f64 still resolves ~2e-12 m.
pub const MAX_COORD: f64 = 1e4;

/// Plane snapping before a boolean (lane 4, `crate::snap`) leaves a vertex alone when it is
/// within this of every plane around it (1 pm): that is rounding, and re-solving it would
/// only trade one rounding for another (and break exact constructions, such as the push
/// prism built on a face's own vertices). Deviations above it, up to POINT_MERGE, are
/// drift and get snapped.
pub const SNAP_FLOOR: f64 = 1e-12;

/// Quantum for the deterministic model hash (1 nm). Positions are rounded to this
/// before hashing so the hash is stable across platforms and builds.
pub const HASH_QUANTUM: f64 = 1e-9;

/// True when `a` and `b` are the same length within POINT_MERGE.
#[inline]
pub fn same_length(a: f64, b: f64) -> bool {
    (a - b).abs() <= POINT_MERGE
}

/// True when every component is finite and within MAX_COORD.
#[inline]
pub fn in_bounds(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_COORD
}
