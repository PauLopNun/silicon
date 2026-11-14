//! Compile a program from the command line and print its assembly:
//!
//! ```text
//! cargo run -p silicon-cc --example cc -- programs maze3d/main.c
//! ```
//!
//! The first argument is the directory files are read from (what
//! `#include` and `.include` paths are relative to), the second the main
//! file inside it. With `--rom` the program is assembled too and the size
//! of its ROM image is printed instead.

use silicon_asm::{Diagnostic, Dir};
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = args.iter().any(|a| a == "--rom");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [root, main] = paths[..] else {
        eprintln!("usage: cc [--rom] <directory> <main file>");
        exit(2);
    };
    let files = Dir(root.into());
    let fail = |errors: Vec<Diagnostic>| -> ! {
        for e in &errors {
            eprintln!("{e}");
        }
        exit(1);
    };
    if rom {
        match silicon_cc::build(main, &files) {
            Ok(program) => println!("{} bytes of ROM", program.rom.len()),
            Err(errors) => fail(errors),
        }
    } else {
        match silicon_cc::compile(main, &files) {
            Ok(assembly) => print!("{assembly}"),
            Err(errors) => fail(errors),
        }
    }
}
