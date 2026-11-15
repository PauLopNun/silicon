//! # Silicon WebAssembly bindings
//!
//! The only crate the browser talks to. It runs inside a Web Worker and
//! exposes simulation state as pointers into WASM linear memory, so the worker
//! can view it with a typed array instead of copying it through JS objects.
//!
//! The toolchain is in here too: the programs travel as source text and are
//! assembled or compiled in the browser when they are chosen.

mod machine;
mod ram;

pub use machine::{Machine, ARCH_STATE_BYTES, GATE_FIELDS};
pub use ram::RamExhibit;

use wasm_bindgen::prelude::*;

/// Version of the simulation core, shown on the page.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The programs the computer can be given, in the order a menu should show
/// them: one per line, as name, title and description separated by tabs.
#[wasm_bindgen]
pub fn program_list() -> String {
    let lines: Vec<String> = silicon_programs::PROGRAMS
        .iter()
        .map(|p| format!("{}\t{}\t{}", p.name, p.title, p.about))
        .collect();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_list_has_three_fields_per_program() {
        let list = program_list();
        let lines: Vec<&str> = list.lines().collect();
        assert_eq!(lines.len(), silicon_programs::PROGRAMS.len());
        for line in &lines {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 3, "{line}");
            assert!(fields.iter().all(|f| !f.is_empty()), "{line}");
        }
        assert!(lines[0].starts_with("maze3d\tMaze\t"));
    }

    #[test]
    fn version_is_the_crate_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
