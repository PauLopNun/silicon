//! Building a program from Rust: instructions, labels and data.
//!
//! This is not the assembler (`crates/asm` reads text). It is the smallest
//! thing that lets a test, a demo or a compiler backend write
//! `p.br(Cond::Nz, top)` instead of counting bytes.
//!
//! ```
//! use silicon_isa::{Cond, Insn, Program, Reg};
//! let mut p = Program::new();
//! p.push(Insn::LdImm(Reg::B, 10));
//! let top = p.here();
//! p.push(Insn::Dec(Reg::B));
//! p.br(Cond::Nz, top);
//! p.push(Insn::Hlt);
//! assert_eq!(p.assemble().unwrap(), [0xD1, 10, 0xC9, 0xF1, 0xFD, 0x00]);
//! ```

use crate::{Cond, Insn, Pair};
use std::fmt;

/// A position in a [`Program`], created by [`Program::label`] or
/// [`Program::here`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Label(usize);

#[derive(Clone, Debug)]
enum Item {
    Insn(Insn),
    Br(Cond, Label),
    Jmp(Label),
    Call(Label),
    LdPair(Pair, Label),
    Bytes(Vec<u8>),
}

impl Item {
    fn len(&self) -> usize {
        match self {
            Item::Insn(insn) => insn.len(),
            Item::Br(..) => 2,
            Item::Jmp(_) | Item::Call(_) | Item::LdPair(..) => 3,
            Item::Bytes(bytes) => bytes.len(),
        }
    }
}

/// Why a [`Program`] could not be assembled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramError {
    /// A label is used but was never placed.
    UnplacedLabel,
    /// A conditional branch at `from` cannot reach `to` (−128…+127 bytes).
    BranchOutOfRange { from: u16, to: u16 },
    /// The program does not fit in the 64 KiB address space.
    TooLarge { bytes: usize },
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ProgramError::UnplacedLabel => write!(f, "a label is used but never placed"),
            ProgramError::BranchOutOfRange { from, to } => {
                write!(f, "branch at {from:#06x} cannot reach {to:#06x}")
            }
            ProgramError::TooLarge { bytes } => write!(f, "program is {bytes} bytes long"),
        }
    }
}

impl std::error::Error for ProgramError {}

/// A program under construction. It is assembled to start at address 0,
/// where the CPU begins executing.
#[derive(Clone, Debug, Default)]
pub struct Program {
    items: Vec<Item>,
    /// For each label, the index of the item it stands in front of.
    labels: Vec<Option<usize>>,
}

impl Program {
    pub fn new() -> Self {
        Program::default()
    }

    /// A new label, to be [placed](Program::place) later: for jumping forward.
    pub fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    /// Make `label` mean "the next thing added".
    ///
    /// # Panics
    /// If the label was already placed.
    pub fn place(&mut self, label: Label) {
        let slot = &mut self.labels[label.0];
        assert!(slot.is_none(), "label placed twice");
        *slot = Some(self.items.len());
    }

    /// A label for the next thing added.
    pub fn here(&mut self) -> Label {
        let label = self.label();
        self.place(label);
        label
    }

    /// Add one instruction.
    pub fn push(&mut self, insn: Insn) -> &mut Self {
        self.items.push(Item::Insn(insn));
        self
    }

    /// Add several instructions.
    pub fn extend(&mut self, insns: &[Insn]) -> &mut Self {
        self.items.extend(insns.iter().map(|&i| Item::Insn(i)));
        self
    }

    /// Conditional branch to a label.
    pub fn br(&mut self, cond: Cond, target: Label) -> &mut Self {
        self.items.push(Item::Br(cond, target));
        self
    }

    /// `jmp` to a label.
    pub fn jmp(&mut self, target: Label) -> &mut Self {
        self.items.push(Item::Jmp(target));
        self
    }

    /// `call` a label.
    pub fn call(&mut self, target: Label) -> &mut Self {
        self.items.push(Item::Call(target));
        self
    }

    /// Load the address of a label into a pair.
    pub fn ld_pair(&mut self, pair: Pair, target: Label) -> &mut Self {
        self.items.push(Item::LdPair(pair, target));
        self
    }

    /// Add raw bytes: tables, strings.
    pub fn bytes(&mut self, data: &[u8]) -> &mut Self {
        self.items.push(Item::Bytes(data.to_vec()));
        self
    }

