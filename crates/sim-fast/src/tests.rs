//! ISA-level tests of the fast model: each one states what `docs/ISA.md`
//! promises and checks it on small hand-assembled programs.

use super::*;
use silicon_isa::flag::{C, N, V, Z};
use silicon_isa::{assemble, form, AluOp, Cond, Insn, Pair, Reg};
use Insn::*;

const A: usize = 0;
const B: usize = 1;

/// Run a program from reset until `hlt`.
fn run(program: &[Insn]) -> (ArchState, Ram) {
    run_bytes(&assemble(program))
}

fn run_bytes(code: &[u8]) -> (ArchState, Ram) {
    let mut ram = Ram::with_program(code);
    let mut cpu = FastCpu::new();
    for _ in 0..1_000_000 {
        if cpu.state().halted() {
            return (cpu.state().clone(), ram);
        }
        cpu.step_cycle(&mut ram);
    }
    panic!("program did not halt: {:?}", cpu.state());
}

#[test]
fn reset_starts_fetching_at_address_zero() {
    let mut ram = Ram::with_program(&assemble(&[Inc(Reg::A), Hlt]));
    let mut cpu = FastCpu::new();
    assert_eq!(*cpu.state(), ArchState::default());
    cpu.step_cycle(&mut ram); // fetch `inc a`
    assert!(cpu.fetched());
    assert_eq!(
        (cpu.state().pc, cpu.state().ir, cpu.state().step),
        (1, 0xC0, 1)
    );
    cpu.step_cycle(&mut ram); // execute it and fetch `hlt` in the same cycle
    assert_eq!((cpu.state().regs[A], cpu.state().ir), (1, 0x00));
    assert!(cpu.state().halted());
    let frozen = cpu.state().clone();
    cpu.step_cycle(&mut ram);
    assert_eq!(cpu.state().cycle, frozen.cycle + 1);
    assert_eq!(cpu.state().pc, frozen.pc, "hlt does not move");
    assert!(!cpu.fetched());
}

/// Cycle counts in the opcode table are the cycles from one opcode fetch to
/// the next, for every one of the 256 opcodes.
#[test]
fn every_opcode_takes_the_documented_number_of_cycles() {
    for opcode in 1..=255u8 {
        let f = form::form_of(opcode);
        for flags in 0..16u8 {
            let mut ram = Ram::new();
            // Operands point somewhere harmless; memory is otherwise zero.
            ram.0[0x4000] = opcode;
            ram.0[0x4001] = 0x10;
            ram.0[0x4002] = 0x20;
            let mut cpu = FastCpu::new();
            cpu.load_state(&ArchState {
                pc: 0x4000,
                sp: 0x8000,
                regs: [0x00, 0x30, 0x00, 0x31, 0x00, 0x32, 0x00, 0x33],
                flags,
                ..ArchState::default()
            });
            cpu.step_cycle(&mut ram);
            assert!(cpu.fetched());
            let mut cycles = 0;
            loop {
                cpu.step_cycle(&mut ram);
                cycles += 1;
                if cpu.fetched() {
                    break;
                }
                assert!(cycles < 10, "opcode {opcode:#04x} never finished");
            }
            let taken = f.name == "BR" && Cond::from_bits(opcode).holds(flags);
            let expected = if taken { f.cycles_taken } else { f.cycles };
            assert_eq!(
                cycles, expected,
                "{} ({opcode:#04x}), flags {flags:#x}",
                f.name
            );
        }
    }
}

