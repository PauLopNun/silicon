//! # Silicon fast engine
//!
//! A cycle-accurate architectural model of the Silicon CPU: same registers,
//! same state after every clock cycle as the gate-level netlist, but computed
//! with ordinary integer arithmetic so games run at playable speed.
//!
//! The CPU model lands in M2 once the ISA is fixed (M1). What is fixed already
//! is the contract with the gate engine: [`ArchState`] is the complete
//! architectural state, and both engines must agree on it after every cycle.

/// Complete architectural state of the CPU at a clock-cycle boundary.
///
/// This is what the lockstep equivalence test compares and what is copied
/// across when the UI switches engines. The field list is provisional until
/// `docs/ISA.md` is written in M1; adding a register means adding it here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArchState {
    /// Program counter.
    pub pc: u16,
    /// Stack pointer.
    pub sp: u16,
    /// General-purpose registers.
    pub regs: [u8; 8],
    /// Flags, packed (bit assignment defined by the ISA).
    pub flags: u8,
    /// Instruction register.
    pub ir: u8,
    /// Position inside the current instruction (micro-step counter).
    pub step: u8,
    /// Clock cycles since reset.
    pub cycle: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_state_is_all_zero() {
        let s = ArchState::default();
        assert_eq!((s.pc, s.sp, s.cycle), (0, 0, 0));
        assert_eq!(s, s.clone());
    }
}
