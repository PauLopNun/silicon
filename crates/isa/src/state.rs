//! The architectural contract shared by both engines: what the CPU's state
//! is, and what it sees outside itself.

/// Bit positions of the four flags inside [`ArchState::flags`].
pub mod flag {
    /// Zero: the last result was 0.
    pub const Z: u8 = 1 << 0;
    /// Carry out of the adder. After a subtraction or compare it means
    /// "no borrow", i.e. `a >= operand` for unsigned values.
    pub const C: u8 = 1 << 1;
    /// Negative: bit 7 of the last result.
    pub const N: u8 = 1 << 2;
    /// Signed overflow of the last addition or subtraction.
    pub const V: u8 = 1 << 3;
    /// The bits of `flags` that exist in hardware.
    pub const MASK: u8 = 0x0F;
}

/// The step counter is three flip-flops.
pub const STEP_MASK: u8 = 0x07;

/// Complete architectural state of the CPU at a clock-cycle boundary.
///
/// Every flip-flop of the gate-level CPU is one bit of this struct (a test
/// enforces it), so copying an `ArchState` from one engine to the other moves
/// the whole machine, even in the middle of an instruction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArchState {
    /// Program counter: address of the next byte to fetch.
    pub pc: u16,
    /// Stack pointer: address of the last byte pushed. The stack grows down.
    pub sp: u16,
    /// The eight registers, indexed by [`crate::Reg`]:
    /// `a b xl xh yl yh zl zh`.
    pub regs: [u8; 8],
    /// Flags, packed as in [`flag`]. Only the low four bits exist.
    pub flags: u8,
    /// Instruction register: the opcode being executed.
    pub ir: u8,
    /// Position inside the current instruction. `0` means "fetch an opcode".
    /// Only the low three bits exist.
    pub step: u8,
    /// Operand latch: holds an immediate address or offset while a
    /// multi-byte instruction is being read.
    pub t: u16,
    /// Clock cycles since reset. Kept by the engine, not by flip-flops.
    pub cycle: u64,
}

impl ArchState {
    /// Value of a 16-bit register pair (`0` = `ba`, `1` = `x`, `2` = `y`,
    /// `3` = `z`).
    #[inline]
    pub fn pair(&self, p: usize) -> u16 {
        u16::from_le_bytes([self.regs[2 * p], self.regs[2 * p + 1]])
    }

    /// Write a 16-bit register pair.
    #[inline]
    pub fn set_pair(&mut self, p: usize, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.regs[2 * p] = lo;
        self.regs[2 * p + 1] = hi;
    }

    /// True once the CPU has executed `hlt`: it will never leave this state.
    #[inline]
    pub fn halted(&self) -> bool {
        self.ir == crate::form::HLT.value && self.step == 1
    }

    /// The same state with the bits that have no flip-flop cleared.
    pub fn masked(mut self) -> Self {
        self.flags &= flag::MASK;
        self.step &= STEP_MASK;
        self
    }
}

/// What the CPU sees outside itself: a 16-bit address space of bytes.
///
/// The CPU performs at most one access per clock cycle.
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);

    /// One clock cycle has passed. Devices that keep time count these.
    ///
    /// The CPU never calls this: whoever drives the clock does (the machine,
    /// the lockstep harness), exactly once after each CPU cycle. Plain memory
    /// ignores it.
    #[inline]
    fn tick(&mut self) {}
}

impl<B: Bus + ?Sized> Bus for &mut B {
    #[inline]
    fn read(&mut self, addr: u16) -> u8 {
        (**self).read(addr)
    }
    #[inline]
    fn write(&mut self, addr: u16, value: u8) {
        (**self).write(addr, value)
    }
    #[inline]
    fn tick(&mut self) {
        (**self).tick()
    }
}
