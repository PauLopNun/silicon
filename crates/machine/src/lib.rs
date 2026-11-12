//! # Silicon machine
//!
//! The whole computer: CPU + memory map + devices (framebuffer, keyboard,
//! frame timer). The machine is engine-agnostic: the CPU behind it is either
//! the gate-level netlist or the fast model, both behind the [`Engine`] trait,
//! and can be swapped mid-run by transferring [`ArchState`].
//!
//! The devices and both engine adapters land in M1/M2. The trait below is the
//! interface the rest of the project (WASM bindings, tests, UI) is written
//! against.

pub use silicon_sim_fast::ArchState;

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

/// What the CPU sees outside itself: a 16-bit address space of bytes.
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);
}

/// A CPU implementation. Both engines implement this identically, cycle for
/// cycle; `tests/lockstep` (M2) enforces it.
pub trait Engine {
    fn kind(&self) -> EngineKind;
    /// Put the CPU in its power-on state.
    fn reset(&mut self);
    /// Advance exactly one clock cycle, performing at most one bus access.
    fn step_cycle(&mut self, bus: &mut dyn Bus);
    /// Snapshot the full architectural state.
    fn state(&self) -> ArchState;
    /// Overwrite the full architectural state (used when switching engines).
    fn load_state(&mut self, state: &ArchState);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_names_are_stable() {
        assert_eq!(EngineKind::Gate.name(), "gate");
        assert_eq!(EngineKind::Fast.name(), "fast");
    }
}
