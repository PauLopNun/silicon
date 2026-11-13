//! # Silicon gate-level engine
//!
//! Evaluates a [`Netlist`] exactly as built: every NAND, every flip-flop.
//! This engine is the source of truth for the 3D visualization.
//!
//! M0 ships the simplest correct implementation (one byte per wire, scalar,
//! levelized). M2 replaces the inner loop with a bit-parallel one behind the
//! same API; see `docs/ARCHITECTURE.md` §4.

use silicon_hdl::{GateKind, HdlError, Netlist, Wire};

/// A running instance of a netlist.
pub struct GateSim {
    netlist: Netlist,
    /// NAND gates as `(a, b, out)` wire indices, in levelized order.
    nands: Vec<[u32; 3]>,
    /// Flip-flops as `(d, q)` wire indices.
    dffs: Vec<[u32; 2]>,
    /// One byte per wire, `0` or `1`. Exposed to the renderer as-is.
    state: Vec<u8>,
    /// Scratch space so that all flip-flops latch simultaneously.
    latch: Vec<u8>,
    cycles: u64,
}

impl GateSim {
    /// Compile `netlist` for simulation. All flip-flops start at 0.
    pub fn new(netlist: Netlist) -> Result<Self, HdlError> {
        let lv = netlist.levelize()?;
        let nands = lv
            .order
            .iter()
            .map(|&i| {
                let g = &netlist.gates[i as usize];
                [g.a.0, g.b.0, g.out.0]
            })
            .collect();
        let dffs: Vec<[u32; 2]> = netlist
            .gates
            .iter()
            .filter(|g| g.kind == GateKind::Dff)
            .map(|g| [g.a.0, g.out.0])
            .collect();
        let mut state = vec![0u8; netlist.wire_count as usize];
        state[Wire::ONE.index()] = 1;
        let latch = vec![0u8; dffs.len()];
        let mut sim = GateSim {
            netlist,
            nands,
            dffs,
            state,
            latch,
            cycles: 0,
        };
        sim.settle();
        Ok(sim)
    }

    pub fn netlist(&self) -> &Netlist {
        &self.netlist
    }

    /// Drive a primary input. Call [`GateSim::settle`] (or `tick`) afterwards.
    pub fn set(&mut self, w: Wire, value: bool) {
        self.state[w.index()] = value as u8;
    }

    /// Current value of any wire.
    pub fn get(&self, w: Wire) -> bool {
        self.state[w.index()] != 0
    }

    /// Drive a group of wires with the bits of `value`, least significant
    /// first. Works for primary inputs and, to load a state, for flip-flop
    /// outputs. Call [`GateSim::settle`] afterwards.
    pub fn set_word(&mut self, wires: &[Wire], value: u64) {
        for (i, w) in wires.iter().enumerate() {
            self.state[w.index()] = (value >> i & 1) as u8;
        }
    }

    /// Read a group of wires as a number, least significant bit first.
    pub fn get_word(&self, wires: &[Wire]) -> u64 {
        wires
            .iter()
            .enumerate()
            .fold(0, |acc, (i, w)| acc | (self.state[w.index()] as u64) << i)
    }

    /// Propagate the combinational logic until every NAND output is up to date.
    /// One pass suffices because the gates are in levelized order.
    pub fn settle(&mut self) {
        let s = &mut self.state;
        for &[a, b, out] in &self.nands {
            s[out as usize] = 1 ^ (s[a as usize] & s[b as usize]);
        }
    }

    /// The clock edge alone: every flip-flop takes the value at its `d` input,
    /// all at once. The combinational logic is *not* re-evaluated; callers
    /// that drive inputs between phases of a cycle (the CPU bus) call
    /// [`GateSim::settle`] themselves.
    pub fn clock(&mut self) {
        for (slot, &[d, _]) in self.latch.iter_mut().zip(&self.dffs) {
            *slot = self.state[d as usize];
        }
        for (&v, &[_, q]) in self.latch.iter().zip(&self.dffs) {
            self.state[q as usize] = v;
        }
        self.cycles += 1;
    }

    /// One clock cycle: settle, clock every flip-flop at once, settle again.
    pub fn tick(&mut self) {
        self.settle();
        self.clock();
        self.settle();
    }

    /// Clock cycles executed so far.
    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Dense wire-state buffer (one byte per wire), for zero-copy rendering.
    pub fn state(&self) -> &[u8] {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use silicon_hdl::{demo, Builder};

    #[test]
    fn xor_truth_table() {
        let nl = demo::xor();
        let (a, b, y) = (nl.inputs[0].1, nl.inputs[1].1, nl.outputs[0].1);
        let mut sim = GateSim::new(nl).unwrap();
        for (va, vb) in [(false, false), (false, true), (true, false), (true, true)] {
            sim.set(a, va);
            sim.set(b, vb);
            sim.settle();
            assert_eq!(sim.get(y), va ^ vb, "{va} xor {vb}");
        }
    }

    #[test]
    fn toggle_flip_flop_divides_the_clock_by_two() {
        let mut b = Builder::new("toggle");
        let q = b.wire();
        let d = b.nand(q, q);
        b.dff_into(q, d);
        b.output("q", q);
        let mut sim = GateSim::new(b.finish().unwrap()).unwrap();
        for cycle in 0..8 {
            assert_eq!(sim.get(q), cycle % 2 == 1);
            sim.tick();
        }
        assert_eq!(sim.cycles(), 8);
    }

    #[test]
    fn flip_flops_latch_simultaneously() {
        // Two flip-flops swapping values must not see each other's new value.
        let mut b = Builder::new("swap");
        let load = b.input("load");
        let (q0, q1) = (b.wire(), b.wire());
        // d0 = load ? 1 : q1  ==  nand(nand(load,1), nand(!load,q1))
        let nload = b.nand(load, load);
        let t = b.nand(nload, q1);
        let d0 = b.nand(nload, t);
        // d1 = load ? 0 : q0  ==  !nand(!load, q0)
        let u = b.nand(nload, q0);
        let d1 = b.nand(u, u);
        b.dff_into(q0, d0);
        b.dff_into(q1, d1);
        let mut sim = GateSim::new(b.finish().unwrap()).unwrap();
        sim.set(load, true);
        sim.tick();
        assert_eq!((sim.get(q0), sim.get(q1)), (true, false));
        sim.set(load, false);
        for i in 0..6 {
            sim.tick();
            let swapped = i % 2 == 0;
            assert_eq!((sim.get(q0), sim.get(q1)), (!swapped, swapped));
        }
    }

    proptest! {
        /// A ripple chain of XORs computes parity, whatever the inputs.
        #[test]
        fn xor_chain_is_parity(bits in proptest::collection::vec(any::<bool>(), 1..40)) {
            let mut b = Builder::new("parity");
            let ins: Vec<_> = (0..bits.len()).map(|i| b.input(&format!("i{i}"))).collect();
            let mut acc = Wire::ZERO;
            for (i, &w) in ins.iter().enumerate() {
                acc = b.scope(&format!("x{i}"), |b| {
                    let n0 = b.nand(acc, w);
                    let n1 = b.nand(acc, n0);
                    let n2 = b.nand(w, n0);
                    b.nand(n1, n2)
                });
            }
            let mut sim = GateSim::new(b.finish().unwrap()).unwrap();
            for (&w, &v) in ins.iter().zip(&bits) {
                sim.set(w, v);
            }
            sim.settle();
            prop_assert_eq!(sim.get(acc), bits.iter().filter(|&&v| v).count() % 2 == 1);
        }
    }
}
