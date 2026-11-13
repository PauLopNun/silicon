//! The control unit: which wires to pull in each cycle of each instruction.
//!
//! It is written as a table, the *microcode* ([`microcode`]): one line per
//! (instruction form, step), listing the control signals that are high in
//! that cycle. [`build`] turns the table into gates the way a PLA does:
//!
//! 1. *decode*: AND terms that recognise opcode bit patterns;
//! 2. *lines*: one product term per table line (pattern AND step);
//! 3. *plane*: every control signal is the OR of the lines that raise it.
//!
//! [`control_word`] reads the same table in plain Rust and is the reference
//! the gates are checked against, for every opcode, step and flag value. What
//! the table *says* is checked separately, by running the whole CPU in
//! lockstep with the fast model, which does not use it.

use silicon_hdl::{Builder, Wire};
use silicon_isa::form::{self, Form};
use silicon_isa::{AluOp, Cond};
use std::collections::HashMap;
use std::sync::OnceLock;

macro_rules! signals {
    ($( $name:ident: $doc:literal, )*) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[derive(Clone, Copy)]
        enum Index { $( $name, )* }
        $( #[doc = $doc] pub const $name: u64 = 1 << Index::$name as u64; )*
        /// Signal names, by bit number of the control word.
        pub const SIGNAL_NAMES: &[&str] = &[ $( stringify!($name), )* ];
        /// One-line description of each signal, by bit number.
        pub const SIGNAL_DOCS: &[&str] = &[ $( $doc, )* ];
    };
}

signals! {
    BASE_SP:   "address unit: base is sp (pc when no BASE_* is high)",
    BASE_PAIR: "address unit: base is the register pair in opcode bits 1..0",
    BASE_T:    "address unit: base is the operand latch t",
    OFF_T:     "address unit: add the low byte of t",
    OFF_SX:    "address unit: sign-extend that byte instead of zero-extending",
    OFF_A:     "address unit: add register a, zero-extended",
    OFF_M1:    "address unit: add −1",
    AGU_CIN:   "address unit: add 1",
    PRE:       "address bus carries base + offset (the base alone otherwise)",
    RD:        "read memory this cycle",
    WR:        "write memory this cycle",
    PC_WE:     "pc ← base + offset",
    SP_WE:     "sp ← base + offset",
    PAIR_WE:   "register pair ← base + offset",
    DOUT_PCH:  "data out is the high byte of pc (port B otherwise)",
    DOUT_PCL:  "data out is the low byte of pc",
    IR_WE:     "instruction register ← data in",
    TL_WE:     "low byte of t ← data in",
    TH_WE:     "high byte of t ← data in",
    A_PD:      "ALU left operand is half of the pair in opcode bits 3..2 (a otherwise)",
    B_PS:      "port B is half of the pair in opcode bits 1..0",
    B_A:       "port B is register a (the register in opcode bits 2..0 otherwise)",
    Y_DATA:    "ALU right operand is data in (port B otherwise)",
    X_ZERO:    "force the ALU left operand to 0x00",
    X_ONES:    "force the ALU left operand to 0xFF",
    Y_INV:     "invert the ALU right operand (subtract)",
    CIN_ONE:   "ALU carry in is 1",
    CIN_C:     "ALU carry in is the C flag",
    FN_AND:    "ALU result is AND (the adder's sum when no FN_* is high)",
    FN_OR:     "ALU result is OR",
    FN_XOR:    "ALU result is XOR",
    FN_SHR:    "ALU result is the left operand shifted right",
    SIN_C:     "the right shift takes the C flag into bit 7",
    SIN_X7:    "the right shift keeps bit 7 (arithmetic shift)",
    WE_Z:      "write the Z flag",
    WE_N:      "write the N flag",
    WE_C:      "write the C flag",
    WE_V:      "write the V flag",
    Z_STICKY:  "Z may only be cleared, not set (adc, sbc)",
    REG_WE:    "write the ALU result to a register (a unless a W_* is high)",
    W_D:       "…the register in opcode bits 5..3",
    W_R:       "…the register in opcode bits 2..0",
    W_PD:      "…half of the pair in opcode bits 3..2",
    W_PS:      "…half of the pair in opcode bits 1..0",
    HALF:      "pair halves above mean the high byte (the low byte otherwise)",
    NS0:       "next step, bit 0",
    NS1:       "next step, bit 1",
    NS2:       "next step, bit 2",
    ALU_HI:    "decode the ALU operation from opcode bits 5..3",
    ALU_LO:    "decode the ALU operation from opcode bits 2..0",
}

