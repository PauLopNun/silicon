//! # Silicon HDL
//!
//! A tiny embedded hardware description language. A design is a tree of named
//! modules whose leaves are exactly two primitives:
//!
//! * `nand` — a 2-input NAND gate (combinational)
//! * `dff`  — a positive-edge D flip-flop (the only state element)
//!
//! Everything else in Silicon (adders, registers, the whole CPU) is built by
//! composing these two inside [`Builder::scope`] blocks. The hierarchy is kept
//! in the compiled [`Netlist`]: every gate knows its module, so it has a stable
//! path such as `cpu/alu/adder8/fa3/nand2` that the simulator and the 3D
//! renderer both use.
//!
//! ```
//! use silicon_hdl::Builder;
//! let mut b = Builder::new("not");
//! let a = b.input("a");
//! let y = b.nand(a, a);
//! b.output("y", y);
//! let netlist = b.finish().unwrap();
//! assert_eq!(netlist.stats().nands, 1);
//! ```

pub mod demo;

use std::fmt;

/// A single-bit net. Wires `0` and `1` are the constants [`Wire::ZERO`] and
/// [`Wire::ONE`]; every other wire is driven by exactly one input or gate.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Wire(pub u32);

impl Wire {
    /// Constant logic 0.
    pub const ZERO: Wire = Wire(0);
    /// Constant logic 1.
    pub const ONE: Wire = Wire(1);

    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index of a module in [`Netlist::modules`]. The top module is always `0`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ModuleId(pub u32);

/// The two primitives every design is made of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateKind {
    /// `out = !(a & b)`
    Nand,
    /// `out` takes the value of `a` at each clock edge. `b` is unused.
    Dff,
}

/// One primitive instance.
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    pub kind: GateKind,
    pub a: Wire,
    pub b: Wire,
    pub out: Wire,
    /// Module that directly contains this gate.
    pub module: ModuleId,
    /// Index among gates of the same kind inside `module` (`nand0`, `nand1`…).
    pub local: u32,
}

/// One node of the module hierarchy.
#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub parent: Option<ModuleId>,
}

/// A compiled design: flat gate list plus the module tree it came from.
#[derive(Clone, Debug)]
pub struct Netlist {
    pub gates: Vec<Gate>,
    /// Total number of wires, including the two constants.
    pub wire_count: u32,
    pub modules: Vec<Module>,
    pub inputs: Vec<(String, Wire)>,
    pub outputs: Vec<(String, Wire)>,
}

/// Errors detected while building or compiling a netlist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HdlError {
    /// A wire was allocated with [`Builder::wire`] but never driven.
    Undriven(Wire),
    /// A wire is driven by more than one source.
    MultipleDrivers(Wire),
    /// A cycle of NAND gates with no flip-flop in it. Holds the path of one
    /// gate on the loop.
    CombinationalLoop(String),
}

impl fmt::Display for HdlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HdlError::Undriven(w) => write!(f, "wire {} is never driven", w.0),
            HdlError::MultipleDrivers(w) => write!(f, "wire {} has more than one driver", w.0),
            HdlError::CombinationalLoop(p) => {
                write!(
                    f,
                    "combinational loop (no flip-flop on the cycle) through {p}"
                )
            }
        }
    }
}

impl std::error::Error for HdlError {}

/// Result of [`Netlist::levelize`]: an evaluation order for the NAND gates in
/// which every gate comes after the gates that feed it.
#[derive(Clone, Debug)]
pub struct Levelized {
    /// NAND gate indices (into [`Netlist::gates`]) in evaluation order.
    pub order: Vec<u32>,
    /// Logic level of every gate (flip-flops are level 0, a NAND fed only by
    /// inputs/flip-flops is level 1).
    pub level: Vec<u32>,
    /// Longest NAND chain between two clock edges: the critical path depth.
    pub depth: u32,
}

/// Headline numbers for a netlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub nands: usize,
    pub dffs: usize,
    pub wires: usize,
    pub modules: usize,
    pub depth: u32,
}

