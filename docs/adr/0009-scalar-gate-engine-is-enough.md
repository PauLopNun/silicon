# ADR 0009 — The scalar gate engine stays; the bit-parallel one is deferred

**Status:** accepted (M2)

## Context
The plan assumed a CPU of about 6,000 NANDs and expected the gate engine to
need bit-parallel evaluation (64 gates per machine word, levels laid out
contiguously) to reach 50,000 cycles/s in WebAssembly. It was the project's
biggest risk. M0 shipped the simplest possible evaluator as a reference: one
byte per wire, one NAND at a time, in levelized order.

## Decision
Measure before optimising. Measured in WASM (Node 22, V8) on the real CPU of
2,603 NANDs, running the demo program, with the netlist evaluated twice per
cycle (once after the bus read, once after the clock edge):

| Engine | Measured | Budget |
|---|---|---|
| gate, scalar | 250 k cycles/s | 50 k |

The scalar evaluator is kept as the gate engine. The bit-parallel engine and
the address/main cone split of ADR 0003 are not built.

## Why
- The budget is met five times over. `pnpm bench` measures it and fails if a
  budget is missed.
- The scalar evaluator's state is one byte per wire, which is exactly the
  buffer the renderer wants (ADR 0004). A bit-packed engine would need an
  unpacking step per frame.
- It is forty lines that anyone can read, in a project whose point is that
  the simulation can be understood.

## Consequences
- ADR 0003's cone split remains a correct description of *when* things are
  valid within a cycle, but the engine simply settles the whole netlist after
  the read. The test that the address cone does not depend on `data_in` stays:
  it is what makes that legal.
- If a later feature needs much more (the gate-level RAM exhibit, a larger
  design, wire-activity counting on every cycle), the first step is the cone
  split (roughly 2×), and only then bit-parallel evaluation.
- On the gate engine a program runs at about 1/16 of the nominal 4 MHz clock.
  That ratio, not the absolute speed, is what the choice of nominal clock in
  M3 has to consider.
