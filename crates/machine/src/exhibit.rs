//! A small memory made of gates, to show how memory works.
//!
//! The computer's own 64 KiB are behavioural, an array behind the bus
//! (ADR 0002): half a million flip-flops would bury the CPU and stop the
//! gate engine. This is the same idea at a size that can be looked at gate
//! by gate: [`WORDS`] words of [`BITS`] bits, with everything a real memory
//! needs around the cells:
//!
//! - `decode`: the address picks one word line out of eight;
//! - `wordN`: each word is a register that takes the data in on a clock
//!   edge when its line is chosen and writing is on, and keeps its value
//!   otherwise;
//! - `read`: a tree of multiplexers puts the chosen word on the data out.
//!
//! Only `nand` and `dff`, like the CPU. It is a separate netlist: the CPU's
//! is not changed by it.

use silicon_hdl::{Builder, Netlist, Wire};

/// Words in the memory.
pub const WORDS: usize = 8;
/// Bits in a word.
pub const BITS: usize = 4;
/// Bits of the address.
pub const ADDRESS_BITS: usize = 3;

/// The memory and where its pins are.
pub struct Ram {
    pub netlist: Netlist,
    /// Inputs: which word, least significant bit first.
    pub addr: Vec<Wire>,
    /// Inputs: the word to write.
    pub data_in: Vec<Wire>,
    /// Input: write on the next clock edge.
    pub write: Wire,
    /// Outputs: the word at `addr`, at once (reading needs no clock).
    pub data_out: Vec<Wire>,
    /// The flip-flops of each word, least significant bit first.
    pub cells: Vec<Vec<Wire>>,
}

pub fn build_ram() -> Ram {
    let mut b = Builder::new("ram");
    let addr = b.inputs("addr", ADDRESS_BITS);
    let data_in = b.inputs("data_in", BITS);
    let write = b.input("write");
    let cells: Vec<Vec<Wire>> = (0..WORDS).map(|_| b.wires(BITS)).collect();

    let lines = b.scope("decode", |b| b.decode(&addr, write));
    for (i, (cell, &line)) in cells.iter().zip(&lines).enumerate() {
        b.scope(&format!("word{i}"), |b| {
            b.register(cell, &data_in, line);
            b.probe("value", cell);
        });
    }
    let data_out = b.scope("read", |b| {
        let words: Vec<&[Wire]> = cells.iter().map(|c| c.as_slice()).collect();
        b.select(&addr, &words)
    });
    b.outputs("data_out", &data_out);
    let netlist = b.finish().expect("the RAM is a valid netlist");
    Ram {
        netlist,
        addr,
        data_in,
        write,
        data_out,
        cells,
    }
}

/// The same memory in plain Rust: what the gates must do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RamReference {
    pub words: [u8; WORDS],
}

impl RamReference {
    /// What is read at `addr`.
    pub fn read(&self, addr: usize) -> u8 {
        self.words[addr % WORDS]
    }

    /// A clock edge with these inputs.
    pub fn clock(&mut self, addr: usize, data: u8, write: bool) {
        if write {
            self.words[addr % WORDS] = data & ((1 << BITS) - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use silicon_hdl::GateKind;
    use silicon_sim_gate::GateSim;

    fn apply(sim: &mut GateSim, ram: &Ram, addr: usize, data: u8, write: bool) {
        sim.set_word(&ram.addr, addr as u64);
        sim.set_word(&ram.data_in, data as u64);
        sim.set(ram.write, write);
        sim.settle();
    }

    #[test]
    fn it_is_made_of_nands_and_one_flip_flop_per_bit() {
        let ram = build_ram();
        let dffs = ram
            .netlist
            .gates
            .iter()
            .filter(|g| g.kind == GateKind::Dff)
            .count();
        assert_eq!(dffs, WORDS * BITS);
        // Every flip-flop is a cell of some word: no hidden state.
        let mut cells: Vec<Wire> = ram.cells.concat();
        cells.sort();
        let mut outputs = ram.netlist.dff_outputs();
        outputs.sort();
        assert_eq!(cells, outputs);
        let paths: Vec<String> = (0..ram.netlist.modules.len())
            .map(|m| ram.netlist.module_path(silicon_hdl::ModuleId(m as u32)))
            .collect();
        assert!(paths.contains(&"ram/decode".to_string()));
        assert!(paths.contains(&"ram/word7".to_string()));
        assert!(paths.contains(&"ram/read".to_string()));
    }

    #[test]
    fn every_word_holds_every_value_and_reads_back_without_a_clock() {
        let ram = build_ram();
        let mut sim = GateSim::new(ram.netlist.clone()).unwrap();
        for value in 0..1u8 << BITS {
            for addr in 0..WORDS {
                apply(&mut sim, &ram, addr, value ^ addr as u8, true);
                sim.clock();
            }
            for addr in 0..WORDS {
                apply(&mut sim, &ram, addr, 0, false);
                let expected = (value ^ addr as u8) & 0xF;
                assert_eq!(sim.get_word(&ram.data_out), expected as u64, "word {addr}");
            }
        }
    }

    proptest! {
        /// Any sequence of reads and writes: the gates and the reference
        /// read the same at every step.
        #[test]
        fn the_gates_agree_with_the_reference(
            steps in proptest::collection::vec((0usize..WORDS, 0u8..16, any::<bool>()), 1..200),
        ) {
            let ram = build_ram();
            let mut sim = GateSim::new(ram.netlist.clone()).unwrap();
            let mut reference = RamReference::default();
            for (addr, data, write) in steps {
                apply(&mut sim, &ram, addr, data, write);
                prop_assert_eq!(sim.get_word(&ram.data_out) as u8, reference.read(addr));
                sim.clock();
                reference.clock(addr, data, write);
                sim.settle();
                prop_assert_eq!(sim.get_word(&ram.data_out) as u8, reference.read(addr));
                for (w, cell) in ram.cells.iter().enumerate() {
                    prop_assert_eq!(sim.get_word(cell) as u8, reference.words[w]);
                }
            }
        }
    }
}
