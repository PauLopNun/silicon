//! Every program of the menu, on both engines at once, compared clock cycle
//! by clock cycle: the registers, the step inside the instruction, and
//! every read and write on the bus (`machine::lockstep`). The other tests
//! compare what the engines put on the screen; this one cannot be satisfied
//! by two different roads to the same picture.

mod common;

use silicon_machine::{lockstep, Board, Engine, FastCpu, GateCpu};
use silicon_programs::PROGRAMS;

/// Clock cycles each program is followed for: several frames of the game,
/// its first maze dug; all of the programs that stop.
const CYCLES: u64 = 600_000;
/// How often the keys change.
const SLICE: u64 = 15_000;

#[test]
fn both_engines_run_every_program_the_same_cycle_by_cycle() {
    let mut gate = GateCpu::new();
    for program in PROGRAMS {
        let rom = common::build(program.name).rom;
        let mut fast = FastCpu::new();
        Engine::reset(&mut gate);
        let mut gate_board = Board::new(&rom).expect("the program fits in ROM");
        let mut fast_board = gate_board.clone();
        // Keys that come and go while it runs: xorshift, the same every time.
        let mut keys = 0x9e37_79b9_7f4a_7c15u64;
        for slice in 0..CYCLES / SLICE {
            keys ^= keys << 13;
            keys ^= keys >> 7;
            keys ^= keys << 17;
            // Nothing held for the first slices, then any combination.
            let held = if slice < 4 { 0 } else { keys as u8 };
            gate_board.set_keys(held);
            fast_board.set_keys(held);
            if let Err(divergence) = lockstep::run(
                &mut gate,
                &mut gate_board,
                &mut fast,
                &mut fast_board,
                SLICE,
            ) {
                panic!("{}, after slice {slice}:\n{divergence}", program.name);
            }
        }
        assert!(
            gate_board == fast_board,
            "{}: the boards differ",
            program.name
        );
        assert_eq!(gate_board.cycles(), CYCLES, "{}", program.name);
        // It was the program that ran, not zeroes: each one has shown
        // something, on the screen or on the console.
        let shown = gate_board.frames() > 0 || !gate_board.console().is_empty();
        assert!(shown, "{} did nothing", program.name);
    }
}