impl Netlist {
    /// Slash-separated path of a module, e.g. `cpu/alu/adder8`.
    pub fn module_path(&self, id: ModuleId) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(m) = cur {
            let module = &self.modules[m.0 as usize];
            parts.push(module.name.as_str());
            cur = module.parent;
        }
        parts.reverse();
        parts.join("/")
    }

    /// Full hierarchical path of a gate, e.g. `cpu/alu/adder8/fa3/nand2`.
    pub fn gate_path(&self, gate: usize) -> String {
        let g = &self.gates[gate];
        let kind = match g.kind {
            GateKind::Nand => "nand",
            GateKind::Dff => "dff",
        };
        format!("{}/{}{}", self.module_path(g.module), kind, g.local)
    }

    /// Topologically order the NAND gates. Inputs, constants and flip-flop
    /// outputs are the starting points, so flip-flops legally break cycles.
    pub fn levelize(&self) -> Result<Levelized, HdlError> {
        let n = self.gates.len();
        // Which NAND gate (if any) drives each wire.
        let mut nand_driver = vec![u32::MAX; self.wire_count as usize];
        for (i, g) in self.gates.iter().enumerate() {
            if g.kind == GateKind::Nand {
                nand_driver[g.out.index()] = i as u32;
            }
        }
        // Kahn's algorithm over NAND→NAND edges.
        let mut pending = vec![0u32; n];
        let mut fanout: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (i, g) in self.gates.iter().enumerate() {
            if g.kind != GateKind::Nand {
                continue;
            }
            for w in [g.a, g.b] {
                let d = nand_driver[w.index()];
                if d != u32::MAX {
                    pending[i] += 1;
                    fanout[d as usize].push(i as u32);
                }
            }
        }
        let mut level = vec![0u32; n];
        let mut order = Vec::with_capacity(n);
        let mut ready: Vec<u32> = (0..n as u32)
            .filter(|&i| self.gates[i as usize].kind == GateKind::Nand && pending[i as usize] == 0)
            .collect();
        for &i in &ready {
            level[i as usize] = 1;
        }
        let mut head = 0;
        while head < ready.len() {
            let i = ready[head];
            head += 1;
            order.push(i);
            for &j in &fanout[i as usize] {
                level[j as usize] = level[j as usize].max(level[i as usize] + 1);
                pending[j as usize] -= 1;
                if pending[j as usize] == 0 {
                    ready.push(j);
                }
            }
        }
        if let Some(stuck) = (0..n).find(|&i| pending[i] != 0) {
            return Err(HdlError::CombinationalLoop(self.gate_path(stuck)));
        }
        let depth = level.iter().copied().max().unwrap_or(0);
        Ok(Levelized {
            order,
            level,
            depth,
        })
    }

    /// Gate/wire counts and critical path depth.
    ///
    /// # Panics
    /// Panics if the netlist has a combinational loop; netlists produced by
    /// [`Builder::finish`] never do.
    pub fn stats(&self) -> Stats {
        let nands = self
            .gates
            .iter()
            .filter(|g| g.kind == GateKind::Nand)
            .count();
        Stats {
            nands,
            dffs: self.gates.len() - nands,
            wires: self.wire_count as usize,
            modules: self.modules.len(),
            depth: self
                .levelize()
                .expect("netlist has a combinational loop")
                .depth,
        }
    }
}

/// Builds a [`Netlist`] one primitive at a time.
pub struct Builder {
    nl: Netlist,
    cur: ModuleId,
    /// Number of drivers per wire (must end up exactly 1).
    drivers: Vec<u8>,
    /// Per-module counters used to name gates: `[nands, dffs]`.
    counters: Vec<[u32; 2]>,
}

