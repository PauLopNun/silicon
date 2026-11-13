//! # Silicon programs
//!
//! The programs of the computer live as source text in `programs/`. This
//! crate carries those sources (so that the browser has them too) and builds
//! them with the toolchain: nothing is assembled ahead of time, a program
//! becomes a ROM image when it is asked for.
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
        name: "stripes",
        title: "Stripes",
        about: "Diagonal stripes that scroll with the timer. Hold ← to reverse them.",
        main: "stripes.asm",
    },
    Program {
        name: "maze3d",
        title: "Maze",
        about:
            "The view from inside a maze, one ray per screen column. Hold ← or → to look around.",
        main: "maze3d/view.asm",
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
    "hello.asm",
    "lib/silicon.inc",
    "maze3d/render.asm",
    "maze3d/tables.inc",
    "maze3d/view.asm",
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
    silicon_asm::assemble(program.main, &Sources)
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
