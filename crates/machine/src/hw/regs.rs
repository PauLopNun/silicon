//! The register file: eight 8-bit registers that are also four 16-bit pairs.
//!
//! Three read ports: `porta` and `portb` (8 bits each, feeding the ALU and
//! the data bus) and `pair` (16 bits, feeding the address unit). Two ways to
//! write, which can happen in the same cycle: one register from the ALU
//! result, and a whole pair from the address unit's sum (post-increment,
//! `lea`). When both target the same register the ALU result wins.
//!
//! The block is built in two visits to the `regs` module: the read ports
//! first, and the flip-flops once the values to be written exist.

use super::control as c;
use silicon_hdl::{Builder, Wire};
use silicon_isa::Reg;

/// What the read ports currently show.
pub struct Ports {
    pub a: Vec<Wire>,
    pub b: Vec<Wire>,
    pub pair: Vec<Wire>,
}

/// The three register numbers of a cycle, each a 3-bit word.
pub struct Selects {
    pub port_a: Vec<Wire>,
    pub port_b: Vec<Wire>,
    pub write: Vec<Wire>,
}

/// Work out which registers this cycle reads and writes, from fields of the
/// opcode and the control word. A "pair half" is `{pair number, HALF}`.
pub fn build_selects(b: &mut Builder, ir: &[Wire], signals: &[Wire]) -> Selects {
    let s = |signal: u64| signals[c::bit(signal)];
    let half = s(c::HALF);
    b.scope("regs", |b| {
        b.scope("select", |b| {
            // Port A: register a (number 0), or a half of the pair in bits 3..2.
            let pd = [half, ir[2], ir[3]];
            let port_a = b.mask_word(&pd, s(c::A_PD));

            // Port B: the register in bits 2..0, a half of the pair in bits
            // 1..0, or register a.
            let ps = [half, ir[0], ir[1]];
            let picked = b.mux_word(s(c::B_PS), &ir[0..3], &ps);
            let not_a = b.not(s(c::B_A));
            let port_b = b.mask_word(&picked, not_a);

            // Write: one of four opcode fields, or register a if none.
            let write = b.select_onehot(&[
                (s(c::W_D), &ir[3..6]),
                (s(c::W_R), &ir[0..3]),
                (s(c::W_PD), &pd),
                (s(c::W_PS), &ps),
            ]);
            Selects {
                port_a,
                port_b,
                write,
            }
        })
    })
}

/// Reference for [`build_selects`]: `(port A, port B, write)` register numbers.
pub fn selects_reference(ir: u8, word: u64) -> (u8, u8, u8) {
    let on = |signal: u64| word & signal != 0;
    let half = on(c::HALF) as u8;
    let pd = (ir >> 2 & 3) << 1 | half;
    let ps = (ir & 3) << 1 | half;
    let port_a = if on(c::A_PD) { pd } else { 0 };
    let port_b = if on(c::B_A) {
        0
    } else if on(c::B_PS) {
        ps
    } else {
        ir & 7
    };
    let mut write = 0;
    if on(c::W_D) {
        write |= ir >> 3 & 7;
    }
    if on(c::W_R) {
        write |= ir & 7;
    }
    if on(c::W_PD) {
        write |= pd;
    }
    if on(c::W_PS) {
        write |= ps;
    }
    (port_a, port_b, write)
}

/// First visit: the read multiplexers. `regs[i]` are the outputs of register
/// `i`'s flip-flops, allocated by the caller before any logic exists.
pub fn build_ports(
    b: &mut Builder,
    regs: &[Vec<Wire>; 8],
    selects: &Selects,
    pair_sel: &[Wire],
) -> Ports {
    b.scope("regs", |b| {
        let all: Vec<&[Wire]> = regs.iter().map(Vec::as_slice).collect();
        let a = b.scope("porta", |b| b.select(&selects.port_a, &all));
        let port_b = b.scope("portb", |b| b.select(&selects.port_b, &all));
        // A pair is two registers side by side, low byte first.
        let pairs: Vec<Vec<Wire>> = regs
            .chunks(2)
            .map(|p| [&p[0][..], &p[1][..]].concat())
            .collect();
        let pairs: Vec<&[Wire]> = pairs.iter().map(Vec::as_slice).collect();
        let pair = b.scope("pair", |b| b.select(pair_sel, &pairs));
        Ports { a, b: port_b, pair }
    })
}

/// Second visit: write decoding and the flip-flops themselves, one module
/// per register (`regs/a`, `regs/xl`…) whose `dff<i>` is bit `i`.
pub fn build_storage(
    b: &mut Builder,
    regs: &[Vec<Wire>; 8],
    result: &[Wire],
    sum: &[Wire],
    selects: &Selects,
    pair_sel: &[Wire],
    signals: &[Wire],
) {
    let s = |signal: u64| signals[c::bit(signal)];
    b.scope("regs", |b| {
        let (write_reg, write_pair) = b.scope("wdec", |b| {
            (
                b.decode(&selects.write, s(c::REG_WE)),
                b.decode(pair_sel, s(c::PAIR_WE)),
            )
        });
        for (i, q) in regs.iter().enumerate() {
            b.scope(Reg::NAMES[i], |b| {
                // Even registers are the low byte of their pair.
                let from_sum = if i % 2 == 0 { &sum[0..8] } else { &sum[8..16] };
                let d = b.mux_word(write_reg[i], from_sum, result);
                let enable = b.or(write_reg[i], write_pair[i / 2]);
                b.register(q, &d, enable);
            });
        }
    })
}