/// Number of control signals.
pub const SIGNAL_COUNT: usize = SIGNAL_NAMES.len();

/// Bit number of a signal mask such as [`RD`].
pub fn bit(signal: u64) -> usize {
    debug_assert_eq!(signal.count_ones(), 1);
    signal.trailing_zeros() as usize
}

// Groups of signals that always travel together.

/// Read the byte at `pc` and step past it.
const OPERAND: u64 = AGU_CIN | RD | PC_WE;
/// Read an opcode into `ir`; the next cycle is step 1 of that instruction.
/// The address is `pc` unless a `BASE_*` signal says otherwise.
const FETCH: u64 = OPERAND | IR_WE | NS0;
/// Route `data in` through the ALU untouched (0 + data) into a register.
const LOAD: u64 = X_ZERO | Y_DATA | REG_WE;
/// Push one byte: address `sp − 1`, and `sp` takes that address.
const PUSH_BYTE: u64 = BASE_SP | OFF_M1 | PRE | SP_WE | WR;
/// Pop one byte: address `sp`, and `sp` takes `sp + 1`.
const POP_BYTE: u64 = BASE_SP | AGU_CIN | SP_WE | RD;
const WE_ZN: u64 = WE_Z | WE_N;
const WE_ZNV: u64 = WE_Z | WE_N | WE_V;
const WE_ZNCV: u64 = WE_Z | WE_N | WE_C | WE_V;

/// The `NS*` signals that make `step` the next step.
const fn next_step(step: u8) -> u64 {
    (if step & 1 != 0 { NS0 } else { 0 })
        | (if step & 2 != 0 { NS1 } else { 0 })
        | (if step & 4 != 0 { NS2 } else { 0 })
}

/// When a microcode line applies, beyond matching opcode and step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    Always,
    /// Only if the branch condition in opcode bits 2..0 holds.
    Taken,
}

/// One line of microcode.
#[derive(Clone, Debug)]
pub struct Line {
    /// Name of the instruction form, or `FETCH` for the opcode fetch cycle.
    pub form: &'static str,
    /// The line applies when `ir & mask == value`…
    pub mask: u8,
    pub value: u8,
    /// …and the step counter equals this…
    pub step: u8,
    /// …and the guard holds.
    pub guard: Guard,
    /// The control signals it raises.
    pub signals: u64,
}

/// Lines for an instruction whose steps follow one another unconditionally.
/// `steps[0]` is step 1. Every step but the last continues to the next one;
/// the last says for itself what follows (`FETCH`, or nothing: step 0).
fn sequence(lines: &mut Vec<Line>, form: &Form, steps: &[u64]) {
    for (i, &signals) in steps.iter().enumerate() {
        let step = i as u8 + 1;
        let last = i + 1 == steps.len();
        lines.push(Line {
            form: form.name,
            mask: form.mask,
            value: form.value,
            step,
            guard: Guard::Always,
            signals: signals | if last { 0 } else { next_step(step + 1) },
        });
    }
}

