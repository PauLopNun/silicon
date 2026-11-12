# ADR 0002 — The CPU is gates; RAM, ROM and devices are behavioural

**Status:** accepted (M0)

## Context
"No fakery" requires that what we show is really computed. 64 KiB of memory out
of flip-flops is ~524,000 DFFs plus address decoding: roughly 100× the CPU.

## Decision
The CPU (datapath and control) is entirely NAND + DFF. Memory and
memory-mapped devices are arrays behind the bus, shared by both engines.

## Why
- The gate engine could not approach 50 k cycles/s with a gate-level RAM.
- Visually, a sea of identical memory cells would dwarf the CPU, the part
  worth exploring.
- Memory content is still real and inspectable; only its implementation is
  abstracted.

## Consequences
- README and UI state this limit explicitly.
- A small gate-level RAM (a few bytes, with decoder) is built as a lesson
  exhibit, driven by the gate engine, to show how memory is made.
