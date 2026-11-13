//! The Silicon CPU as hardware: a netlist of NAND gates and D flip-flops.
//!
//! | Module | Block | Reference it is tested against |
//! |---|---|---|
//! | [`control`] | microcode table and the PLA generated from it | [`control::control_word`] |
//! | [`alu`] | 8-bit arithmetic-logic unit | [`alu::reference`] |
//! | [`agu`] | 16-bit address unit | [`agu::reference`] |
//! | [`regs`] | register file | [`regs::storage_reference`], [`regs::selects_reference`] |
//! | [`cpu`] | all of the above wired together | the fast model, in lockstep |
//!
//! The generic parts (gates, multiplexers, adders, registers) come from
//! `silicon_hdl`; this directory is only what is specific to this CPU.

pub mod agu;
pub mod alu;
pub mod control;
pub mod cpu;
pub mod regs;

pub use cpu::{build, Cpu, Pins, StateWires};