fn build_microcode() -> Vec<Line> {
    let mut m = Vec::new();

    // Step 0, whatever is in ir: fetch an opcode.
    m.push(Line {
        form: "FETCH",
        mask: 0,
        value: 0,
        step: 0,
        guard: Guard::Always,
        signals: FETCH,
    });

    // Instructions whose last cycle leaves the bus and the address unit free
    // end with `| FETCH`: they fetch the next opcode in that same cycle.

    sequence(&mut m, &form::HLT, &[next_step(1)]); // stay in step 1 for ever
    sequence(&mut m, &form::MOV_SP_X, &[BASE_PAIR | SP_WE]);
    sequence(
        &mut m,
        &form::RET,
        &[POP_BYTE | TL_WE, POP_BYTE | TH_WE, BASE_T | FETCH],
    );
    sequence(&mut m, &form::JMP_Z, &[BASE_PAIR | FETCH]);

    sequence(&mut m, &form::LD_A_PTR, &[BASE_PAIR | RD | LOAD]);
    sequence(
        &mut m,
        &form::LD_A_PTR_INC,
        &[BASE_PAIR | AGU_CIN | PAIR_WE | RD | LOAD],
    );
    sequence(&mut m, &form::ST_PTR_A, &[BASE_PAIR | WR | B_A]);
    sequence(
        &mut m,
        &form::ST_PTR_INC_A,
        &[BASE_PAIR | AGU_CIN | PAIR_WE | WR | B_A],
    );
    sequence(
        &mut m,
        &form::LD_A_IDX,
        &[BASE_PAIR | OFF_A | PRE | RD | LOAD],
    );

    sequence(
        &mut m,
        &form::LD_PAIR_IMM,
        &[OPERAND | LOAD | W_PS, OPERAND | LOAD | W_PS | HALF],
    );
    sequence(
        &mut m,
        &form::PUSH,
        &[PUSH_BYTE | B_PS | HALF, PUSH_BYTE | B_PS],
    );
    sequence(
        &mut m,
        &form::POP,
        &[POP_BYTE | LOAD | W_PS, POP_BYTE | LOAD | W_PS | HALF],
    );

    sequence(
        &mut m,
        &form::LEA_SP,
        &[OPERAND | TL_WE, BASE_SP | OFF_T | PAIR_WE],
    );
    sequence(
        &mut m,
        &form::LEA_PAIR,
        &[OPERAND | TL_WE, BASE_PAIR | OFF_T | OFF_SX | PAIR_WE],
    );
    sequence(&mut m, &form::LEA_IDX, &[BASE_PAIR | OFF_A | PAIR_WE]);

    // 16-bit add: the 8-bit adder twice, low bytes then high bytes + carry.
    sequence(
        &mut m,
        &form::ADD_PAIR,
        &[
            A_PD | B_PS | REG_WE | W_PD | WE_ZNCV,
            A_PD | B_PS | HALF | CIN_C | Z_STICKY | REG_WE | W_PD | WE_ZNCV | FETCH,
        ],
    );

    // mov is "0 + rs" through the adder, written to rd.
    sequence(&mut m, &form::MOV, &[X_ZERO | REG_WE | W_D | FETCH]);
    sequence(&mut m, &form::ALU_REG, &[ALU_HI | FETCH]);
    sequence(
        &mut m,
        &form::INC,
        &[X_ZERO | CIN_ONE | REG_WE | W_R | WE_ZNV | FETCH],
    );
    // dec adds 0xFF.
    sequence(
        &mut m,
        &form::DEC,
        &[X_ONES | REG_WE | W_R | WE_ZNV | FETCH],
    );

    sequence(&mut m, &form::LD_IMM, &[OPERAND | LOAD | W_R]);
    sequence(
        &mut m,
        &form::LD_SP,
        &[OPERAND | TL_WE, BASE_SP | OFF_T | PRE | RD | LOAD | W_R],
    );
    sequence(
        &mut m,
        &form::ST_SP,
        &[OPERAND | TL_WE, BASE_SP | OFF_T | PRE | WR],
    );
    sequence(&mut m, &form::ALU_IMM, &[OPERAND | Y_DATA | ALU_LO]);

    // Conditional branch: read the offset; only if the condition holds go on
    // to step 2, which adds it to pc.
    let br = &form::BR;
    let line = |step, guard, signals| Line {
        form: br.name,
        mask: br.mask,
        value: br.value,
        step,
        guard,
        signals,
    };
    m.push(line(1, Guard::Always, OPERAND | TL_WE));
    m.push(line(1, Guard::Taken, next_step(2)));
    m.push(line(2, Guard::Always, OFF_T | OFF_SX | PC_WE));

    sequence(
        &mut m,
        &form::JMP,
        &[OPERAND | TL_WE, OPERAND | TH_WE, BASE_T | FETCH],
    );
    sequence(
        &mut m,
        &form::CALL,
        &[
            OPERAND | TL_WE,
            OPERAND | TH_WE,
            PUSH_BYTE | DOUT_PCH,
            PUSH_BYTE | DOUT_PCL,
            BASE_T | FETCH,
        ],
    );
    sequence(
        &mut m,
        &form::LD_A_ABS,
        &[OPERAND | TL_WE, OPERAND | TH_WE, BASE_T | RD | LOAD],
    );
    sequence(
        &mut m,
        &form::ST_ABS_A,
        &[OPERAND | TL_WE, OPERAND | TH_WE, BASE_T | WR | B_A],
    );
    sequence(
        &mut m,
        &form::LEA_SP_SP,
        &[OPERAND | TL_WE, BASE_SP | OFF_T | OFF_SX | SP_WE],
    );

    let shift = FN_SHR | REG_WE | WE_ZN | WE_C | FETCH;
    sequence(&mut m, &form::SHR_A, &[shift]);
    sequence(&mut m, &form::ROR_A, &[shift | SIN_C]);
    sequence(&mut m, &form::ASR_A, &[shift | SIN_X7]);

    m
}

