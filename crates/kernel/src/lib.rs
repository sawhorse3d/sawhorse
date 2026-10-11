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
pub mod error;
pub mod hash;
pub mod ids;
pub mod mesh;
pub mod model;
pub mod object;
pub mod plane;
pub mod sketch;
#[doc(hidden)]
pub mod testing;
pub mod tolerance;
pub mod validate;

pub use error::{BuildError, KernelError, PushPullError, StickyError};
pub use glam::{DVec2, DVec3};
pub use model::{Model, ModelStats, Op, OpOutcome};

/// Kernel crate version, reported to the UI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
