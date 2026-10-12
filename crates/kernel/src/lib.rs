//! Sawhorse geometry kernel.
//!
//! Pure Rust: no UI, no IO, no network. Compiled to `wasm32-unknown-unknown` and run in a
//! Web Worker in both the browser build and the Tauri webview (ADR-0001).
//!
//! All lengths are f64 metres, Z up. Units convert only at the edges.
//!
//! Two tiers:
//! - [`sketch::Sketch`]: a persistent planar graph whose closed circuits become regions.
//! - [`object::Object`]: a watertight half-edge solid ([`brep::Brep`]) born by extruding a
//!   region, changed by push/pull.
//!
//! Every change goes through [`model::Model::apply`]: clone, mutate, validate, commit.

#![warn(missing_debug_implementations)]

pub mod boolean;
pub mod brep;
pub mod cutlist;
pub mod document;
pub mod draw;
pub mod error;
pub mod faceops;
pub mod hash;
pub mod ids;
pub mod intersect;
pub mod looks;
pub mod mesh;
pub mod meshbuild;
pub mod model;
pub mod nesting;
pub mod nodes;
pub mod object;
pub mod offset;
pub mod plane;
pub mod push;
pub mod refbatch;
pub mod sever;
pub mod sketch;
pub mod snap;
pub mod snapshot;
pub mod stock;
pub mod sweep;
#[doc(hidden)]
pub mod testing;
pub mod tolerance;
pub mod transform;
pub mod validate;

pub use document::Document;
pub use error::{
    BuildError, DocumentError, FaceEditError, KernelError, OffsetError, PushError, SketchError,
    StockError, StructureError, SweepError, TransformError,
};
pub use glam::{DVec2, DVec3};
pub use model::{Model, ModelStats, Op, OpOutcome};

/// Kernel crate version, reported to the UI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
