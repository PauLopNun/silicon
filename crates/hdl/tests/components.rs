//! Every component of the library against its plain Rust meaning, simulated
//! gate by gate. Small input spaces are checked exhaustively.

use proptest::prelude::*;
use silicon_hdl::arith::add_reference;
use silicon_hdl::gates::reference;
use silicon_hdl::word::constant;
use silicon_hdl::{Builder, Netlist, Wire};
use silicon_sim_gate::GateSim;

/// Build a combinational circuit with `n_in` input bits, then call `check`
/// with a function that evaluates it for a given input value.
fn circuit(
    n_in: usize,
    build: impl FnOnce(&mut Builder, &[Wire]) -> Vec<Wire>,
) -> (impl FnMut(u64) -> u64, Netlist) {
    let mut b = Builder::new("test");
    let ins = b.inputs("in", n_in);
    let outs = build(&mut b, &ins);
    b.outputs("out", &outs);
    let netlist = b.finish().expect("valid netlist");
    let mut sim = GateSim::new(netlist.clone()).unwrap();
    let eval = move |value: u64| {
        sim.set_word(&ins, value);
        sim.settle();
        sim.get_word(&outs)
    };
    (eval, netlist)
}

fn bit(value: u64, i: usize) -> bool {
    value >> i & 1 == 1
}

#[test]
fn two_input_gates_truth_tables() {
    type Gate = fn(&mut Builder, Wire, Wire) -> Wire;
    type Meaning = fn(bool, bool) -> bool;
    // (name, how to build it, what it means, how many NANDs it costs)
    let gates: [(&str, Gate, Meaning, usize); 4] = [
        ("and", |b, x, y| b.and(x, y), |x, y| x & y, 2),
        ("or", |b, x, y| b.or(x, y), |x, y| x | y, 3),
        ("nor", |b, x, y| b.nor(x, y), |x, y| !(x | y), 4),
        ("xor", |b, x, y| b.xor(x, y), |x, y| x ^ y, 4),
    ];
    for (name, gate, meaning, nands) in gates {
        let (mut eval, nl) = circuit(2, |b, i| vec![gate(b, i[0], i[1])]);
        assert_eq!(nl.stats().nands, nands, "{name} gate count");
        for v in 0..4 {
            assert_eq!(
                eval(v) == 1,
                meaning(bit(v, 0), bit(v, 1)),
                "{name}({v:02b})"
            );
        }
    }
    let (mut eval, _) = circuit(1, |b, i| vec![b.not(i[0])]);
    assert_eq!((eval(0), eval(1)), (1, 0));
}

#[test]
fn mux_selects_between_two_wires() {
    let (mut eval, nl) = circuit(3, |b, i| vec![b.mux(i[0], i[1], i[2])]);
    assert_eq!(nl.stats().nands, 4);
    for v in 0..8 {
        assert_eq!(
            eval(v) == 1,
            reference::mux(bit(v, 0), bit(v, 1), bit(v, 2))
        );
    }
}

#[test]
fn wide_and_or_nand_trees() {
    for n in 0..=9usize {
        let (mut and, _) = circuit(n, |b, i| vec![b.and_all(i)]);
        let (mut nand, _) = circuit(n, |b, i| vec![b.nand_all(i)]);
        let (mut or, _) = circuit(n, |b, i| vec![b.or_all(i)]);
        let all = (1u64 << n) - 1;
        for v in 0..=all {
            assert_eq!(and(v) == 1, v == all, "and_all, {n} inputs, {v:b}");
            assert_eq!(nand(v) == 1, v != all, "nand_all, {n} inputs, {v:b}");
            assert_eq!(or(v) == 1, v != 0, "or_all, {n} inputs, {v:b}");
        }
    }
}

#[test]
fn full_adder_truth_table() {
    let (mut eval, nl) = circuit(3, |b, i| {
        let (s, c) = b.full_adder(i[0], i[1], i[2]);
        vec![s, c]
    });
    assert_eq!(nl.stats().nands, 9);
    for v in 0..8u64 {
        assert_eq!(eval(v), v.count_ones() as u64, "a + b + cin for {v:03b}");
    }
}

#[test]
fn adder8_exhaustive() {
    // Inputs: x (8), y (8), cin (1) = 2^17 vectors.
    let (mut eval, nl) = circuit(17, |b, i| {
        let s = b.scope("adder8", |b| b.adder(&i[0..8], &i[8..16], i[16]));
        let mut out = s.sum.clone();
        out.push(s.carry_out());
        out
    });
    assert_eq!(nl.stats().nands, 72);
    assert_eq!(nl.gate_path(9 * 3 + 2), "test/adder8/fa3/nand2");
    for v in 0..1u64 << 17 {
        let (x, y, cin) = (v & 0xFF, v >> 8 & 0xFF, bit(v, 16));
        let (sum, carry) = add_reference(x, y, cin, 8);
        assert_eq!(eval(v), sum | (carry as u64) << 8, "{x} + {y} + {cin}");
    }
}

