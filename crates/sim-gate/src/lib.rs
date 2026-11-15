//! # Silicon gate-level engine
//!
//! Evaluates a [`Netlist`] exactly as built: every NAND, every flip-flop.
//! This engine is the source of truth for the 3D visualization.
//!
//! The implementation is the simplest correct one: one word per wire, one
//! NAND at a time, in levelized order. Measured in WebAssembly it is several
//! times faster than it has to be (ADR 0009).
//!
//! Besides the level of every wire it keeps count of how often each one
//! changed, so that a display that looks sixty times a second can show
//! everything that happened in between and not only where things stood at
//! the instant it looked.

use silicon_hdl::{GateKind, HdlError, Netlist, Wire};

/// The level a wire had when the counts of changes were last cleared is
/// kept in this bit of its word; see [`GateSim::words`].
const START: u32 = 31;
/// The counts of changes are cleared by themselves after this many clock
/// cycles, long before one could reach the [`START`] bit: a wire changes at
/// most a few times in a cycle.
const COUNTING_LIMIT: u64 = 1 << 27;

/// A running instance of a netlist.
pub struct GateSim {
    netlist: Netlist,
    /// NAND gates as `(a, b, out)` wire indices, in levelized order.
    nands: Vec<[u32; 3]>,
    /// Flip-flops as `(d, q)` wire indices.
    dffs: Vec<[u32; 2]>,
    /// One word per wire, which is both its level and the count of its
    /// changes.
    ///
    /// **Bit 0 is the level.** Every time the level changes, one is added to
    /// the word. Adding one flips bit 0, so bit 0 goes on being the level,
    /// and the word as a whole has gone up by exactly the number of changes.
    /// Clearing the counts leaves the level in bit 0 and a copy of it in bit
    /// [`START`]; the count is then the low 31 bits minus that copy.
    ///
    /// This is what lets the inner loop count changes with one extra load
    /// and no second array. Measured in WebAssembly on the CPU (`pnpm
    /// bench`): 260 k cycles/s without counting, 150 k with a saturating
    /// count kept in an array beside the levels, 205 k this way.
    words: Vec<u32>,
    /// Scratch space so that all flip-flops latch simultaneously.
    latch: Vec<u32>,
    cycles: u64,
    /// The cycle at which the counts were last cleared.
    counting_since: u64,
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
        let mut words = vec![0u32; netlist.wire_count as usize];
        words[Wire::ONE.index()] = 1;
        let latch = vec![0u32; dffs.len()];
        let mut sim = GateSim {
            netlist,
            nands,
            dffs,
            words,
            latch,
            cycles: 0,
            counting_since: 0,
        };
        sim.settle();
        // Settling from the all-zero start is not something that happened.
        sim.clear_toggles();
        Ok(sim)
    }

    pub fn netlist(&self) -> &Netlist {
        &self.netlist
    }

    /// Number of wires, including the two constants.
    pub fn wire_count(&self) -> usize {
        self.words.len()
    }

    /// Drive a primary input. Call [`GateSim::settle`] (or `tick`) afterwards.
    pub fn set(&mut self, w: Wire, value: bool) {
        self.drive(w.index(), value as u32);
    }

    /// Give a wire a level (bit 0 of `value`), counting the change if it is one.
    #[inline]
    fn drive(&mut self, wire: usize, value: u32) {
        let word = self.words[wire];
        self.words[wire] = word.wrapping_add((value ^ word) & 1);
    }

    /// Current value of any wire.
    pub fn get(&self, w: Wire) -> bool {
        self.words[w.index()] & 1 != 0
    }

    /// Drive a group of wires with the bits of `value`, least significant
    /// first. Works for primary inputs and, to load a state, for flip-flop
    /// outputs. Call [`GateSim::settle`] afterwards.
    pub fn set_word(&mut self, wires: &[Wire], value: u64) {
        for (i, w) in wires.iter().enumerate() {
            self.drive(w.index(), (value >> i) as u32);
        }
    }

    /// Read a group of wires as a number, least significant bit first.
    pub fn get_word(&self, wires: &[Wire]) -> u64 {
        wires.iter().enumerate().fold(0, |acc, (i, w)| {
            acc | ((self.words[w.index()] & 1) as u64) << i
        })
    }

    /// Propagate the combinational logic until every NAND output is up to date.
    /// One pass suffices because the gates are in levelized order.
    ///
    /// This loop is the gate engine's speed. Only bit 0 of a word is a
    /// level, so only bit 0 of `value` means anything: the rest of it is
    /// whatever the counts of the two inputs happened to give, and the mask
    /// drops it. See [`GateSim::words`] for why adding the change to the
    /// word both sets the level and counts.
    pub fn settle(&mut self) {
        let s = &mut self.words;
        for &[a, b, out] in &self.nands {
            let value = !(s[a as usize] & s[b as usize]);
            let word = s[out as usize];
            s[out as usize] = word.wrapping_add((value ^ word) & 1);
        }
    }

    /// The clock edge alone: every flip-flop takes the value at its `d` input,
    /// all at once. The combinational logic is *not* re-evaluated; callers
    /// that drive inputs between phases of a cycle (the CPU bus) call
    /// [`GateSim::settle`] themselves.
    pub fn clock(&mut self) {
        for (slot, &[d, _]) in self.latch.iter_mut().zip(&self.dffs) {
            *slot = self.words[d as usize];
        }
        for (&v, &[_, q]) in self.latch.iter().zip(&self.dffs) {
            let word = self.words[q as usize];
            self.words[q as usize] = word.wrapping_add((v ^ word) & 1);
        }
        self.cycles += 1;
        if self.cycles - self.counting_since >= COUNTING_LIMIT {
            self.clear_toggles();
        }
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

    /// The level of every wire, by index: 0 or 1.
    pub fn levels(&self) -> impl ExactSizeIterator<Item = u8> + '_ {
        self.words.iter().map(|&word| (word & 1) as u8)
    }

    /// For every wire, by index: how many times its level has changed since
    /// the counts were last cleared.
    ///
    /// Whoever wants the counts clears them after reading them. If nobody
    /// does for 2²⁷ clock cycles they are cleared anyway, so a count never
    /// covers more than that.
    pub fn toggles(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.words
            .iter()
            .map(|&word| (word & !(1 << START)) - (word >> START))
    }

    /// Start counting changes afresh.
    pub fn clear_toggles(&mut self) {
        for word in &mut self.words {
            let level = *word & 1;
            *word = level | level << START;
        }
        self.counting_since = self.cycles;
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

    #[test]
    fn toggles_count_every_change_of_every_wire() {
        // q divides the clock by two; d is its complement.
        let mut b = Builder::new("toggle");
        let q = b.wire();
        let d = b.nand(q, q);
        b.dff_into(q, d);
        let hold = b.input("hold");
        let mut sim = GateSim::new(b.finish().unwrap()).unwrap();
        let count = |sim: &GateSim, w: Wire| sim.toggles().nth(w.index()).unwrap();
        // Power-on is not a change.
        assert!(sim.toggles().all(|n| n == 0));
        assert_eq!(sim.toggles().len(), sim.wire_count());

        for _ in 0..5 {
            sim.tick();
        }
        assert_eq!(count(&sim, q), 5);
        assert_eq!(count(&sim, d), 5);
        // The constants never change, and neither does an input left alone.
        assert_eq!(count(&sim, Wire::ZERO), 0);
        assert_eq!(count(&sim, Wire::ONE), 0);
        assert_eq!(count(&sim, hold), 0);

        // Driving an input counts when the level changes, and only then.
        sim.set(hold, true);
        sim.set(hold, true);
        sim.set_word(&[hold], 0);
        assert_eq!(count(&sim, hold), 2);

        // Settling again changes nothing, so it counts nothing.
        sim.settle();
        assert_eq!(count(&sim, d), 5);

        // Clearing the counts leaves every level as it is, whether the
        // wire is at 1 (q, after five cycles) or at 0 (d).
        assert_eq!((sim.get(q), sim.get(d)), (true, false));
        sim.clear_toggles();
        assert!(sim.toggles().all(|n| n == 0));
        assert_eq!((sim.get(q), sim.get(d)), (true, false));
        assert_eq!(sim.levels().nth(Wire::ONE.index()), Some(1));
        sim.tick();
        assert_eq!((count(&sim, q), count(&sim, d)), (1, 1));
        assert_eq!((sim.get(q), sim.get(d)), (false, true));
    }

    #[test]
    fn counts_left_unread_are_cleared_before_they_can_overflow() {
        let mut b = Builder::new("toggle");
        let q = b.wire();
        let d = b.nand(q, q);
        b.dff_into(q, d);
        let mut sim = GateSim::new(b.finish().unwrap()).unwrap();
        // Pretend the counting began one cycle short of the limit ago.
        sim.cycles = COUNTING_LIMIT - 2;
        sim.tick();
        assert_eq!(sim.toggles().nth(q.index()), Some(1));
        sim.tick();
        // Cleared at the clock edge; what the edge changed settles after it.
        assert_eq!(sim.toggles().nth(q.index()), Some(0));
        assert_eq!(sim.toggles().nth(d.index()), Some(1));
        // The levels went on as if nothing had happened.
        assert!(!sim.get(q));
        sim.tick();
        assert!(sim.get(q));
    }

    proptest! {
        /// The counts are exactly the changes seen by looking at every wire
        /// after every step, on circuits with feedback through flip-flops.
        #[test]
        fn toggles_agree_with_watching_the_wires(
            shape in proptest::collection::vec((0usize..64, 0usize..64), 4..40),
            drive in proptest::collection::vec(any::<u8>(), 1..30),
        ) {
            let mut b = Builder::new("random");
            let ins: Vec<_> = (0..3).map(|i| b.input(&format!("i{i}"))).collect();
            let state: Vec<_> = (0..3).map(|_| b.wire()).collect();
            let mut pool: Vec<Wire> = ins.iter().chain(&state).copied().collect();
            for &(x, y) in &shape {
                let w = b.nand(pool[x % pool.len()], pool[y % pool.len()]);
                pool.push(w);
            }
            for (i, &q) in state.iter().enumerate() {
                b.dff_into(q, pool[pool.len() - 1 - i]);
            }
            let mut sim = GateSim::new(b.finish().unwrap()).unwrap();
            let mut watched = vec![0u32; sim.wire_count()];
            let mut before: Vec<u8> = sim.levels().collect();
            let mut look = |sim: &GateSim| {
                for (w, (now, was)) in sim.levels().zip(before.iter_mut()).enumerate() {
                    watched[w] += (now != *was) as u32;
                    *was = now;
                }
            };
            for &bits in &drive {
                sim.set_word(&ins, bits as u64);
                look(&sim);
                sim.settle();
                look(&sim);
                sim.clock();
                look(&sim);
                sim.settle();
                look(&sim);
            }
            prop_assert_eq!(sim.toggles().collect::<Vec<_>>(), watched);
        }

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
