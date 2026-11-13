//! # Silicon WebAssembly bindings
//!
//! The only crate the browser talks to. It runs inside a Web Worker and
//! exposes simulation state as pointers into WASM linear memory, so the worker
//! can view it with a typed array instead of copying it through JS objects.

mod machine;

pub use machine::{Machine, ARCH_STATE_BYTES, GATE_FIELDS};

use wasm_bindgen::prelude::*;

/// Version of the simulation core, shown on the page.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_the_crate_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