proptest! {
    #[test]
    fn adder16_matches_integer_addition(x: u16, y: u16, cin: bool) {
        let (mut eval, _) = circuit(33, |b, i| {
            let s = b.adder(&i[0..16], &i[16..32], i[32]);
            let mut out = s.sum.clone();
            out.push(s.carry_out());
            out
        });
        let input = x as u64 | (y as u64) << 16 | (cin as u64) << 32;
        let (sum, carry) = add_reference(x as u64, y as u64, cin, 16);
        prop_assert_eq!(eval(input), sum | (carry as u64) << 16);
    }

    #[test]
    fn intermediate_carries_are_exposed(x: u8, y: u8) {
        let (mut eval, _) = circuit(16, |b, i| b.adder(&i[0..8], &i[8..16], Wire::ZERO).carries);
        let carries = eval(x as u64 | (y as u64) << 8);
        for i in 0..8 {
            let mask = (1u64 << (i + 1)) - 1;
            let expected = (x as u64 & mask) + (y as u64 & mask) > mask;
            prop_assert_eq!(bit(carries, i), expected, "carry out of bit {}", i);
        }
    }
}

#[test]
fn word_helpers_exhaustive() {
    // not / mask / xor on 4-bit words with a 1-bit control.
    let (mut not, _) = circuit(4, |b, i| b.not_word(i));
    let (mut mask, _) = circuit(5, |b, i| b.mask_word(&i[0..4], i[4]));
    let (mut xor, _) = circuit(5, |b, i| b.xor_word(&i[0..4], i[4]));
    let (mut mux, _) = circuit(9, |b, i| b.mux_word(i[8], &i[0..4], &i[4..8]));
    for v in 0..16u64 {
        assert_eq!(not(v), !v & 0xF);
        assert_eq!(mask(v), 0);
        assert_eq!(mask(v | 16), v);
        assert_eq!(xor(v), v);
        assert_eq!(xor(v | 16), !v & 0xF);
    }
    for v in 0..512u64 {
        let expected = if bit(v, 8) { v >> 4 & 0xF } else { v & 0xF };
        assert_eq!(mux(v), expected);
    }
}

#[test]
fn select_picks_one_of_eight_words() {
    // 8 words of 2 bits + 3 select bits = 19 inputs.
    let (mut eval, nl) = circuit(19, |b, i| {
        let words: Vec<&[Wire]> = i[0..16].chunks(2).collect();
        b.select(&i[16..19], &words)
    });
    // 7 two-way muxes per bit, 3 NANDs each, plus one inverter per select.
    assert_eq!(nl.stats().nands, 2 * 7 * 3 + 3);
    for v in 0..1u64 << 19 {
        let sel = v >> 16;
        assert_eq!(eval(v), v >> (2 * sel) & 3);
    }
}

#[test]
fn select_onehot_is_an_and_or_of_the_selected_words() {
    // 3 words of 2 bits + 3 select wires.
    let (mut eval, _) = circuit(9, |b, i| {
        b.select_onehot(&[(i[6], &i[0..2]), (i[7], &i[2..4]), (i[8], &i[4..6])])
    });
    for v in 0..512u64 {
        let mut expected = 0;
        for k in 0..3 {
            if bit(v, 6 + k) {
                expected |= v >> (2 * k) & 3;
            }
        }
        assert_eq!(eval(v), expected, "{v:09b}");
    }
}

#[test]
fn decoder_lines_are_one_hot() {
    let (mut eval, _) = circuit(4, |b, i| b.decode(&i[0..3], i[3]));
    for v in 0..8u64 {
        assert_eq!(eval(v), 0, "disabled");
        assert_eq!(eval(v | 8), 1 << v);
    }
    let (mut always, _) = circuit(2, |b, i| b.decode(i, Wire::ONE));
    for v in 0..4u64 {
        assert_eq!(always(v), 1 << v);
    }
}

#[test]
fn constants_need_no_gates() {
    let (mut eval, nl) = circuit(0, |_, _| constant(0xA5, 8));
    assert_eq!(nl.stats().nands, 0);
    assert_eq!(eval(0), 0xA5);
}

proptest! {
    /// A register follows `d` only on cycles where `enable` is high.
    #[test]
    fn register_loads_when_enabled(steps in proptest::collection::vec((any::<u8>(), any::<bool>()), 1..40)) {
        let mut b = Builder::new("test");
        let d = b.inputs("d", 8);
        let enable = b.input("en");
        let q = b.wires(8);
        b.scope("r", |b| b.register(&q, &d, enable));
        let nl = b.finish().unwrap();
        prop_assert_eq!(nl.stats().dffs, 8);
        prop_assert_eq!(nl.gate_path(nl.gates.len() - 1), "test/r/dff7");
        let mut sim = GateSim::new(nl).unwrap();
        let mut model = 0u8;
        for (value, en) in steps {
            sim.set_word(&d, value as u64);
            sim.set(enable, en);
            sim.tick();
            if en {
                model = value;
            }
            prop_assert_eq!(sim.get_word(&q) as u8, model);
        }
    }
}
