//! The instruction forms as plain data: one entry per family of opcodes.
//!
//! This table is the single place where the encoding is written down. The
//! decoder, the control-unit generator and the opcode table in `docs/ISA.md`
//! are all checked against it.

/// One family of opcodes that share a bit pattern and a behaviour.
#[derive(Debug, PartialEq, Eq)]
pub struct Form {
    /// Stable identifier, e.g. `LD_A_PTR`.
    pub name: &'static str,
    /// An opcode belongs to this form when `opcode & mask == value`.
    pub mask: u8,
    pub value: u8,
    /// Assembly syntax. Placeholders: `r`/`rd`/`rs` 8-bit register,
    /// `pp`/`pd`/`ps` register pair, `n` unsigned byte, `d` signed byte,
    /// `nn` 16-bit value, `op` ALU operation, `cc` condition.
    pub syntax: &'static str,
    /// Length in bytes, opcode included.
    pub bytes: u8,
    /// Clock cycles the instruction adds to a program, including the fetch of
    /// the opcode that follows it. `0` for `hlt`, which never completes.
    pub cycles: u8,
    /// Same, when a conditional branch is taken.
    pub cycles_taken: u8,
    /// Flags written, as a subset of `ZCNV`.
    pub flags: &'static str,
    /// What it does, in register-transfer notation.
    pub effect: &'static str,
}

impl Form {
    /// Does `opcode` belong to this form?
    #[inline]
    pub const fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.value
    }
}

macro_rules! forms {
    ($( $id:ident = $mask:literal, $value:literal, $syntax:literal, $bytes:literal,
        $cycles:literal, $taken:literal, $flags:literal, $effect:literal; )*) => {
        $(
            pub static $id: Form = Form {
                name: stringify!($id),
                mask: $mask,
                value: $value,
                syntax: $syntax,
                bytes: $bytes,
                cycles: $cycles,
                cycles_taken: $taken,
                flags: $flags,
                effect: $effect,
            };
        )*
        /// Every form, in opcode order.
        pub static FORMS: &[&Form] = &[ $( &$id ),* ];
    };
}

