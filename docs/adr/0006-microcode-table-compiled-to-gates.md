# ADR 0006 — The control unit is a table compiled to gates; the reference model does not use it

**Status:** accepted (M1)

## Context
The control unit decides, for every opcode and step, which of ~50 control
signals are high. It could be (a) hand-written logic, signal by signal;
(b) a microcode ROM addressed by opcode and step; (c) a table in the source,
compiled into AND/OR logic.

Separately, the hardware has to be checked against something.

## Decision
- (c): `machine::hw::control::microcode()` is a table with one line per
  (instruction form, step) listing the signals raised. `control::build` turns
  it into a PLA: shared opcode-pattern terms, one product term per line, and
  an OR plane in which partial sums common to several signals are built once.
- `control::control_word` evaluates the same table in plain Rust; the gates
  are compared with it for every opcode, step and flag value.
- The fast model (`crates/sim-fast`) is written from `docs/ISA.md` and must
  never import the microcode or the control signals. Lockstep between the
  gate-level CPU and that model is what validates the table itself.

## Why
- The table is readable as documentation ("`push`: step 1 raises these
  signals") and is the only place an instruction's behaviour is written for
  the hardware.
- A ROM (b) would be a behavioural block inside the CPU, against rule 2; built
  from gates it is this same PLA, only larger.
- Two tests, two questions. *Do the gates implement the table?* is answered
  exhaustively. *Is the table right?* cannot be answered by anything derived
  from the table, hence the independent model.

## Consequences
- The control unit is 579 NANDs (985 before partial sums were shared).
- The gate layout of the control unit is generated, so its gate paths
  (`cpu/control/plane/nand17`) change when the microcode changes. Paths in the
  datapath are stable.
- A change to an instruction is made twice on purpose: once in the microcode,
  once in the fast model.
