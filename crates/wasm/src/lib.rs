//! # Silicon WebAssembly bindings
//!
//! The only crate the browser talks to. It runs inside a Web Worker and
//! exposes simulation state as pointers into WASM linear memory, so the worker
//! can view it with a typed array instead of copying it through JS objects.
//!
//! M0 exposes a four-gate XOR demo to prove the whole pipeline
//! (Rust → WASM → worker → renderer). The real `Machine` binding replaces it
//! in M2.

use silicon_hdl::{demo, Wire};
use silicon_machine::EngineKind;
use silicon_sim_gate::GateSim;
use wasm_bindgen::prelude::*;

/// Version of the simulation core, shown in the debug overlay.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// A live gate-level XOR built from four NAND gates.
#[wasm_bindgen]
pub struct XorDemo {
    sim: GateSim,
    a: Wire,
    b: Wire,
    y: Wire,
}

impl Default for XorDemo {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl XorDemo {
    #[wasm_bindgen(constructor)]
    pub fn new() -> XorDemo {
        let nl = demo::xor();
        let (a, b, y) = (nl.inputs[0].1, nl.inputs[1].1, nl.outputs[0].1);
        XorDemo {
            sim: GateSim::new(nl).expect("demo netlist is valid"),
            a,
            b,
            y,
        }
    }

    /// Name of the engine producing the state ("gate" here, always).
    pub fn engine(&self) -> String {
        EngineKind::Gate.name().to_string()
    }

    /// Drive both inputs and let the gates settle.
    pub fn set_inputs(&mut self, a: bool, b: bool) {
        self.sim.set(self.a, a);
        self.sim.set(self.b, b);
        self.sim.settle();
    }

    pub fn output(&self) -> bool {
        self.sim.get(self.y)
    }

    /// Address of the wire-state buffer (one byte per wire) in WASM memory.
    pub fn state_ptr(&self) -> *const u8 {
        self.sim.state().as_ptr()
    }

    pub fn state_len(&self) -> usize {
        self.sim.state().len()
    }

    pub fn gate_count(&self) -> usize {
        self.sim.netlist().gates.len()
    }

    /// Hierarchical path of gate `i`, e.g. `demo/xor/nand2`.
    pub fn gate_path(&self, i: usize) -> String {
        self.sim.netlist().gate_path(i)
    }

    /// Wire indices `[a, b, out]` of gate `i` into the state buffer.
    pub fn gate_wires(&self, i: usize) -> Vec<u32> {
        let g = &self.sim.netlist().gates[i];
        vec![g.a.0, g.b.0, g.out.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_is_a_working_xor_with_readable_state() {
        let mut d = XorDemo::new();
        assert_eq!(d.gate_count(), 4);
        assert_eq!(d.gate_path(3), "demo/xor/nand3");
        for (a, b) in [(false, false), (false, true), (true, false), (true, true)] {
            d.set_inputs(a, b);
            assert_eq!(d.output(), a ^ b);
            let out_wire = d.gate_wires(3)[2] as usize;
            assert_eq!(d.sim.state()[out_wire] != 0, a ^ b);
        }
        assert_eq!(d.engine(), "gate");
    }
}
