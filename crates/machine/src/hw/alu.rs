//! The arithmetic-logic unit: everything that computes an 8-bit value.
//!
//! ```text
//!   port A ──► xsel ──► X ─┬──────────────► adder8 ─┐
//!              (0x00/0xFF) ├──► logic (and/or/xor) ─┤
//!                          └──► >> 1 ───────────────┼─► mux ─► result
//!   port B ──┐                                      │
//!   data in ─┴► ysel ─► yinv ─► Y ─► adder8, logic ─┘
//! ```
//!
//! Every value written to a register passes through here, even a plain
//! `mov` (computed as `0 + source`), so there is a single write path.

use super::control as c;
use silicon_hdl::{Builder, Wire};

/// Data inputs of the ALU.
pub struct AluIn<'a> {
    /// Port A of the register file.
    pub a: &'a [Wire],
    /// Port B of the register file.
    pub b: &'a [Wire],
    /// The data bus.
    pub data_in: &'a [Wire],
    /// The C flag.
    pub carry: Wire,
}

/// Outputs of the ALU.
pub struct AluOut {
    pub result: Vec<Wire>,
    /// Carry out of the adder.
    pub carry_out: Wire,
    /// Signed overflow of the adder.
    pub overflow: Wire,
    /// The result is zero.
    pub zero: Wire,
    /// Bit 0 of the left operand: what a right shift pushes out.
    pub x0: Wire,
}

/// Build the ALU. `signals` is the control word, one wire per signal.
pub fn build(b: &mut Builder, input: &AluIn, signals: &[Wire]) -> AluOut {
    let s = |signal: u64| signals[c::bit(signal)];
    b.scope("alu", |b| {
        // Left operand: port A, or a constant for mov / inc / dec.
        let x: Vec<Wire> = b.scope("xsel", |b| {
            let keep = b.not(s(c::X_ZERO));
            let not_ones = b.not(s(c::X_ONES));
            input
                .a
                .iter()
                .map(|&a| {
                    let kept = b.nand(a, keep);
                    b.nand(kept, not_ones)
                })
                .collect()
        });
        // Right operand: port B or the data bus, optionally inverted.
        let y = b.scope("ysel", |b| b.mux_word(s(c::Y_DATA), input.b, input.data_in));
        let y = b.scope("yinv", |b| b.xor_word(&y, s(c::Y_INV)));

        let (carry_in, shift_in) = b.scope("carry", |b| {
            let none = b.not(s(c::CIN_ONE));
            let from_flag = b.nand(s(c::CIN_C), input.carry);
            let carry_in = b.nand(none, from_flag);
            let from_c = b.nand(s(c::SIN_C), input.carry);
            let from_sign = b.nand(s(c::SIN_X7), x[7]);
            (carry_in, b.nand(from_c, from_sign))
        });

        let sum = b.scope("adder8", |b| b.adder(&x, &y, carry_in));
        let (and, or, xor) = b.scope("logic", |b| {
            let pairs: Vec<(Wire, Wire)> = x.iter().copied().zip(y.iter().copied()).collect();
            let and = b.scope("and8", |b| {
                pairs.iter().map(|&(x, y)| b.and(x, y)).collect::<Vec<_>>()
            });
            let or = b.scope("or8", |b| {
                pairs.iter().map(|&(x, y)| b.or(x, y)).collect::<Vec<_>>()
            });
            let xor = b.scope("xor8", |b| {
                pairs.iter().map(|&(x, y)| b.xor(x, y)).collect::<Vec<_>>()
            });
            (and, or, xor)
        });
        // A right shift is only wiring: bit i takes bit i + 1.
        let mut shifted = x[1..].to_vec();
        shifted.push(shift_in);

        let result = b.scope("mux", |b| {
            let others = [s(c::FN_AND), s(c::FN_OR), s(c::FN_XOR), s(c::FN_SHR)];
            let inverted = b.not_word(&others);
            let fn_add = b.and_all(&inverted);
            b.select_onehot(&[
                (fn_add, &sum.sum),
                (s(c::FN_AND), &and),
                (s(c::FN_OR), &or),
                (s(c::FN_XOR), &xor),
                (s(c::FN_SHR), &shifted),
            ])
        });

        let (zero, overflow) = b.scope("status", |b| {
            let inverted = b.not_word(&result);
            let zero = b.and_all(&inverted);
            // Overflow: the carry into the sign bit differs from the carry out.
            let overflow = b.xor(sum.carries[6], sum.carries[7]);
            (zero, overflow)
        });

        b.probe("x", &x);
        b.probe("y", &y);
        b.probe("result", &result);
        AluOut {
            result,
            carry_out: sum.carry_out(),
            overflow,
            zero,
            x0: x[0],
        }
    })
}

/// Reference result of the ALU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AluRef {
    pub result: u8,
    pub carry_out: bool,
    pub overflow: bool,
    pub zero: bool,
    pub x0: bool,
}

