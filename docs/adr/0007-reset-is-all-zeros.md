# ADR 0007 — Reset is "every flip-flop at 0"; ROM sits at address 0

**Status:** accepted (M1)

## Context
The CPU needs a defined starting state. Common designs read a reset vector
from a fixed address, which takes a reset input, extra control states and
logic to force the vector address onto the bus. The only state primitive here
is a plain `dff` with no set or clear input.

## Decision
- Reset means every flip-flop holds 0: `pc` = 0, `sp` = 0, step 0. The first
  cycle fetches the opcode at `0x0000`.
- The netlist has no reset input. The simulator powers flip-flops on at 0 and
  resets by loading the all-zero state.
- The memory map follows from it: ROM at `0x0000`–`0x7FFF`, RAM at the top.
  With `sp` = 0 the first push wraps to `0xFFFF`, so the stack works without
  being initialised.
- Opcode `00` is `hlt`.

## Why
- No reset logic at all: nothing to build, explain or get wrong, and the
  primitive stays as simple as it can be.
- "Reset" and "load a state" are the same operation, which the engines need
  anyway for switching.
- Empty memory is safe to run into: it halts.

## Consequences
- The provisional map of M0 (RAM low, ROM high) was swapped.
- There is no reset wire to see in the 3D view. If a lesson needs one, it
  costs one AND gate per flip-flop and a new ADR.
- Programs that want a different stack set it with `mov sp, x`.