/// The instruction pointer ends up just past the instruction, for every
/// opcode that does not jump.
#[test]
fn every_opcode_has_the_documented_length() {
    for opcode in 1..=255u8 {
        let f = form::form_of(opcode);
        if ["RET", "JMP_Z", "JMP", "CALL", "BR"].contains(&f.name) {
            continue;
        }
        let mut ram = Ram::new();
        ram.0[0x4000] = opcode;
        let mut cpu = FastCpu::new();
        cpu.load_state(&ArchState {
            pc: 0x4000,
            ..ArchState::default()
        });
        cpu.step_cycle(&mut ram);
        loop {
            cpu.step_cycle(&mut ram);
            if cpu.fetched() {
                break;
            }
        }
        // The next opcode has been fetched too, hence the extra 1.
        assert_eq!(cpu.state().pc, 0x4000 + f.bytes as u16 + 1, "{}", f.name);
    }
}

#[test]
fn undefined_steps_fall_back_to_fetch() {
    let mut ram = Ram::new();
    let mut cpu = FastCpu::new();
    let before = ArchState {
        ir: 0x40, // mov a, a has a single step
        step: 5,
        pc: 0x1234,
        ..ArchState::default()
    };
    cpu.load_state(&before);
    cpu.step_cycle(&mut ram);
    let after = cpu.state();
    assert_eq!((after.step, after.pc, after.ir), (0, 0x1234, 0x40));
}

#[test]
fn mov_and_load_immediate() {
    let (s, _) = run(&[
        LdImm(Reg::A, 0x11),
        LdImm(Reg::Zh, 0x77),
        Mov(Reg::B, Reg::A),
        Mov(Reg::Xl, Reg::Zh),
        Mov(Reg::A, Reg::A),
        Hlt,
    ]);
    assert_eq!(s.regs, [0x11, 0x11, 0x77, 0, 0, 0, 0, 0x77]);
    assert_eq!(s.flags, 0, "moves and loads leave the flags alone");
}

#[test]
fn alu_group_with_register_and_immediate_operands() {
    let (s, _) = run(&[
        LdImm(Reg::A, 200),
        LdImm(Reg::B, 100),
        Alu(AluOp::Add, Reg::B), // 300 -> 44, carry
        Hlt,
    ]);
    assert_eq!((s.regs[A], s.flags), (44, C));

    let (s, _) = run(&[LdImm(Reg::A, 5), AluImm(AluOp::Sub, 5), Hlt]);
    assert_eq!((s.regs[A], s.flags), (0, Z | C));

    let (s, _) = run(&[LdImm(Reg::A, 5), AluImm(AluOp::Cmp, 6), Hlt]);
    assert_eq!((s.regs[A], s.flags), (5, N), "5 < 6: borrow, so no carry");

    let (s, _) = run(&[LdImm(Reg::A, 0x7F), AluImm(AluOp::Add, 1), Hlt]);
    assert_eq!((s.regs[A], s.flags), (0x80, N | V));

    let (s, _) = run(&[
        LdImm(Reg::A, 0b1100),
        AluImm(AluOp::And, 0b1010),
        AluImm(AluOp::Or, 0b0001),
        AluImm(AluOp::Xor, 0xFF),
        Hlt,
    ]);
    assert_eq!((s.regs[A], s.flags), (0b1111_0110, N));
}

#[test]
fn right_shifts_and_their_left_aliases() {
    let (s, _) = run(&[LdImm(Reg::A, 0b1000_0101), ShrA, Hlt]);
    assert_eq!((s.regs[A], s.flags), (0b0100_0010, C));
    let (s, _) = run(&[LdImm(Reg::A, 0b1000_0101), ShrA, RorA, Hlt]);
    assert_eq!((s.regs[A], s.flags), (0b1010_0001, N));
    let (s, _) = run(&[LdImm(Reg::A, 0b1000_0100), AsrA, Hlt]);
    assert_eq!((s.regs[A], s.flags), (0b1100_0010, N));
    // shl a is `add a, a`; rol a is `adc a, a`.
    let (s, _) = run(&[
        LdImm(Reg::A, 0b1000_0001),
        Alu(AluOp::Add, Reg::A),
        Alu(AluOp::Adc, Reg::A),
        Hlt,
    ]);
    assert_eq!(s.regs[A], 0b0000_0101);
}

