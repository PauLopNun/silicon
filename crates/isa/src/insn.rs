//! Instructions as a Rust value: encode to bytes, decode from bytes, print
//! as assembly. The assembler and the disassembler are both thin layers over
//! this type.

use crate::flag;
use crate::form::{self, Form};
use std::fmt;

/// An 8-bit register. The discriminant is its 3-bit number in an opcode.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Reg {
    A = 0,
    B = 1,
    Xl = 2,
    Xh = 3,
    Yl = 4,
    Yh = 5,
    Zl = 6,
    Zh = 7,
}

impl Reg {
    pub const ALL: [Reg; 8] = [
        Reg::A,
        Reg::B,
        Reg::Xl,
        Reg::Xh,
        Reg::Yl,
        Reg::Yh,
        Reg::Zl,
        Reg::Zh,
    ];
    pub const NAMES: [&'static str; 8] = ["a", "b", "xl", "xh", "yl", "yh", "zl", "zh"];

    /// The register selected by the low three bits of `bits`.
    #[inline]
    pub fn from_bits(bits: u8) -> Reg {
        Reg::ALL[(bits & 7) as usize]
    }
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        Reg::NAMES[self.index()]
    }
}

/// A 16-bit register pair. Pair `n` is registers `2n` (low byte) and `2n + 1`
/// (high byte), so `ba` is `b:a` and `x` is `xh:xl`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pair {
    Ba = 0,
    X = 1,
    Y = 2,
    Z = 3,
}

impl Pair {
    pub const ALL: [Pair; 4] = [Pair::Ba, Pair::X, Pair::Y, Pair::Z];
    pub const NAMES: [&'static str; 4] = ["ba", "x", "y", "z"];

    #[inline]
    pub fn from_bits(bits: u8) -> Pair {
        Pair::ALL[(bits & 3) as usize]
    }
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        Pair::NAMES[self.index()]
    }
    /// The register holding the low byte.
    pub fn lo(self) -> Reg {
        Reg::ALL[2 * self.index()]
    }
    /// The register holding the high byte.
    pub fn hi(self) -> Reg {
        Reg::ALL[2 * self.index() + 1]
    }
}

/// The eight operations of the ALU instruction group.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AluOp {
    Add = 0,
    Adc = 1,
    Sub = 2,
    Sbc = 3,
    And = 4,
    Or = 5,
    Xor = 6,
    Cmp = 7,
}

impl AluOp {
    pub const ALL: [AluOp; 8] = [
        AluOp::Add,
        AluOp::Adc,
        AluOp::Sub,
        AluOp::Sbc,
        AluOp::And,
        AluOp::Or,
        AluOp::Xor,
        AluOp::Cmp,
    ];
    pub const NAMES: [&'static str; 8] = ["add", "adc", "sub", "sbc", "and", "or", "xor", "cmp"];

    #[inline]
    pub fn from_bits(bits: u8) -> AluOp {
        AluOp::ALL[(bits & 7) as usize]
    }
    pub fn name(self) -> &'static str {
        AluOp::NAMES[self as usize]
    }
}

/// Branch conditions. Bit 0 of the number negates the test.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cond {
    /// Zero (`Z = 1`): equal.
    Z = 0,
    /// Not zero.
    Nz = 1,
    /// Carry (`C = 1`): unsigned `>=` after a compare.
    C = 2,
    /// No carry: unsigned `<` after a compare.
    Nc = 3,
    /// Negative (`N = 1`).
    N = 4,
    /// Plus (`N = 0`).
    P = 5,
    /// Signed less than (`N != V`).
    Lt = 6,
    /// Signed greater or equal (`N = V`).
    Ge = 7,
}

