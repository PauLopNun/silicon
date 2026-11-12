//! # Silicon C-like compiler
//!
//! A deliberately small language (`u8`/`i8`/`u16`/`i16`, arrays, pointers,
//! functions, `if`/`while`/`for`, inline asm) that compiles to Silicon
//! assembly. Implemented in M3; it reports errors with the same
//! [`Diagnostic`] type as the assembler.

pub use silicon_asm::{Diagnostic, Pos};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_the_assembler_diagnostic_type() {
        let d = Diagnostic {
            file: "a.c".into(),
            pos: Pos { line: 1, col: 1 },
            message: "x".into(),
        };
        assert_eq!(d.to_string(), "a.c:1:1: error: x");
    }
}
