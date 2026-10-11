//! Sawhorse geometry kernel.
//!
//! Pure Rust: no UI, no IO, no network. Compiled to `wasm32-unknown-unknown` and run in a
//! Web Worker in both the browser build and the Tauri webview (ADR-0001).
//!
//! All lengths are f64 metres, Z up. Units convert only at the edges.

#![warn(missing_debug_implementations)]

pub mod boolean;
pub mod tolerance;

pub use glam::{DVec2, DVec3};

/// Kernel crate version, reported to the UI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
