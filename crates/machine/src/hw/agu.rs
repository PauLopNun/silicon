//! The address unit: everything that computes a 16-bit address.
//!
//! ```text
//!   pc ─┐
//!   sp ─┤                 ┌────────────────────────► (post) ─┐
//!   pp ─┼─► base ─────────┤                                  ├─► address bus
//!   t  ─┘                 └─► adder16 ─► sum ──────► (pre) ──┘
//!   0 / t.lo / a / −1 ─► offset ─┘        └─► written back to pc, sp or pp
//! ```
//!
//! One adder serves every kind of address arithmetic: stepping `pc`, pushing
//! and popping, post-incrementing a pointer, indexing, relative branches.
//! Nothing here depends on the data bus, which is what lets memory be read
//! within the cycle (ADR 0003).

use super::control as c;
use silicon_hdl::{Builder, Wire};

/// Data inputs of the address unit: four 16-bit candidates for the base, and
/// register `a` as a possible offset.
pub struct AguIn<'a> {
    pub pc: &'a [Wire],
    pub sp: &'a [Wire],
    /// The register pair selected by opcode bits 1..0.
    pub pair: &'a [Wire],
    /// The operand latch.
    pub t: &'a [Wire],
    pub a: &'a [Wire],
}

/// Outputs of the address unit.
pub struct AguOut {
    /// The address bus.
    pub addr: Vec<Wire>,
    /// `base + offset`: the value written back to `pc`, `sp` or a pair.
    pub sum: Vec<Wire>,
}

/// Build the address unit. `signals` is the control word.
pub fn build(b: &mut Builder, input: &AguIn, signals: &[Wire]) -> AguOut {
    let s = |signal: u64| signals[c::bit(signal)];
    b.scope("agu", |b| {
        let base = b.scope("base", |b| {
            let others = [s(c::BASE_SP), s(c::BASE_PAIR), s(c::BASE_T)];
            let inverted = b.not_word(&others);
            let base_pc = b.and_all(&inverted);
            b.select_onehot(&[
                (base_pc, input.pc),
                (s(c::BASE_SP), input.sp),
                (s(c::BASE_PAIR), input.pair),
                (s(c::BASE_T), input.t),
            ])
        });

        // Low byte: t.lo, a, or all ones. High byte: all ones for −1 and for
        // a negative sign-extended t.lo, zero otherwise.
        let offset = b.scope("offset", |b| {
            let not_m1 = b.not(s(c::OFF_M1));
            let mut offset: Vec<Wire> = (0..8)
                .map(|i| {
                    let from_t = b.nand(input.t[i], s(c::OFF_T));
                    let from_a = b.nand(input.a[i], s(c::OFF_A));
                    b.nand_all(&[from_t, from_a, not_m1])
                })
                .collect();
            let negative = b.nand(s(c::OFF_SX), input.t[7]);
            let high = b.nand(not_m1, negative);
            offset.extend([high; 8]);
            offset
        });

        let sum = b
            .scope("adder16", |b| b.adder(&base, &offset, s(c::AGU_CIN)))
            .sum;
        let addr = b.scope("addrmux", |b| b.mux_word(s(c::PRE), &base, &sum));

        b.probe("base", &base);
        b.probe("sum", &sum);
        AguOut { addr, sum }
    })
}

