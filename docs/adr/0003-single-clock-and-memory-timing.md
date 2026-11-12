# ADR 0003 — Single clock, one bus access per cycle, combinational read

**Status:** accepted (M0); details may be refined in M1

## Context
The two engines must agree every cycle, and the gate engine needs a simple,
fast evaluation schedule.

## Decision
- One global clock; all flip-flops are positive-edge and latch together.
- One CPU cycle = one evaluation of the netlist = at most one bus access.
- Reads are combinational within the cycle. The netlist compiler splits the
  logic into an *address cone* (depends only on flip-flop outputs) and a *main
  cone* (may depend on `data_in`); the machine performs the read between them.
- Writes commit at the clock edge.

## Why
- No multi-phase clocks or latches: easy to explain ("everything changes on the
  tick") and each cycle is a single levelized pass.
- "One bus access per cycle" gives the lockstep test a precise thing to compare.

## Consequences
- The address bus must not depend combinationally on `data_in`; the compiler
  reports an error naming the offending path if it does.
- Instructions take several cycles (fetch, operands, execute); the cycle counts
  are documented per instruction in `docs/ISA.md`.
