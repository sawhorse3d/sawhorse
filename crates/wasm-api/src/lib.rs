//! wasm-bindgen surface of the Sawhorse kernel (filled in by the end-to-end spike).

use wasm_bindgen::prelude::*;

/// Kernel version string, for the UI's about box and the worker handshake.
#[wasm_bindgen]
pub fn kernel_version() -> String {
    sawhorse_kernel::VERSION.to_string()
}