impl Cond {
    pub const ALL: [Cond; 8] = [
        Cond::Z,
        Cond::Nz,
        Cond::C,
        Cond::Nc,
        Cond::N,
        Cond::P,
        Cond::Lt,
        Cond::Ge,
    ];
    /// Branch mnemonics, e.g. `bnz`.
    pub const NAMES: [&'static str; 8] = ["bz", "bnz", "bc", "bnc", "bn", "bp", "blt", "bge"];

    #[inline]
    pub fn from_bits(bits: u8) -> Cond {
        Cond::ALL[(bits & 7) as usize]
    }
    pub fn name(self) -> &'static str {
        Cond::NAMES[self as usize]
    }
    /// Is the condition true for this value of the flags register?
    pub fn holds(self, flags: u8) -> bool {
        let n = flags & flag::N != 0;
        let v = flags & flag::V != 0;
        match self {
            Cond::Z => flags & flag::Z != 0,
            Cond::Nz => flags & flag::Z == 0,
            Cond::C => flags & flag::C != 0,
            Cond::Nc => flags & flag::C == 0,
            Cond::N => n,
            Cond::P => !n,
            Cond::Lt => n != v,
            Cond::Ge => n == v,
        }
    }
}

/// One machine instruction with its operands.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Insn {
    /// `hlt`
    Hlt,
    /// `mov sp, x`
    MovSpX,
    /// `ret`
    Ret,
    /// `jmp z`
    JmpZ,
    /// `ld a, [pp]`
    LdAPtr(Pair),
    /// `ld a, [pp+]`
    LdAPtrInc(Pair),
    /// `st [pp], a`
    StPtrA(Pair),
    /// `st [pp+], a`
    StPtrIncA(Pair),
    /// `ld a, [pp+a]`
    LdAIdx(Pair),
    /// `ld pp, nn`
    LdPairImm(Pair, u16),
    /// `push pp`
    Push(Pair),
    /// `pop pp`
    Pop(Pair),
    /// `lea pp, [sp+n]`
    LeaSp(Pair, u8),
    /// `lea pp, [pp+d]`
    LeaPair(Pair, i8),
    /// `lea pp, [pp+a]`
    LeaIdx(Pair),
    /// `add pd, ps`
    AddPair(Pair, Pair),
    /// `mov rd, rs`
    Mov(Reg, Reg),
    /// `op a, rs`
    Alu(AluOp, Reg),
    /// `inc r`
    Inc(Reg),
    /// `dec r`
    Dec(Reg),
    /// `ld r, n`
    LdImm(Reg, u8),
    /// `ld r, [sp+n]`
    LdSp(Reg, u8),
    /// `st [sp+n], r`
    StSp(u8, Reg),
    /// `op a, n`
    AluImm(AluOp, u8),
    /// `bcc d`: the offset is relative to the instruction that follows.
    Br(Cond, i8),
    /// `jmp nn`
    Jmp(u16),
    /// `call nn`
    Call(u16),
    /// `ld a, [nn]`
    LdAAbs(u16),
    /// `st [nn], a`
    StAbsA(u16),
    /// `lea sp, [sp+d]`
    LeaSpSp(i8),
    /// `shr a`
    ShrA,
    /// `ror a`
    RorA,
    /// `asr a`
    AsrA,
}

impl Insn {
    /// `nop` is spelled `mov a, a`.
    pub const NOP: Insn = Insn::Mov(Reg::A, Reg::A);

