//! The whole computer, as the worker sees it.
//!
//! Bulk state (the screen, every wire of the CPU) is exposed as a pointer and
//! a length into WASM memory, to be viewed with a typed array. Static
//! information about the netlist is returned once as flat arrays.

use silicon_hdl::{GateKind, Netlist};
use silicon_isa::{form, Insn};
use silicon_machine::board::RomTooLarge;
use silicon_machine::hw::control;
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
    /// The level of each wire, as of the last [`Machine::look_at_wires`].
    levels: Vec<u8>,
    /// How busy each wire had been until then, since the look before it.
    activity: Vec<u8>,
    /// The clock cycle at which the wires' changes began to be counted.
    activity_since: u64,
}

/// Every gate of a netlist, [`GATE_FIELDS`] numbers each: kind (0 = NAND,
/// 1 = DFF), input wire `a`, input wire `b`, output wire, module index.
pub(crate) fn gate_table(netlist: &Netlist) -> Vec<u32> {
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

/// The logic depth of every gate of a netlist: 0 for a flip-flop, 1 for a
/// NAND fed only by inputs and flip-flops, and so on. This is the order in
/// which the gate engine evaluates them, and how many gate delays after a
/// clock edge a gate's output is settled.
pub(crate) fn gate_levels(netlist: &Netlist) -> Vec<u32> {
    netlist
        .levelize()
        .expect("a netlist that simulates has no combinational loop")
        .level
}

/// Parent of each module of a netlist, by index; the top module's is -1.
pub(crate) fn module_parents(netlist: &Netlist) -> Vec<i32> {
    netlist
        .modules
        .iter()
        .map(|m| m.parent.map_or(-1, |p| p.0 as i32))
        .collect()
}

/// Names of the modules of a netlist, by index, separated by newlines.
pub(crate) fn module_names(netlist: &Netlist) -> String {
    let names: Vec<&str> = netlist.modules.iter().map(|m| m.name.as_str()).collect();
    names.join("\n")
}

/// How busy a wire has been, as one byte: 0 if it did not change at all,
/// otherwise 1 to 255 on a square-root scale of changes per clock cycle,
/// 255 being a change in every cycle. The square root keeps a wire that
/// changed once in thousands of cycles apart from one that never did.
pub fn activity_byte(toggles: u32, cycles: u64) -> u8 {
    if toggles == 0 {
        return 0;
    }
    let rate = (toggles as f64 / cycles.max(1) as f64).min(1.0);
    ((255.0 * rate.sqrt()).round() as u8).max(1)
}

impl Machine {
    /// The constructor without the JavaScript error type, for native tests.
    pub fn with_program(program: &[u8]) -> Result<Machine, RomTooLarge> {
        let core = Core::new(program)?;
        let wires = core.gate().sim().wire_count();
        let mut machine = Machine {
            core,
            levels: vec![0; wires],
            activity: vec![0; wires],
            activity_since: 0,
        };
        machine.look_at_wires();
        Ok(machine)
    }

    /// [`Machine::load`] without the JavaScript error type. The error is
    /// what the toolchain reported, one line per problem.
    pub fn load_named(&mut self, name: &str) -> Result<(), String> {
        let rom = rom_of(name)?;
        self.core.load(&rom).map_err(|e| e.to_string())?;
        self.activity_since = 0;
        Ok(())
    }

    /// The activity buffer, for native tests.
    pub fn activity(&self) -> &[u8] {
        &self.activity
    }

    /// The level buffer, for native tests.
    pub fn levels(&self) -> &[u8] {
        &self.levels
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
        self.activity_since = 0;
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

    /// Look at every wire of the gate-level CPU: note its level at this
    /// instant, and how busy it has been since the last look. What was noted
    /// is read through [`Machine::wires_ptr`] and [`Machine::activity_ptr`]
    /// and stays as it is until the next look.
    ///
    /// The levels are where things stand; the activity is everything that
    /// happened on the way, which at hundreds of thousands of cycles a
    /// second is most of what there is to see. While the fast engine runs
    /// the CPU it computes no wires: the levels stay as the gate engine left
    /// them and the activity is all zero.
    pub fn look_at_wires(&mut self) {
        let now = self.core.board().cycles();
        let cycles = now.saturating_sub(self.activity_since);
        let sim = self.core.gate().sim();
        for (byte, level) in self.levels.iter_mut().zip(sim.levels()) {
            *byte = level;
        }
        for (byte, count) in self.activity.iter_mut().zip(sim.toggles()) {
            *byte = activity_byte(count, cycles);
        }
        self.core.clear_wire_toggles();
        self.activity_since = now;
    }

    /// While the fast engine runs the program, find out what the gates are
    /// doing by running a sample of it on them: the next `cycles` clock
    /// cycles are computed on the gate engine, the wires are looked at (as
    /// by [`Machine::look_at_wires`], the activity being that of exactly
    /// these cycles), and the CPU goes back to the fast engine.
    ///
    /// The cycles are cycles of the program like any others: the two engines
    /// agree on every one, so the program cannot tell. What the page shows
    /// from this is true of the cycles sampled and says nothing of the rest.
    ///
    /// Returns `false`, having done nothing, if the gate engine is already
    /// running the CPU: then every cycle is on the gates and there is
    /// nothing to sample.
    pub fn sample_on_gates(&mut self, cycles: u32) -> bool {
        if self.core.engine() == EngineKind::Gate {
            return false;
        }
        // Handing over clears the counts of changes: they start with the sample.
        self.core.set_engine(EngineKind::Gate);
        self.activity_since = self.core.board().cycles();
        self.core.run(cycles as u64);
        self.look_at_wires();
        self.core.set_engine(EngineKind::Fast);
        true
    }

    /// Address of the wire levels as of the last [`Machine::look_at_wires`]:
    /// one byte per wire, 0 or 1.
    pub fn wires_ptr(&self) -> *const u8 {
        self.levels.as_ptr()
    }

    pub fn wires_len(&self) -> usize {
        self.levels.len()
    }

    /// Address of the activity of the wires as of the last
    /// [`Machine::look_at_wires`]: one byte per wire, see [`activity_byte`].
    pub fn activity_ptr(&self) -> *const u8 {
        self.activity.as_ptr()
    }

    pub fn activity_len(&self) -> usize {
        self.activity.len()
    }

    /// Break a gate of the CPU (by its index in [`Machine::gate_table`]):
    /// `level` 0 or 1 sticks its output at that level, anything else mends
    /// it. Breaking one moves the CPU to the gate engine, which it cannot
    /// leave while any gate is broken.
    pub fn break_gate(&mut self, gate: usize, level: i32) {
        let gates = self.core.gate().sim().netlist().gates.len();
        if gate >= gates {
            return;
        }
        let level = match level {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
        self.core.break_gate(gate, level);
    }

    /// Mend every broken gate.
    pub fn mend_all(&mut self) {
        let broken: Vec<usize> = self.core.faults().iter().map(|&(g, _)| g).collect();
        for gate in broken {
            self.core.break_gate(gate, None);
        }
    }

    /// The broken gates: for each, its index and the level it is stuck at.
    pub fn faults(&self) -> Vec<u32> {
        self.core
            .faults()
            .iter()
            .flat_map(|&(gate, level)| [gate as u32, level as u32])
            .collect()
    }

    /// Run to the start of the next instruction: until an opcode has been
    /// read, or the CPU has halted. At most 16 cycles, more than the longest
    /// instruction takes.
    pub fn step_instruction(&mut self) {
        for _ in 0..16 {
            self.core.run(1);
            // Step 1 is reached only by reading an opcode (or by sitting in hlt).
            let state = self.core.state();
            if state.step == 1 || state.halted() {
                break;
            }
        }
    }

    /// What the CPU is doing in the clock cycle it is about to run, for
    /// people. One line per item, fields separated by tabs:
    ///
    /// 1. where the running instruction began (or -1 if not known yet), the
    ///    instruction as assembly, its form, and the step it is in;
    /// 2. then one line per control signal the control unit raises in this
    ///    cycle: its name and what it does.
    ///
    /// The signals come from the microcode table read in plain Rust
    /// ([`control::control_word`]), which the gates of the control unit are
    /// tested against for every opcode, step and flag value.
    pub fn explain(&self) -> String {
        let state = self.core.state();
        let (at, text) = match self.core.opcode_at() {
            Some(at) => {
                let bytes: Vec<u8> = (0..3)
                    .map(|i| self.core.board().peek(at.wrapping_add(i)))
                    .collect();
                let text =
                    Insn::decode(&bytes).map_or_else(String::new, |(insn, _)| insn.to_string());
                (at as i32, text)
            }
            None => (-1, String::new()),
        };
        let form = if state.step == 0 {
            "FETCH"
        } else {
            form::form_of(state.ir).name
        };
        let mut lines = vec![format!("{at}\t{text}\t{form}\t{}", state.step)];
        let word = control::control_word(state.ir, state.step, state.flags);
        for bit in 0..control::SIGNAL_COUNT {
            if word >> bit & 1 == 1 {
                lines.push(format!(
                    "{}\t{}",
                    control::SIGNAL_NAMES[bit],
                    control::SIGNAL_DOCS[bit]
                ));
            }
        }
        lines.join("\n")
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
        gate_table(self.core.gate().sim().netlist())
    }

    /// The logic depth of every gate: how many gate delays after a clock
    /// edge its output is settled (0 for a flip-flop).
    pub fn gate_levels(&self) -> Vec<u32> {
        gate_levels(self.core.gate().sim().netlist())
    }

    /// Parent of each module, by index; the top module's parent is -1.
    pub fn module_parents(&self) -> Vec<i32> {
        module_parents(self.core.gate().sim().netlist())
    }

    /// Names of the modules, by index, separated by newlines.
    pub fn module_names(&self) -> String {
        module_names(self.core.gate().sim().netlist())
    }

    /// The public name of a gate: `cpu/alu/adder8/fa3/nand2`.
    pub fn gate_path(&self, gate: usize) -> String {
        self.core.gate().sim().netlist().gate_path(gate)
    }

    /// Names of the probes (the groups of wires worth a label, such as
    /// `cpu/pc:value`), by index, separated by newlines.
    pub fn probe_names(&self) -> String {
        let netlist = self.core.gate().sim().netlist();
        let names: Vec<&str> = netlist.probes.iter().map(|p| p.name.as_str()).collect();
        names.join("\n")
    }

    /// The wires of every probe, in the order of [`Machine::probe_names`]:
    /// for each probe its number of wires, then the wires, least significant
    /// bit first.
    pub fn probe_wires(&self) -> Vec<u32> {
        let netlist = self.core.gate().sim().netlist();
        let mut out = Vec::new();
        for probe in &netlist.probes {
            out.push(probe.wires.len() as u32);
            out.extend(probe.wires.iter().map(|w| w.0));
        }
        out
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
    fn activity_is_a_square_root_scale_of_changes_per_cycle() {
        assert_eq!(activity_byte(0, 1000), 0);
        // One change in a million cycles is still not nothing.
        assert_eq!(activity_byte(1, 1_000_000), 1);
        assert_eq!(activity_byte(1000, 1000), 255);
        assert_eq!(activity_byte(250, 1000), 128);
        // A wire can change twice in a cycle: that is as busy as it gets.
        assert_eq!(activity_byte(2000, 1000), 255);
        // A single step: whatever changed, changed in every cycle there was.
        assert_eq!(activity_byte(1, 1), 255);
        assert_eq!(activity_byte(1, 0), 255);
        let mut last = 0;
        for toggles in 0..=1000 {
            let byte = activity_byte(toggles, 1000);
            assert!(byte >= last);
            last = byte;
        }
    }

    #[test]
    fn activity_is_what_the_wires_did_since_it_was_last_taken() {
        let mut m = Machine::demo();
        // The fast engine computes no wires.
        m.run(10_000);
        m.look_at_wires();
        assert!(m.activity().iter().all(|&a| a == 0));
        assert_eq!(m.activity_len(), m.wires_len());

        // Handing the CPU to the gates is not activity either.
        m.use_gate_engine(true);
        m.look_at_wires();
        assert!(m.activity().iter().all(|&a| a == 0));

        m.run(5_000);
        m.look_at_wires();
        let busy = m.activity().iter().filter(|&&a| a > 0).count();
        assert!(busy > 500, "{busy} wires changed");
        // The constants never change.
        assert_eq!(m.activity()[..2], [0, 0]);
        // The lowest bit of the step counter changes more often than the
        // highest bit of the stack pointer.
        let gate = m.core.gate();
        let (step0, sp15) = (gate.state_wires().step[0], gate.state_wires().sp[15]);
        assert!(m.activity()[step0.index()] > 100);
        assert!(m.activity()[sp15.index()] < 10);

        // Nothing ran since: nothing changed.
        m.look_at_wires();
        assert!(m.activity().iter().all(|&a| a == 0));

        // One cycle: every wire that changed did so in the only cycle there was.
        m.run(1);
        m.look_at_wires();
        assert!(m.activity().contains(&255));
        assert!(m.activity().iter().all(|&a| a == 0 || a == 255));

        // A reset is not activity, and the count of cycles starts again with it.
        m.run(1_000);
        m.reset();
        m.look_at_wires();
        assert!(m.activity().iter().all(|&a| a == 0));
        m.run(100);
        m.look_at_wires();
        assert!(m.activity().iter().any(|&a| a > 0));
    }

    #[test]
    fn gates_and_probes_have_their_public_names() {
        let m = Machine::demo();
        let gates = m.gate_table().len() / GATE_FIELDS;
        assert!(m.gate_path(0).starts_with("cpu/"));
        let last = m.gate_path(gates - 1);
        assert!(last.contains("/nand") || last.contains("/dff"), "{last}");

        let names: Vec<String> = m.probe_names().lines().map(String::from).collect();
        let table = m.probe_wires();
        let wires = m.wires_len() as u32;
        let mut at = 0;
        let mut pc = None;
        for name in &names {
            let n = table[at] as usize;
            let probe = &table[at + 1..at + 1 + n];
            assert!(probe.iter().all(|&w| w < wires), "{name}");
            if name == "cpu/pc:value" {
                pc = Some(probe.to_vec());
            }
            at += 1 + n;
        }
        assert_eq!(at, table.len());
        let state = m.core.gate().state_wires();
        let expected: Vec<u32> = state.pc.iter().map(|w| w.0).collect();
        assert_eq!(pc, Some(expected));
    }

    #[test]
    fn explain_names_the_instruction_its_step_and_its_signals() {
        let mut m = Machine::running("hello").unwrap();
        // Power-on: nothing read yet, about to fetch.
        let first = m.explain();
        let head: Vec<&str> = first.lines().next().unwrap().split('\t').collect();
        assert_eq!(head, ["-1", "", "FETCH", "0"]);
        assert!(first.contains("IR_WE\tinstruction register ← data in"));
        assert!(first.contains("RD\t"));

        // One instruction at a time, each just fetched: pc is one past its opcode.
        for n in 0..6 {
            m.step_instruction();
            let text = m.explain();
            let head: Vec<&str> = text.lines().next().unwrap().split('\t').collect();
            let at: u16 = head[0].parse().unwrap();
            assert_eq!(head[3], "1", "{text}");
            assert!(!head[1].is_empty() && head[2] != "FETCH", "{text}");
            // The text is the instruction found at that address.
            let bytes: Vec<u8> = (0..3).map(|i| m.core.board().peek(at + i)).collect();
            let (insn, _) = Insn::decode(&bytes).unwrap();
            assert_eq!(head[1], insn.to_string());
            assert_eq!(head[2], form::form_of(bytes[0]).name);
            assert_eq!(m.core.state().pc, at + 1, "{text}");
            if n == 0 {
                assert_eq!(at, 0, "the program starts at address 0");
            }
            // Every signal line is a known signal with its description.
            for line in text.lines().skip(1) {
                let (name, doc) = line.split_once('\t').unwrap();
                let bit = control::SIGNAL_NAMES
                    .iter()
                    .position(|&n| n == name)
                    .unwrap();
                assert_eq!(doc, control::SIGNAL_DOCS[bit]);
            }
        }
    }

    #[test]
    fn sampling_on_the_gates_shows_the_wires_and_changes_nothing() {
        // Two computers playing the same game with the same keys: one only
        // on the fast engine, the other sampled on the gates in every frame.
        let mut plain = Machine::running("maze3d").unwrap();
        let mut sampled = Machine::running("maze3d").unwrap();
        for frame in 0..40u32 {
            let keys = (frame / 7 % 4) as u8 | 1;
            plain.set_keys(keys);
            sampled.set_keys(keys);
            plain.run(66_667);
            sampled.run(66_667 - 256);
            assert!(sampled.sample_on_gates(256));
            assert_eq!(sampled.engine(), "fast");
            assert_eq!(sampled.cycles(), plain.cycles());
            assert_eq!(sampled.arch_state(), plain.arch_state(), "frame {frame}");
        }
        assert!(sampled.core.board() == plain.core.board());
        assert_eq!(sampled.frames(), plain.frames());

        // What was seen is the sample: levels, and activity over its cycles.
        let busy = sampled.activity().iter().filter(|&&a| a > 0).count();
        assert!(busy > 300, "{busy} wires changed during the sample");
        let gate = sampled.core.gate();
        let pc: u16 = gate
            .state_wires()
            .pc
            .iter()
            .enumerate()
            .map(|(i, w)| (sampled.levels()[w.index()] as u16) << i)
            .sum();
        assert_eq!(pc, sampled.core.state().pc, "the wires of pc spell pc");
        // One change in a sample of 256 cycles is told apart from none.
        assert_eq!(activity_byte(1, 256), 16);

        // On the gate engine there is nothing to sample.
        sampled.use_gate_engine(true);
        let cycles = sampled.cycles();
        assert!(!sampled.sample_on_gates(256));
        assert_eq!(sampled.cycles(), cycles);
    }

    #[test]
    fn gate_levels_follow_the_wiring() {
        let m = Machine::demo();
        let levels = m.gate_levels();
        let gates = m.gate_table();
        assert_eq!(levels.len() * GATE_FIELDS, gates.len());
        // The level of the gate driving each wire; inputs and constants are 0.
        let mut of_wire = vec![0u32; m.wires_len()];
        for (g, gate) in gates.chunks(GATE_FIELDS).enumerate() {
            of_wire[gate[3] as usize] = if gate[0] == 1 { 0 } else { levels[g] };
        }
        for (g, gate) in gates.chunks(GATE_FIELDS).enumerate() {
            if gate[0] == 1 {
                assert_eq!(levels[g], 0, "a flip-flop starts the count again");
            } else {
                let deepest = of_wire[gate[1] as usize].max(of_wire[gate[2] as usize]);
                assert_eq!(levels[g], deepest + 1, "gate {g}");
            }
        }
        // The critical path of the CPU, as measured in M1.
        assert_eq!(levels.iter().max(), Some(&67));
    }

    #[test]
    fn gates_can_be_broken_and_mended_from_the_page() {
        let mut m = Machine::running("hello").unwrap();
        m.break_gate(5, 1);
        m.break_gate(9, 0);
        assert_eq!(m.faults(), [5, 1, 9, 0]);
        assert_eq!(m.engine(), "gate");
        m.use_gate_engine(false);
        assert_eq!(m.engine(), "gate");
        m.break_gate(5, -1);
        assert_eq!(m.faults(), [9, 0]);
        // A gate that does not exist is ignored, not a crash.
        m.break_gate(1_000_000, 1);
        assert_eq!(m.faults(), [9, 0]);
        m.mend_all();
        assert!(m.faults().is_empty());
        m.use_gate_engine(false);
        assert_eq!(m.engine(), "fast");
    }

    #[test]
    fn step_instruction_stops_at_hlt() {
        let mut m = Machine::running("hello").unwrap();
        for _ in 0..1000 {
            m.step_instruction();
        }
        assert!(m.halted());
        assert!(m
            .explain()
            .starts_with(&format!("{}\thlt\t", m.core.opcode_at().unwrap())));
        let cycles = m.cycles();
        m.step_instruction();
        assert_eq!(m.cycles(), cycles + 1.0);
    }

    #[test]
    fn a_program_that_is_too_large_is_refused() {
        assert!(Machine::with_program(&vec![0; 0x8001]).is_err());
        assert!(!Machine::with_program(&[0]).unwrap().halted());
    }
}
