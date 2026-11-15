//! The whole computer: a [`Board`] and the CPU, which is run by one of two
//! interchangeable engines.

use crate::board::{Board, RomTooLarge};
use crate::{ArchState, Bus, Engine, EngineKind, FastCpu, GateCpu};

/// A Silicon computer.
///
/// Both engines are always present; exactly one is *active* and holds the
/// CPU's state. [`Machine::set_engine`] moves the state across, which is
/// possible at any clock cycle because the engines agree on every one.
pub struct Machine {
    board: Board,
    fast: FastCpu,
    gate: GateCpu,
    active: EngineKind,
    /// Where the opcode in `ir` was read from, once one has been read since
    /// power-on or since the state was last overwritten. Not part of the
    /// CPU: an observation of its bus, for showing which instruction runs.
    opcode_at: Option<u16>,
}

impl Machine {
    /// A powered-on computer with `program` in ROM, running on the fast
    /// engine.
    pub fn new(program: &[u8]) -> Result<Self, RomTooLarge> {
        Ok(Machine {
            board: Board::new(program)?,
            fast: FastCpu::new(),
            gate: GateCpu::new(),
            active: EngineKind::Fast,
            opcode_at: None,
        })
    }

    /// Put another program in ROM and start it from power-on. The engine
    /// choice and the keys being held are kept. If the program does not
    /// fit, nothing changes.
    pub fn load(&mut self, program: &[u8]) -> Result<(), RomTooLarge> {
        self.board.load(program)?;
        self.fast.reset();
        Engine::reset(&mut self.gate);
        self.opcode_at = None;
        Ok(())
    }

    /// Back to the power-on state: CPU, RAM, framebuffer and devices. The
    /// ROM, the engine choice and the keys being held are kept.
    pub fn reset(&mut self) {
        self.board.reset();
        self.fast.reset();
        Engine::reset(&mut self.gate);
        self.opcode_at = None;
    }

    /// The engine currently running the CPU.
    pub fn engine(&self) -> EngineKind {
        self.active
    }

    /// Hand the CPU over to the other engine, mid-instruction if need be.
    pub fn set_engine(&mut self, kind: EngineKind) {
        if kind == self.active {
            return;
        }
        match kind {
            EngineKind::Gate => self.gate.load_state(self.fast.state()),
            EngineKind::Fast => self.fast.load_state(&Engine::state(&self.gate)),
        }
        self.active = kind;
    }

    /// Run `cycles` clock cycles on the active engine.
    pub fn run(&mut self, cycles: u64) {
        match self.active {
            // Statically dispatched: this loop is the fast engine's speed.
            EngineKind::Fast => {
                for _ in 0..cycles {
                    self.fast.step_cycle(&mut self.board);
                    self.board.tick();
                    // An opcode fetch always leaves pc one past the opcode.
                    if self.fast.fetched() {
                        self.opcode_at = Some(self.fast.state().pc.wrapping_sub(1));
                    }
                }
            }
            EngineKind::Gate => {
                for _ in 0..cycles {
                    self.gate.step_cycle(&mut self.board);
                    self.board.tick();
                    if self.gate.fetched() {
                        let pc = self.gate.sim().get_word(&self.gate.state_wires().pc) as u16;
                        self.opcode_at = Some(pc.wrapping_sub(1));
                    }
                }
            }
        }
    }

    /// Run until the CPU halts or `max_cycles` have passed; returns the
    /// number of cycles run.
    pub fn run_until_halt(&mut self, max_cycles: u64) -> u64 {
        let mut done = 0;
        while done < max_cycles && !self.halted() {
            self.run(1);
            done += 1;
        }
        done
    }

    /// The CPU's architectural state, from whichever engine is active.
    pub fn state(&self) -> ArchState {
        match self.active {
            EngineKind::Fast => self.fast.state().clone(),
            EngineKind::Gate => Engine::state(&self.gate),
        }
    }

    /// Overwrite the CPU's state (save states, tests). Memory is untouched.
    pub fn load_state(&mut self, state: &ArchState) {
        self.opcode_at = None;
        match self.active {
            EngineKind::Fast => self.fast.load_state(state),
            EngineKind::Gate => self.gate.load_state(state),
        }
    }

    /// Where the instruction now running began: the address its opcode was
    /// read from. `None` until an opcode has been read since power-on (or
    /// since [`Machine::load_state`]). Both engines say when they read one,
    /// so this is the same on either.
    pub fn opcode_at(&self) -> Option<u16> {
        self.opcode_at
    }

    /// Has the CPU executed `hlt`?
    pub fn halted(&self) -> bool {
        match self.active {
            EngineKind::Fast => self.fast.state().halted(),
            EngineKind::Gate => Engine::state(&self.gate).halted(),
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    /// For the host side of the devices: pressing keys, taking console output.
    pub fn board_mut(&mut self) -> &mut Board {
        &mut self.board
    }

    /// The gate-level CPU, for looking at its wires. They are live only while
    /// the gate engine is active; otherwise they show the state at the moment
    /// it was last active.
    pub fn gate(&self) -> &GateCpu {
        &self.gate
    }

    /// Start counting the changes of the gate-level CPU's wires afresh
    /// (see [`GateCpu::toggles`]).
    pub fn clear_wire_toggles(&mut self) {
        self.gate.clear_toggles();
    }
}
