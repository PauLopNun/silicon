//! # Silicon machine
//!
//! The CPU as hardware ([`hw`]) and the two ways of running it, both behind
//! the [`Engine`] trait:
//!
//! * [`GateCpu`]: the real NAND/DFF netlist in the gate-level simulator;
//! * [`FastCpu`]: the arithmetic model from `silicon_sim_fast`.
//!
//! They are interchangeable cycle for cycle: [`lockstep`] is the harness that
//! proves it, and [`ArchState`] is what is copied across to switch from one
//! to the other mid-run.
//!
//! Around the CPU sits the [`Board`] (ROM, RAM, framebuffer, keys, timer,
//! console), and [`Machine`] is the two together: the whole computer.
//!
//! ```
//! use silicon_isa::{assemble, Insn, Reg};
//! use silicon_machine::{board::map, EngineKind, Machine};
//!
//! // Print "!" on the console and stop.
//! let program = assemble(&[Insn::LdImm(Reg::A, b'!'), Insn::StAbsA(map::OUT), Insn::Hlt]);
//! let mut machine = Machine::new(&program).unwrap();
//! machine.set_engine(EngineKind::Gate); // every NAND is simulated
//! machine.run_until_halt(100);
//! assert_eq!(machine.board().console(), b"!");
//! ```

pub mod board;
mod computer;
pub mod demo;
pub mod exhibit;
pub mod gate;
pub mod hw;
pub mod lockstep;

pub use board::Board;
pub use computer::Machine;
pub use gate::GateCpu;
pub use silicon_isa::{ArchState, Bus};
pub use silicon_sim_fast::{FastCpu, Ram};

/// Which implementation is currently executing CPU cycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    /// Real NAND/DFF netlist. Slow, exact, drives the visualization.
    Gate,
    /// Cycle-accurate architectural model. Fast, proven equivalent.
    Fast,
}

impl EngineKind {
    /// Short name shown in the UI badge and used in URLs.
    pub fn name(self) -> &'static str {
        match self {
            EngineKind::Gate => "gate",
            EngineKind::Fast => "fast",
        }
    }
}

/// A CPU implementation. Both engines implement this identically, cycle for
/// cycle; the lockstep tests enforce it.
pub trait Engine {
    fn kind(&self) -> EngineKind;
    /// Put the CPU in its power-on state: every flip-flop at 0.
    fn reset(&mut self);
    /// Advance exactly one clock cycle, performing at most one bus access.
    fn step_cycle(&mut self, bus: &mut dyn Bus);
    /// Snapshot the full architectural state.
    fn state(&self) -> ArchState;
    /// Overwrite the full architectural state (used when switching engines).
    fn load_state(&mut self, state: &ArchState);
}

impl Engine for FastCpu {
    fn kind(&self) -> EngineKind {
        EngineKind::Fast
    }
    fn reset(&mut self) {
        FastCpu::reset(self);
    }
    fn step_cycle(&mut self, bus: &mut dyn Bus) {
        FastCpu::step_cycle(self, bus);
    }
    fn state(&self) -> ArchState {
        FastCpu::state(self).clone()
    }
    fn load_state(&mut self, state: &ArchState) {
        FastCpu::load_state(self, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_names_are_stable() {
        assert_eq!(EngineKind::Gate.name(), "gate");
        assert_eq!(EngineKind::Fast.name(), "fast");
    }

    #[test]
    fn both_engines_power_on_in_the_same_state() {
        let gate = GateCpu::new();
        let fast = FastCpu::new();
        assert_eq!(Engine::state(&gate), ArchState::default());
        assert_eq!(Engine::state(&fast), ArchState::default());
    }
}
