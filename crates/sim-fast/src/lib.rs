//! # Silicon fast engine
//!
//! A cycle-accurate architectural model of the Silicon CPU: the same
//! registers and the same state after every clock cycle as the gate-level
//! netlist, computed with ordinary integer arithmetic.
//!
//! This file is written from `docs/ISA.md`, instruction by instruction, and
//! deliberately shares no code with the hardware's control tables: the
//! lockstep test compares two independent implementations.
//!
//! ```
//! use silicon_isa::{assemble, AluOp, Insn, Reg};
//! use silicon_sim_fast::{FastCpu, Ram};
//!
//! let mut ram = Ram::with_program(&assemble(&[
//!     Insn::LdImm(Reg::A, 40),
//!     Insn::AluImm(AluOp::Add, 2),
//!     Insn::Hlt,
//! ]));
//! let mut cpu = FastCpu::new();
//! while !cpu.state().halted() {
//!     cpu.step_cycle(&mut ram);
//! }
//! assert_eq!(cpu.state().regs[0], 42);
//! ```

pub mod alu;

pub use silicon_isa::{ArchState, Bus};

use silicon_isa::{AluOp, Cond};

/// 64 KiB of plain read/write memory: the simplest possible [`Bus`], used by
/// tests and examples.
#[derive(Clone, PartialEq, Eq)]
pub struct Ram(pub Vec<u8>);

impl Ram {
    /// All zeroes.
    pub fn new() -> Self {
        Ram(vec![0; 0x1_0000])
    }

    /// All zeroes except `program`, placed at address 0 where the CPU starts.
    pub fn with_program(program: &[u8]) -> Self {
        let mut ram = Ram::new();
        ram.0[..program.len()].copy_from_slice(program);
        ram
    }
}

impl Default for Ram {
    fn default() -> Self {
        Ram::new()
    }
}

impl Bus for Ram {
    #[inline]
    fn read(&mut self, addr: u16) -> u8 {
        self.0[addr as usize]
    }
    #[inline]
    fn write(&mut self, addr: u16, value: u8) {
        self.0[addr as usize] = value;
    }
}

/// What the step counter does at the end of a cycle.
enum Seq {
    /// Go on to the next step of the same instruction.
    Next,
    /// The instruction is over; the next cycle fetches an opcode.
    Done,
    /// The instruction is over and the bus was free, so this same cycle also
    /// fetched the next opcode from the given address.
    Fetch(u16),
    /// `hlt`: nothing changes.
    Stay,
}

/// The CPU, modelled at the level of registers and clock cycles.
#[derive(Clone, Debug, Default)]
pub struct FastCpu {
    state: ArchState,
    fetched: bool,
}

impl FastCpu {
    /// A CPU in its power-on state: every register is zero.
    pub fn new() -> Self {
        FastCpu::default()
    }

    /// Back to the power-on state.
    pub fn reset(&mut self) {
        *self = FastCpu::default();
    }

    /// The architectural state after the last completed cycle.
    #[inline]
    pub fn state(&self) -> &ArchState {
        &self.state
    }

    /// Overwrite the whole state (used when switching engines).
    pub fn load_state(&mut self, state: &ArchState) {
        self.state = state.clone().masked();
        self.fetched = false;
    }

    /// Did the last cycle load an opcode into the instruction register?
    /// Counting these counts instructions.
    #[inline]
    pub fn fetched(&self) -> bool {
        self.fetched
    }

    /// Advance exactly one clock cycle, with at most one bus access.
    pub fn step_cycle<B: Bus + ?Sized>(&mut self, bus: &mut B) {
        let s = &mut self.state;
        s.cycle = s.cycle.wrapping_add(1);

        let seq = if s.step == 0 {
            Seq::Fetch(s.pc)
        } else {
            execute(s, bus)
        };

        self.fetched = false;
        match seq {
            Seq::Next => s.step += 1,
            Seq::Done => s.step = 0,
            Seq::Stay => {}
            Seq::Fetch(addr) => {
                s.ir = bus.read(addr);
                s.pc = addr.wrapping_add(1);
                s.step = 1;
                self.fetched = true;
            }
        }
    }
}

/// Read the byte at `pc` and move past it.
#[inline]
fn operand<B: Bus + ?Sized>(s: &mut ArchState, bus: &mut B) -> u8 {
    let value = bus.read(s.pc);
    s.pc = s.pc.wrapping_add(1);
    value
}

#[inline]
fn set_t_lo(s: &mut ArchState, value: u8) {
    s.t = (s.t & 0xFF00) | value as u16;
}

