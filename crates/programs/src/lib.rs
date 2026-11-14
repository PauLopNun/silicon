//! # Silicon programs
//!
//! The programs of the computer live as source text in `programs/`, in
//! assembly (`.asm`) or in the C-like language of `docs/LANGUAGE.md` (`.c`).
//! This crate carries those sources (so that the browser has them too) and
//! builds them with the toolchain: nothing is built ahead of time, a
//! program becomes a ROM image when it is asked for.
//!
//! ```
//! let hello = silicon_programs::build("hello").unwrap();
//! assert_eq!(hello.rom[0], 0x19); // ld x, message
//! ```

pub mod tables;

use silicon_asm::{Assembly, Diagnostic, Files, Pos};

/// A program that can be loaded into the computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program {
    /// Short name, used in URLs and file names.
    pub name: &'static str,
    /// Name shown to people.
    pub title: &'static str,
    /// One sentence about what it does and how to use it.
    pub about: &'static str,
    /// Path of its main source file inside `programs/`.
    pub main: &'static str,
}

/// Every program, in the order a menu should show them.
pub const PROGRAMS: &[Program] = &[
    Program {
        name: "maze3d",
        title: "Maze",
        about: "Find the way out of the maze: ↑ ↓ walk, ← → turn. The map in the corner \
                shows you (the bright dot) and the way out (the dimmer one).",
        main: "maze3d/main.c",
    },
    Program {
        name: "bounce",
        title: "Bounce",
        about: "Five balls bouncing around the screen. Hold A and they leave trails.",
        main: "bounce.c",
    },
    Program {
        name: "stripes",
        title: "Stripes",
        about: "Diagonal stripes that scroll with the timer. Hold ← to reverse them.",
        main: "stripes.asm",
    },
    Program {
        name: "primes",
        title: "Primes",
        about: "Writes the prime numbers below 1000 on the console and stops.",
        main: "primes.c",
    },
    Program {
        name: "hello",
        title: "Hello",
        about: "Writes a line on the console and stops.",
        main: "hello.asm",
    },
];

macro_rules! sources {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_str!(concat!("../../../programs/", $path)))),*]
    };
}

/// Every file under `programs/`, by its path there. (A test checks that
/// none is missing.)
pub const FILES: &[(&str, &str)] = sources![
    "bounce.c",
    "hello.asm",
    "lib/console.h",
    "lib/silicon.h",
    "lib/silicon.inc",
    "maze3d/main.c",
    "maze3d/render.asm",
    "maze3d/tables.inc",
    "maze3d/view.asm",
    "maze3d/walk.h",
    "primes.c",
    "stripes.asm",
];

/// The files of `programs/` as the toolchain reads them.
pub struct Sources;

impl Files for Sources {
    fn read(&self, path: &str) -> Result<String, String> {
        FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, text)| text.to_string())
            .ok_or_else(|| "no such file".to_string())
    }
}

/// The program called `name`, if there is one.
pub fn find(name: &str) -> Option<&'static Program> {
    PROGRAMS.iter().find(|p| p.name == name)
}

/// Build a program from its source into a ROM image.
pub fn build(name: &str) -> Result<Assembly, Vec<Diagnostic>> {
    let Some(program) = find(name) else {
        return Err(vec![Diagnostic {
            file: name.to_string(),
            pos: Pos { line: 1, col: 1 },
            message: format!("there is no program called `{name}`"),
        }]);
    };
    build_file(program.main)
}

/// Build the file `main` of `programs/` into a ROM image: compiled if it is
/// C (`.c`), assembled otherwise. This also reaches the files that are not
/// programs of the menu, like the renderer of maze3d on its own
/// (`maze3d/view.asm`).
pub fn build_file(main: &str) -> Result<Assembly, Vec<Diagnostic>> {
    if main.ends_with(".c") {
        silicon_cc::build(main, &Sources)
    } else {
        silicon_asm::assemble(main, &Sources)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_programs_are_an_error_not_a_panic() {
        let errors = build("nothing").unwrap_err();
        assert_eq!(errors[0].message, "there is no program called `nothing`");
        assert!(find("stripes").is_some());
    }

    #[test]
    fn names_are_unique_and_usable_in_a_url() {
        for (i, p) in PROGRAMS.iter().enumerate() {
            assert!(p
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
            assert!(PROGRAMS[..i].iter().all(|q| q.name != p.name), "{}", p.name);
            assert!(Sources.read(p.main).is_ok(), "{} has no source", p.name);
        }
    }
}
