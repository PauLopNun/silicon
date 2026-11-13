//! The lockstep harness: run two engines side by side, one clock cycle at a
//! time, and stop at the first cycle in which they disagree about anything
//! observable: the architectural state or what happened on the bus.

use crate::{ArchState, Bus, Engine, EngineKind};
use std::fmt;

/// What a CPU did on the bus during one cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BusOp {
    #[default]
    Idle,
    Read {
        addr: u16,
        value: u8,
    },
    Write {
        addr: u16,
        value: u8,
    },
}

/// Wraps a [`Bus`] and remembers the accesses of the current cycle.
pub struct Traced<'a> {
    inner: &'a mut dyn Bus,
    /// The last access since [`Traced::begin_cycle`].
    pub op: BusOp,
    /// How many accesses there were; more than one breaks the ISA contract.
    pub count: u32,
}

impl<'a> Traced<'a> {
    pub fn new(inner: &'a mut dyn Bus) -> Self {
        Traced {
            inner,
            op: BusOp::Idle,
            count: 0,
        }
    }

    pub fn begin_cycle(&mut self) {
        self.op = BusOp::Idle;
        self.count = 0;
    }
}

impl Bus for Traced<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        let value = self.inner.read(addr);
        self.op = BusOp::Read { addr, value };
        self.count += 1;
        value
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.inner.write(addr, value);
        self.op = BusOp::Write { addr, value };
        self.count += 1;
    }

    fn tick(&mut self) {
        self.inner.tick();
    }
}

/// The first cycle in which two engines disagreed.
#[derive(Clone, Debug)]
pub struct Divergence {
    /// Number of cycles that had matched before this one.
    pub after_cycles: u64,
    /// The state both engines agreed on before the cycle.
    pub before: ArchState,
    pub kinds: (EngineKind, EngineKind),
    pub states: (ArchState, ArchState),
    pub bus: (BusOp, BusOp),
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (ka, kb) = (self.kinds.0.name(), self.kinds.1.name());
        writeln!(
            f,
            "engines diverged after {} matching cycles",
            self.after_cycles
        )?;
        writeln!(
            f,
            "  executing: {} (ir {:#04x}, step {})",
            silicon_isa::form::form_of(self.before.ir).syntax,
            self.before.ir,
            self.before.step
        )?;
        writeln!(f, "  before:  {:x?}", self.before)?;
        writeln!(f, "  {ka:<5} -> {:x?}", self.states.0)?;
        writeln!(f, "  {kb:<5} -> {:x?}", self.states.1)?;
        writeln!(f, "  {ka:<5} bus: {:x?}", self.bus.0)?;
        write!(f, "  {kb:<5} bus: {:x?}", self.bus.1)
    }
}

impl std::error::Error for Divergence {}

/// Step `a` and `b` together for `cycles` clock cycles, each on its own
/// memory, comparing after every cycle. The engines must start in the same
/// state and the memories with the same contents.
pub fn run(
    a: &mut dyn Engine,
    a_mem: &mut dyn Bus,
    b: &mut dyn Engine,
    b_mem: &mut dyn Bus,
    cycles: u64,
) -> Result<(), Box<Divergence>> {
    let mut a_bus = Traced::new(a_mem);
    let mut b_bus = Traced::new(b_mem);
    let mut before = a.state();
    for done in 0..cycles {
        a_bus.begin_cycle();
        b_bus.begin_cycle();
        a.step_cycle(&mut a_bus);
        b.step_cycle(&mut b_bus);
        a_bus.tick();
        b_bus.tick();
        let (sa, sb) = (a.state(), b.state());
        let one_access_each = a_bus.count <= 1 && b_bus.count <= 1;
        if sa != sb || a_bus.op != b_bus.op || !one_access_each {
            return Err(Box::new(Divergence {
                after_cycles: done,
                before,
                kinds: (a.kind(), b.kind()),
                states: (sa, sb),
                bus: (a_bus.op, b_bus.op),
            }));
        }
        before = sa;
    }
    Ok(())
}