/// The microcode table.
pub fn microcode() -> &'static [Line] {
    static TABLE: OnceLock<Vec<Line>> = OnceLock::new();
    TABLE.get_or_init(build_microcode)
}

/// What each of the eight ALU operations adds to a cycle that raises
/// `ALU_HI` or `ALU_LO`: a second, smaller table decoded from three opcode
/// bits.
pub fn alu_signals(op: AluOp) -> u64 {
    match op {
        AluOp::Add => REG_WE | WE_ZNCV,
        AluOp::Adc => REG_WE | WE_ZNCV | CIN_C | Z_STICKY,
        AluOp::Sub => REG_WE | WE_ZNCV | Y_INV | CIN_ONE,
        AluOp::Sbc => REG_WE | WE_ZNCV | Y_INV | CIN_C | Z_STICKY,
        AluOp::And => REG_WE | WE_ZN | FN_AND,
        AluOp::Or => REG_WE | WE_ZN | FN_OR,
        AluOp::Xor => REG_WE | WE_ZN | FN_XOR,
        AluOp::Cmp => WE_ZNCV | Y_INV | CIN_ONE,
    }
}

/// Reference: the control word for a given instruction register, step
/// counter and flags, read straight from the tables.
pub fn control_word(ir: u8, step: u8, flags: u8) -> u64 {
    let taken = Cond::from_bits(ir).holds(flags);
    let mut word = 0;
    for line in microcode() {
        let guard = line.guard == Guard::Always || taken;
        if line.step == step && ir & line.mask == line.value && guard {
            word |= line.signals;
        }
    }
    if word & ALU_HI != 0 {
        word |= alu_signals(AluOp::from_bits(ir >> 3));
    }
    if word & ALU_LO != 0 {
        word |= alu_signals(AluOp::from_bits(ir));
    }
    word
}

/// Recognises opcode bit patterns, sharing sub-terms between patterns. The
/// opcode is cut into three fields (bits 7..6, 5..3, 2..0) because that is
/// how the encoding is laid out; a pattern is the AND of one term per field.
struct Decoder {
    /// Each opcode bit and its complement.
    literals: [[Wire; 2]; 8],
    fields: HashMap<(u8, u8), Wire>,
    patterns: HashMap<(u8, u8), Wire>,
}

impl Decoder {
    const FIELDS: [u8; 3] = [0xC0, 0x38, 0x07];

