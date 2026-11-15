//! The memory made of gates (`silicon_machine::exhibit`), for the page to
//! show and to be played with: choose an address and a word, write it on a
//! clock edge, read it back. It runs in its own gate-level simulator, apart
//! from the computer.

use crate::machine::{activity_byte, gate_levels, gate_table, module_names, module_parents};
use silicon_machine::exhibit::{self, Ram};
use silicon_sim_gate::GateSim;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct RamExhibit {
    ram: Ram,
    sim: GateSim,
    levels: Vec<u8>,
    activity: Vec<u8>,
    /// Clock edges since the wires were last looked at.
    edges: u64,
}

impl Default for RamExhibit {
    fn default() -> Self {
        RamExhibit::new()
    }
}

#[wasm_bindgen]
impl RamExhibit {
    /// The memory with every word at 0, nothing being written, address 0.
    #[wasm_bindgen(constructor)]
    pub fn new() -> RamExhibit {
        let ram = exhibit::build_ram();
        let sim = GateSim::new(ram.netlist.clone()).expect("the RAM simulates");
        let wires = sim.wire_count();
        let mut exhibit = RamExhibit {
            ram,
            sim,
            levels: vec![0; wires],
            activity: vec![0; wires],
            edges: 0,
        };
        exhibit.look_at_wires();
        exhibit
    }

    /// Set the inputs: which word, the word to write, and whether the next
    /// clock edge writes. Reading follows at once: no clock is needed.
    pub fn set_inputs(&mut self, addr: u8, data: u8, write: bool) {
        self.sim.set_word(&self.ram.addr, addr as u64);
        self.sim.set_word(&self.ram.data_in, data as u64);
        self.sim.set(self.ram.write, write);
        self.sim.settle();
    }

    /// One clock edge: if writing is on, the chosen word takes the data in.
    pub fn clock(&mut self) {
        self.sim.clock();
        self.sim.settle();
        self.edges += 1;
    }

    /// What the memory puts on its data out: the word at the address.
    pub fn read(&self) -> u8 {
        self.sim.get_word(&self.ram.data_out) as u8
    }

    /// Every word, as its flip-flops hold it.
    pub fn words(&self) -> Vec<u8> {
        self.ram
            .cells
            .iter()
            .map(|cell| self.sim.get_word(cell) as u8)
            .collect()
    }

    /// Words, bits per word, address bits.
    pub fn shape(&self) -> Vec<u32> {
        vec![
            exhibit::WORDS as u32,
            exhibit::BITS as u32,
            exhibit::ADDRESS_BITS as u32,
        ]
    }

    /// Note every wire's level, and how busy it has been since the last
    /// look; read through [`RamExhibit::wires_ptr`] and
    /// [`RamExhibit::activity_ptr`]. A change of the inputs counts as one
    /// step, like a clock edge.
    pub fn look_at_wires(&mut self) {
        let steps = self.edges.max(1);
        for (byte, level) in self.levels.iter_mut().zip(self.sim.levels()) {
            *byte = level;
        }
        for (byte, count) in self.activity.iter_mut().zip(self.sim.toggles()) {
            *byte = activity_byte(count, steps);
        }
        self.sim.clear_toggles();
        self.edges = 0;
    }

    pub fn wires_ptr(&self) -> *const u8 {
        self.levels.as_ptr()
    }

    pub fn wires_len(&self) -> usize {
        self.levels.len()
    }

    pub fn activity_ptr(&self) -> *const u8 {
        self.activity.as_ptr()
    }

    pub fn gate_table(&self) -> Vec<u32> {
        gate_table(&self.ram.netlist)
    }

    pub fn gate_levels(&self) -> Vec<u32> {
        gate_levels(&self.ram.netlist)
    }

    pub fn module_parents(&self) -> Vec<i32> {
        module_parents(&self.ram.netlist)
    }

    pub fn module_names(&self) -> String {
        module_names(&self.ram.netlist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silicon_machine::exhibit::RamReference;

    #[test]
    fn written_words_are_read_back() {
        let mut ram = RamExhibit::new();
        let mut reference = RamReference::default();
        for (addr, data) in [(3u8, 0xA), (0, 0x5), (7, 0xF), (3, 0x1)] {
            ram.set_inputs(addr, data, true);
            ram.clock();
            reference.clock(addr as usize, data, true);
        }
        ram.set_inputs(3, 0, false);
        assert_eq!(ram.read(), 0x1);
        // Clocking with writing off changes nothing.
        ram.clock();
        assert_eq!(ram.words(), reference.words);
        assert_eq!(ram.shape(), [8, 4, 3]);
    }

    #[test]
    fn its_wires_and_its_netlist_are_described_like_the_cpu() {
        let mut ram = RamExhibit::new();
        let gates = ram.gate_table();
        assert_eq!(gates.len() % crate::GATE_FIELDS, 0);
        assert_eq!(gates.chunks(5).filter(|g| g[0] == 1).count(), 32);
        let names: Vec<String> = ram.module_names().lines().map(String::from).collect();
        assert_eq!(names[0], "ram");
        assert_eq!(names.len(), ram.module_parents().len());
        ram.look_at_wires();
        assert!(ram.activity.iter().all(|&a| a == 0));
        ram.set_inputs(5, 0xF, true);
        ram.clock();
        ram.look_at_wires();
        assert!(ram.activity.iter().any(|&a| a > 0));
        assert_eq!(ram.wires_len(), ram.levels.len());
    }
}
