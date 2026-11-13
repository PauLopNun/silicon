//! The CPU: the blocks of this directory wired together, and nothing else.
//!
//! ```text
//!                  ┌───────────────────────────────────────────────┐
//!   data in ──────►│ ir, t          control ◄── ir, step, flags    │
//!                  │   │               │ control word (50 wires)   │
//!                  │   ▼               ▼                           │
//!                  │  regs ──► alu ──► regs, flags                 │
//!                  │   │ pair                                      │
//!                  │   ▼                                           │
//!   address bus ◄──│  agu ◄── pc, sp, t      agu.sum ─► pc, sp, regs
//!   data out ◄─────│  dout ◄── regs port B, pc                     │
//!   rd, wr ◄───────│  control                                      │
//!                  └───────────────────────────────────────────────┘
//! ```
//!
//! All state is declared first, as bare wires; the combinational blocks read
//! those wires; the flip-flops are placed last, once their inputs exist.

use super::{agu, alu, control as c, regs};
use silicon_hdl::{Builder, Netlist, Wire};
use silicon_isa::Reg;

/// The wires through which the CPU talks to the rest of the machine.
#[derive(Clone, Debug)]
pub struct Pins {
    /// Input: the byte being read from memory.
    pub data_in: Vec<Wire>,
    /// Output: the address bus. Depends only on flip-flops.
    pub addr: Vec<Wire>,
    /// Output: the byte to write.
    pub data_out: Vec<Wire>,
    /// Output: this cycle reads `addr`.
    pub rd: Wire,
    /// Output: this cycle writes `data_out` to `addr`.
    pub wr: Wire,
}

/// The output wire of every flip-flop in the CPU, grouped as in
/// [`silicon_isa::ArchState`]. Words are least significant bit first.
#[derive(Clone, Debug)]
pub struct StateWires {
    pub pc: Vec<Wire>,
    pub sp: Vec<Wire>,
    pub regs: [Vec<Wire>; 8],
    /// `Z C N V`, matching the bit order of `ArchState::flags`.
    pub flags: Vec<Wire>,
    pub ir: Vec<Wire>,
    pub step: Vec<Wire>,
    pub t: Vec<Wire>,
}

impl StateWires {
    /// Every state wire, once.
    pub fn all(&self) -> Vec<Wire> {
        let mut all = Vec::new();
        all.extend(&self.pc);
        all.extend(&self.sp);
        for r in &self.regs {
            all.extend(r);
        }
        all.extend(&self.flags);
        all.extend(&self.ir);
        all.extend(&self.step);
        all.extend(&self.t);
        all
    }
}

/// The gate-level CPU and the map from its wires to the outside world.
#[derive(Clone, Debug)]
pub struct Cpu {
    pub netlist: Netlist,
    pub pins: Pins,
    pub state: StateWires,
    /// The control word, one wire per signal (see [`c::SIGNAL_NAMES`]).
    pub signals: Vec<Wire>,
}

