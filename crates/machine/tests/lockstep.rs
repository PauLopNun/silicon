//! Equivalence of the two engines: the gate-level netlist and the fast model
//! must agree on the architectural state and on every bus access after every
//! single clock cycle.
//!
//! Because all 256 opcodes are defined, any byte sequence is a program, and
//! any `ArchState` is a state the CPU can be placed in. So the main test is
//! brutally simple: random memory, random state, run, compare.

use proptest::prelude::*;
use silicon_isa::{assemble, AluOp, ArchState, Cond, Insn, Pair, Reg};
use silicon_machine::{lockstep, Engine, FastCpu, GateCpu, Ram};

/// xorshift64*: a small deterministic generator, so failures reproduce.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn fill(&mut self, memory: &mut [u8]) {
        for chunk in memory.chunks_mut(8) {
            let bytes = self.next().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }

    fn state(&mut self) -> ArchState {
        let (a, b, c) = (self.next(), self.next(), self.next());
        ArchState {
            pc: a as u16,
            sp: (a >> 16) as u16,
            t: (a >> 32) as u16,
            ir: (a >> 48) as u8,
            flags: (a >> 56) as u8 & 0x0F,
            regs: b.to_le_bytes(),
            // Mostly at an instruction boundary or early in an instruction,
            // sometimes at a step the instruction does not even have.
            step: [0, 0, 1, 1, 1, 2, 2, 3, 4, 5, 6, 7][(c % 12) as usize],
            cycle: 0,
        }
    }
}

/// Run both engines from `state` on copies of `memory`; panic with a full
/// report at the first difference. Returns the final state.
fn check(gate: &mut GateCpu, memory: &Ram, state: &ArchState, cycles: u64) -> ArchState {
    let mut fast = FastCpu::new();
    gate.load_state(state);
    fast.load_state(state);
    let (mut gate_mem, mut fast_mem) = (memory.clone(), memory.clone());
    if let Err(divergence) = lockstep::run(gate, &mut gate_mem, &mut fast, &mut fast_mem, cycles) {
        panic!("{divergence}");
    }
    assert!(gate_mem == fast_mem, "memories differ after the run");
    Engine::state(gate)
}

thread_local! {
    /// Building the netlist takes a moment; one instance per test thread.
    static GATE: std::cell::RefCell<GateCpu> = std::cell::RefCell::new(GateCpu::new());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Random bytes as a program, from a random state.
    #[test]
    fn random_programs_from_random_states(seed: u64) {
        let mut rng = Rng(seed | 1);
        let mut memory = Ram::new();
        rng.fill(&mut memory.0);
        let state = rng.state();
        GATE.with(|gate| check(&mut gate.borrow_mut(), &memory, &state, 400));
    }

    /// Each opcode on its own, from the first step, with random operands and
    /// random register contents: nothing depends on what random programs
    /// happen to reach.
    #[test]
    fn every_opcode_from_random_register_contents(seed: u64) {
        let mut rng = Rng(seed | 1);
        let mut memory = Ram::new();
        rng.fill(&mut memory.0);
        GATE.with(|gate| {
            let mut gate = gate.borrow_mut();
            for opcode in 0..=255u8 {
                let mut state = rng.state();
                state.step = 0;
                memory.0[state.pc as usize] = opcode;
                check(&mut gate, &memory, &state, 8);
            }
        });
    }
}

/// A long deterministic soak: many restarts, each a fresh random machine.
#[test]
fn soak_two_hundred_thousand_cycles() {
    let mut rng = Rng(0x5111_C0DE);
    let mut gate = GateCpu::new();
    let mut memory = Ram::new();
    let mut halted = 0;
    for _ in 0..400 {
        rng.fill(&mut memory.0);
        let state = rng.state();
        let end = check(&mut gate, &memory, &state, 500);
        halted += end.halted() as u32;
    }
    // Sanity: random programs do run for a while, they are not all stuck.
    assert!(halted < 300, "{halted} of 400 runs ended in hlt");
}

