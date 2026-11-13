# ADR 0010 — After writing the raycaster: the instruction set stays, the clock stays at 4 MHz

**Status:** accepted (M3)

## Context
Two questions were left open by M1 and M2, to be answered by measurement
before the assembler and the compiler made the encoding expensive to change:

1. Is the instruction set good enough for the one demanding program the
   computer has to run, a raycaster on a CPU that cannot multiply?
2. Which clock rate is "nominal"? The game has to be playable on the fast
   engine and still show something on the gate engine, which is slower by a
   factor that depends on the host (16 to 40 so far).

So the first thing written in M3, in assembly, was the renderer of `maze3d`
(`programs/maze3d/render.asm`): one ray per screen column through a 16×16
map, wall height from the distance, 128 columns.

## Measurement
`cargo test -p silicon-programs --test maze3d -- --nocapture` prints it.
Over 162 viewpoints (five places looking all around, four more against
walls and along the axes):

| | Clock cycles per frame |
|---|---|
| cheapest frame | 81,421 |
| mean | 86,060 |
| dearest frame | 91,838 |

Where a mean frame goes:

| Part | Cycles | Share | Per unit |
|---|---|---|---|
| writing the screen | 35,428 | 41% | 16 per byte, 2,048 bytes; the rest is noting where walls begin and end |
| multiplying | 21,594 | 25% | 84 per 8×16-bit product, two per ray |
| setting up a ray | 13,571 | 15% | 106 per ray |
| distance to height | 9,646 | 11% | 75 per ray |
| walking the map | 5,622 | 6% | 14 to 16 per cell crossed |

## Decision
- **The instruction set is not changed.** `docs/ISA.md` as it left M1 is what
  the toolchain is built for.
- **The nominal clock stays at 4 MHz.**
- Game time is CPU time: `maze3d` moves the player a fixed amount per frame
  and does not wait for the timer.

## Why
- At 4 MHz a frame takes 21.5 ms: 46 frames per second from the renderer
  alone, against a budget of 10. There is room for the game around it, in
  compiled code that will be several times slower than this.
- No instruction is missing in the places that cost. The two loops that
  dominate are already tight:
  - the screen loop is nine instructions per byte, every one of them work
    (read the change, apply it, write, clear, step down a row, count);
  - multiplication is table lookups, `a·b = sq(a+b) − sq(a−b)`, and what it
    lacks is a multiplier, which the CPU does not have on purpose (ADR 0005).
- What was missed while writing 400 lines, and why none of it is added:
  - *Loading registers other than `a` through a pointer.* Costs a `mov`, one
    cycle, each time. All 256 opcodes are taken; freeing some means removing
    something else.
  - *Shifts by more than one, and left shifts.* `add a, a` and repeated
    `shr a` cover it at a cycle each; a barrel shifter would be a few hundred
    gates for a few cycles per ray.
  - *Adding a constant to a register that is not `a`.* `lea pp, [pp+d]`
    already does it for pairs in 3 cycles.
- The gate engine cannot reach any plausible nominal rate (it runs at
  0.1 to 0.27 MHz depending on the host), so the nominal rate only decides
  how the game feels on the fast engine. Lower than 4 MHz would give away
  smoothness there and gain nothing on the gate engine; higher would draw
  more frames than a display shows.
- 4 MHz also puts the timer tick (65,536 cycles) at 61 per second, close to
  a display's refresh, for programs that do want to pace themselves by it.
- Not waiting for the timer keeps the gate engine honest and useful: there a
  frame takes a third of a second to a second, and every one of those frames
  is new. A game that waited for ticks would spend that time idle.

## Consequences
- On the gate engine `maze3d` draws one to three frames per second and the
  player moves that much more slowly in real time. The page says which
  engine is running and at what real speed; nothing is hidden.
- "Unlimited" clock makes the game run as fast as the host can: that is what
  the control means.
- The cost of a frame varies by ±6% with the view, and the player's speed
  with it. Not perceptible.
- If the game's own logic ever threatens the frame rate, the first places to
  look are the screen loop (drawing two rows per iteration saves about 4,000
  cycles) and the ray set-up, not the instruction set.
- The frame cost is pinned by a test, so a change that makes the renderer
  much slower or faster shows up and this ADR gets revisited.