#[inline]
fn set_t_hi(s: &mut ArchState, value: u8) {
    s.t = (s.t & 0x00FF) | (value as u16) << 8;
}

/// The low byte of `t` as a signed offset.
#[inline]
fn t_signed(s: &ArchState) -> u16 {
    s.t as u8 as i8 as u16
}

/// The low byte of `t` as an unsigned offset.
#[inline]
fn t_unsigned(s: &ArchState) -> u16 {
    s.t & 0x00FF
}

/// One execution cycle (step ≥ 1) of the instruction in `ir`.
///
/// Register numbers: `a`=0 `b`=1 `xl`=2 `xh`=3 `yl`=4 `yh`=5 `zl`=6 `zh`=7.
/// Pair `p` is registers `2p` (low) and `2p + 1` (high).
#[inline]
fn execute<B: Bus + ?Sized>(s: &mut ArchState, bus: &mut B) -> Seq {
    use Seq::*;
    const A: usize = 0;
    let ir = s.ir;
    let r = (ir & 7) as usize; // register in bits 2..0
    let p = (ir & 3) as usize; // pair in bits 1..0
    let (lo, hi) = (2 * p, 2 * p + 1);

    match (ir, s.step) {
        // hlt
        (0x00, 1) => Stay,
        // mov sp, x
        (0x01, 1) => {
            s.sp = s.pair(1);
            Done
        }
        // ret
        (0x02, 1) => {
            let v = bus.read(s.sp);
            set_t_lo(s, v);
            s.sp = s.sp.wrapping_add(1);
            Next
        }
        (0x02, 2) => {
            let v = bus.read(s.sp);
            set_t_hi(s, v);
            s.sp = s.sp.wrapping_add(1);
            Next
        }
        (0x02, 3) => Fetch(s.t),
        // jmp z
        (0x03, 1) => Fetch(s.pair(3)),
        // ld a, [pp]
        (0x04..=0x07, 1) => {
            s.regs[A] = bus.read(s.pair(p));
            Done
        }
        // ld a, [pp+]   (for pp = ba the loaded value wins over the increment)
        (0x08..=0x0B, 1) => {
            let addr = s.pair(p);
            let v = bus.read(addr);
            s.set_pair(p, addr.wrapping_add(1));
            s.regs[A] = v;
            Done
        }
        // st [pp], a
        (0x0C..=0x0F, 1) => {
            bus.write(s.pair(p), s.regs[A]);
            Done
        }
        // st [pp+], a
        (0x10..=0x13, 1) => {
            let addr = s.pair(p);
            bus.write(addr, s.regs[A]);
            s.set_pair(p, addr.wrapping_add(1));
            Done
        }
        // ld a, [pp+a]
        (0x14..=0x17, 1) => {
            let addr = s.pair(p).wrapping_add(s.regs[A] as u16);
            s.regs[A] = bus.read(addr);
            Done
        }
        // ld pp, nn
        (0x18..=0x1B, 1) => {
            s.regs[lo] = operand(s, bus);
            Next
        }
        (0x18..=0x1B, 2) => {
            s.regs[hi] = operand(s, bus);
            Done
        }
        // push pp   (high byte first, so the pair sits in memory low byte first)
        (0x1C..=0x1F, 1) => {
            s.sp = s.sp.wrapping_sub(1);
            bus.write(s.sp, s.regs[hi]);
            Next
        }
        (0x1C..=0x1F, 2) => {
            s.sp = s.sp.wrapping_sub(1);
            bus.write(s.sp, s.regs[lo]);
            Done
        }
        // pop pp
        (0x20..=0x23, 1) => {
            s.regs[lo] = bus.read(s.sp);
            s.sp = s.sp.wrapping_add(1);
            Next
        }
        (0x20..=0x23, 2) => {
            s.regs[hi] = bus.read(s.sp);
            s.sp = s.sp.wrapping_add(1);
            Done
        }
        // lea pp, [sp+n]  /  lea pp, [pp+d]
        (0x24..=0x2B, 1) => {
            let v = operand(s, bus);
            set_t_lo(s, v);
            Next
        }
        (0x24..=0x27, 2) => {
            s.set_pair(p, s.sp.wrapping_add(t_unsigned(s)));
            Done
        }
        (0x28..=0x2B, 2) => {
            s.set_pair(p, s.pair(p).wrapping_add(t_signed(s)));
            Done
        }
        // lea pp, [pp+a]
        (0x2C..=0x2F, 1) => {
            s.set_pair(p, s.pair(p).wrapping_add(s.regs[A] as u16));
            Done
        }
        // add pd, ps   (two 8-bit additions chained through the carry)
        (0x30..=0x3F, 1) => {
            let d = 2 * ((ir >> 2) & 3) as usize;
            let (v, f) = alu::alu(AluOp::Add, s.regs[d], s.regs[lo], s.flags);
            s.regs[d] = v;
            s.flags = f;
            Next
        }
        (0x30..=0x3F, 2) => {
            let d = 2 * ((ir >> 2) & 3) as usize + 1;
            let (v, f) = alu::alu(AluOp::Adc, s.regs[d], s.regs[hi], s.flags);
            s.regs[d] = v;
            s.flags = f;
            Fetch(s.pc)
        }
        // mov rd, rs
        (0x40..=0x7F, 1) => {
            s.regs[((ir >> 3) & 7) as usize] = s.regs[r];
            Fetch(s.pc)
        }
        // op a, rs
        (0x80..=0xBF, 1) => {
            let (v, f) = alu::alu(AluOp::from_bits(ir >> 3), s.regs[A], s.regs[r], s.flags);
            s.regs[A] = v;
            s.flags = f;
            Fetch(s.pc)
        }
        // inc r
        (0xC0..=0xC7, 1) => {
            let (v, f) = alu::inc(s.regs[r], s.flags);
            s.regs[r] = v;
            s.flags = f;
            Fetch(s.pc)
        }
        // dec r
        (0xC8..=0xCF, 1) => {
            let (v, f) = alu::dec(s.regs[r], s.flags);
            s.regs[r] = v;
            s.flags = f;
            Fetch(s.pc)
        }
        // ld r, n
        (0xD0..=0xD7, 1) => {
            s.regs[r] = operand(s, bus);
            Done
        }
        // ld r, [sp+n]  /  st [sp+n], r
        (0xD8..=0xE7, 1) => {
            let v = operand(s, bus);
            set_t_lo(s, v);
            Next
        }
        (0xD8..=0xDF, 2) => {
            s.regs[r] = bus.read(s.sp.wrapping_add(t_unsigned(s)));
            Done
        }
        (0xE0..=0xE7, 2) => {
            bus.write(s.sp.wrapping_add(t_unsigned(s)), s.regs[r]);
            Done
        }
        // op a, n
        (0xE8..=0xEF, 1) => {
            let n = operand(s, bus);
            let (v, f) = alu::alu(AluOp::from_bits(ir), s.regs[A], n, s.flags);
            s.regs[A] = v;
            s.flags = f;
            Done
        }
        // bcc d
        (0xF0..=0xF7, 1) => {
            let v = operand(s, bus);
            set_t_lo(s, v);
            if Cond::from_bits(ir).holds(s.flags) {
                Next
            } else {
                Done
            }
        }
        (0xF0..=0xF7, 2) => {
            s.pc = s.pc.wrapping_add(t_signed(s));
            Done
        }
        // jmp nn / call nn / ld a, [nn] / st [nn], a / lea sp, [sp+d]:
        // read the operand into t
        (0xF8..=0xFC, 1) => {
            let v = operand(s, bus);
            set_t_lo(s, v);
            Next
        }
        (0xF8..=0xFB, 2) => {
            let v = operand(s, bus);
            set_t_hi(s, v);
            Next
        }
        // jmp nn
        (0xF8, 3) => Fetch(s.t),
        // call nn
        (0xF9, 3) => {
            s.sp = s.sp.wrapping_sub(1);
            bus.write(s.sp, (s.pc >> 8) as u8);
            Next
        }
        (0xF9, 4) => {
            s.sp = s.sp.wrapping_sub(1);
            bus.write(s.sp, s.pc as u8);
            Next
        }
        (0xF9, 5) => Fetch(s.t),
        // ld a, [nn]
        (0xFA, 3) => {
            s.regs[A] = bus.read(s.t);
            Done
        }
        // st [nn], a
        (0xFB, 3) => {
            bus.write(s.t, s.regs[A]);
            Done
        }
        // lea sp, [sp+d]
        (0xFC, 2) => {
            s.sp = s.sp.wrapping_add(t_signed(s));
            Done
        }
        // shr a / ror a / asr a
        (0xFD..=0xFF, 1) => {
            let a = s.regs[A];
            let fill = match ir {
                0xFD => false,
                0xFE => s.flags & silicon_isa::flag::C != 0,
                _ => a & 0x80 != 0,
            };
            let (v, f) = alu::shift_right(a, fill, s.flags);
            s.regs[A] = v;
            s.flags = f;
            Fetch(s.pc)
        }
        // A step number the instruction does not have. Unreachable from
        // reset; defined so that both engines agree on every possible state.
        _ => Done,
    }
}

#[cfg(test)]
mod tests;