/// Build the CPU netlist.
pub fn build() -> Cpu {
    let mut b = Builder::new("cpu");
    let data_in = b.inputs("data_in", 8);

    // All the state there is: 127 flip-flop outputs.
    let state = StateWires {
        pc: b.wires(16),
        sp: b.wires(16),
        regs: std::array::from_fn(|_| b.wires(8)),
        flags: b.wires(4),
        ir: b.wires(8),
        step: b.wires(3),
        t: b.wires(16),
    };
    let (z, carry) = (state.flags[0], state.flags[1]);
    let pair_sel = &state.ir[0..2];

    let signals = c::build(&mut b, &state.ir, &state.step, &state.flags);
    let s = |signal: u64| signals[c::bit(signal)];

    let selects = regs::build_selects(&mut b, &state.ir, &signals);
    let ports = regs::build_ports(&mut b, &state.regs, &selects, pair_sel);

    let address = agu::build(
        &mut b,
        &agu::AguIn {
            pc: &state.pc,
            sp: &state.sp,
            pair: &ports.pair,
            t: &state.t,
            a: &state.regs[Reg::A.index()],
        },
        &signals,
    );

    let alu = alu::build(
        &mut b,
        &alu::AluIn {
            a: &ports.a,
            b: &ports.b,
            data_in: &data_in,
            carry,
        },
        &signals,
    );

    // The flags: each has its own write enable.
    b.scope("flags", |b| {
        // adc / sbc may clear Z but not set it.
        let was_clear = b.not(z);
        let blocked = b.nand(s(c::Z_STICKY), was_clear);
        let new_z = b.and(alu.zero, blocked);
        // A right shift pushes bit 0 into the carry.
        let new_c = b.mux(s(c::FN_SHR), alu.carry_out, alu.x0);
        let new_n = alu.result[7];
        let next = [new_z, new_c, new_n, alu.overflow];
        let enables = [s(c::WE_Z), s(c::WE_C), s(c::WE_N), s(c::WE_V)];
        for i in 0..4 {
            b.register(&state.flags[i..=i], &next[i..=i], enables[i]);
        }
        b.probe("value", &state.flags);
    });

    regs::build_storage(
        &mut b,
        &state.regs,
        &alu.result,
        &address.sum,
        &selects,
        pair_sel,
        &signals,
    );

    b.scope("pc", |b| {
        b.register(&state.pc, &address.sum, s(c::PC_WE));
        b.probe("value", &state.pc);
    });
    b.scope("sp", |b| {
        b.register(&state.sp, &address.sum, s(c::SP_WE));
        b.probe("value", &state.sp);
    });
    b.scope("t", |b| {
        b.register(&state.t[0..8], &data_in, s(c::TL_WE));
        b.register(&state.t[8..16], &data_in, s(c::TH_WE));
        b.probe("value", &state.t);
    });
    b.scope("ir", |b| {
        b.register(&state.ir, &data_in, s(c::IR_WE));
        b.probe("value", &state.ir);
    });
    // The step counter loads the control unit's "next step" every cycle.
    b.scope("step", |b| {
        let next = [s(c::NS0), s(c::NS1), s(c::NS2)];
        for (&q, &d) in state.step.iter().zip(&next) {
            b.dff_into(q, d);
        }
        b.probe("value", &state.step);
    });

    // What a write puts on the data bus: port B, or a byte of pc for call.
    let data_out = b.scope("dout", |b| {
        let is_pc = b.or(s(c::DOUT_PCH), s(c::DOUT_PCL));
        let from_b = b.not(is_pc);
        b.select_onehot(&[
            (from_b, &ports.b),
            (s(c::DOUT_PCH), &state.pc[8..16]),
            (s(c::DOUT_PCL), &state.pc[0..8]),
        ])
    });

    b.outputs("addr", &address.addr);
    b.outputs("data_out", &data_out);
    b.output("rd", s(c::RD));
    b.output("wr", s(c::WR));
    b.probe("signals", &signals);

    let pins = Pins {
        data_in,
        addr: address.addr,
        data_out,
        rd: s(c::RD),
        wr: s(c::WR),
    };
    let netlist = b.finish().expect("the CPU is a valid netlist");
    Cpu {
        netlist,
        pins,
        state,
        signals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silicon_hdl::GateKind;
    use std::collections::HashSet;

    /// Rule 5 of the project: every flip-flop is a bit of `ArchState`.
    #[test]
    fn no_hidden_state() {
        let cpu = build();
        let mapped = cpu.state.all();
        let unique: HashSet<Wire> = mapped.iter().copied().collect();
        assert_eq!(unique.len(), mapped.len(), "a wire is mapped twice");
        let dffs: HashSet<Wire> = cpu.netlist.dff_outputs().into_iter().collect();
        for (i, g) in cpu.netlist.gates.iter().enumerate() {
            if g.kind == GateKind::Dff {
                assert!(
                    unique.contains(&g.out),
                    "{} is a flip-flop that ArchState does not know about",
                    cpu.netlist.gate_path(i)
                );
            }
        }
        assert_eq!(dffs, unique, "a state wire is not driven by a flip-flop");
        // pc 16 + sp 16 + regs 64 + flags 4 + ir 8 + step 3 + t 16.
        assert_eq!(dffs.len(), 127);
    }

    /// ADR 0003: the address bus and the strobes are known before memory
    /// answers, so they must not depend on the data bus within a cycle.
    #[test]
    fn address_and_strobes_do_not_depend_on_data_in() {
        let cpu = build();
        let mut roots = cpu.pins.addr.clone();
        roots.extend([cpu.pins.rd, cpu.pins.wr]);
        let cone = cpu.netlist.fanin_cone(&roots);
        for (i, w) in cpu.pins.data_in.iter().enumerate() {
            assert!(
                !cone.wires[w.index()],
                "the address cone reads data_in[{i}]"
            );
        }
        // And data_in does reach the flip-flops (the check above would be
        // vacuous if the pins were not connected to anything).
        let d_inputs: Vec<Wire> = cpu
            .netlist
            .gates
            .iter()
            .filter(|g| g.kind == GateKind::Dff)
            .map(|g| g.a)
            .collect();
        let main = cpu.netlist.fanin_cone(&d_inputs);
        assert!(cpu.pins.data_in.iter().all(|w| main.wires[w.index()]));
    }

    #[test]
    fn only_nand_and_dff_and_the_paths_are_stable() {
        let cpu = build();
        let nl = &cpu.netlist;
        let paths: HashSet<String> = (0..nl.gates.len()).map(|i| nl.gate_path(i)).collect();
        assert_eq!(paths.len(), nl.gates.len(), "gate paths are unique");
        for expected in [
            "cpu/alu/adder8/fa3/nand2",
            "cpu/agu/adder16/fa15/nand8",
            "cpu/regs/a/dff0",
            "cpu/regs/zh/dff7",
            "cpu/pc/dff15",
            "cpu/sp/dff0",
            "cpu/t/dff8",
            "cpu/ir/dff7",
            "cpu/step/dff2",
            "cpu/flags/dff3",
            "cpu/control/lines/nand0",
            "cpu/dout/nand0",
        ] {
            assert!(paths.contains(expected), "missing gate {expected}");
        }
        // Flip-flop numbering inside a register is the bit number.
        for (i, g) in nl.gates.iter().enumerate() {
            if g.out == cpu.state.pc[9] {
                assert_eq!(nl.gate_path(i), "cpu/pc/dff9");
            }
            if g.out == cpu.state.flags[1] {
                assert_eq!(nl.gate_path(i), "cpu/flags/dff1");
            }
        }
        assert_eq!(nl.probe("cpu/pc:value"), Some(cpu.state.pc.as_slice()));
    }

    /// Size and depth of the design. Printed with `--nocapture`; the bounds
    /// catch an accidental blow-up, the exact numbers live in PROGRESS.md.
    #[test]
    fn size_is_within_budget() {
        let cpu = build();
        let stats = cpu.netlist.stats();
        // NAND count per block, and per sub-block one level down.
        let mut by_block: Vec<(String, usize)> = Vec::new();
        for g in cpu
            .netlist
            .gates
            .iter()
            .filter(|g| g.kind == GateKind::Nand)
        {
            let path = cpu.netlist.module_path(g.module);
            let mut parts = path.split('/').skip(1);
            let block = parts.next().unwrap_or("(top)").to_string();
            let sub = format!("{block}/{}", parts.next().unwrap_or("."));
            for name in [block, sub] {
                match by_block.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, n)) => *n += 1,
                    None => by_block.push((name, 1)),
                }
            }
        }
        by_block.sort();
        println!("{stats:?}");
        for (block, n) in &by_block {
            let indent = if block.contains('/') { "    " } else { "  " };
            println!("{indent}{block:<18} {n:>5} NAND");
        }
        assert!(stats.nands < 4000, "{} NANDs", stats.nands);
        assert!(stats.depth < 120, "depth {}", stats.depth);
        assert_eq!(stats.dffs, 127);
    }
}