#[test]
fn counting_loop_sums_one_to_ten() {
    // b counts down from 10; a accumulates.
    let (s, _) = run(&[
        LdImm(Reg::B, 10),
        Alu(AluOp::Add, Reg::B), // loop:
        Dec(Reg::B),
        Br(Cond::Nz, -4),
        Hlt,
    ]);
    assert_eq!((s.regs[A], s.regs[B]), (55, 0));
    // 1 reset fetch + ld (2) + 10 × (add 1 + dec 1) + 9 taken (3) + 1 not taken (2).
    assert_eq!(s.cycle, 1 + 2 + 20 + 27 + 2);
}

#[test]
fn all_branch_conditions() {
    // For every condition and flag value: does the branch skip the `inc b`?
    for cond in Cond::ALL {
        for flags in 0..16u8 {
            let code = assemble(&[Br(cond, 1), Inc(Reg::B), Hlt]);
            let mut ram = Ram::with_program(&code);
            let mut cpu = FastCpu::new();
            cpu.load_state(&ArchState {
                flags,
                ..ArchState::default()
            });
            while !cpu.state().halted() {
                cpu.step_cycle(&mut ram);
            }
            assert_eq!(
                cpu.state().regs[B] == 0,
                cond.holds(flags),
                "{cond:?} {flags:#x}"
            );
        }
    }
}

#[test]
fn backward_and_forward_branch_offsets_are_relative_to_the_next_instruction() {
    // 0: bz +2   2: hlt   3: hlt   4: inc a   5: hlt
    let (s, _) = run(&[
        Alu(AluOp::Xor, Reg::A), // Z = 1
        Br(Cond::Z, 2),
        Hlt,
        Hlt,
        Inc(Reg::A),
        Hlt,
    ]);
    assert_eq!(s.regs[A], 1);
}

#[test]
fn sixteen_bit_load_and_add() {
    let (s, _) = run(&[
        LdPairImm(Pair::X, 0x12FF),
        LdPairImm(Pair::Y, 0x0001),
        AddPair(Pair::X, Pair::Y),
        Hlt,
    ]);
    assert_eq!(s.pair(1), 0x1300);
    assert_eq!(s.flags, 0);

    let (s, _) = run(&[
        LdPairImm(Pair::Ba, 0xFFFF),
        LdPairImm(Pair::Z, 0x0001),
        AddPair(Pair::Ba, Pair::Z),
        Hlt,
    ]);
    assert_eq!((s.pair(0), s.flags), (0, Z | C), "Z covers all 16 bits");

    // Low byte zero but high byte not: Z must be clear.
    let (s, _) = run(&[LdPairImm(Pair::X, 0x0180), AddPair(Pair::X, Pair::X), Hlt]);
    assert_eq!((s.pair(1), s.flags & Z), (0x0300, 0));
    // High byte zero but low byte not.
    let (s, _) = run(&[LdPairImm(Pair::X, 0x0001), AddPair(Pair::X, Pair::X), Hlt]);
    assert_eq!((s.pair(1), s.flags & Z), (0x0002, 0));
}

#[test]
fn sixteen_bit_compare_through_sbc() {
    // cmp lo; sbc hi leaves Z, C and N^V describing the 16-bit comparison.
    let compare = |x: u16, y: u16| {
        let (s, _) = run(&[
            LdPairImm(Pair::X, x),
            LdPairImm(Pair::Y, y),
            Mov(Reg::A, Reg::Xl),
            Alu(AluOp::Cmp, Reg::Yl),
            Mov(Reg::A, Reg::Xh),
            Alu(AluOp::Sbc, Reg::Yh),
            Hlt,
        ]);
        s.flags
    };
    for (x, y) in [
        (5u16, 5u16),
        (0x0100, 0x00FF),
        (0x00FF, 0x0100),
        (0x8000, 0x7FFF),
        (0x1234, 0x1234),
    ] {
        let f = compare(x, y);
        assert_eq!(f & Z != 0, x == y, "{x:#x} vs {y:#x}");
        assert_eq!(f & C != 0, x >= y);
        assert_eq!(Cond::Lt.holds(f), (x as i16) < (y as i16));
    }
}