impl Builder {
    /// Start a design whose top module is called `top`.
    pub fn new(top: &str) -> Self {
        Builder {
            nl: Netlist {
                gates: Vec::new(),
                wire_count: 2,
                modules: vec![Module {
                    name: top.to_string(),
                    parent: None,
                }],
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
            cur: ModuleId(0),
            drivers: vec![1, 1], // the two constants are driven by definition
            counters: vec![[0, 0]],
        }
    }

    /// Allocate a wire that will be driven later with [`Builder::dff_into`].
    /// This is how feedback (state) is expressed.
    pub fn wire(&mut self) -> Wire {
        let w = Wire(self.nl.wire_count);
        self.nl.wire_count += 1;
        self.drivers.push(0);
        w
    }

    /// Declare a primary input of the whole design.
    pub fn input(&mut self, name: &str) -> Wire {
        let w = self.wire();
        self.drivers[w.index()] = 1;
        self.nl.inputs.push((name.to_string(), w));
        w
    }

    /// Declare a primary output of the whole design.
    pub fn output(&mut self, name: &str, w: Wire) {
        self.nl.outputs.push((name.to_string(), w));
    }

    /// Place a NAND gate in the current module and return its output.
    pub fn nand(&mut self, a: Wire, b: Wire) -> Wire {
        let out = self.wire();
        self.push(GateKind::Nand, a, b, out);
        out
    }

    /// Place a flip-flop in the current module and return its output `q`.
    pub fn dff(&mut self, d: Wire) -> Wire {
        let q = self.wire();
        self.push(GateKind::Dff, d, Wire::ZERO, q);
        q
    }

    /// Place a flip-flop whose output is the pre-allocated wire `q`.
    pub fn dff_into(&mut self, q: Wire, d: Wire) {
        self.push(GateKind::Dff, d, Wire::ZERO, q);
    }

    /// Build a child module named `name`; gates created inside `f` belong to it.
    pub fn scope<R>(&mut self, name: &str, f: impl FnOnce(&mut Self) -> R) -> R {
        let parent = self.cur;
        let id = ModuleId(self.nl.modules.len() as u32);
        self.nl.modules.push(Module {
            name: name.to_string(),
            parent: Some(parent),
        });
        self.counters.push([0, 0]);
        self.cur = id;
        let r = f(self);
        self.cur = parent;
        r
    }

    fn push(&mut self, kind: GateKind, a: Wire, b: Wire, out: Wire) {
        let counter = &mut self.counters[self.cur.0 as usize][kind as usize];
        let local = *counter;
        *counter += 1;
        self.drivers[out.index()] = self.drivers[out.index()].saturating_add(1);
        self.nl.gates.push(Gate {
            kind,
            a,
            b,
            out,
            module: self.cur,
            local,
        });
    }

    /// Validate the design (single driver per wire, no combinational loops)
    /// and return the netlist.
    pub fn finish(self) -> Result<Netlist, HdlError> {
        for (i, &d) in self.drivers.iter().enumerate() {
            match d {
                0 => return Err(HdlError::Undriven(Wire(i as u32))),
                1 => {}
                _ => return Err(HdlError::MultipleDrivers(Wire(i as u32))),
            }
        }
        self.nl.levelize()?;
        Ok(self.nl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_demo_shape() {
        let nl = demo::xor();
        let s = nl.stats();
        assert_eq!((s.nands, s.dffs, s.depth), (4, 0, 3));
        assert_eq!(nl.gate_path(0), "demo/xor/nand0");
        assert_eq!(nl.gate_path(3), "demo/xor/nand3");
    }

    #[test]
    fn levelized_order_respects_dependencies() {
        let nl = demo::xor();
        let lv = nl.levelize().unwrap();
        let mut seen = vec![false; nl.wire_count as usize];
        for (_, w) in &nl.inputs {
            seen[w.index()] = true;
        }
        for &g in &lv.order {
            let g = &nl.gates[g as usize];
            assert!(seen[g.a.index()] && seen[g.b.index()]);
            seen[g.out.index()] = true;
        }
    }

    #[test]
    fn nested_scopes_build_paths() {
        let mut b = Builder::new("cpu");
        let a = b.input("a");
        let y = b.scope("alu", |b| b.scope("inv", |b| b.nand(a, a)));
        let q = b.scope("reg", |b| b.dff(y));
        b.output("q", q);
        let nl = b.finish().unwrap();
        assert_eq!(nl.gate_path(0), "cpu/alu/inv/nand0");
        assert_eq!(nl.gate_path(1), "cpu/reg/dff0");
    }

    #[test]
    fn dff_breaks_feedback_loops() {
        // q' = !q : legal, because the loop goes through a flip-flop.
        let mut b = Builder::new("toggle");
        let q = b.wire();
        let d = b.nand(q, q);
        b.dff_into(q, d);
        assert!(b.finish().is_ok());
    }

    #[test]
    fn combinational_loop_is_rejected() {
        // An SR latch made of raw NANDs has no flip-flop on its cycle.
        let mut b = Builder::new("latch");
        let s = b.input("s");
        let q = b.wire();
        let nq = b.nand(q, s);
        // Drive `q` combinationally from `nq` by hand.
        b.push(GateKind::Nand, nq, s, q);
        assert!(matches!(b.finish(), Err(HdlError::CombinationalLoop(_))));
    }

    #[test]
    fn undriven_and_multiply_driven_wires_are_rejected() {
        let mut b = Builder::new("bad");
        let w = b.wire();
        let _ = b.nand(w, w);
        assert_eq!(b.finish().unwrap_err(), HdlError::Undriven(w));

        let mut b = Builder::new("bad");
        let a = b.input("a");
        let q = b.wire();
        b.dff_into(q, a);
        b.dff_into(q, a);
        assert_eq!(b.finish().unwrap_err(), HdlError::MultipleDrivers(q));
    }
}
