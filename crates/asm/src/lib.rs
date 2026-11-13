//! # Silicon assembler
//!
//! Turns Silicon assembly into a ROM image. The instructions are the ones of
//! `docs/ISA.md`, written exactly as that document writes them; every byte
//! comes out of `silicon_isa::Insn::encode`, so the assembler cannot disagree
//! with the hardware about the encoding.
//!
//! ```
//! let program = silicon_asm::assemble_source("count.asm", "
//!         ld   b, 10
//! loop:   dec  b
//!         bnz  loop
//!         hlt
//! ").unwrap();
//! assert_eq!(program.rom, [0xD1, 10, 0xC9, 0xF1, 0xFD, 0x00]);
//! assert_eq!(program.symbols["loop"], 2);
//! ```
//!
//! ## The language
//!
//! One statement per line; `;` starts a comment.
//!
//! ```text
//! name:                 a label: the address of what follows
//! .name:                a local label, private to the label above it
//!                       (from elsewhere it is `label.name`; written that
//!                       way with a colon, it defines exactly that name)
//! name = expr           a constant
//!
//! .rom                  what follows goes in ROM, after what is already there
//! .ram                  what follows names RAM, from 0x9000 up
//! .org   addr           continue at this address
//! .byte  1, 'c', "text" bytes
//! .word  expr, …        16-bit values, low byte first
//! .fill  count, value   the same byte many times (value defaults to 0)
//! .res   count          leave room without storing anything: RAM variables
//! .align n              continue at the next multiple of n
//! .include "file"       assemble another file here
//! ```
//!
//! Expressions use C operators and precedence (`| ^ & << >> + - * / %`, unary
//! `-` and `~`, parentheses), `lo(e)` and `hi(e)` for the two bytes of a
//! 16-bit value, and `$` for the address of the current statement. Numbers
//! are decimal, `0x` hexadecimal or `0b` binary, with `_` allowed between
//! digits; `'c'` is a character.
//!
//! Besides the machine instructions there are two conveniences:
//!
//! * `nop`, which is `mov a, a`;
//! * `jz`, `jnz`, `jc`, `jnc`, `jn`, `jp`, `jlt`, `jge`: conditional jumps
//!   to any address. Each becomes the branch of the same name when the
//!   target is within its reach, and the opposite branch around a `jmp`
//!   (5 bytes) when it is not.
//!
//! A program is a ROM image starting at address 0, so only addresses below
//! `0x8000` can be given contents. Variables are names for places in RAM:
//!
//! ```text
//!         .ram
//! score:  .res 2
//!         .rom
//! ```
//!
//! ROM and RAM each keep their own "next free address", so any file can add
//! to both without knowing what the others used.
//!
//! Errors are [`Diagnostic`]s: a file, a line, a column and a sentence. The
//! whole toolchain reports errors this way, so an editor can underline them.

mod assembler;
pub use assembler::resolve as resolve_include;
mod expr;
mod lex;
mod listing;
mod parse;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;

/// Size of the ROM (`docs/ISA.md` §4): the part of memory a program image
/// can fill.
pub const ROM_SIZE: usize = 0x8000;

/// Where RAM begins (`docs/ISA.md` §4): the address `.ram` starts at.
pub const RAM_START: u16 = 0x9000;

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

/// Where source files come from: a directory, a map in memory, the browser.
pub trait Files {
    /// The contents of the file at `path` (always `/`-separated), or why it
    /// cannot be had.
    fn read(&self, path: &str) -> Result<String, String>;
}

impl<S: std::hash::BuildHasher> Files for HashMap<String, String, S> {
    fn read(&self, path: &str) -> Result<String, String> {
        self.get(path).cloned().ok_or("no such file".into())
    }
}

impl Files for BTreeMap<String, String> {
    fn read(&self, path: &str) -> Result<String, String> {
        self.get(path).cloned().ok_or("no such file".into())
    }
}

/// A list of `(path, contents)` pairs.
impl<const N: usize> Files for [(&str, &str); N] {
    fn read(&self, path: &str) -> Result<String, String> {
        self.iter()
            .find(|(name, _)| *name == path)
            .map(|(_, text)| text.to_string())
            .ok_or("no such file".into())
    }
}

/// Files below a directory of the file system.
pub struct Dir(pub PathBuf);

impl Files for Dir {
    fn read(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(self.0.join(path)).map_err(|e| e.to_string())
    }
}

/// One line of source and what it turned into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLine {
    /// Index into [`Assembly::files`].
    pub file: usize,
    /// 1-based line number in that file.
    pub line: u32,
    pub text: String,
    /// Where the line's statement sits; `None` if it has no address (a
    /// blank line, a comment, a constant).
    pub addr: Option<u16>,
    /// How many bytes of the ROM, starting at `addr`, come from this line.
    pub len: u16,
}

/// A program, assembled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assembly {
    /// The ROM image: element 0 is address 0, and it ends with the last byte
    /// the program defines. Gaps left by `.org` and `.align` are 0.
    pub rom: Vec<u8>,
    /// Every label and constant with its value.
    pub symbols: BTreeMap<String, i64>,
    /// Names of the source files, for [`SourceLine::file`].
    pub files: Vec<String>,
    /// Every source line in the order it was assembled, included files in
    /// place: the map between addresses and source text.
    pub lines: Vec<SourceLine>,
}

/// Assemble the file `main`, reading it and anything it includes from
/// `files`. On failure, every error found is returned, in source order.
pub fn assemble(main: &str, files: &dyn Files) -> Result<Assembly, Vec<Diagnostic>> {
    assembler::Assembler::new(files).run(main)
}

/// Assemble one piece of text that includes no other file. `name` is only
/// used in error messages.
pub fn assemble_source(name: &str, source: &str) -> Result<Assembly, Vec<Diagnostic>> {
    assemble(name, &[(name, source)])
}

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

    #[test]
    fn files_can_be_maps_or_lists() {
        let list = [("a.asm", "hlt")];
        assert_eq!(list.read("a.asm"), Ok("hlt".into()));
        assert_eq!(list.read("b.asm"), Err("no such file".into()));
        let map: BTreeMap<String, String> = [("a.asm".to_string(), "ret".to_string())].into();
        assert_eq!(map.read("a.asm"), Ok("ret".into()));
        let missing = Dir(PathBuf::from("/nonexistent")).read("a.asm");
        assert!(missing.is_err());
    }
}