    /// The form (opcode family) of this instruction.
    pub fn form(&self) -> &'static Form {
        use Insn::*;
        match self {
            Hlt => &form::HLT,
            MovSpX => &form::MOV_SP_X,
            Ret => &form::RET,
            JmpZ => &form::JMP_Z,
            LdAPtr(_) => &form::LD_A_PTR,
            LdAPtrInc(_) => &form::LD_A_PTR_INC,
            StPtrA(_) => &form::ST_PTR_A,
            StPtrIncA(_) => &form::ST_PTR_INC_A,
            LdAIdx(_) => &form::LD_A_IDX,
            LdPairImm(..) => &form::LD_PAIR_IMM,
            Push(_) => &form::PUSH,
            Pop(_) => &form::POP,
            LeaSp(..) => &form::LEA_SP,
            LeaPair(..) => &form::LEA_PAIR,
            LeaIdx(_) => &form::LEA_IDX,
            AddPair(..) => &form::ADD_PAIR,
            Mov(..) => &form::MOV,
            Alu(..) => &form::ALU_REG,
            Inc(_) => &form::INC,
            Dec(_) => &form::DEC,
            LdImm(..) => &form::LD_IMM,
            LdSp(..) => &form::LD_SP,
            StSp(..) => &form::ST_SP,
            AluImm(..) => &form::ALU_IMM,
            Br(..) => &form::BR,
            Jmp(_) => &form::JMP,
            Call(_) => &form::CALL,
            LdAAbs(_) => &form::LD_A_ABS,
            StAbsA(_) => &form::ST_ABS_A,
            LeaSpSp(_) => &form::LEA_SP_SP,
            ShrA => &form::SHR_A,
            RorA => &form::ROR_A,
            AsrA => &form::ASR_A,
        }
    }

    /// The first byte of the encoding.
    pub fn opcode(&self) -> u8 {
        use Insn::*;
        let base = self.form().value;
        let fields = match *self {
            LdAPtr(p)
            | LdAPtrInc(p)
            | StPtrA(p)
            | StPtrIncA(p)
            | LdAIdx(p)
            | LdPairImm(p, _)
            | Push(p)
            | Pop(p)
            | LeaSp(p, _)
            | LeaPair(p, _)
            | LeaIdx(p) => p as u8,
            AddPair(d, s) => (d as u8) << 2 | s as u8,
            Mov(d, s) => (d as u8) << 3 | s as u8,
            Alu(op, s) => (op as u8) << 3 | s as u8,
            Inc(r) | Dec(r) | LdImm(r, _) | LdSp(r, _) | StSp(_, r) => r as u8,
            AluImm(op, _) => op as u8,
            Br(c, _) => c as u8,
            _ => 0,
        };
        base | fields
    }

    /// Length of the encoding in bytes.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.form().bytes as usize
    }

    /// Append the encoding to `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        use Insn::*;
        out.push(self.opcode());
        match *self {
            LdPairImm(_, nn) | Jmp(nn) | Call(nn) | LdAAbs(nn) | StAbsA(nn) => {
                out.extend_from_slice(&nn.to_le_bytes())
            }
            LeaSp(_, n) | LdImm(_, n) | LdSp(_, n) | StSp(n, _) | AluImm(_, n) => out.push(n),
            LeaPair(_, d) | Br(_, d) | LeaSpSp(d) => out.push(d as u8),
            _ => {}
        }
    }

    /// The encoding as a fresh vector.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3);
        self.encode(&mut out);
        out
    }

    /// Decode the instruction at the start of `bytes`, returning it with its
    /// length. Every opcode is a valid instruction, so the only way to fail
    /// is for `bytes` to end before the operands do.
    pub fn decode(bytes: &[u8]) -> Option<(Insn, usize)> {
        use Insn::*;
        let &op = bytes.first()?;
        let f = form::form_of(op);
        let len = f.bytes as usize;
        if bytes.len() < len {
            return None;
        }
        let n = bytes.get(1).copied().unwrap_or(0);
        let nn = u16::from_le_bytes([n, bytes.get(2).copied().unwrap_or(0)]);
        let d = n as i8;
        let p = Pair::from_bits(op);
        let r = Reg::from_bits(op);
        let insn = match f.value {
            0x00 => Hlt,
            0x01 => MovSpX,
            0x02 => Ret,
            0x03 => JmpZ,
            0x04 => LdAPtr(p),
            0x08 => LdAPtrInc(p),
            0x0C => StPtrA(p),
            0x10 => StPtrIncA(p),
            0x14 => LdAIdx(p),
            0x18 => LdPairImm(p, nn),
            0x1C => Push(p),
            0x20 => Pop(p),
            0x24 => LeaSp(p, n),
            0x28 => LeaPair(p, d),
            0x2C => LeaIdx(p),
            0x30 => AddPair(Pair::from_bits(op >> 2), p),
            0x40 => Mov(Reg::from_bits(op >> 3), r),
            0x80 => Alu(AluOp::from_bits(op >> 3), r),
            0xC0 => Inc(r),
            0xC8 => Dec(r),
            0xD0 => LdImm(r, n),
            0xD8 => LdSp(r, n),
            0xE0 => StSp(n, r),
            0xE8 => AluImm(AluOp::from_bits(op), n),
            0xF0 => Br(Cond::from_bits(op), d),
            0xF8 => Jmp(nn),
            0xF9 => Call(nn),
            0xFA => LdAAbs(nn),
            0xFB => StAbsA(nn),
            0xFC => LeaSpSp(d),
            0xFD => ShrA,
            0xFE => RorA,
            0xFF => AsrA,
            other => unreachable!("form value {other:#04x} has no decoder"),
        };
        Some((insn, len))
    }

    /// Assembly text with branch targets resolved, given the address the
    /// instruction sits at.
    pub fn disassemble(&self, addr: u16) -> String {
        match *self {
            Insn::Br(c, d) => {
                let target = addr.wrapping_add(2).wrapping_add(d as u16);
                format!("{} {target:#06x}", c.name())
            }
            other => other.to_string(),
        }
    }
}

