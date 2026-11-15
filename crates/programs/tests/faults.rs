//! Breaking gates on purpose: a real program on the real netlist goes wrong,
//! the way a chip with a stuck-at fault does, and mending the gates gives
//! back exactly the computer there was.

mod common;

use common::{boot, build};
use silicon_machine::{EngineKind, Machine};

/// What `hello` prints in a given number of cycles, and whether it halted.
fn run(machine: &mut Machine, cycles: u64) -> (String, bool) {
    machine.run_until_halt(cycles);
    (
        String::from_utf8_lossy(machine.board().console()).into_owned(),
        machine.halted(),
    )
}

/// The gates of a module of the CPU, by path.
fn gates_in(machine: &Machine, module: &str) -> Vec<usize> {
    let netlist = machine.gate().sim().netlist();
    (0..netlist.gates.len())
        .filter(|&g| netlist.gate_path(g).rsplit_once('/').unwrap().0 == module)
        .collect()
}

#[test]
fn a_stuck_gate_in_the_cpu_breaks_a_real_program() {
    let program = build("hello");
    let mut clean = boot(&program, EngineKind::Gate);
    let expected = run(&mut clean, 20_000);
    assert_eq!(expected, ("Hello from Silicon!\n".to_string(), true));

    // Every gate of the lowest full adder of the address unit, stuck at 0
    // and at 1 in turn. That adder adds 1 to pc for every byte fetched.
    let mut machine = boot(&program, EngineKind::Gate);
    let gates = gates_in(&machine, "cpu/agu/adder16/fa0");
    assert_eq!(gates.len(), 9);
    let mut broken = 0;
    for &gate in &gates {
        for level in [false, true] {
            machine.reset();
            machine.break_gate(gate, Some(level));
            if run(&mut machine, 20_000) != expected {
                broken += 1;
            }
            machine.break_gate(gate, None);
        }
    }
    // Most single faults in that adder change what the program does. (Some
    // cannot: a gate already at the level it is stuck at whenever it
    // matters.) At least half of them must show.
    assert!(broken >= 9, "only {broken} of 18 faults changed anything");

    // Mended, it is the same computer again, cycle for cycle.
    assert!(machine.faults().is_empty());
    machine.reset();
    assert_eq!(run(&mut machine, 20_000), expected);
    assert_eq!(machine.state(), clean.state());
}

#[test]
fn while_a_gate_is_broken_only_the_gate_engine_runs() {
    let program = build("hello");
    let mut machine = boot(&program, EngineKind::Fast);
    let gate = gates_in(&machine, "cpu/alu/adder8/fa0")[0];

    machine.break_gate(gate, Some(true));
    assert_eq!(machine.engine(), EngineKind::Gate);
    machine.set_engine(EngineKind::Fast);
    assert_eq!(
        machine.engine(),
        EngineKind::Gate,
        "the fast engine has no gates to break"
    );

    // It is the hardware that is broken: a reset does not mend it.
    machine.reset();
    assert_eq!(machine.faults(), [(gate, true)]);

    machine.break_gate(gate, None);
    machine.set_engine(EngineKind::Fast);
    assert_eq!(machine.engine(), EngineKind::Fast);
}

#[test]
fn a_stuck_flip_flop_of_the_program_counter_makes_the_cpu_lose_its_way() {
    let program = build("hello");
    let mut machine = boot(&program, EngineKind::Gate);
    let pc = gates_in(&machine, "cpu/pc");
    let netlist = machine.gate().sim().netlist();
    let flip_flops: Vec<usize> = pc
        .into_iter()
        .filter(|&g| netlist.gate_path(g).contains("/dff"))
        .collect();
    assert_eq!(flip_flops.len(), 16, "pc is sixteen flip-flops");
    // Bit 0 of pc stuck at 0: only even addresses can be fetched.
    machine.break_gate(flip_flops[0], Some(false));
    for _ in 0..200 {
        machine.run(1);
        assert_eq!(machine.state().pc & 1, 0);
    }
    assert_ne!(run(&mut machine, 20_000).0, "Hello from Silicon!\n");
}
