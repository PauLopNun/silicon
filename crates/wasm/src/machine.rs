//! The whole computer, as the worker sees it.
//!
//! Bulk state (the screen, every wire of the CPU) is exposed as a pointer and
//! a length into WASM memory, to be viewed with a typed array. Static
//! information about the netlist is returned once as flat arrays.

use silicon_hdl::GateKind;
use silicon_machine::board::RomTooLarge;
use silicon_machine::{demo, EngineKind, Machine as Core};
use wasm_bindgen::prelude::*;

/// Number of `u32` entries per gate in [`Machine::gate_table`].
pub const GATE_FIELDS: usize = 5;
/// Number of bytes returned by [`Machine::arch_state`].
pub const ARCH_STATE_BYTES: usize = 17;

/// A Silicon computer: CPU, memory and devices.
#[wasm_bindgen]
pub struct Machine {
    core: Core,
}

impl Machine {
    /// The constructor without the JavaScript error type, for native tests.
    pub fn with_program(program: &[u8]) -> Result<Machine, RomTooLarge> {
        Ok(Machine {
            core: Core::new(program)?,
        })
    }

    /// [`Machine::load`] without the JavaScript error type. The error is
    /// what the toolchain reported, one line per problem.
    pub fn load_named(&mut self, name: &str) -> Result<(), String> {
        let rom = rom_of(name)?;
        self.core.load(&rom).map_err(|e| e.to_string())
    }
}

/// Build one of the programs of `programs/` from its source.
fn rom_of(name: &str) -> Result<Vec<u8>, String> {
    match silicon_programs::build(name) {
        Ok(assembly) => Ok(assembly.rom),
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            Err(lines.join("\n"))
        }
    }
}

#[wasm_bindgen]
impl Machine {
    /// A computer with `program` in ROM. Throws if it does not fit.
    #[wasm_bindgen(constructor)]
    pub fn new(program: &[u8]) -> Result<Machine, JsError> {
        Machine::with_program(program).map_err(|e| JsError::new(&e.to_string()))
    }

    /// A computer running the built-in demo program (scrolling stripes).
    pub fn demo() -> Machine {
        Machine::with_program(&demo::stripes()).expect("the demo fits in ROM")
    }

