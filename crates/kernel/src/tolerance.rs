//! Every geometric tolerance in the kernel, named, in one place (ADR-0001).
//!
//! All lengths are f64 metres. Nothing else in the kernel may hard-code an epsilon;
//! add a constant here with a comment saying what it decides.

/// Numerical noise floor for lengths (1 nm). Differences below this are rounding.
pub const LENGTH_EPS: f64 = 1e-9;

/// Two points closer than this are the same point (0.01 mm). Drives sketch endpoint
/// merging, T-junction detection and the smallest edge the kernel will create.
pub const POINT_MERGE: f64 = 1e-5;

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