    /// Resolve the labels and encode everything.
    pub fn assemble(&self) -> Result<Vec<u8>, ProgramError> {
        // Every item has a fixed length, so one pass gives all addresses.
        let mut starts = Vec::with_capacity(self.items.len() + 1);
        let mut at = 0usize;
        for item in &self.items {
            starts.push(at);
            at += item.len();
        }
        starts.push(at);
        if at > 0x1_0000 {
            return Err(ProgramError::TooLarge { bytes: at });
        }
        let address = |label: Label| -> Result<u16, ProgramError> {
            let item = self.labels[label.0].ok_or(ProgramError::UnplacedLabel)?;
            Ok(starts[item] as u16)
        };

        let mut out = Vec::with_capacity(at);
        for (item, &start) in self.items.iter().zip(&starts) {
            match item {
                Item::Insn(insn) => insn.encode(&mut out),
                Item::Bytes(bytes) => out.extend_from_slice(bytes),
                Item::Jmp(l) => Insn::Jmp(address(*l)?).encode(&mut out),
                Item::Call(l) => Insn::Call(address(*l)?).encode(&mut out),
                Item::LdPair(p, l) => Insn::LdPairImm(*p, address(*l)?).encode(&mut out),
                Item::Br(cond, l) => {
                    let to = address(*l)?;
                    let from = start as u16;
                    // Relative to the instruction after the branch.
                    let offset = to as i32 - (start as i32 + 2);
                    let offset = i8::try_from(offset)
                        .map_err(|_| ProgramError::BranchOutOfRange { from, to })?;
                    Insn::Br(*cond, offset).encode(&mut out);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Reg;

    #[test]
    fn forward_and_backward_labels() {
        let mut p = Program::new();
        let end = p.label();
        let top = p.here(); // 0
        p.push(Insn::Dec(Reg::B)); // 0
        p.br(Cond::Z, end); // 1: to 7 → offset +4
        p.jmp(top); // 3
        p.push(Insn::NOP); // 6
        p.place(end); // 7
        p.push(Insn::Hlt);
        assert_eq!(
            p.assemble().unwrap(),
            [0xC9, 0xF0, 0x04, 0xF8, 0x00, 0x00, 0x40, 0x00]
        );
    }

    #[test]
    fn labels_can_name_data_and_subroutines() {
        let mut p = Program::new();
        let (table, sub) = (p.label(), p.label());
        p.ld_pair(Pair::Y, table); // 0
        p.call(sub); // 3
        p.push(Insn::Hlt); // 6
        p.place(sub);
        p.push(Insn::Ret); // 7
        p.place(table);
        p.bytes(&[1, 2, 3]); // 8
        assert_eq!(
            p.assemble().unwrap(),
            [0x1A, 0x08, 0x00, 0xF9, 0x07, 0x00, 0x00, 0x02, 1, 2, 3]
        );
    }

    #[test]
    fn a_label_at_the_very_end_is_fine() {
        let mut p = Program::new();
        let end = p.label();
        p.jmp(end);
        p.place(end);
        assert_eq!(p.assemble().unwrap(), [0xF8, 0x03, 0x00]);
    }

    #[test]
    fn branches_reach_exactly_as_far_as_a_signed_byte() {
        let build = |gap: usize, backward: bool| {
            let mut p = Program::new();
            let target = p.label();
            if backward {
                p.place(target);
                p.bytes(&vec![0x40; gap]);
                p.br(Cond::Z, target);
            } else {
                p.br(Cond::Z, target);
                p.bytes(&vec![0x40; gap]);
                p.place(target);
            }
            p.assemble()
        };
        assert_eq!(build(127, false).unwrap()[1], 127);
        assert_eq!(
            build(128, false),
            Err(ProgramError::BranchOutOfRange { from: 0, to: 130 })
        );
        // Backward: the offset counts from after the 2-byte branch.
        assert_eq!(build(126, true).unwrap()[127], 0x80);
        assert!(build(127, true).is_err());
    }

    #[test]
    fn unplaced_labels_are_reported() {
        let mut p = Program::new();
        let nowhere = p.label();
        p.jmp(nowhere);
        assert_eq!(p.assemble(), Err(ProgramError::UnplacedLabel));
        assert_eq!(
            ProgramError::BranchOutOfRange {
                from: 0x10,
                to: 0x200
            }
            .to_string(),
            "branch at 0x0010 cannot reach 0x0200"
        );
    }

    #[test]
    #[should_panic(expected = "label placed twice")]
    fn placing_a_label_twice_is_a_bug() {
        let mut p = Program::new();
        let l = p.here();
        p.place(l);
    }
}