    /// A computer running one of the programs of [`crate::program_list`],
    /// built from its source here and now. Throws if there is no such
    /// program.
    pub fn running(name: &str) -> Result<Machine, JsError> {
        let rom = rom_of(name).map_err(|e| JsError::new(&e))?;
        Machine::with_program(&rom).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Give the computer another program and start it from power-on. The
    /// engine choice and held keys stay. Throws, changing nothing, if there
    /// is no such program.
    pub fn load(&mut self, name: &str) -> Result<(), JsError> {
        self.load_named(name).map_err(|e| JsError::new(&e))
    }

    /// Back to power-on. The program, the engine choice and held keys stay.
    pub fn reset(&mut self) {
        self.core.reset();
    }

    /// Run this many clock cycles on the active engine.
    pub fn run(&mut self, cycles: u32) {
        self.core.run(cycles as u64);
    }

    /// `"gate"` or `"fast"`.
    pub fn engine(&self) -> String {
        self.core.engine().name().to_string()
    }

    /// Choose the engine: the gate-level netlist (`true`) or the fast model.
    /// The CPU's state moves across; the program does not notice.
    pub fn use_gate_engine(&mut self, gate: bool) {
        self.core.set_engine(if gate {
            EngineKind::Gate
        } else {
            EngineKind::Fast
        });
    }

    /// The keys held down, as the bits of the `KEYS` register.
    pub fn set_keys(&mut self, keys: u8) {
        self.core.board_mut().set_keys(keys);
    }

    pub fn halted(&self) -> bool {
        self.core.halted()
    }

    /// Clock cycles since reset.
    pub fn cycles(&self) -> f64 {
        self.core.board().cycles() as f64
    }

    /// Frames the program has declared finished since reset.
    pub fn frames(&self) -> f64 {
        self.core.board().frames() as f64
    }

    /// Address of the screen memory: 2048 bytes, four 2-bit pixels each.
    pub fn framebuffer_ptr(&self) -> *const u8 {
        self.core.board().framebuffer().as_ptr()
    }

    pub fn framebuffer_len(&self) -> usize {
        self.core.board().framebuffer().len()
    }

    /// Address of the wire levels of the gate-level CPU: one byte per wire,
    /// 0 or 1. Live only while the gate engine is active.
    pub fn wires_ptr(&self) -> *const u8 {
        self.core.gate().sim().state().as_ptr()
    }

    pub fn wires_len(&self) -> usize {
        self.core.gate().sim().state().len()
    }

    /// Console output since the last call.
    pub fn take_console(&mut self) -> Vec<u8> {
        self.core.board_mut().take_console()
    }

    /// The CPU registers, packed: `pc` (2 bytes, low first), `sp` (2),
    /// `a b xl xh yl yh zl zh` (8), flags, `ir`, step, `t` (2).
    pub fn arch_state(&self) -> Vec<u8> {
        let s = self.core.state();
        let mut out = Vec::with_capacity(ARCH_STATE_BYTES);
        out.extend_from_slice(&s.pc.to_le_bytes());
        out.extend_from_slice(&s.sp.to_le_bytes());
        out.extend_from_slice(&s.regs);
        out.extend_from_slice(&[s.flags, s.ir, s.step]);
        out.extend_from_slice(&s.t.to_le_bytes());
        out
    }

    /// Every gate of the CPU, five numbers each: kind (0 = NAND, 1 = DFF),
    /// input wire `a`, input wire `b`, output wire, module index.
    pub fn gate_table(&self) -> Vec<u32> {
        let netlist = self.core.gate().sim().netlist();
        let mut out = Vec::with_capacity(netlist.gates.len() * GATE_FIELDS);
        for g in &netlist.gates {
            let kind = match g.kind {
                GateKind::Nand => 0,
                GateKind::Dff => 1,
            };
            out.extend_from_slice(&[kind, g.a.0, g.b.0, g.out.0, g.module.0]);
        }
        out
    }

    /// Parent of each module, by index; the top module's parent is -1.
    pub fn module_parents(&self) -> Vec<i32> {
        let netlist = self.core.gate().sim().netlist();
        netlist
            .modules
            .iter()
            .map(|m| m.parent.map_or(-1, |p| p.0 as i32))
            .collect()
    }

    /// Names of the modules, by index, separated by newlines.
    pub fn module_names(&self) -> String {
        let netlist = self.core.gate().sim().netlist();
        let names: Vec<&str> = netlist.modules.iter().map(|m| m.name.as_str()).collect();
        names.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silicon_machine::demo::stripes_frame;

    #[test]
    fn the_demo_runs_and_switches_engines() {
        let mut m = Machine::demo();
        assert_eq!(m.engine(), "fast");
        m.run(30_000);
        assert_eq!(m.frames(), 1.0);
        m.use_gate_engine(true);
        assert_eq!(m.engine(), "gate");
        m.run(70_000);
        assert_eq!((m.frames(), m.cycles()), (2.0, 100_000.0));
        assert!(m.core.board().framebuffer() == stripes_frame(1));
        assert_eq!(m.framebuffer_len(), 2048);
        assert!(!m.halted());
    }

    #[test]
    fn every_program_of_the_list_loads_and_runs() {
        let mut m = Machine::demo();
        m.use_gate_engine(true);
        m.set_keys(0x10);
        for line in crate::program_list().lines() {
            let name = line.split('\t').next().unwrap();
            m.load_named(name).unwrap();
            assert_eq!((m.cycles(), m.frames()), (0.0, 0.0), "{name}");
            m.run(2_000);
            assert_eq!(m.cycles(), 2_000.0, "{name}");
        }
        // The engine and the keys are the computer's, not the program's.
        assert_eq!(m.engine(), "gate");
        assert_eq!(m.core.board().keys(), 0x10);
    }

    #[test]
    fn loading_starts_the_program_from_power_on() {
        let mut m = Machine::demo();
        m.run(100_000);
        m.load_named("hello").unwrap();
        m.run(1_000);
        assert!(m.halted());
        assert_eq!(m.take_console(), b"Hello from Silicon!\n");
        assert_eq!(m.frames(), 0.0);
        assert!(m.core.board().framebuffer().iter().all(|&b| b == 0));
    }

    #[test]
    fn an_unknown_program_is_refused_and_nothing_changes() {
        let mut m = Machine::demo();
        m.run(1_000);
        let error = m.load_named("nothing").unwrap_err();
        assert_eq!(
            error,
            "nothing:1:1: error: there is no program called `nothing`"
        );
        assert_eq!(m.cycles(), 1_000.0);
    }

    #[test]
    fn arch_state_is_packed_as_documented() {
        let mut m = Machine::demo();
        m.run(1000);
        let (packed, s) = (m.arch_state(), m.core.state());
        assert_eq!(packed.len(), ARCH_STATE_BYTES);
        assert_eq!(u16::from_le_bytes([packed[0], packed[1]]), s.pc);
        assert_eq!(u16::from_le_bytes([packed[2], packed[3]]), s.sp);
        assert_eq!(packed[4..12], s.regs);
        assert_eq!(packed[12..15], [s.flags, s.ir, s.step]);
        assert_eq!(u16::from_le_bytes([packed[15], packed[16]]), s.t);
    }

    #[test]
    fn netlist_metadata_is_consistent() {
        let m = Machine::demo();
        let gates = m.gate_table();
        let parents = m.module_parents();
        let names: Vec<String> = m.module_names().lines().map(String::from).collect();
        assert_eq!(gates.len() % GATE_FIELDS, 0);
        assert_eq!(names.len(), parents.len());
        assert_eq!((names[0].as_str(), parents[0]), ("cpu", -1));
        let wires = m.wires_len() as u32;
        let mut dffs = 0;
        for g in gates.chunks(GATE_FIELDS) {
            assert!(g[0] <= 1 && g[1] < wires && g[2] < wires && g[3] < wires);
            assert!((g[4] as usize) < names.len());
            dffs += g[0];
        }
        assert_eq!(dffs, 127);
        // Every module but the top one hangs from an earlier module.
        for (i, &p) in parents.iter().enumerate().skip(1) {
            assert!(p >= 0 && (p as usize) < i);
        }
    }

    #[test]
    fn a_program_that_is_too_large_is_refused() {
        assert!(Machine::with_program(&vec![0; 0x8001]).is_err());
        assert!(!Machine::with_program(&[0]).unwrap().halted());
    }
}