/// Hand-written programs covering every instruction form, run from reset to
/// `hlt` on both engines.
#[test]
fn directed_programs_from_reset() {
    use Insn::*;
    let programs: Vec<Vec<Insn>> = vec![
        // Arithmetic and flags.
        vec![
            LdImm(Reg::A, 200),
            LdImm(Reg::B, 100),
            Alu(AluOp::Add, Reg::B),
            Alu(AluOp::Adc, Reg::B),
            Alu(AluOp::Sub, Reg::A),
            AluImm(AluOp::Sbc, 1),
            AluImm(AluOp::Xor, 0x55),
            AluImm(AluOp::Cmp, 0xAA),
            ShrA,
            RorA,
            AsrA,
            Inc(Reg::Zh),
            Dec(Reg::Xl),
            Hlt,
        ],
        // Loop with a taken and a not-taken branch.
        vec![
            LdImm(Reg::B, 10),
            Alu(AluOp::Add, Reg::B),
            Dec(Reg::B),
            Br(Cond::Nz, -4),
            Hlt,
        ],
        // 16-bit work, pointers, the stack.
        vec![
            LdPairImm(Pair::X, 0x80FF),
            LdPairImm(Pair::Y, 0x0001),
            AddPair(Pair::X, Pair::Y),
            Push(Pair::X),
            Pop(Pair::Z),
            StPtrIncA(Pair::Z),
            StPtrA(Pair::Z),
            LdAPtrInc(Pair::Y),
            LdAPtr(Pair::Y),
            LdAIdx(Pair::Y),
            LeaIdx(Pair::X),
            LeaPair(Pair::X, -3),
            LeaSpSp(-6),
            StSp(2, Reg::Xl),
            LdSp(Reg::B, 2),
            LeaSp(Pair::Ba, 5),
            MovSpX,
            Hlt,
        ],
    ];
    let mut gate = GateCpu::new();
    for program in &programs {
        let memory = Ram::with_program(&assemble(program));
        let end = check(&mut gate, &memory, &ArchState::default(), 400);
        assert!(end.halted(), "program did not reach hlt: {end:x?}");
    }

    // The control-flow program is laid out by hand so the addresses are clear.
    let mut code = vec![0u8; 0x40];
    let mut place = |addr: usize, program: &[Insn]| {
        let bytes = assemble(program);
        code[addr..addr + bytes.len()].copy_from_slice(&bytes);
    };
    place(
        0x00,
        &[
            Call(0x0010),
            StAbsA(0xC000),
            LdPairImm(Pair::Z, 0x0020),
            JmpZ,
        ],
    );
    place(0x10, &[LdImm(Reg::A, 0x42), Ret]);
    place(0x20, &[LdAAbs(0xC000), Mov(Reg::B, Reg::A), Jmp(0x0030)]);
    place(0x30, &[Hlt]);
    let memory = Ram::with_program(&code);
    let end = check(&mut gate, &memory, &ArchState::default(), 200);
    assert!(end.halted());
    assert_eq!((end.regs[0], end.regs[1], end.sp), (0x42, 0x42, 0));
}

/// Switching engines mid-run, at every possible cycle, changes nothing.
#[test]
fn engines_can_be_swapped_at_any_cycle() {
    use Insn::*;
    let code = assemble(&[
        LdPairImm(Pair::X, 0x9000),
        LdImm(Reg::B, 3),
        Call(0x000C),        // 5
        Dec(Reg::B),         // 8
        Br(Cond::Nz, -6),    // 9: back to the call
        Hlt,                 // 11
        Mov(Reg::A, Reg::B), // 12 (0x0C)
        StPtrIncA(Pair::X),
        Ret,
    ]);
    // Reference run: the fast engine alone.
    let mut reference = FastCpu::new();
    let mut reference_mem = Ram::with_program(&code);
    while !reference.state().halted() {
        reference.step_cycle(&mut reference_mem);
    }
    let total = reference.state().cycle;
    assert_eq!(reference_mem.0[0x9000..0x9003], [3, 2, 1]);

    let mut gate = GateCpu::new();
    for switch_at in 0..=total {
        // Fast up to the switch, gate afterwards…
        let mut memory = Ram::with_program(&code);
        let mut fast = FastCpu::new();
        for _ in 0..switch_at {
            fast.step_cycle(&mut memory);
        }
        gate.load_state(fast.state());
        for _ in switch_at..total {
            gate.step_cycle(&mut memory);
        }
        assert_eq!(
            Engine::state(&gate),
            *reference.state(),
            "fast→gate at {switch_at}"
        );
        assert!(memory == reference_mem);

        // …and gate up to the switch, fast afterwards.
        let mut memory = Ram::with_program(&code);
        gate.reset();
        for _ in 0..switch_at {
            gate.step_cycle(&mut memory);
        }
        fast.load_state(&Engine::state(&gate));
        for _ in switch_at..total {
            fast.step_cycle(&mut memory);
        }
        assert_eq!(
            *fast.state(),
            *reference.state(),
            "gate→fast at {switch_at}"
        );
        assert!(memory == reference_mem);
    }
}

/// Both engines report opcode fetches on the same cycles, which is how the
/// UI counts instructions.
#[test]
fn fetch_cycles_agree() {
    let mut rng = Rng(77);
    let mut memory = Ram::new();
    let mut gate = GateCpu::new();
    let mut fast = FastCpu::new();
    let mut fetches = 0;
    // Random programs often run into a `hlt`, so use many short runs.
    for _ in 0..40 {
        rng.fill(&mut memory.0);
        let state = rng.state();
        gate.load_state(&state);
        fast.load_state(&state);
        let mut gate_mem = memory.clone();
        let mut fast_mem = memory.clone();
        for _ in 0..200 {
            gate.step_cycle(&mut gate_mem);
            fast.step_cycle(&mut fast_mem);
            assert_eq!(gate.fetched(), fast.fetched());
            fetches += fast.fetched() as u32;
        }
    }
    assert!(fetches > 1000, "only {fetches} fetches in 8000 cycles");
}
