# ADR 0005 — Shape of the instruction set

**Status:** accepted (M1)

## Context
The CPU must be small enough to explore gate by gate, yet fast enough to run a
raycaster: 16-bit pointers and arithmetic, table lookups and a real stack on
an 8-bit datapath. Options: (a) a pure accumulator machine with one or two
index registers; (b) a load/store machine with three-operand instructions;
(c) an accumulator for arithmetic plus a small register file whose registers
pair up into 16-bit pointers.

## Decision
Option (c). See `docs/ISA.md` for the specification.

- Eight 8-bit registers; neighbours form four 16-bit pairs. ALU instructions
  are `a ← a op operand`.
- Fields sit at fixed bit positions in the opcode (register in bits 2..0,
  pair in bits 1..0, and so on).
- All 256 opcodes are instructions. Nothing is undefined.
- Instructions are 1–3 bytes. An instruction whose last step leaves the bus
  free fetches the next opcode in that step.
- One 16-bit adder, separate from the ALU, does all address arithmetic.

## Why
- An accumulator ALU needs one write path and no destination decoding for
  arithmetic, and two-operand encodings fit in one byte. (b) needs three
  register fields and therefore wider instructions and more fetch cycles.
- Pairs give 16-bit pointers without 16-bit registers: the same 64 flip-flops
  serve both uses. (a) spends its time shuffling pointers through memory.
- Fixed fields mean the register selects are wires from the instruction
  register rather than decoded signals.
- With every opcode defined, any byte string is a valid program and any
  `ArchState` is a legal state. The equivalence test can therefore run random
  memory from random states with no generator and no filtering, which is what
  makes it thorough and simple.
- A separate address adder lets a pointer be incremented, or an indexed
  address formed, in the same cycle the ALU is busy with the data.
- Overlapping fetch with the last step halves the cost of the most common
  instructions (register moves and arithmetic: 1 cycle instead of 2).

## Consequences
- Operations on registers other than `a` go through `a` (except `inc`, `dec`
  and `mov`). Compilers targeting accumulator machines know how to do this.
- No multiply, divide or multi-bit shift. Tables and loops instead.
- The encoding is dense; adding an instruction means repurposing opcodes.
- Fetch overlap makes the cycle count of an instruction depend on whether its
  last step uses the bus; the opcode table lists the totals.
