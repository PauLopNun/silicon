//! Helpers shared by the tests of the compiler: compile a program, run it
//! on the computer, look at what it printed.
#![allow(dead_code)]

use silicon_asm::Assembly;
use silicon_machine::{EngineKind, Machine};
use std::collections::BTreeMap;

/// A few functions every test program can `#include "test.h"`: printing on
/// the console is how programs report results.
pub const TEST_H: &str = r#"
void putc(u8 c) { *(u8*)0x8804 = c; }

void print(u8* s) {
    while (*s) putc(*s++);
}

void print_u16(u16 n) {
    if (n >= 10) print_u16(n / 10);
    putc('0' + (u8)(n % 10));
}

void print_i16(i16 n) {
    if (n < 0) {
        putc('-');
        print_u16((u16)(0 - n));
    } else {
        print_u16((u16)n);
    }
}

// A number and a space.
void show(u16 n) { print_u16(n); putc(' '); }
void show_i(i16 n) { print_i16(n); putc(' '); }
"#;

/// The files of a test: the program as `main.c`, and `test.h`.
pub fn files(source: &str) -> BTreeMap<String, String> {
    [("main.c", source), ("test.h", TEST_H)]
        .into_iter()
        .map(|(name, text)| (name.to_string(), text.to_string()))
        .collect()
}

/// Compile and assemble, or stop with the errors laid out for reading.
pub fn build(source: &str) -> Assembly {
    match silicon_cc::build("main.c", &files(source)) {
        Ok(program) => program,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            let assembly = silicon_cc::compile("main.c", &files(source)).unwrap_or_default();
            panic!("the program does not build:\n{}\n{assembly}", lines.join("\n"));
        }
    }
}

/// The assembly generated for a program.
pub fn assembly(source: &str) -> String {
    silicon_cc::compile("main.c", &files(source)).expect("the program compiles")
}

/// A program that has run to its end.
pub struct Run {
    pub program: Assembly,
    pub machine: Machine,
    /// Clock cycles it took.
    pub cycles: u64,
}

impl Run {
    /// What the program printed.
    pub fn output(&self) -> String {
        String::from_utf8_lossy(self.machine.board().console()).into_owned()
    }

    /// The byte at a symbol of the program.
    pub fn byte(&self, symbol: &str) -> u8 {
        self.machine.board().peek(self.program.symbols[symbol] as u16)
    }

    /// The 16-bit value at a symbol of the program.
    pub fn word(&self, symbol: &str) -> u16 {
        let at = self.program.symbols[symbol] as u16;
        let board = self.machine.board();
        u16::from_le_bytes([board.peek(at), board.peek(at + 1)])
    }
}

/// Run a program until `main` returns.
pub fn run_on(source: &str, engine: EngineKind, limit: u64) -> Run {
    let program = build(source);
    let mut machine = Machine::new(&program.rom).expect("the program fits in ROM");
    machine.set_engine(engine);
    let cycles = machine.run_until_halt(limit);
    assert!(
        machine.halted(),
        "the program is still running after {limit} cycles; so far it printed {:?}",
        String::from_utf8_lossy(machine.board().console())
    );
    // `main` returned to the `hlt` right after the call to it, with the
    // stack as it found it.
    let state = machine.state();
    assert_eq!(state.sp, 0, "the stack is not balanced");
    Run {
        program,
        machine,
        cycles,
    }
}

pub fn run(source: &str) -> Run {
    run_on(source, EngineKind::Fast, 20_000_000)
}

/// What a program prints.
pub fn output(source: &str) -> String {
    run(source).output()
}

/// The errors of a program that must not compile, as `file:line:col: message`
/// without the word "error".
pub fn errors(source: &str) -> Vec<String> {
    let errors = silicon_cc::build("main.c", &files(source)).expect_err("the program should be rejected");
    errors
        .iter()
        .map(|e| format!("{}:{}:{}: {}", e.file, e.pos.line, e.pos.col, e.message))
        .collect()
}
