//! Helpers shared by the tests of the programs.
#![allow(dead_code)]

use silicon_asm::Assembly;
use silicon_machine::board::map;
use silicon_machine::{Bus, EngineKind, Machine};

/// Build a program, or stop with its errors laid out for reading.
pub fn build(name: &str) -> Assembly {
    match silicon_programs::build(name) {
        Ok(assembly) => assembly,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            panic!("`{name}` does not build:\n{}", lines.join("\n"));
        }
    }
}

/// A computer with the program in ROM, on the given engine.
pub fn boot(program: &Assembly, engine: EngineKind) -> Machine {
    let mut machine = Machine::new(&program.rom).expect("the program fits in ROM");
    machine.set_engine(engine);
    machine
}

/// Run until the program has finished `frames` frames; returns the cycles
/// that took.
///
/// # Panics
/// If that takes more than `limit` cycles.
pub fn run_frames(machine: &mut Machine, frames: u64, limit: u64) -> u64 {
    let target = machine.board().frames() + frames;
    let mut cycles = 0;
    while machine.board().frames() < target {
        assert!(cycles < limit, "no frame after {limit} cycles");
        assert!(!machine.halted(), "the program halted");
        machine.run(1);
        cycles += 1;
    }
    cycles
}

/// The address of a symbol of the program.
pub fn symbol(program: &Assembly, name: &str) -> u16 {
    let value = *program
        .symbols
        .get(name)
        .unwrap_or_else(|| panic!("the program has no symbol `{name}`"));
    u16::try_from(value).expect("an address")
}

/// Write a 16-bit value into the machine's memory, low byte first.
pub fn poke16(machine: &mut Machine, addr: u16, value: u16) {
    let [lo, hi] = value.to_le_bytes();
    machine.board_mut().write(addr, lo);
    machine.board_mut().write(addr + 1, hi);
}

/// The screen as text, one character per pixel: ` `, `.`, `+`, `#` from the
/// darkest shade to the brightest.
pub fn picture(framebuffer: &[u8]) -> String {
    let mut out = String::with_capacity((map::SCREEN_WIDTH + 1) * map::SCREEN_HEIGHT);
    for row in framebuffer.chunks(map::ROW_BYTES) {
        for x in 0..map::SCREEN_WIDTH {
            let shade = row[x / 4] >> (2 * (x % 4)) & 3;
            out.push([' ', '.', '+', '#'][shade as usize]);
        }
        out.push('\n');
    }
    out
}

/// Where the time goes: run until the program has finished `frames` more
/// frames and count the clock cycles spent in each of the given parts of
/// the program. A part is a label; it runs up to the next label of the list
/// (in address order). Returns the cycles of each part, in the order given,
/// and the total.
pub fn profile(
    machine: &mut Machine,
    program: &Assembly,
    parts: &[&str],
    frames: u64,
) -> (Vec<u64>, u64) {
    let mut starts: Vec<(u16, usize)> = parts
        .iter()
        .enumerate()
        .map(|(i, name)| (symbol(program, name), i))
        .collect();
    starts.sort();
    let mut cycles = vec![0u64; parts.len()];
    let mut total = 0;
    let target = machine.board().frames() + frames;
    while machine.board().frames() < target {
        // `pc` has already moved past the opcode being executed.
        let at = machine.state().pc.wrapping_sub(1);
        if let Some(&(_, part)) = starts.iter().rev().find(|(start, _)| *start <= at) {
            cycles[part] += 1;
        }
        total += 1;
        machine.run(1);
    }
    (cycles, total)
}