/// `+5` / `-3`, for offsets inside brackets.
struct Signed(i8);

impl fmt::Display for Signed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:+}", self.0)
    }
}

impl fmt::Display for Insn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Insn::*;
        match *self {
            Hlt => write!(f, "hlt"),
            MovSpX => write!(f, "mov sp, x"),
            Ret => write!(f, "ret"),
            JmpZ => write!(f, "jmp z"),
            LdAPtr(p) => write!(f, "ld a, [{}]", p.name()),
            LdAPtrInc(p) => write!(f, "ld a, [{}+]", p.name()),
            StPtrA(p) => write!(f, "st [{}], a", p.name()),
            StPtrIncA(p) => write!(f, "st [{}+], a", p.name()),
            LdAIdx(p) => write!(f, "ld a, [{}+a]", p.name()),
            LdPairImm(p, nn) => write!(f, "ld {}, {nn:#06x}", p.name()),
            Push(p) => write!(f, "push {}", p.name()),
            Pop(p) => write!(f, "pop {}", p.name()),
            LeaSp(p, n) => write!(f, "lea {}, [sp+{n}]", p.name()),
            LeaPair(p, d) => write!(f, "lea {0}, [{0}{1}]", p.name(), Signed(d)),
            LeaIdx(p) => write!(f, "lea {0}, [{0}+a]", p.name()),
            AddPair(d, s) => write!(f, "add {}, {}", d.name(), s.name()),
            Mov(d, s) => write!(f, "mov {}, {}", d.name(), s.name()),
            Alu(op, s) => write!(f, "{} a, {}", op.name(), s.name()),
            Inc(r) => write!(f, "inc {}", r.name()),
            Dec(r) => write!(f, "dec {}", r.name()),
            LdImm(r, n) => write!(f, "ld {}, {n:#04x}", r.name()),
            LdSp(r, n) => write!(f, "ld {}, [sp+{n}]", r.name()),
            StSp(n, r) => write!(f, "st [sp+{n}], {}", r.name()),
            AluImm(op, n) => write!(f, "{} a, {n:#04x}", op.name()),
            Br(c, d) => write!(f, "{} {}", c.name(), Signed(d)),
            Jmp(nn) => write!(f, "jmp {nn:#06x}"),
            Call(nn) => write!(f, "call {nn:#06x}"),
            LdAAbs(nn) => write!(f, "ld a, [{nn:#06x}]"),
            StAbsA(nn) => write!(f, "st [{nn:#06x}], a"),
            LeaSpSp(d) => write!(f, "lea sp, [sp{}]", Signed(d)),
            ShrA => write!(f, "shr a"),
            RorA => write!(f, "ror a"),
            AsrA => write!(f, "asr a"),
        }
    }
}