#[test]
fn pointer_loads_and_stores_with_post_increment() {
    // Copy four bytes from 0x0100 to 0x9000.
    let mut code = assemble(&[
        LdPairImm(Pair::X, 0x0100),
        LdPairImm(Pair::Y, 0x9000),
        LdImm(Reg::B, 4),
        LdAPtrInc(Pair::X), // loop:
        StPtrIncA(Pair::Y),
        Dec(Reg::B),
        Br(Cond::Nz, -5),
        LdAPtr(Pair::X),
        StPtrA(Pair::Y),
        Hlt,
    ]);
    code.resize(0x0100, 0);
    code.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x99]);
    let (s, ram) = run_bytes(&code);
    assert_eq!(ram.0[0x9000..0x9005], [0xDE, 0xAD, 0xBE, 0xEF, 0x99]);
    assert_eq!((s.pair(1), s.pair(2)), (0x0104, 0x9004));
}

#[test]
fn post_increment_through_ba_keeps_the_loaded_byte() {
    let mut code = assemble(&[LdPairImm(Pair::Ba, 0x01FF), LdAPtrInc(Pair::Ba), Hlt]);
    code.resize(0x0200, 0);
    code[0x01FF] = 0x5A;
    let (s, _) = run_bytes(&code);
    // ba + 1 = 0x0200: b takes the new high byte, a takes the loaded value.
    assert_eq!((s.regs[A], s.regs[B]), (0x5A, 0x02));
}

#[test]
fn table_lookup_and_pointer_indexing() {
    let mut code = assemble(&[
        LdPairImm(Pair::X, 0x0200),
        LdImm(Reg::A, 3),
        LdAIdx(Pair::X), // a = table[3]
        Mov(Reg::B, Reg::A),
        LdImm(Reg::A, 0xFF),
        LeaIdx(Pair::X), // x += 255
        LdAPtr(Pair::X),
        Hlt,
    ]);
    code.resize(0x0300, 0);
    code[0x0203] = 0x42;
    code[0x02FF] = 0x24;
    let (s, _) = run_bytes(&code);
    assert_eq!((s.regs[B], s.regs[A], s.pair(1)), (0x42, 0x24, 0x02FF));
}

#[test]
fn push_and_pop_use_a_descending_little_endian_stack() {
    let (s, ram) = run(&[
        LdPairImm(Pair::X, 0xABCD),
        Push(Pair::X),
        LdSp(Reg::A, 0), // low byte is at the top of the stack
        LdSp(Reg::B, 1),
        Pop(Pair::Z),
        Hlt,
    ]);
    // sp starts at 0, so the first push lands at the very top of memory.
    assert_eq!((ram.0[0xFFFE], ram.0[0xFFFF]), (0xCD, 0xAB));
    assert_eq!((s.regs[A], s.regs[B]), (0xCD, 0xAB));
    assert_eq!((s.pair(3), s.sp), (0xABCD, 0x0000));
}

#[test]
fn call_and_ret() {
    // 0: call 5   3: inc b   4: hlt   5: inc a   6: ret
    let (s, ram) = run(&[Call(0x0005), Inc(Reg::B), Hlt, Inc(Reg::A), Ret]);
    assert_eq!((s.regs[A], s.regs[B], s.sp), (1, 1, 0));
    assert_eq!(
        (ram.0[0xFFFE], ram.0[0xFFFF]),
        (0x03, 0x00),
        "return address"
    );
}

