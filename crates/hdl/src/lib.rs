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

pub mod arith;
pub mod demo;
pub mod gates;
pub mod word;

use std::collections::HashMap;
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

/// A named group of wires worth looking at from outside (a bus, a control
/// signal): what the UI labels and what tests read. `name` is the path of the
/// module that declared it, a colon, and a local name: `cpu/alu:result`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub name: String,
    /// Least significant bit first.
    pub wires: Vec<Wire>,
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
    pub probes: Vec<Probe>,
}

/// The combinational fan-in of a set of wires: everything that can change
/// them within one clock cycle. See [`Netlist::fanin_cone`].
#[derive(Clone, Debug)]
pub struct Cone {
    /// Per gate: is this NAND inside the cone?
    pub gates: Vec<bool>,
    /// Per wire: does a root depend on it within the cycle? Includes the
    /// roots themselves and the inputs / flip-flop outputs the cone ends at.
    pub wires: Vec<bool>,
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

    /// The wires named `name[0]`, `name[1]`… among the inputs, in bit order.
    pub fn input_word(&self, name: &str) -> Vec<Wire> {
        word_named(&self.inputs, name)
    }

    /// The wires named `name[0]`, `name[1]`… among the outputs, in bit order.
    pub fn output_word(&self, name: &str) -> Vec<Wire> {
        word_named(&self.outputs, name)
    }

