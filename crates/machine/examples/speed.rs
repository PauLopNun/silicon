//! Rough speed of both engines on this machine, in clock cycles per second.
//!
//! ```sh
//! cargo run --release -p silicon-machine --example speed
//! ```
//!
//! This is a sanity check, not a benchmark suite (that arrives with the
//! optimised engines in M2): it answers "are we in the right ballpark for
//! the budgets in docs/ARCHITECTURE.md §7?".

use silicon_isa::{assemble, AluOp, Cond, Insn, Pair, Reg};
use silicon_machine::{Engine, FastCpu, GateCpu, Ram};
use std::time::Instant;

fn main() {
    use Insn::*;
    // An endless loop that mixes register work, 16-bit adds, memory reads
    // and branches, and never writes (so it cannot overwrite itself).
    let code = assemble(&[
        LdPairImm(Pair::X, 0x4000),
        LdAPtrInc(Pair::X), // 3: loop
        Alu(AluOp::Add, Reg::B),
        Mov(Reg::B, Reg::A),
        Inc(Reg::Yl),
        AddPair(Pair::Z, Pair::Y),
        Br(Cond::Nz, -8),
        Jmp(0x0003),
    ]);

    let mut fast = FastCpu::new();
    let mut ram = Ram::with_program(&code);
    let cycles = 200_000_000u64;
    let start = Instant::now();
    for _ in 0..cycles {
        fast.step_cycle(&mut ram);
    }
    let fast_rate = cycles as f64 / start.elapsed().as_secs_f64();
    println!("fast engine: {:>8.1} M cycles/s", fast_rate / 1e6);

    let mut gate = GateCpu::new();
    let mut ram = Ram::with_program(&code);
    let cycles = 1_000_000u64;
    let start = Instant::now();
    for _ in 0..cycles {
        gate.step_cycle(&mut ram);
    }
    let gate_rate = cycles as f64 / start.elapsed().as_secs_f64();
    let nands = gate.sim().netlist().stats().nands;
    println!(
        "gate engine: {:>8.1} k cycles/s  ({} NANDs, scalar reference evaluator)",
        gate_rate / 1e3,
        nands
    );

    // Same program, same number of cycles: the engines must agree.
    let mut check = FastCpu::new();
    let mut ram = Ram::with_program(&code);
    for _ in 0..cycles {
        check.step_cycle(&mut ram);
    }
    assert_eq!(Engine::state(&gate), *check.state(), "engines disagree");
    println!("state after {cycles} cycles matches on both engines");
}
