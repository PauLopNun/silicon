//! The gate engine: the CPU netlist of [`crate::hw`], run by the gate-level
//! simulator and connected to a [`Bus`].

use crate::hw::{self, control, Pins, StateWires};
use crate::{ArchState, Bus, Engine, EngineKind};
use silicon_sim_gate::GateSim;

/// The CPU, simulated one NAND at a time.
pub struct GateCpu {
    sim: GateSim,
    pins: Pins,
    state: StateWires,
    /// The control signal that loads the instruction register.
    ir_we: silicon_hdl::Wire,
    cycle: u64,
    fetched: bool,
}

impl GateCpu {
    /// Build the netlist and power it on: every flip-flop is 0.
    pub fn new() -> Self {
        let cpu = hw::build();
        let ir_we = cpu.signals[control::bit(control::IR_WE)];
        let sim = GateSim::new(cpu.netlist).expect("the CPU netlist simulates");
        GateCpu {
            sim,
            pins: cpu.pins,
            state: cpu.state,
            ir_we,
            cycle: 0,
            fetched: false,
        }
    }

    /// The simulator, for looking at individual wires.
    pub fn sim(&self) -> &GateSim {
        &self.sim
    }

    /// Where the CPU's pins are in the netlist.
    pub fn pins(&self) -> &Pins {
        &self.pins
    }

    /// Where the CPU's flip-flops are in the netlist.
    pub fn state_wires(&self) -> &StateWires {
        &self.state
    }

    /// Did the last cycle load an opcode into the instruction register?
    pub fn fetched(&self) -> bool {
        self.fetched
    }
}

impl Default for GateCpu {
    fn default() -> Self {
        GateCpu::new()
    }
}

impl Engine for GateCpu {
    fn kind(&self) -> EngineKind {
        EngineKind::Gate
    }

    fn reset(&mut self) {
        self.load_state(&ArchState::default());
    }

    /// One clock cycle, in the order of ADR 0003:
    ///
    /// 1. the address and the strobes are already valid: they depend only on
    ///    flip-flops, and the logic was settled after the last clock edge;
    /// 2. on a read, memory answers and the data bus is driven;
    /// 3. everything else settles, ending at the flip-flops' inputs;
    /// 4. a write is committed and the clock ticks.
    fn step_cycle(&mut self, bus: &mut dyn Bus) {
        let addr = self.sim.get_word(&self.pins.addr) as u16;
        let rd = self.sim.get(self.pins.rd);
        let wr = self.sim.get(self.pins.wr);

        // The data bus idles at 0 when nothing is being read.
        let data = if rd { bus.read(addr) } else { 0 };
        self.sim.set_word(&self.pins.data_in, data as u64);
        self.sim.settle();

        if wr {
            bus.write(addr, self.sim.get_word(&self.pins.data_out) as u8);
        }
        self.fetched = self.sim.get(self.ir_we);
        self.sim.clock();
        self.sim.settle();
        self.cycle = self.cycle.wrapping_add(1);
    }

    fn state(&self) -> ArchState {
        let word = |wires: &[silicon_hdl::Wire]| self.sim.get_word(wires);
        ArchState {
            pc: word(&self.state.pc) as u16,
            sp: word(&self.state.sp) as u16,
            regs: std::array::from_fn(|i| word(&self.state.regs[i]) as u8),
            flags: word(&self.state.flags) as u8,
            ir: word(&self.state.ir) as u8,
            step: word(&self.state.step) as u8,
            t: word(&self.state.t) as u16,
            cycle: self.cycle,
        }
    }

    /// Force every flip-flop to the given state and let the logic settle.
    fn load_state(&mut self, state: &ArchState) {
        let w = &self.state;
        self.sim.set_word(&w.pc, state.pc as u64);
        self.sim.set_word(&w.sp, state.sp as u64);
        for (wires, &value) in w.regs.iter().zip(&state.regs) {
            self.sim.set_word(wires, value as u64);
        }
        self.sim.set_word(&w.flags, state.flags as u64);
        self.sim.set_word(&w.ir, state.ir as u64);
        self.sim.set_word(&w.step, state.step as u64);
        self.sim.set_word(&w.t, state.t as u64);
        self.sim.set_word(&self.pins.data_in, 0);
        self.sim.settle();
        self.cycle = state.cycle;
        self.fetched = false;
    }
}
