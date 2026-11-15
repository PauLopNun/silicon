//! The whole computer: programs that use the devices, on both engines, and
//! engine switching while they run.

mod common;

use common::Rng;
use silicon_isa::{assemble, AluOp, Cond, Insn, Pair, Reg};
use silicon_machine::board::{key, map};
use silicon_machine::{lockstep, Board, Engine, EngineKind, FastCpu, GateCpu, Machine};
use Insn::*;

/// Prints "OK", paints the first screen row, declares a frame, reads the
/// keys and pushes them on the stack.
fn demo_program() -> Vec<u8> {
    assemble(&[
        LdImm(Reg::A, b'O'),
        StAbsA(map::OUT),
        LdImm(Reg::A, b'K'),
        StAbsA(map::OUT),
        LdPairImm(Pair::X, map::FB_START),
        LdImm(Reg::B, map::ROW_BYTES as u8),
        // loop: four pixels of shades 0, 1, 2, 3 per byte
        LdImm(Reg::A, 0b1110_0100),
        StPtrIncA(Pair::X),
        Dec(Reg::B),
        Br(Cond::Nz, -6),
        StAbsA(map::FRAME),
        LdAAbs(map::KEYS),
        Mov(Reg::B, Reg::A),
        Push(Pair::Ba),
        Hlt,
    ])
}

#[test]
fn a_program_drives_every_device_on_both_engines() {
    let mut machines = Vec::new();
    for kind in [EngineKind::Fast, EngineKind::Gate] {
        let mut machine = Machine::new(&demo_program()).unwrap();
        machine.set_engine(kind);
        machine.board_mut().set_keys(key::RIGHT | key::A);
        let cycles = machine.run_until_halt(10_000);
        assert!(machine.halted(), "{kind:?} did not halt");
        assert_eq!(machine.engine(), kind);

        let board = machine.board();
        assert_eq!(board.console(), b"OK");
        assert_eq!(board.frames(), 1);
        assert_eq!(board.cycles(), cycles);
        for x in 0..map::SCREEN_WIDTH {
            assert_eq!(board.pixel(x, 0) as usize, x % 4, "pixel {x} of row 0");
            assert_eq!(board.pixel(x, 1), 0, "row 1 is untouched");
        }
        let state = machine.state();
        assert_eq!(state.regs[1], key::RIGHT | key::A);
        assert_eq!(state.sp, 0xFFFE, "first push lands at the top of RAM");
        assert_eq!(board.ram()[0x6FFE..], [key::RIGHT | key::A; 2]);
        machines.push(machine);
    }
    assert_eq!(machines[0].state(), machines[1].state());
    assert!(machines[0].board() == machines[1].board());
}

#[test]
fn a_program_can_wait_for_the_timer() {
    // loop: ld a, [TIMER_HI] / or a, a / bz loop / hlt
    let program = assemble(&[
        LdAAbs(map::TIMER_HI),
        Alu(AluOp::Or, Reg::A),
        Br(Cond::Z, -6),
        Hlt,
    ]);
    for kind in [EngineKind::Fast, EngineKind::Gate] {
        let mut machine = Machine::new(&program).unwrap();
        machine.set_engine(kind);
        let cycles = machine.run_until_halt(100_000);
        assert!(machine.halted());
        // The timer's high byte turns 1 after 65,536 cycles; the loop takes
        // a few more to notice.
        assert!(
            (65_536..65_536 + 16).contains(&cycles),
            "{kind:?}: {cycles}"
        );
    }
}

#[test]
fn reset_restarts_the_program_from_a_clean_machine() {
    let mut machine = Machine::new(&demo_program()).unwrap();
    machine.run_until_halt(10_000);
    let (first_state, first_board) = (machine.state(), machine.board().clone());
    machine.reset();
    assert_eq!(machine.state(), Default::default());
    assert!(machine.board() == &Board::new(&demo_program()).unwrap());
    machine.run_until_halt(10_000);
    assert_eq!(machine.state(), first_state);
    assert!(machine.board() == &first_board);
}

/// A ROM of random bytes and a random CPU state that starts inside it, with
/// one pointer aimed at the device registers so that random programs use them.
fn random_machine(rng: &mut Rng) -> (Vec<u8>, silicon_isa::ArchState) {
    let mut rom = vec![0u8; map::ROM_SIZE];
    rng.fill(&mut rom);
    let mut state = rng.state();
    state.pc &= 0x7FFF;
    if rng.next() & 1 == 0 {
        state.set_pair(1, map::KEYS + (rng.next() % 6) as u16);
    }
    (rom, state)
}

/// Gate netlist against fast model on the full memory map, cycle by cycle,
/// with keys changing while the program runs.
#[test]
fn both_engines_agree_on_the_whole_machine() {
    let mut rng = Rng(0xB0A2D);
    let mut gate = GateCpu::new();
    for _ in 0..300 {
        let (rom, state) = random_machine(&mut rng);
        let mut fast = FastCpu::new();
        gate.load_state(&state);
        fast.load_state(&state);
        let mut gate_board = Board::new(&rom).unwrap();
        let mut fast_board = gate_board.clone();
        for _ in 0..10 {
            let keys = rng.next() as u8;
            gate_board.set_keys(keys);
            fast_board.set_keys(keys);
            if let Err(divergence) =
                lockstep::run(&mut gate, &mut gate_board, &mut fast, &mut fast_board, 60)
            {
                panic!("{divergence}");
            }
        }
        assert!(gate_board == fast_board, "boards differ after the run");
        assert_eq!(gate_board.cycles(), 600);
    }
}