    fn new(b: &mut Builder, ir: &[Wire]) -> Self {
        let mut literals = [[Wire::ZERO; 2]; 8];
        for (i, &w) in ir.iter().enumerate() {
            literals[i] = [b.not(w), w];
        }
        Decoder {
            literals,
            fields: HashMap::new(),
            patterns: HashMap::new(),
        }
    }

    /// High when the bits of `ir` selected by `mask` equal `value`.
    /// `None` when the mask is empty (always true).
    fn pattern(&mut self, b: &mut Builder, mask: u8, value: u8) -> Option<Wire> {
        if mask == 0 {
            return None;
        }
        if let Some(&w) = self.patterns.get(&(mask, value)) {
            return Some(w);
        }
        let mut terms = Vec::new();
        for field in Self::FIELDS {
            let (m, v) = (mask & field, value & field);
            if m == 0 {
                continue;
            }
            let term = match self.fields.get(&(m, v)) {
                Some(&w) => w,
                None => {
                    let literals: Vec<Wire> = (0..8usize)
                        .filter(|&bit| m >> bit & 1 == 1)
                        .map(|bit| self.literals[bit][(v >> bit & 1) as usize])
                        .collect();
                    let w = b.and_all(&literals);
                    self.fields.insert((m, v), w);
                    w
                }
            };
            terms.push(term);
        }
        let w = b.and_all(&terms);
        self.patterns.insert((mask, value), w);
        Some(w)
    }
}

/// The OR plane. `sets[i]` lists the active-low lines that raise signal `i`;
/// the result is one active-high wire per set.
///
/// Many signals are raised by nearly the same lines (every operand read
/// raises `RD`, `PC_WE` and `AGU_CIN`), so partial sums are shared: while
/// some pair of lines appears together in two or more sets, that pair is
/// combined once and replaces both lines in every set that has them. Each
/// such step costs two NANDs and saves two in every set it shortens.
fn sum_plane(b: &mut Builder, mut sets: Vec<Vec<Wire>>) -> Vec<Wire> {
    for set in &mut sets {
        set.sort_unstable();
        set.dedup();
    }
    loop {
        let mut pairs: HashMap<(Wire, Wire), u32> = HashMap::new();
        for set in &sets {
            for (i, &x) in set.iter().enumerate() {
                for &y in &set[i + 1..] {
                    *pairs.entry((x, y)).or_default() += 1;
                }
            }
        }
        // The most shared pair; ties go to the lowest wire numbers so that
        // the netlist does not depend on hash order.
        let best = pairs
            .into_iter()
            .max_by_key(|&(pair, count)| (count, std::cmp::Reverse(pair)));
        let Some(((x, y), count)) = best else { break };
        if count < 2 {
            break;
        }
        // Lines are active-low, so "either line" is the AND of the two.
        let either = b.and(x, y);
        for set in &mut sets {
            if set.binary_search(&x).is_ok() && set.binary_search(&y).is_ok() {
                set.retain(|&w| w != x && w != y);
                set.push(either); // the newest wire has the highest number
            }
        }
    }
    // A NAND of active-low lines is the OR of the lines.
    let mut sums: HashMap<Vec<Wire>, Wire> = HashMap::new();
    sets.into_iter()
        .map(|set| match sums.get(&set) {
            Some(&w) => w,
            None => {
                let w = b.nand_all(&set);
                sums.insert(set, w);
                w
            }
        })
        .collect()
}