#[test]
fn stack_frames_with_sp_relative_access() {
    let (s, ram) = run(&[
        LeaSpSp(-4), // four bytes of locals
        LdImm(Reg::A, 0x11),
        StSp(0, Reg::A),
        LdImm(Reg::Xl, 0x22),
        StSp(3, Reg::Xl),
        LdSp(Reg::B, 0),
        LdSp(Reg::Zh, 3),
        LeaSp(Pair::Y, 3), // y = &local3
        LdAPtr(Pair::Y),
        LeaSpSp(4),
        Hlt,
    ]);
    assert_eq!((ram.0[0xFFFC], ram.0[0xFFFF]), (0x11, 0x22));
    assert_eq!((s.regs[B], s.regs[7], s.regs[A]), (0x11, 0x22, 0x22));
    assert_eq!((s.pair(2), s.sp), (0xFFFF, 0));
}

#[test]
fn lea_with_signed_offsets_moves_pointers_both_ways() {
    let (s, _) = run(&[
        LdPairImm(Pair::X, 0x1000),
        LeaPair(Pair::X, -1),
        LdPairImm(Pair::Z, 0x10FF),
        LeaPair(Pair::Z, 127),
        Hlt,
    ]);
    assert_eq!((s.pair(1), s.pair(3)), (0x0FFF, 0x117E));
    assert_eq!(s.flags, 0, "address arithmetic leaves the flags alone");
}

#[test]
fn absolute_load_store_and_jumps() {
    let (s, ram) = run(&[
        LdImm(Reg::A, 0x77),        // 0
        StAbsA(0xA000),             // 2
        LdImm(Reg::A, 0),           // 5
        LdAAbs(0xA000),             // 7
        Jmp(0x000E),                // 10
        Hlt,                        // 13
        LdPairImm(Pair::Z, 0x0013), // 14 (0x0E)
        JmpZ,                       // 17
        Hlt,                        // 18
        Inc(Reg::B),                // 19 (0x13)
        Hlt,
    ]);
    assert_eq!((ram.0[0xA000], s.regs[A], s.regs[B]), (0x77, 0x77, 1));
}

#[test]
fn moving_the_stack_pointer() {
    let (s, ram) = run(&[
        LdPairImm(Pair::X, 0xC000),
        MovSpX,
        Push(Pair::X),
        LeaSp(Pair::Y, 0), // read sp back
        Hlt,
    ]);
    assert_eq!((s.sp, s.pair(2)), (0xBFFE, 0xBFFE));
    assert_eq!((ram.0[0xBFFE], ram.0[0xBFFF]), (0x00, 0xC0));
}

#[test]
fn inc_and_dec_keep_the_carry_for_multi_byte_loops() {
    let (s, _) = run(&[
        LdImm(Reg::A, 0xFF),
        AluImm(AluOp::Add, 1), // C = 1
        Inc(Reg::B),
        Dec(Reg::Xl),
        Hlt,
    ]);
    assert_eq!(s.flags & C, C);
    assert_eq!((s.regs[B], s.regs[2]), (1, 0xFF));
    assert_eq!(s.flags & (Z | N | V), N);
}

#[test]
fn state_can_be_saved_and_restored_mid_instruction() {
    let code = assemble(&[
        Call(0x0005),
        Inc(Reg::B),
        Hlt,
        LdPairImm(Pair::X, 0xBEEF),
        Ret,
    ]);
    let (reference, _) = run_bytes(&code);
    // Stop after every possible cycle, move the state into a fresh CPU, and
    // finish there: the result must not depend on where the cut was made.
    for cut in 0..reference.cycle {
        let mut ram = Ram::with_program(&code);
        let mut first = FastCpu::new();
        for _ in 0..cut {
            first.step_cycle(&mut ram);
        }
        let mut second = FastCpu::new();
        second.load_state(first.state());
        while !second.state().halted() {
            second.step_cycle(&mut ram);
        }
        assert_eq!(*second.state(), reference, "cut at cycle {cut}");
    }
}