/// Encode a sequence of instructions into one byte vector.
pub fn assemble(program: &[Insn]) -> Vec<u8> {
    let mut out = Vec::new();
    for insn in program {
        insn.encode(&mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_then_encode_is_the_identity_for_every_opcode() {
        for opcode in 0..=255u8 {
            for operands in [[0x00, 0x00], [0x34, 0x12], [0xFF, 0x80], [0x80, 0xFF]] {
                let bytes = [opcode, operands[0], operands[1]];
                let (insn, len) = Insn::decode(&bytes).unwrap();
                assert_eq!(len, form::form_of(opcode).bytes as usize);
                assert_eq!(insn.len(), len);
                assert_eq!(insn.opcode(), opcode, "{insn}");
                assert_eq!(insn.to_bytes(), bytes[..len], "{insn}");
                assert!(insn.form().matches(opcode), "{insn}");
            }
        }
    }

    #[test]
    fn decode_reports_truncated_operands() {
        assert_eq!(Insn::decode(&[]), None);
        assert_eq!(Insn::decode(&[0xF8, 0x00]), None); // jmp needs two bytes
        assert_eq!(Insn::decode(&[0xD0]), None); // ld r, n needs one
        assert_eq!(Insn::decode(&[0x40]), Some((Insn::NOP, 1)));
    }

    #[test]
    fn known_encodings() {
        use Insn::*;
        let cases: [(Insn, &[u8], &str); 14] = [
            (Hlt, &[0x00], "hlt"),
            (LdAPtrInc(Pair::X), &[0x09], "ld a, [x+]"),
            (
                LdPairImm(Pair::Y, 0x8000),
                &[0x1A, 0x00, 0x80],
                "ld y, 0x8000",
            ),
            (LeaPair(Pair::Z, -1), &[0x2B, 0xFF], "lea z, [z-1]"),
            (AddPair(Pair::X, Pair::Ba), &[0x34], "add x, ba"),
            (Mov(Reg::Xl, Reg::A), &[0x50], "mov xl, a"),
            (Alu(AluOp::Xor, Reg::A), &[0xB0], "xor a, a"),
            (Dec(Reg::B), &[0xC9], "dec b"),
            (LdSp(Reg::Zh, 4), &[0xDF, 0x04], "ld zh, [sp+4]"),
            (StSp(2, Reg::A), &[0xE0, 0x02], "st [sp+2], a"),
            (AluImm(AluOp::Cmp, 0x80), &[0xEF, 0x80], "cmp a, 0x80"),
            (Br(Cond::Nz, -4), &[0xF1, 0xFC], "bnz -4"),
            (Call(0x0123), &[0xF9, 0x23, 0x01], "call 0x0123"),
            (LeaSpSp(-8), &[0xFC, 0xF8], "lea sp, [sp-8]"),
        ];
        for (insn, bytes, text) in cases {
            assert_eq!(insn.to_bytes(), bytes, "{text}");
            assert_eq!(insn.to_string(), text);
            assert_eq!(Insn::decode(bytes), Some((insn, bytes.len())));
        }
    }

    #[test]
    fn disassembly_resolves_branch_targets() {
        assert_eq!(Insn::Br(Cond::Z, -2).disassemble(0x0100), "bz 0x0100");
        assert_eq!(Insn::Br(Cond::Ge, 5).disassemble(0x0100), "bge 0x0107");
        assert_eq!(Insn::Ret.disassemble(0x0100), "ret");
    }

    #[test]
    fn pairs_are_made_of_adjacent_registers() {
        assert_eq!((Pair::Ba.lo(), Pair::Ba.hi()), (Reg::A, Reg::B));
        assert_eq!((Pair::Z.lo(), Pair::Z.hi()), (Reg::Zl, Reg::Zh));
    }

    #[test]
    fn conditions_come_in_complementary_pairs() {
        for flags in 0..16u8 {
            for c in Cond::ALL.chunks(2) {
                assert_ne!(c[0].holds(flags), c[1].holds(flags));
            }
        }
        assert!(Cond::Lt.holds(flag::N));
        assert!(Cond::Lt.holds(flag::V));
        assert!(Cond::Ge.holds(flag::N | flag::V));
    }

    #[test]
    fn assemble_concatenates_encodings() {
        let code = assemble(&[Insn::LdImm(Reg::A, 7), Insn::Inc(Reg::A), Insn::Hlt]);
        assert_eq!(code, [0xD0, 0x07, 0xC0, 0x00]);
    }
}