/// One machine stays on the fast engine; the other is switched back and
/// forth at random moments. They must never tell the difference.
#[test]
fn switching_engines_at_random_moments_changes_nothing() {
    let mut rng = Rng(0xC0FFEE);
    let mut alive_at_100 = 0;
    for _ in 0..100 {
        let (rom, state) = random_machine(&mut rng);
        let mut reference = Machine::new(&rom).unwrap();
        let mut switching = Machine::new(&rom).unwrap();
        reference.load_state(&state);
        switching.load_state(&state);
        let mut remaining = 600u64;
        while remaining > 0 {
            let chunk = (rng.next() % 60 + 1).min(remaining);
            let keys = rng.next() as u8;
            reference.board_mut().set_keys(keys);
            switching.board_mut().set_keys(keys);
            switching.set_engine(if rng.next() & 1 == 0 {
                EngineKind::Gate
            } else {
                EngineKind::Fast
            });
            let was_before_100 = remaining > 500;
            reference.run(chunk);
            switching.run(chunk);
            remaining -= chunk;
            assert_eq!(switching.state(), reference.state());
            if was_before_100 && remaining <= 500 {
                alive_at_100 += !reference.halted() as u32;
            }
        }
        assert!(switching.board() == reference.board());
    }
    // Sanity: the random programs were still doing something for a while.
    assert!(
        alive_at_100 >= 30,
        "only {alive_at_100} of 100 ran past 100 cycles"
    );
}

#[test]
fn the_gate_cpu_inside_a_machine_is_inspectable() {
    let mut machine = Machine::new(&demo_program()).unwrap();
    machine.set_engine(EngineKind::Gate);
    machine.run(40);
    let gate = machine.gate();
    // The wires of the program counter spell the same number as the state.
    let pc = gate.sim().get_word(&gate.state_wires().pc) as u16;
    assert_eq!(pc, machine.state().pc);
    assert_eq!(Engine::state(gate), machine.state());
    assert_eq!(
        gate.sim().wire_count(),
        gate.sim().netlist().wire_count as usize
    );
}

/// Giving the computer another program is the same as building a new
/// computer around it: nothing of the old one is left behind, on either
/// engine. The engine choice and the keys held stay.
#[test]
fn loading_a_program_leaves_nothing_of_the_one_before() {
    let second = assemble(&[
        LdAAbs(map::KEYS),
        StAbsA(map::OUT),
        StAbsA(map::RAM_START),
        StAbsA(map::FRAME),
        Hlt,
    ]);
    for kind in [EngineKind::Fast, EngineKind::Gate] {
        let mut machine = Machine::new(&demo_program()).unwrap();
        machine.set_engine(kind);
        machine.board_mut().set_keys(key::A | key::UP);
        // Stop in the middle of an instruction, with things on the screen,
        // on the console and on the stack.
        machine.run(150);
        assert!(!machine.halted());
        machine.load(&second).unwrap();

        let mut fresh = Machine::new(&second).unwrap();
        fresh.set_engine(kind);
        fresh.board_mut().set_keys(key::A | key::UP);
        assert_eq!(machine.engine(), kind);
        for cycle in 0..40 {
            assert_eq!(machine.state(), fresh.state(), "{kind:?}, cycle {cycle}");
            machine.run(1);
            fresh.run(1);
        }
        assert!(machine.halted());
        assert!(machine.board().rom() == fresh.board().rom());
        assert!(machine.board().ram() == fresh.board().ram());
        assert!(machine.board().framebuffer() == fresh.board().framebuffer());
        assert_eq!(machine.board().console(), [key::A | key::UP]);
        assert_eq!(machine.board().frames(), 1);

        // A program that does not fit changes nothing.
        let before = machine.state();
        assert!(machine.load(&vec![0; map::ROM_SIZE + 1]).is_err());
        assert_eq!(machine.state(), before);
        assert!(machine.board().rom() == fresh.board().rom());
    }
}

/// The computer knows where the running instruction began, the same on
/// both engines at every cycle, and it is where the assembler put it.
#[test]
fn both_engines_say_where_the_running_instruction_began() {
    let insns = [
        LdImm(Reg::A, 1),
        StAbsA(map::OUT),
        Call(9),
        Hlt,
        // 9: a subroutine
        Inc(Reg::A),
        Ret,
    ];
    let program = assemble(&insns);
    // Where each instruction starts, from the encoding itself.
    let mut starts = Vec::new();
    let mut at = 0u16;
    for insn in &insns {
        starts.push(at);
        at += insn.len() as u16;
    }
    assert_eq!(starts[4], 9);

    let mut fast = Machine::new(&program).unwrap();
    let mut gate = Machine::new(&program).unwrap();
    gate.set_engine(EngineKind::Gate);
    assert_eq!(fast.opcode_at(), None);
    let mut seen = Vec::new();
    for _ in 0..60 {
        fast.run(1);
        gate.run(1);
        assert_eq!(
            fast.opcode_at(),
            gate.opcode_at(),
            "at cycle {}",
            fast.board().cycles()
        );
        let at = fast.opcode_at().unwrap();
        assert!(
            starts.contains(&at),
            "{at} is not where an instruction starts"
        );
        if seen.last() != Some(&at) {
            seen.push(at);
        }
        if fast.halted() {
            break;
        }
    }
    // ld, st, call, then the subroutine, back, and hlt.
    assert_eq!(seen, [0, 2, 5, 9, 10, 8]);
    assert_eq!(fast.opcode_at(), Some(8));

    // Starting again forgets it until the first opcode is read.
    fast.reset();
    assert_eq!(fast.opcode_at(), None);
    fast.run(1);
    assert_eq!(fast.opcode_at(), Some(0));
}