/// Plain Rust meaning of the ALU for any control word.
pub fn reference(a: u8, b: u8, data_in: u8, carry: bool, word: u64) -> AluRef {
    let on = |signal: u64| word & signal != 0;
    let mut x = a;
    if on(c::X_ZERO) {
        x = 0;
    }
    if on(c::X_ONES) {
        x = 0xFF;
    }
    let mut y = if on(c::Y_DATA) { data_in } else { b };
    if on(c::Y_INV) {
        y = !y;
    }
    let carry_in = on(c::CIN_ONE) || (on(c::CIN_C) && carry);
    let shift_in = (on(c::SIN_C) && carry) || (on(c::SIN_X7) && x & 0x80 != 0);

    let wide = x as u16 + y as u16 + carry_in as u16;
    let sum = wide as u8;
    let mut result = 0;
    if !on(c::FN_AND | c::FN_OR | c::FN_XOR | c::FN_SHR) {
        result |= sum;
    }
    if on(c::FN_AND) {
        result |= x & y;
    }
    if on(c::FN_OR) {
        result |= x | y;
    }
    if on(c::FN_XOR) {
        result |= x ^ y;
    }
    if on(c::FN_SHR) {
        result |= x >> 1 | (shift_in as u8) << 7;
    }
    AluRef {
        result,
        carry_out: wide > 0xFF,
        overflow: (x ^ sum) & (y ^ sum) & 0x80 != 0,
        zero: result == 0,
        x0: x & 1 != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use silicon_sim_gate::GateSim;

    /// The ALU alone, with every control signal as a primary input.
    struct Bench {
        sim: GateSim,
        a: Vec<Wire>,
        b: Vec<Wire>,
        data_in: Vec<Wire>,
        carry: Wire,
        signals: Vec<Wire>,
        out: AluOut,
    }

    impl Bench {
        fn new() -> Self {
            let mut bld = Builder::new("test");
            let a = bld.inputs("a", 8);
            let b = bld.inputs("b", 8);
            let data_in = bld.inputs("data_in", 8);
            let carry = bld.input("carry");
            let signals = bld.inputs("signal", c::SIGNAL_COUNT);
            let input = AluIn {
                a: &a,
                b: &b,
                data_in: &data_in,
                carry,
            };
            let out = build(&mut bld, &input, &signals);
            let sim = GateSim::new(bld.finish().unwrap()).unwrap();
            Bench {
                sim,
                a,
                b,
                data_in,
                carry,
                signals,
                out,
            }
        }

        fn eval(&mut self, a: u8, b: u8, data_in: u8, carry: bool, word: u64) -> AluRef {
            self.sim.set_word(&self.a, a as u64);
            self.sim.set_word(&self.b, b as u64);
            self.sim.set_word(&self.data_in, data_in as u64);
            self.sim.set(self.carry, carry);
            self.sim.set_word(&self.signals, word);
            self.sim.settle();
            AluRef {
                result: self.sim.get_word(&self.out.result) as u8,
                carry_out: self.sim.get(self.out.carry_out),
                overflow: self.sim.get(self.out.overflow),
                zero: self.sim.get(self.out.zero),
                x0: self.sim.get(self.out.x0),
            }
        }
    }

    /// Every pair of operand values, for each way the microcode drives the
    /// ALU (16 control settings × 2 carry values × 65,536 operand pairs).
    #[test]
    fn exhaustive_over_operands_for_every_microcoded_operation() {
        use silicon_isa::AluOp;
        let mut settings: Vec<u64> = AluOp::ALL.iter().map(|&op| c::alu_signals(op)).collect();
        settings.extend([
            c::X_ZERO,              // mov / load
            c::X_ZERO | c::CIN_ONE, // inc
            c::X_ONES,              // dec
            c::FN_SHR,              // shr
            c::FN_SHR | c::SIN_C,   // ror
            c::FN_SHR | c::SIN_X7,  // asr
            c::CIN_C | c::Z_STICKY, // high half of a 16-bit add
            c::X_ZERO | c::Y_DATA,  // load from the bus
        ]);
        let mut bench = Bench::new();
        for &word in &settings {
            for carry in [false, true] {
                for a in 0..=255u8 {
                    for b in 0..=255u8 {
                        // data_in is b's complement so that Y_DATA is visible.
                        let got = bench.eval(a, b, !b, carry, word);
                        let want = reference(a, b, !b, carry, word);
                        assert_eq!(got, want, "a={a:#x} b={b:#x} carry={carry} word={word:#x}");
                    }
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4000))]
        /// Any control word at all, including ones the microcode never uses.
        #[test]
        fn matches_reference_for_arbitrary_control_words(
            a: u8, b: u8, data_in: u8, carry: bool, word in 0..1u64 << c::SIGNAL_COUNT
        ) {
            thread_local!(static BENCH: std::cell::RefCell<Bench> = std::cell::RefCell::new(Bench::new()));
            let got = BENCH.with(|bench| bench.borrow_mut().eval(a, b, data_in, carry, word));
            prop_assert_eq!(got, reference(a, b, data_in, carry, word));
        }
    }

    #[test]
    fn gate_paths_follow_the_block_structure() {
        let bench = Bench::new();
        let nl = bench.sim.netlist();
        let paths: Vec<String> = (0..nl.gates.len()).map(|i| nl.gate_path(i)).collect();
        for expected in [
            "test/alu/xsel/nand0",
            "test/alu/adder8/fa3/nand2",
            "test/alu/logic/xor8/nand31",
            "test/alu/mux/nand0",
            "test/alu/status/nand0",
        ] {
            assert!(paths.iter().any(|p| p == expected), "missing {expected}");
        }
        assert_eq!(nl.probe("test/alu:result").map(<[Wire]>::len), Some(8));
    }
}