/// Reference for the storage: the registers after one clock edge.
pub fn storage_reference(
    regs: [u8; 8],
    result: u8,
    sum: u16,
    write: u8,
    pair: u8,
    word: u64,
) -> [u8; 8] {
    let mut next = regs;
    if word & c::PAIR_WE != 0 {
        let [lo, hi] = sum.to_le_bytes();
        next[2 * pair as usize] = lo;
        next[2 * pair as usize + 1] = hi;
    }
    if word & c::REG_WE != 0 {
        next[write as usize] = result;
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use silicon_sim_gate::GateSim;

    #[test]
    fn selects_match_reference_for_every_opcode_and_setting() {
        let mut b = Builder::new("test");
        let ir = b.inputs("ir", 8);
        let signals = b.inputs("signal", c::SIGNAL_COUNT);
        let selects = build_selects(&mut b, &ir, &signals);
        let mut sim = GateSim::new(b.finish().unwrap()).unwrap();
        // Every combination the microcode can ask for, and a few it cannot.
        let a_modes = [0, c::A_PD];
        let b_modes = [0, c::B_PS, c::B_A];
        let w_modes = [0, c::W_D, c::W_R, c::W_PD, c::W_PS];
        for opcode in 0..=255u8 {
            for &am in &a_modes {
                for &bm in &b_modes {
                    for &wm in &w_modes {
                        for half in [0, c::HALF] {
                            let word = am | bm | wm | half;
                            sim.set_word(&ir, opcode as u64);
                            sim.set_word(&signals, word);
                            sim.settle();
                            let got = (
                                sim.get_word(&selects.port_a) as u8,
                                sim.get_word(&selects.port_b) as u8,
                                sim.get_word(&selects.write) as u8,
                            );
                            assert_eq!(
                                got,
                                selects_reference(opcode, word),
                                "{opcode:#x} {word:#x}"
                            );
                        }
                    }
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// Random sequences of writes; after each clock edge every register
        /// and every read port must agree with the reference.
        #[test]
        fn storage_and_ports_follow_the_reference(
            steps in proptest::collection::vec(
                (any::<u8>(), any::<u16>(), 0..8u8, 0..4u8, any::<bool>(), any::<bool>(), 0..8u8, 0..8u8),
                1..60,
            )
        ) {
            let mut b = Builder::new("test");
            let result = b.inputs("result", 8);
            let sum = b.inputs("sum", 16);
            let port_a = b.inputs("sel_a", 3);
            let port_b = b.inputs("sel_b", 3);
            let write = b.inputs("write", 3);
            let pair_sel = b.inputs("pair_sel", 2);
            let signals = b.inputs("signal", c::SIGNAL_COUNT);
            let regs: [Vec<Wire>; 8] = std::array::from_fn(|_| b.wires(8));
            let selects = Selects { port_a, port_b, write };
            let ports = build_ports(&mut b, &regs, &selects, &pair_sel);
            build_storage(&mut b, &regs, &result, &sum, &selects, &pair_sel, &signals);
            let nl = b.finish().unwrap();
            prop_assert_eq!(nl.stats().dffs, 64);
            let mut sim = GateSim::new(nl).unwrap();

            let mut model = [0u8; 8];
            for (res, s, w, p, reg_we, pair_we, ra, rb) in steps {
                let word = if reg_we { c::REG_WE } else { 0 } | if pair_we { c::PAIR_WE } else { 0 };
                sim.set_word(&result, res as u64);
                sim.set_word(&sum, s as u64);
                sim.set_word(&selects.write, w as u64);
                sim.set_word(&pair_sel, p as u64);
                sim.set_word(&signals, word);
                sim.set_word(&selects.port_a, ra as u64);
                sim.set_word(&selects.port_b, rb as u64);
                sim.tick();
                model = storage_reference(model, res, s, w, p, word);
                for (i, q) in regs.iter().enumerate() {
                    prop_assert_eq!(sim.get_word(q) as u8, model[i], "register {}", Reg::NAMES[i]);
                }
                prop_assert_eq!(sim.get_word(&ports.a) as u8, model[ra as usize]);
                prop_assert_eq!(sim.get_word(&ports.b) as u8, model[rb as usize]);
                let pair = u16::from_le_bytes([model[2 * p as usize], model[2 * p as usize + 1]]);
                prop_assert_eq!(sim.get_word(&ports.pair) as u16, pair);
            }
        }
    }

    #[test]
    fn each_register_is_a_module_whose_dffs_are_its_bits() {
        let mut b = Builder::new("test");
        let zeros = b.inputs("z", 16);
        let regs: [Vec<Wire>; 8] = std::array::from_fn(|_| b.wires(8));
        let selects = Selects {
            port_a: zeros[0..3].to_vec(),
            port_b: zeros[0..3].to_vec(),
            write: zeros[0..3].to_vec(),
        };
        let signals = vec![zeros[0]; c::SIGNAL_COUNT];
        build_storage(
            &mut b,
            &regs,
            &zeros[0..8],
            &zeros,
            &selects,
            &zeros[0..2],
            &signals,
        );
        let nl = b.finish().unwrap();
        for (i, gate) in nl.gates.iter().enumerate() {
            if gate.out == regs[Reg::Yh.index()][5] {
                assert_eq!(nl.gate_path(i), "test/regs/yh/dff5");
            }
        }
    }
}
