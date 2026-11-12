# ADR 0001 — Hardware is described with an embedded Rust DSL

**Status:** accepted (M0)

## Context
The design must be built only from `nand` and `dff`, hierarchically, with the
hierarchy preserved. Options: (a) a small text HDL with its own parser,
(b) a builder API embedded in Rust.

## Decision
Embedded Rust DSL (`silicon_hdl::Builder`). A module is a Rust function;
`b.scope("name", |b| …)` creates a hierarchy level.

## Why
- Parameterised hardware (an N-bit adder, a decoder generated from the opcode
  table) is ordinary Rust loops and functions; a text HDL would need its own
  generics and generate constructs.
- No parser, no second language to document and test.
- Type checking and IDE support for free.

## Consequences
- Reading the hardware source requires reading Rust. Mitigation: the component
  library is written in a deliberately plain style, and the site shows the
  structure visually.
- A text export (for other tools) can be added later from `Netlist`.
