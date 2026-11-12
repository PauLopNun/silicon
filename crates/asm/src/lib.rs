//! # Silicon assembler
//!
//! Turns Silicon assembly into ROM images. Implemented in M3 (labels,
//! expressions, macros, data directives, includes, listings). What is fixed
//! now is how errors are reported everywhere in the toolchain: with a precise
//! source position, so the in-browser editor can underline them.

use std::fmt;

/// A position in a source file. Lines and columns are 1-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

/// A toolchain error with the place it happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub pos: Pos,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: error: {}",
            self.file, self.pos.line, self.pos.col, self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_print_file_line_and_column() {
        let d = Diagnostic {
            file: "maze3d.asm".into(),
            pos: Pos { line: 12, col: 5 },
            message: "unknown instruction `lod`".into(),
        };
        assert_eq!(
            d.to_string(),
            "maze3d.asm:12:5: error: unknown instruction `lod`"
        );
    }
}
