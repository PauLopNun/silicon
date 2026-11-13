//! # Silicon C compiler
//!
//! A small language with the look of C, compiled to Silicon assembly.
//! `docs/LANGUAGE.md` is its manual; in one breath: the types are `u8`,
//! `i8`, `u16`, `i16`, pointers and arrays of those; there are functions,
//! `if`, `while`, `do`, `for`, the operators of C, and `asm { … }` for
//! the places where only assembly will do.
//!
//! ```
//! let source = "
//!     u8 total;
//!     void main() {
//!         for (u8 i = 1; i <= 10; i++) total += i;
//!     }
//! ";
//! let program = silicon_cc::build("sum.c", &[("sum.c", source)]).unwrap();
//! assert_eq!(program.symbols["_total"], 0x9000);
//! ```
//!
//! The compiler is a pipeline of four small steps, one module each:
//!
//! | Step | Module | From → to |
//! |---|---|---|
//! | reading | `lex` | text → tokens, following `#include` |
//! | grammar | `parse` | tokens → a tree of what was written |
//! | meaning | `sema` | that tree → a typed tree: names resolved, constants computed |
//! | code | `gen` | the typed tree → assembly text |
//!
//! and then the assembler takes over. Errors are the assembler's
//! [`Diagnostic`]s: a file, a line, a column and a sentence.

mod ast;
mod gen;
mod ir;
mod lex;
mod parse;
mod runtime;
mod sema;

pub use silicon_asm::{Assembly, Diagnostic, Files, Pos};

/// Compile the program whose main file is `main` into assembly text.
pub fn compile(main: &str, files: &dyn Files) -> Result<String, Vec<Diagnostic>> {
    let (tokens, names) = lex::Lexer::new(files).run(main).map_err(|e| vec![e])?;
    let items = parse::Parser::new(&tokens).program().map_err(|(loc, message)| {
        vec![Diagnostic {
            file: names[loc.file].clone(),
            pos: Pos {
                line: loc.line,
                col: loc.col,
            },
            message,
        }]
    })?;
    let program = sema::Sema::run(&items, &names)?;
    gen::Gen::run(&program, &names)
}

/// The name the generated assembly goes by: errors in it that cannot be
/// traced to the source are reported there.
pub fn assembly_name(main: &str) -> String {
    format!("{main}.s")
}

/// Compile and assemble: from source files to a ROM image.
///
/// Assembly files that the program includes (from an `asm` block) are read
/// from `files` like everything else, relative to the main file.
pub fn build(main: &str, files: &dyn Files) -> Result<Assembly, Vec<Diagnostic>> {
    struct WithGenerated<'a> {
        name: &'a str,
        text: &'a str,
        files: &'a dyn Files,
    }
    impl Files for WithGenerated<'_> {
        fn read(&self, path: &str) -> Result<String, String> {
            if path == self.name {
                Ok(self.text.to_string())
            } else {
                self.files.read(path)
            }
        }
    }
    let text = compile(main, files)?;
    let name = assembly_name(main);
    let all = WithGenerated {
        name: &name,
        text: &text,
        files,
    };
    silicon_asm::assemble(&name, &all)
}
