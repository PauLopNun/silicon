# ADR 0008 — Device reads are pure, and device time is counted in clock cycles

**Status:** accepted (M2)

## Context
The computer needs a screen, keys, a way to pace a game and a way for test
programs to report results. Two properties of the rest of the design
constrain how: both engines must behave identically cycle for cycle, and
programs must give the same result in every test run and at every simulation
speed (the gate engine is roughly a hundred times slower than the fast one).

## Decision
- The framebuffer is plain memory; the renderer shows those bytes as they are.
- Device registers live in one page (`0x8800`), specified in `docs/ISA.md` §4.
- **Reading a register has no side effect.** No read-to-clear, no FIFOs.
- **The timer is the clock-cycle counter**, exposed as two bytes. There is no
  wall-clock time anywhere in the machine.
- `FRAME` is a marker: the program writes to it when it has finished drawing.
  The machine only counts those writes; the host uses the count to report the
  frame rate the program really achieves.
- `OUT` appends a byte to a console buffer the host can read.
- Keys are a level, not events: the register holds what is held down now.
- Whoever drives the clock calls `Bus::tick` once per cycle; the CPU engines
  know nothing about device time.

## Why
- Pure reads keep two boards identical when given the same accesses, which is
  what lets lockstep compare whole machines, and they let a debugger or the UI
  look at any address without disturbing the program.
- Cycle-counted time makes runs reproducible (golden framebuffer tests, engine
  switching mid-game) and makes the slowdown in gate mode honest: the game
  slows down exactly as much as the simulated clock does.
- A frame marker, rather than a vertical-sync signal, keeps the display free
  of timing the simulation would have to fake.

## Consequences
- A game runs at the speed of the simulated clock. On the gate engine it is
  slow, and the UI says so.
- The host must apply key changes at a cycle boundary.
- A program reading the 16-bit timer byte by byte must allow for the low byte
  wrapping between the two reads.
- No interrupts: programs poll.