forms! {
    HLT          = 0xFF, 0x00, "hlt",             1, 0, 0, "",     "stop until reset";
    MOV_SP_X     = 0xFF, 0x01, "mov sp, x",       1, 2, 2, "",     "sp ← x";
    RET          = 0xFF, 0x02, "ret",             1, 3, 3, "",     "pc ← mem16[sp]; sp ← sp + 2";
    JMP_Z        = 0xFF, 0x03, "jmp z",           1, 1, 1, "",     "pc ← z";
    LD_A_PTR     = 0xFC, 0x04, "ld a, [pp]",      1, 2, 2, "",     "a ← mem[pp]";
    LD_A_PTR_INC = 0xFC, 0x08, "ld a, [pp+]",     1, 2, 2, "",     "a ← mem[pp]; pp ← pp + 1";
    ST_PTR_A     = 0xFC, 0x0C, "st [pp], a",      1, 2, 2, "",     "mem[pp] ← a";
    ST_PTR_INC_A = 0xFC, 0x10, "st [pp+], a",     1, 2, 2, "",     "mem[pp] ← a; pp ← pp + 1";
    LD_A_IDX     = 0xFC, 0x14, "ld a, [pp+a]",    1, 2, 2, "",     "a ← mem[pp + a]";
    LD_PAIR_IMM  = 0xFC, 0x18, "ld pp, nn",       3, 3, 3, "",     "pp ← nn";
    PUSH         = 0xFC, 0x1C, "push pp",         1, 3, 3, "",     "sp ← sp − 2; mem16[sp] ← pp";
    POP          = 0xFC, 0x20, "pop pp",          1, 3, 3, "",     "pp ← mem16[sp]; sp ← sp + 2";
    LEA_SP       = 0xFC, 0x24, "lea pp, [sp+n]",  2, 3, 3, "",     "pp ← sp + n";
    LEA_PAIR     = 0xFC, 0x28, "lea pp, [pp+d]",  2, 3, 3, "",     "pp ← pp + d";
    LEA_IDX      = 0xFC, 0x2C, "lea pp, [pp+a]",  1, 2, 2, "",     "pp ← pp + a";
    ADD_PAIR     = 0xF0, 0x30, "add pd, ps",      1, 2, 2, "ZCNV", "pd ← pd + ps";
    MOV          = 0xC0, 0x40, "mov rd, rs",      1, 1, 1, "",     "rd ← rs";
    ALU_REG      = 0xC0, 0x80, "op a, rs",        1, 1, 1, "*",    "a ← a op rs";
    INC          = 0xF8, 0xC0, "inc r",           1, 1, 1, "ZNV",  "r ← r + 1";
    DEC          = 0xF8, 0xC8, "dec r",           1, 1, 1, "ZNV",  "r ← r − 1";
    LD_IMM       = 0xF8, 0xD0, "ld r, n",         2, 2, 2, "",     "r ← n";
    LD_SP        = 0xF8, 0xD8, "ld r, [sp+n]",    2, 3, 3, "",     "r ← mem[sp + n]";
    ST_SP        = 0xF8, 0xE0, "st [sp+n], r",    2, 3, 3, "",     "mem[sp + n] ← r";
    ALU_IMM      = 0xF8, 0xE8, "op a, n",         2, 2, 2, "*",    "a ← a op n";
    BR           = 0xF8, 0xF0, "bcc d",           2, 2, 3, "",     "if cc: pc ← pc + d";
    JMP          = 0xFF, 0xF8, "jmp nn",          3, 3, 3, "",     "pc ← nn";
    CALL         = 0xFF, 0xF9, "call nn",         3, 5, 5, "",     "sp ← sp − 2; mem16[sp] ← pc; pc ← nn";
    LD_A_ABS     = 0xFF, 0xFA, "ld a, [nn]",      3, 4, 4, "",     "a ← mem[nn]";
    ST_ABS_A     = 0xFF, 0xFB, "st [nn], a",      3, 4, 4, "",     "mem[nn] ← a";
    LEA_SP_SP    = 0xFF, 0xFC, "lea sp, [sp+d]",  2, 3, 3, "",     "sp ← sp + d";
    SHR_A        = 0xFF, 0xFD, "shr a",           1, 1, 1, "ZCN",  "a ← a >> 1; bit 7 ← 0; C ← old a₀";
    ROR_A        = 0xFF, 0xFE, "ror a",           1, 1, 1, "ZCN",  "a ← a >> 1; bit 7 ← old C; C ← old a₀";
    ASR_A        = 0xFF, 0xFF, "asr a",           1, 1, 1, "ZCN",  "a ← a >> 1; bit 7 kept; C ← old a₀";
}

/// The form an opcode belongs to. Every one of the 256 opcodes has exactly
/// one (checked by a test), so this never fails.
pub fn form_of(opcode: u8) -> &'static Form {
    FORMS
        .iter()
        .find(|f| f.matches(opcode))
        .expect("every opcode belongs to a form")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_opcode_belongs_to_exactly_one_form() {
        for opcode in 0..=255u8 {
            let hits: Vec<_> = FORMS.iter().filter(|f| f.matches(opcode)).collect();
            assert_eq!(hits.len(), 1, "opcode {opcode:#04x} matches {hits:?}");
        }
    }

    #[test]
    fn forms_are_well_formed_and_sorted() {
        for f in FORMS {
            assert_eq!(
                f.value & !f.mask,
                0,
                "{}: value has bits outside mask",
                f.name
            );
            assert!((1..=3).contains(&f.bytes), "{}", f.name);
            assert!(f.cycles_taken >= f.cycles, "{}", f.name);
        }
        for pair in FORMS.windows(2) {
            assert!(
                pair[0].value < pair[1].value,
                "{} is out of order",
                pair[1].name
            );
        }
        let opcodes: u32 = FORMS.iter().map(|f| 1u32 << (!f.mask).count_ones()).sum();
        assert_eq!(opcodes, 256);
    }
}
