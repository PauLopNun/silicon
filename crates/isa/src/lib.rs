//! # Silicon ISA
//!
//! The definition of the Silicon CPU as seen by software, as plain data and
//! plain types with no behaviour attached:
//!
//! * [`form`]: the opcode map, one entry per family of opcodes;
//! * [`Insn`]: an instruction as a value, with `encode`, `decode` and
//!   `Display`;
//! * [`ArchState`] and [`Bus`]: the contract both engines implement.
//!
//! The prose specification is `docs/ISA.md`; a test keeps its opcode table
//! identical to [`form::FORMS`].
//!
//! ```
//! use silicon_isa::{Insn, Reg};
//! let bytes = Insn::LdImm(Reg::A, 42).to_bytes();
//! assert_eq!(bytes, [0xD0, 42]);
//! assert_eq!(Insn::decode(&bytes).unwrap().0.to_string(), "ld a, 0x2a");
//! ```

pub mod form;
mod insn;
mod state;

pub use form::Form;
pub use insn::{assemble, AluOp, Cond, Insn, Pair, Reg};
pub use state::{flag, ArchState, Bus, STEP_MASK};

/// The opcode table of `docs/ISA.md`, generated from [`form::FORMS`].
pub fn opcode_table_markdown() -> String {
    let mut out = String::from(
        "| Opcode | Instruction | Bytes | Cycles | Flags | Effect |\n|---|---|---|---|---|---|\n",
    );
    for f in form::FORMS {
        let last = f.value | !f.mask;
        let opcode = if f.mask == 0xFF {
            format!("`{:02X}`", f.value)
        } else {
            format!("`{:02X}`–`{:02X}`", f.value, last)
        };
        let cycles = match (f.cycles, f.cycles_taken) {
            (0, _) => "—".to_string(),
            (a, b) if a == b => a.to_string(),
            (a, b) => format!("{a} / {b} taken"),
        };
        let flags = match f.flags {
            "" => "—",
            "*" => "by op",
            other => other,
        };
        out.push_str(&format!(
            "| {opcode} | `{}` | {} | {cycles} | {flags} | {} |\n",
            f.syntax, f.bytes, f.effect
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_are_little_endian_views_of_the_registers() {
        let mut s = ArchState::default();
        s.set_pair(1, 0x1234);
        assert_eq!((s.regs[2], s.regs[3]), (0x34, 0x12));
        assert_eq!(s.pair(1), 0x1234);
        s.regs[0] = 0xCD;
        s.regs[1] = 0xAB;
        assert_eq!(s.pair(0), 0xABCD);
    }

    #[test]
    fn masked_clears_bits_without_flip_flops() {
        let s = ArchState {
            flags: 0xFF,
            step: 0xFF,
            ..ArchState::default()
        }
        .masked();
        assert_eq!((s.flags, s.step), (0x0F, 0x07));
    }

    #[test]
    fn halted_means_sitting_in_hlt() {
        let mut s = ArchState::default();
        assert!(!s.halted()); // step 0: about to fetch
        s.step = 1;
        assert!(s.halted());
        s.ir = 0x40;
        assert!(!s.halted());
    }

    /// `docs/ISA.md` embeds the opcode table between two marker comments.
    /// Run with `SILICON_BLESS=1` to rewrite it after changing a form.
    #[test]
    fn isa_md_opcode_table_is_up_to_date() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ISA.md");
        let text = std::fs::read_to_string(path)
            .expect("docs/ISA.md exists")
            .replace("\r\n", "\n");
        let (begin, end) = ("<!-- opcode-table:begin -->\n", "<!-- opcode-table:end -->");
        let start = text.find(begin).expect("begin marker") + begin.len();
        let stop = text.find(end).expect("end marker");
        let table = opcode_table_markdown();
        if text[start..stop] != table {
            if std::env::var_os("SILICON_BLESS").is_some() {
                let updated = format!("{}{}{}", &text[..start], table, &text[stop..]);
                std::fs::write(path, updated).unwrap();
            } else {
                panic!("docs/ISA.md opcode table is stale; rerun with SILICON_BLESS=1");
            }
        }
    }
}
