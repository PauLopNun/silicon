//! Multi-bit building blocks. A *word* is a slice of wires, least
//! significant bit first.
//!
//! Like the gates, these place their NANDs in the current module; the caller
//! decides where a block boundary (a [`Builder::scope`]) goes.

use crate::{Builder, Wire};

/// The constant `value` as an `n`-bit word (no gates: wires 0 and 1).
pub fn constant(value: u64, n: usize) -> Vec<Wire> {
    (0..n)
        .map(|i| {
            if value >> i & 1 == 1 {
                Wire::ONE
            } else {
                Wire::ZERO
            }
        })
        .collect()
}

impl Builder {
    /// Bitwise NOT of a word.
    pub fn not_word(&mut self, a: &[Wire]) -> Vec<Wire> {
        a.iter().map(|&w| self.not(w)).collect()
    }

    /// Every bit of `a` ANDed with the single wire `enable`.
    pub fn mask_word(&mut self, a: &[Wire], enable: Wire) -> Vec<Wire> {
        a.iter().map(|&w| self.and(w, enable)).collect()
    }

    /// Every bit of `a` XORed with the single wire `invert`: a controlled
    /// inverter, the trick that turns an adder into a subtractor.
    pub fn xor_word(&mut self, a: &[Wire], invert: Wire) -> Vec<Wire> {
        a.iter().map(|&w| self.xor(w, invert)).collect()
    }

    /// `sel ? a1 : a0` for whole words; the select is inverted once.
    pub fn mux_word(&mut self, sel: Wire, a0: &[Wire], a1: &[Wire]) -> Vec<Wire> {
        assert_eq!(a0.len(), a1.len(), "mux inputs differ in width");
        let nsel = self.not(sel);
        a0.iter()
            .zip(a1)
            .map(|(&w0, &w1)| self.mux_with(sel, nsel, w0, w1))
            .collect()
    }

    /// Pick `inputs[sel]` with a tree of 2-way multiplexers. `sel` is a
    /// binary number, least significant bit first, and there must be exactly
    /// `2^sel.len()` inputs.
    pub fn select(&mut self, sel: &[Wire], inputs: &[&[Wire]]) -> Vec<Wire> {
        assert_eq!(
            inputs.len(),
            1 << sel.len(),
            "need 2^n inputs for n select bits"
        );
        let mut layer: Vec<Vec<Wire>> = inputs.iter().map(|w| w.to_vec()).collect();
        for &s in sel {
            let ns = self.not(s);
            layer = layer
                .chunks(2)
                .map(|pair| {
                    pair[0]
                        .iter()
                        .zip(&pair[1])
                        .map(|(&w0, &w1)| self.mux_with(s, ns, w0, w1))
                        .collect()
                })
                .collect();
        }
        layer.pop().expect("at least one input")
    }

    /// AND-OR multiplexer for selects that are already one-hot: the output is
    /// the word whose select wire is high, and zero if none is.
    pub fn select_onehot(&mut self, choices: &[(Wire, &[Wire])]) -> Vec<Wire> {
        let width = choices.first().map_or(0, |c| c.1.len());
        (0..width)
            .map(|bit| {
                let terms: Vec<Wire> = choices
                    .iter()
                    .map(|&(sel, word)| self.nand(sel, word[bit]))
                    .collect();
                self.nand_all(&terms)
            })
            .collect()
    }

    /// Binary decoder: `2^n` lines, line `i` is high when `sel == i` and
    /// `enable` is high.
    pub fn decode(&mut self, sel: &[Wire], enable: Wire) -> Vec<Wire> {
        let nsel = self.not_word(sel);
        (0..1usize << sel.len())
            .map(|value| {
                let mut literals: Vec<Wire> = (0..sel.len())
                    .map(|bit| {
                        if value >> bit & 1 == 1 {
                            sel[bit]
                        } else {
                            nsel[bit]
                        }
                    })
                    .collect();
                if enable != Wire::ONE {
                    literals.push(enable);
                }
                self.and_all(&literals)
            })
            .collect()
    }

    /// A register: one flip-flop per bit of the pre-allocated word `q`, which
    /// takes `d` on a clock edge when `enable` is high and keeps its value
    /// otherwise. The flip-flops are created in bit order, so inside its
    /// module bit `i` is `dff<i>`.
    pub fn register(&mut self, q: &[Wire], d: &[Wire], enable: Wire) {
        assert_eq!(q.len(), d.len(), "register width mismatch");
        let hold = self.not(enable);
        for (&q, &d) in q.iter().zip(d) {
            let next = self.mux_with(enable, hold, q, d);
            self.dff_into(q, next);
        }
    }
}