/// Build the control unit. Inputs are flip-flop outputs: the instruction
/// register (8), the step counter (3) and the flags (`Z C N V`). The result
/// holds one wire per control signal, indexed by [`bit`].
pub fn build(b: &mut Builder, ir: &[Wire], step: &[Wire], flags: &[Wire]) -> Vec<Wire> {
    assert_eq!((ir.len(), step.len(), flags.len()), (8, 3, 4));
    b.scope("control", |b| {
        let table = microcode();

        // Is the step counter equal to k? Only the steps the table uses.
        let step_is: HashMap<u8, Wire> = b.scope("steps", |b| {
            let inverted = b.not_word(step);
            let mut used: Vec<u8> = table.iter().map(|l| l.step).collect();
            used.sort_unstable();
            used.dedup();
            used.into_iter()
                .map(|k| {
                    let literals: Vec<Wire> = (0..3)
                        .map(|i| {
                            if k >> i & 1 == 1 {
                                step[i]
                            } else {
                                inverted[i]
                            }
                        })
                        .collect();
                    (k, b.and_all(&literals))
                })
                .collect()
        });

        // Branch condition: pick a flag with opcode bits 2..1, flip with bit 0.
        let taken = b.scope("cond", |b| {
            let (z, c, n, v) = (flags[0], flags[1], flags[2], flags[3]);
            let lt = b.xor(n, v);
            let picked = b.select(&ir[1..3], &[&[z], &[c], &[n], &[lt]])[0];
            b.xor(picked, ir[0])
        });

        // One active-low product term per microcode line.
        let mut decoder = b.scope("decode", |b| Decoder::new(b, ir));
        let lines: Vec<Wire> = table
            .iter()
            .map(|line| {
                let mut terms = vec![step_is[&line.step]];
                if let Some(p) = b.scope("decode", |b| decoder.pattern(b, line.mask, line.value)) {
                    terms.push(p);
                }
                if line.guard == Guard::Taken {
                    terms.push(taken);
                }
                b.scope("lines", |b| b.nand_all(&terms))
            })
            .collect();

        let raised_by_table = |signal: usize| -> Vec<Wire> {
            table
                .iter()
                .zip(&lines)
                .filter(|(line, _)| line.signals >> signal & 1 == 1)
                .map(|(_, &w)| w)
                .collect()
        };

        // The two signals that ask for the ALU operation decoder come first,
        // because that decoder feeds the rest of the plane.
        let alu_sets = vec![raised_by_table(bit(ALU_HI)), raised_by_table(bit(ALU_LO))];
        let alu = b.scope("plane", |b| sum_plane(b, alu_sets));
        let (alu_hi, alu_lo) = (alu[0], alu[1]);

        // Second-level decoder: one active-low line per ALU operation.
        let op_lines: Vec<Wire> = b.scope("aluop", |b| {
            let enable = b.or(alu_hi, alu_lo);
            let op: Vec<Wire> = (0..3)
                .map(|i| {
                    let hi = b.nand(alu_hi, ir[3 + i]);
                    let lo = b.nand(alu_lo, ir[i]);
                    b.nand(hi, lo)
                })
                .collect();
            let inverted = b.not_word(&op);
            (0..8)
                .map(|k| {
                    let mut literals: Vec<Wire> = (0..3)
                        .map(|i| if k >> i & 1 == 1 { op[i] } else { inverted[i] })
                        .collect();
                    literals.push(enable);
                    b.nand_all(&literals)
                })
                .collect()
        });

        // Every other signal: the table lines that raise it, plus the ALU
        // operations that do.
        let others: Vec<usize> = (0..SIGNAL_COUNT)
            .filter(|&signal| signal != bit(ALU_HI) && signal != bit(ALU_LO))
            .collect();
        let sets: Vec<Vec<Wire>> = others
            .iter()
            .map(|&signal| {
                let mut raised = raised_by_table(signal);
                for (k, &line) in op_lines.iter().enumerate() {
                    if alu_signals(AluOp::from_bits(k as u8)) >> signal & 1 == 1 {
                        raised.push(line);
                    }
                }
                raised
            })
            .collect();
        let sums = b.scope("plane", |b| sum_plane(b, sets));

        let mut signals = vec![Wire::ZERO; SIGNAL_COUNT];
        signals[bit(ALU_HI)] = alu_hi;
        signals[bit(ALU_LO)] = alu_lo;
        for (&signal, &wire) in others.iter().zip(&sums) {
            signals[signal] = wire;
        }
        signals
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use silicon_sim_gate::GateSim;

    #[test]
    fn every_form_has_microcode_and_nothing_else_does() {
        let table = microcode();
        for f in form::FORMS {
            let steps: Vec<u8> = table
                .iter()
                .filter(|l| l.form == f.name && l.guard == Guard::Always)
                .map(|l| l.step)
                .collect();
            assert!(!steps.is_empty(), "{} has no microcode", f.name);
            assert_eq!(
                steps,
                (1..=steps.len() as u8).collect::<Vec<_>>(),
                "{}",
                f.name
            );
        }
        for line in table {
            assert!(
                line.form == "FETCH" || form::FORMS.iter().any(|f| f.name == line.form),
                "{} is not a form",
                line.form
            );
        }
    }

    /// Select signals that must never be high together.
    #[test]
    fn microcode_never_raises_conflicting_signals() {
        let exclusive = [
            BASE_SP | BASE_PAIR | BASE_T,
            OFF_T | OFF_A | OFF_M1,
            RD | WR,
            DOUT_PCH | DOUT_PCL,
            B_PS | B_A,
            X_ZERO | X_ONES,
            CIN_ONE | CIN_C,
            FN_AND | FN_OR | FN_XOR | FN_SHR,
            SIN_C | SIN_X7,
            W_D | W_R | W_PD | W_PS,
            ALU_HI | ALU_LO,
            PC_WE | SP_WE | PAIR_WE,
        ];
        for ir in 0..=255u8 {
            for step in 0..8u8 {
                for flags in [0u8, 0x0F] {
                    let word = control_word(ir, step, flags);
                    for group in exclusive {
                        assert!(
                            (word & group).count_ones() <= 1,
                            "ir {ir:#04x} step {step}: conflict in {:?}",
                            (0..SIGNAL_COUNT)
                                .filter(|&i| (word & group) >> i & 1 == 1)
                                .map(|i| SIGNAL_NAMES[i])
                                .collect::<Vec<_>>()
                        );
                    }
                    // The sign-extension flag only makes sense with OFF_T,
                    // and an opcode fetch never happens while writing memory.
                    assert!(word & OFF_SX == 0 || word & OFF_T != 0);
                    assert!(word & IR_WE == 0 || word & RD != 0);
                }
            }
        }
    }

    #[test]
    fn steps_no_instruction_has_do_nothing() {
        for ir in 0..=255u8 {
            let steps = form::form_of(ir).cycles_taken.max(1);
            for step in (steps + 1)..8 {
                // (cycle counts include the following fetch, so `steps` is an
                // upper bound on the last real step)
                assert_eq!(control_word(ir, step, 0x0F), 0, "ir {ir:#04x} step {step}");
            }
        }
    }

    /// The gates against the table for every opcode, step and flag value.
    #[test]
    fn netlist_matches_the_table_exhaustively() {
        let mut b = Builder::new("test");
        let ir = b.inputs("ir", 8);
        let step = b.inputs("step", 3);
        let flags = b.inputs("flags", 4);
        let signals = build(&mut b, &ir, &step, &flags);
        b.outputs("signal", &signals);
        let netlist = b.finish().unwrap();
        let mut sim = GateSim::new(netlist).unwrap();
        for input in 0..1u64 << 15 {
            let (i, s, f) = (input as u8, (input >> 8 & 7) as u8, (input >> 11) as u8);
            sim.set_word(&ir, i as u64);
            sim.set_word(&step, s as u64);
            sim.set_word(&flags, f as u64);
            sim.settle();
            let got = sim.get_word(&signals);
            let want = control_word(i, s, f);
            if got != want {
                let diff: Vec<&str> = (0..SIGNAL_COUNT)
                    .filter(|&k| (got ^ want) >> k & 1 == 1)
                    .map(|k| SIGNAL_NAMES[k])
                    .collect();
                panic!("ir {i:#04x} step {s} flags {f:#x}: signals differ: {diff:?}");
            }
        }
    }
}