    /// The wires of a probe, by full name (`cpu/alu:result`).
    pub fn probe(&self, name: &str) -> Option<&[Wire]> {
        self.probes
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.wires.as_slice())
    }

    /// Output wires of all flip-flops, in gate order.
    pub fn dff_outputs(&self) -> Vec<Wire> {
        self.gates
            .iter()
            .filter(|g| g.kind == GateKind::Dff)
            .map(|g| g.out)
            .collect()
    }

    /// Walk backwards from `roots` through NAND gates, stopping at primary
    /// inputs, constants and flip-flop outputs. The result says which gates
    /// must be evaluated to know the roots, and which wires they depend on
    /// within a single clock cycle.
    pub fn fanin_cone(&self, roots: &[Wire]) -> Cone {
        let mut nand_driver = vec![u32::MAX; self.wire_count as usize];
        for (i, g) in self.gates.iter().enumerate() {
            if g.kind == GateKind::Nand {
                nand_driver[g.out.index()] = i as u32;
            }
        }
        let mut cone = Cone {
            gates: vec![false; self.gates.len()],
            wires: vec![false; self.wire_count as usize],
        };
        let mut stack: Vec<Wire> = roots.to_vec();
        while let Some(w) = stack.pop() {
            if std::mem::replace(&mut cone.wires[w.index()], true) {
                continue;
            }
            let driver = nand_driver[w.index()];
            if driver != u32::MAX {
                cone.gates[driver as usize] = true;
                let g = &self.gates[driver as usize];
                stack.push(g.a);
                stack.push(g.b);
            }
        }
        cone
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

/// Collect `name[0]`, `name[1]`… from a list of named wires.
fn word_named(named: &[(String, Wire)], name: &str) -> Vec<Wire> {
    let mut bits: Vec<(usize, Wire)> = named
        .iter()
        .filter_map(|(n, w)| {
            let index = n.strip_prefix(name)?.strip_prefix('[')?.strip_suffix(']')?;
            Some((index.parse().ok()?, *w))
        })
        .collect();
    bits.sort_unstable_by_key(|&(i, _)| i);
    bits.into_iter().map(|(_, w)| w).collect()
}

/// Builds a [`Netlist`] one primitive at a time.
pub struct Builder {
    nl: Netlist,
    cur: ModuleId,
    /// Number of drivers per wire (must end up exactly 1).
    drivers: Vec<u8>,
    /// Per-module counters used to name gates: `[nands, dffs]`.
    counters: Vec<[u32; 2]>,
    /// `(parent, name)` → child module, so that a scope can be re-entered.
    children: HashMap<(u32, String), ModuleId>,
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
                probes: Vec::new(),
            },
            cur: ModuleId(0),
            drivers: vec![1, 1], // the two constants are driven by definition
            counters: vec![[0, 0]],
            children: HashMap::new(),
        }
    }

    /// Allocate `n` wires to be driven later (a register's outputs, usually).
    pub fn wires(&mut self, n: usize) -> Vec<Wire> {
        (0..n).map(|_| self.wire()).collect()
    }

    /// Declare an `n`-bit primary input; bit `i` is named `name[i]`.
    pub fn inputs(&mut self, name: &str, n: usize) -> Vec<Wire> {
        (0..n)
            .map(|i| self.input(&format!("{name}[{i}]")))
            .collect()
    }

    /// Declare a multi-bit primary output; bit `i` is named `name[i]`.
    pub fn outputs(&mut self, name: &str, wires: &[Wire]) {
        for (i, &w) in wires.iter().enumerate() {
            self.output(&format!("{name}[{i}]"), w);
        }
    }

    /// Give a group of wires a name that tests and the UI can look up:
    /// `<path of the current module>:<name>`.
    pub fn probe(&mut self, name: &str, wires: &[Wire]) {
        let name = format!("{}:{name}", self.nl.module_path(self.cur));
        debug_assert!(
            self.nl.probes.iter().all(|p| p.name != name),
            "probe {name} declared twice"
        );
        self.nl.probes.push(Probe {
            name,
            wires: wires.to_vec(),
        });
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

    /// Enter the child module `name` of the current module, creating it the
    /// first time; gates created inside `f` belong to it.
    ///
    /// Entering the same name again continues the same module (its gate
    /// numbering carries on), so sibling modules can never share a path. That
    /// is what lets a block be built in two visits: a register's read side
    /// before the logic that computes its next value, its flip-flops after.
    ///
    /// # Panics
    /// If `name` is not lowercase ASCII letters, digits and `_`.
    pub fn scope<R>(&mut self, name: &str, f: impl FnOnce(&mut Self) -> R) -> R {
        assert!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'),
            "module name {name:?} must be lowercase letters, digits or '_'"
        );
        let parent = self.cur;
        let id = match self.children.get(&(parent.0, name.to_string())) {
            Some(&id) => id,
            None => {
                let id = ModuleId(self.nl.modules.len() as u32);
                self.nl.modules.push(Module {
                    name: name.to_string(),
                    parent: Some(parent),
                });
                self.counters.push([0, 0]);
                self.children.insert((parent.0, name.to_string()), id);
                id
            }
        };
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
    fn entering_a_scope_again_continues_the_same_module() {
        let mut b = Builder::new("cpu");
        let a = b.input("a");
        let q = b.wire();
        // First visit: the read side.
        let n = b.scope("reg", |b| b.nand(q, a));
        let d = b.scope("logic", |b| b.nand(n, n));
        // Second visit: the flip-flop, once its input exists.
        b.scope("reg", |b| {
            let extra = b.nand(d, d);
            b.dff_into(q, extra);
        });
        let nl = b.finish().unwrap();
        assert_eq!(nl.modules.len(), 3, "cpu, reg, logic");
        let paths: Vec<String> = (0..nl.gates.len()).map(|i| nl.gate_path(i)).collect();
        assert_eq!(
            paths,
            [
                "cpu/reg/nand0",
                "cpu/logic/nand0",
                "cpu/reg/nand1",
                "cpu/reg/dff0"
            ]
        );
    }

    #[test]
    #[should_panic(expected = "must be lowercase")]
    fn module_names_are_validated() {
        Builder::new("top").scope("Bad/Name", |_| ());
    }

    #[test]
    fn probes_and_words_can_be_looked_up_by_name() {
        let mut b = Builder::new("top");
        let d = b.inputs("d", 3);
        let inverted = b.scope("inv", |b| {
            let out = b.not_word(&d);
            b.probe("out", &out);
            out
        });
        b.outputs("q", &inverted);
        let nl = b.finish().unwrap();
        assert_eq!(nl.input_word("d"), d);
        assert_eq!(nl.output_word("q"), inverted);
        assert_eq!(nl.probe("top/inv:out"), Some(inverted.as_slice()));
        assert_eq!(nl.probe("top/inv:nope"), None);
        assert!(nl.input_word("missing").is_empty());
    }

    #[test]
    fn fanin_cone_stops_at_flip_flops_and_inputs() {
        // out = nand(q, a) where q is a flip-flop fed by nand(b, b).
        let mut b = Builder::new("top");
        let (a, bb) = (b.input("a"), b.input("b"));
        let nb = b.nand(bb, bb);
        let q = b.dff(nb);
        let out = b.nand(q, a);
        let unrelated = b.nand(a, a);
        let nl = b.finish().unwrap();
        let cone = nl.fanin_cone(&[out]);
        assert_eq!(cone.gates, [false, false, true, false]);
        assert!(cone.wires[out.index()] && cone.wires[q.index()] && cone.wires[a.index()]);
        // `b` only reaches `out` through the flip-flop: not within a cycle.
        assert!(!cone.wires[bb.index()] && !cone.wires[nb.index()]);
        assert!(!cone.wires[unrelated.index()]);
        assert_eq!(nl.dff_outputs(), [q]);
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