/// Plain Rust meaning of the address unit: `(address bus, sum)`.
pub fn reference(pc: u16, sp: u16, pair: u16, t: u16, a: u8, word: u64) -> (u16, u16) {
    let on = |signal: u64| word & signal != 0;
    // The base multiplexer is an AND-OR, so several selects OR together.
    let mut base = 0;
    if !on(c::BASE_SP | c::BASE_PAIR | c::BASE_T) {
        base |= pc;
    }
    if on(c::BASE_SP) {
        base |= sp;
    }
    if on(c::BASE_PAIR) {
        base |= pair;
    }
    if on(c::BASE_T) {
        base |= t;
    }
    let mut offset = 0u16;
    if on(c::OFF_T) {
        offset |= t & 0xFF;
    }
    if on(c::OFF_A) {
        offset |= a as u16;
    }
    if on(c::OFF_M1) {
        offset |= 0xFFFF;
    }
    if on(c::OFF_SX) && t & 0x80 != 0 {
        offset |= 0xFF00;
    }
    let sum = base
        .wrapping_add(offset)
        .wrapping_add(on(c::AGU_CIN) as u16);
    (if on(c::PRE) { sum } else { base }, sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use silicon_sim_gate::GateSim;

    struct Bench {
        sim: GateSim,
        inputs: [Vec<Wire>; 5],
        signals: Vec<Wire>,
        out: AguOut,
    }

    impl Bench {
        fn new() -> Self {
            let mut b = Builder::new("test");
            let pc = b.inputs("pc", 16);
            let sp = b.inputs("sp", 16);
            let pair = b.inputs("pair", 16);
            let t = b.inputs("t", 16);
            let a = b.inputs("a", 8);
            let signals = b.inputs("signal", c::SIGNAL_COUNT);
            let input = AguIn {
                pc: &pc,
                sp: &sp,
                pair: &pair,
                t: &t,
                a: &a,
            };
            let out = build(&mut b, &input, &signals);
            let sim = GateSim::new(b.finish().unwrap()).unwrap();
            Bench {
                sim,
                inputs: [pc, sp, pair, t, a],
                signals,
                out,
            }
        }

        fn eval(&mut self, values: [u16; 5], word: u64) -> (u16, u16) {
            for (wires, value) in self.inputs.iter().zip(values) {
                self.sim.set_word(wires, value as u64);
            }
            self.sim.set_word(&self.signals, word);
            self.sim.settle();
            (
                self.sim.get_word(&self.out.addr) as u16,
                self.sim.get_word(&self.out.sum) as u16,
            )
        }
    }

    /// The address-unit part of every control word the microcode produces.
    fn microcoded_settings() -> Vec<u64> {
        let mask = c::BASE_SP
            | c::BASE_PAIR
            | c::BASE_T
            | c::OFF_T
            | c::OFF_SX
            | c::OFF_A
            | c::OFF_M1
            | c::AGU_CIN
            | c::PRE;
        let mut settings: Vec<u64> = c::microcode().iter().map(|l| l.signals & mask).collect();
        settings.sort_unstable();
        settings.dedup();
        settings
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(3000))]
        #[test]
        fn matches_reference_for_microcoded_settings(
            pc: u16, sp: u16, pair: u16, t: u16, a: u8, pick: prop::sample::Index
        ) {
            thread_local!(static BENCH: std::cell::RefCell<Bench> = std::cell::RefCell::new(Bench::new()));
            let settings = microcoded_settings();
            let word = settings[pick.index(settings.len())];
            let got = BENCH.with(|b| b.borrow_mut().eval([pc, sp, pair, t, a as u16], word));
            prop_assert_eq!(got, reference(pc, sp, pair, t, a, word), "word {:#x}", word);
        }

        #[test]
        fn matches_reference_for_arbitrary_control_words(
            pc: u16, sp: u16, pair: u16, t: u16, a: u8, word in 0..1u64 << c::SIGNAL_COUNT
        ) {
            thread_local!(static BENCH: std::cell::RefCell<Bench> = std::cell::RefCell::new(Bench::new()));
            let got = BENCH.with(|b| b.borrow_mut().eval([pc, sp, pair, t, a as u16], word));
            prop_assert_eq!(got, reference(pc, sp, pair, t, a, word));
        }
    }

    #[test]
    fn carries_ripple_across_the_whole_word() {
        let mut bench = Bench::new();
        // pc + 1 across the byte boundary and around the top.
        assert_eq!(
            bench.eval([0x00FF, 0, 0, 0, 0], c::AGU_CIN),
            (0x00FF, 0x0100)
        );
        assert_eq!(
            bench.eval([0xFFFF, 0, 0, 0, 0], c::AGU_CIN),
            (0xFFFF, 0x0000)
        );
        // push: sp − 1 on the bus.
        let push = c::BASE_SP | c::OFF_M1 | c::PRE;
        assert_eq!(bench.eval([0, 0x0000, 0, 0, 0], push), (0xFFFF, 0xFFFF));
        assert_eq!(bench.eval([0, 0x0100, 0, 0, 0], push), (0x00FF, 0x00FF));
        // branch back by 2 from 0x0100.
        let branch = c::OFF_T | c::OFF_SX;
        assert_eq!(bench.eval([0x0100, 0, 0, 0x12FE, 0], branch).1, 0x00FE);
        // table[a] with a = 0xFF.
        let index = c::BASE_PAIR | c::OFF_A | c::PRE;
        assert_eq!(bench.eval([0, 0, 0x1280, 0, 0xFF], index).0, 0x137F);
    }
}
