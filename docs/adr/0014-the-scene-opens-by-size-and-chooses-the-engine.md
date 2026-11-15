# ADR 0014 — The scene opens by size on screen, and what is in view chooses the engine

**Status:** accepted (M4)

## Context
The CPU is shown as a scene to move about in, from the whole of it (about
160 × 115 gate cells, 4 units tall) down to the pins of one gate (a tenth
of a unit). Three questions had real alternatives:

1. When does a block stop being a closed box and show its parts?
2. How is "everything that happened between two frames" shown, when the
   gate engine runs 200,000 cycles a second and the page looks 60 times?
3. Which engine runs, now that the page can show gates or not?

## Decisions

### 1. A block opens by how large it looks, continuously
Each module's openness is `smoothstep(opensAt, openAt, s)`, where `s` is the
side of a square of the module's area, in pixels, at the distance of the
nearest point of its block to the eye (`lod.ts`). Open is the lid's
transparency; what is inside is seen through it in proportion, and a module
inside a closed one is not drawn at all. `opensAt`/`openAt` are 230/460 px,
scaled down to 40% in small pictures (a phone held upright).

Rejected: discrete levels chosen by camera distance (they pop, and need
hysteresis to stop flapping), and by the module under the camera (a block
next to the one you are in would stay shut while filling half the screen).
A size-only rule has no state, so a still camera gives a still picture, and
the tests can sweep the camera from far away to one gate and check that no
module's openness changes by more than 3% for a 1% move (`lod.test.ts`, on
a tree six levels deep, twice the CPU's depth).

### 2. Activity is counted in the engine, not sampled by the page
`sim-gate` counts every change of every wire. The level and the count share
one `u32` per wire: bit 0 is the level, a change adds one (which flips bit
0), and clearing keeps the level and a copy of it in bit 31. One extra load
per NAND, no second array. Measured in WASM: 260 k cycles/s without
counting, 150 k with a saturating count kept in a second array, 205 k this
way. The page receives one byte per wire per frame, on a square-root scale
of changes per cycle, so that a wire that changed once in a frame still
shows.

Rejected: sending the levels more often (still a sample, and 60× the
traffic), and keeping only "changed since last frame" bits (a wire that
toggles every cycle and one that toggled once would look the same).

### 3. Auto: the gate engine while a block of the CPU is at least half open
The default engine mode is `auto`: the gate engine runs exactly while one of
the CPU's blocks is at least half open (`lod.entered` of the CPU ≥ 0.5),
which is when gates are plainly in view. Otherwise the fast engine runs the
program at 4 MHz, and what is glimpsed of the wires is drawn without colour,
labelled as not live. `fast` and `gate` can still be chosen, by button or
by link (`?engine=gate`).

First tried: "the CPU's lid is open". The page opens with the whole CPU
filling the view, which already counted, so the game started at 10
frames/s instead of 45. The page checks now test that the page opens on the
fast engine and stays there.

## Consequences
- The engine changes only while the camera moves, never while it is still.
- Showing a wire's activity costs about a fifth of the gate engine's speed,
  always, whether or not anyone is looking. Budget is still met four times
  over.
- The levels of detail, the routing of the wires and the floor plan are all
  pure functions of the netlist and the camera, tested without a browser;
  only `view.ts` touches WebGL.
