# Progress

Read this and `CLAUDE.md` at the start of every session.

## Status

| Milestone | State |
|---|---|
| M0 — Plan and scaffold | **done** (2026-11-12) |
| M1 — ISA and hardware design | **done** (2026-11-13) |
| M2 — Simulation engines | **done** (2026-11-13) |
| M3 — Toolchain and programs | **done** (2026-11-14) |
| M4 — 3D visualization | **in progress** |
| M5 — Learning tools | **in progress** |
| M6 — Polish and launch | — |

Everything is on `main` and pushed (2026-11-15).

## M5 — where it stands (2026-11-15)

Chosen first: **explain mode**. Done, on `main`:

- **Explain** pauses the computer on the gate engine. A panel shows, for the
  cycle about to run: the instruction (address and assembly), its step and
  form in the microcode table, what it does with memory, the blocks it sets
  to work (each one flies the camera there) and every control signal raised
  with its meaning. **Next cycle** and **Next instruction** step through.
  In the scene, the wires that changed in the step just run glow.
- The facts come from the core: `Machine::explain` reads the microcode
  table through `control_word`, which the control unit's gates are tested
  against. `Machine::opcode_at` (where the running instruction began) is
  observed on both engines and tested to agree with them, cycle by cycle,
  and with the assembler. It costs the fast engine nothing measurable.
- Tests: Rust (283), Vitest (121, including the explanation read from the
  real core for 200 cycles), a page check (12 in all).

- **Breaking a gate** (stuck-at faults, as in chip testing): click any gate
  in the scene, stick its output at 0 or 1, mend it, or mend all. Broken
  gates are red and striped. One stuck NAND in the ALU's adder visibly
  corrupts the maze (`docs/media/m5-broken-gate.png`). Free at run time: a
  stuck NAND is rewired to take its inputs from the constants, a stuck
  flip-flop latches a constant. While a gate is broken only the gate engine
  runs (the fast one has no gates to break; the core refuses it). Broken
  gates survive resets and new programs. Tests: the simulator (truth tables
  while stuck and after mending, a stuck flip-flop through clocks and
  loads), real programs (`crates/programs/tests/faults.rs`: faults in the
  address adder break `hello`; pc bit 0 stuck at 0 fetches only even
  addresses; mended, the computer is cycle for cycle the original), a page
  check (13 in all).

Next for M5: the RAM made of gates promised by ADR 0002; guided lessons.

## M4 — where it stands (2026-11-15)

Done, on `main`:

- **The scene** (`web/src/scene/`): the CPU drawn from its module tree. Every
  module is a block whose lid fades away as it grows on screen, showing its
  parts, down to single gates with their pins and the wires between them.
  Six draw calls; the state of the whole circuit is two small textures
  (one texel per wire, one per module). Positions are whole parts plus
  rests subtracted from the eye in the shader (camera-relative rendering).
- **Layout** from the tree, deterministic: a packer for everything, a hand
  floor plan for `cpu` and `cpu/regs` (`floorplan.ts`, checked against the
  real netlist by a test).
- **Wires** routed as trunks and branches at the height of the lowest module
  holding all their gates. A test checks on the real CPU that every wire is
  one connected piece reaching exactly its pins.
- **Wire activity**: `sim-gate` counts changes per wire at a cost of ~20%
  of its speed (level and count share one word; see its comment for the
  three measurements). The page gets an activity byte per wire with every
  frame; busy wires glow and busy blocks warm up.
- **Camera**: drag, wheel/pinch, right-drag to turn, click a block to fly
  into it, double-click a gate; breadcrumb trail; `?at=cpu/alu/adder8/fa3`.
- **Engine by view** (`auto`, the default): the gate engine while a block of
  the CPU is at least half open, the fast one otherwise. The page opens on
  the fast engine at 4 MHz.
- `web/scripts/browser.mjs`: the installed Chrome driven over the DevTools
  protocol with nothing to install; `look.mjs` takes screenshots of the
  built page. (Adding Playwright as a dependency was refused by the
  permission system in this environment, so this replaces it.)

Verified: `cargo test --workspace` 280 tests; clippy and fmt clean; Vitest
115 tests (layout, LOD with a no-popping sweep over a tree six levels deep,
camera, picking, routing, and the real CPU's scene); screenshots of the
built page at desktop and phone size.

Still to do for M4:

1. ~~Wires between blocks clutter what is below them~~ — done: a module's wires fade in with its lid and down to a faint trace once the camera goes into one of its parts; they are drawn after the lids and before the letters, so names stay readable.
2. ~~Page checks in the repository and in CI~~ — done: `pnpm check` (`web/scripts/check.mjs`), 11 checks on the built page in a real Chrome: every program runs, primes prints, the maze stands still between keys and turns with one, bounce moves, pause and step, the auto engine on load / zoom in / zoom out, click into a block with trail and link, a link to a gate, an engine asked for by link, the phone layout; any console error fails a check. An injected bug (auto always on the gate engine) fails 4 of them.
3. ~~ADR and ARCHITECTURE~~ — done: ADR 0014; ARCHITECTURE §4–6 and §8 describe the scene as built.
4. ~~Frame time~~ — measured on the main thread (`web/scripts/frametime.mjs`): 0.1–0.2 ms per picture at the whole CPU, inside the registers and over gates (the browser rounds its clock to 0.1 ms), about 1% of 16.6 ms. **The graphics card's share is not measured**: a browser without a window does not wait for it. It needs a real window on a real GPU, by hand.
5. Not verified: Firefox, Safari, a real touch screen.

## M3 — what was done

- **Assembler** (`crates/asm`): the syntax of `docs/ISA.md` to a ROM image,
  every byte through `Insn::encode`. Labels with locals, constants,
  expressions, data directives, includes, a listing and a map from addresses
  to source lines; conditional jumps of any distance.
- **Compiler** (`crates/cc`, `docs/LANGUAGE.md`): a language that looks like
  C. Four integer types, pointers, arrays, functions, C's operators and
  statements, `#include`, `asm { … }`. No integer promotion and nothing
  undefined (ADR 0012). Code generation is one accumulator and frames
  reached from `sp` (ADR 0011); no optimiser.
- **Programs** (`programs/`, `crates/programs`), built from source on
  demand, in the browser too:

  | Program | In | ROM | What |
  |---|---|---|---|
  | `maze3d` | C + assembly | 4,799 B | the game: a maze dug anew for each level, walked through in first person, with a map and a way out |
  | `bounce` | C | 602 B | five balls, paced by the timer |
  | `primes` | C | 359 B | a sieve, printed on the console |
  | `stripes`, `hello` | assembly | 61 B, 34 B | the M2 demo; a line on the console |

- **The game** (`programs/maze3d/main.c`): the renderer measured in ADR 0010
  stays in assembly; the maze generator, the walking with sliding along
  walls, the map and the levels are C.
- **The page** offers the programs, shows the console, has the computer's
  keys on screen (they light up for the keyboard too), and a link can name a
  program and an engine: `?program=bounce&engine=gate`. Which program is
  running comes from the simulation, like everything else shown.

### How it was verified

- `cargo test --workspace`: **274 tests** (131 at the end of M2).
  `cargo fmt --check` and `cargo clippy -D warnings` clean. `pnpm typecheck`,
  `pnpm test` (44 Vitest tests), `pnpm build`, `pnpm bench`: pass.
- **The compiler against a reference** in plain Rust
  (`crates/cc/tests/random.rs`): random programs (all types and operators,
  calls, pointers, loops) must print what the reference computes. 1,500 per
  run; 20,000 were run once without a failure. Multiplication, division and
  shifts against Rust's for all 65,536 pairs of bytes and 20,900 16-bit
  pairs (`arithmetic.rs`). 29 feature tests, 12 tests of error messages.
- **Mutation check**: thirteen bugs injected one at a time into the compiler
  and its runtime. The first round caught eight of nine; the one that got
  through (the sign of an 8-bit quotient) is why the exhaustive arithmetic
  test exists. With it, all thirteen are caught.
- **The game against its rules** in plain Rust (`crates/programs/tests/game.rs`):
  the maze of every level is the reference's and all 256 are proper mazes;
  over a scripted walk the player and the whole screen match after every
  frame; a player that only follows the map with the keys gets out of three
  mazes.
- **Both engines**: lockstep cycle by cycle on every program, 3 M cycles with
  keys changing (`crates/programs/tests/lockstep.rs`), on top of M2's 5 M on
  random programs. A computer handed to the gates for one frame in
  twenty-nine of the game stays identical to one that never left the fast
  engine.
- **The production build in headless Chrome**, driven through the DevTools
  protocol, 40 checks: every program runs and is what the menu says; the
  game draws 42–45 frames/s at 4.00 MHz and 2.5 on the gate engine at
  270 kHz; `bounce` and `stripes` 61; keys from the keyboard and from the
  page; reset, pause, single step; links with and without a valid program;
  a phone-sized window; no console errors or warnings. Screenshot:
  `docs/media/m3-maze-on-gates.png`.

**What the browser found that the tests had not.** The game's map and the
balls of `bounce` flickered: both were right when a frame ended, which is
when every test looked, and wrong for most of the time in between, which is
when a display looks. Fixed in the programs, not hidden in the machine
(ADR 0013), with tests that sample the screen between frames; they fail on
the old programs. The console panel was also squeezed to nothing by the
layout. Neither would have been found without loading the page.

**Not verified:** any browser other than Chrome; a real touch screen (the
phone layout was checked in an emulated window only); CI on Linux for these
commits.

### Metrics

| | |
|---|---|
| CPU | 2,603 NAND, 127 DFF (unchanged: ADR 0010 kept the instruction set) |
| A frame of the game | 88,600 clock cycles walking: 45 frames/s at 4 MHz (budget 10) |
| The same on the gate engine | 2.5–3 frames/s at 250–270 kHz |
| The renderer alone | 81,641 – 92,058 cycles, mean 86,280 |
| Engines in WASM (`pnpm bench`) | fast 280 M cycles/s, gate 0.26 M |
| WASM, with the toolchain and the sources inside | 408 kB raw, 156 kB gzipped (budget 400 kB gzipped) |
| JS | 134 kB gzipped (budget 300); almost all Three.js |
| Building `maze3d` from source in WASM | 12 ms |
| ROM used by the largest program | 4.8 of 32 KiB |

### Decisions

- ADR 0010 — the instruction set and the 4 MHz clock stay (measured on the
  renderer, written first).
- ADR 0011 — compiled code: one accumulator, frames from `sp`, arguments on
  the stack, everything caller-saved.
- ADR 0012 — the language looks like C and is not C.
- ADR 0014 (M4) — modules open by their size on screen; the wire activity is
  counted in the gate engine; on auto, the view chooses the engine.
- ADR 0013 — the screen is on view all the time; there is no page flip and
  programs draw in place.
- Programs travel as source and are built where they run; nothing is
  assembled ahead of time.

### Bugs found and fixed on the way

Each has a regression test.

- `const` inside a function was an ordinary variable and could be assigned.
- A function with a result could run off its end and return whatever was in
  the accumulator; it no longer compiles.
- `1 << 40` between untyped numbers was 0 instead of an error.
- Assembler: a label written in full (`f.L1:`, as the compiler writes its
  own) began a new scope, cutting a function's `asm` blocks off from each
  other's local labels.
- `maze3d`: the map flickered; the level-end shutter was erased by the
  renderer as it closed. `bounce`: the balls blinked.
- Page: arrow keys were taken from a focused list, so the clock could not be
  chosen from the keyboard.

## Known limitations

- **Compiled code is naive.** Every use of a variable is a load from the
  stack; there is no register allocation. Enough for the game's logic (3%
  of a frame), not for inner loops, which is what `asm { … }` is for.
- The language has no `struct`, `switch` or function pointers, and a
  function's variables must fit in 255 bytes (ADR 0011).
- The game is a maze and a way out: no score, no time limit, nothing that
  moves but the player. 256 levels, then they repeat.
- On the gate engine a level change takes about three seconds (the shutter
  is paced by counting) and digging a maze a third of a second.
- **The page check is still an ad-hoc script outside the repository.** It
  has now found a class of bug the test suite could not see. It belongs in
  the repository and in CI (Playwright, M4).
- The page has no source or disassembly view. The toolchain already
  produces the map from addresses to source lines (`Assembly::lines`).
- State reaches the page by copying (ADR 0004's fallback path); the
  `SharedArrayBuffer` path is not implemented.
- No wire-activity buffer: the gate view shows the level at the instant of
  each frame, which at 270 kHz is a sample, not everything that happened.
- The gate view is a flat grid; its labels overlap on a phone. No zoom, no
  wires drawn, no hierarchy: M4.
- Engine switching is manual. Switching by camera distance arrives with M4.

## Next: M4

1. Prototype the hierarchical scene and the LOD cross-fade on a synthetic
   module tree before anything else (risk 1).
2. Layout from the module tree: deterministic packing, hints for the top
   levels. Camera-relative rendering.
3. Wires as an instanced mesh; the wire-activity buffer in the worker.
4. Engine switching by camera distance.
5. Playwright: the checks of the ad-hoc script (loads, every program runs,
   the screen is still while nothing is held, no console errors), in CI.

## Earlier milestones

### M2 — simulation engines (2026-11-13)

- **The whole computer** (`crates/machine`): `Board` implements the memory
  map of `docs/ISA.md` §4 with keys, a frame marker, a cycle-counting timer
  and a console. `Machine` is a board plus both engines; `set_engine` hands
  the CPU over at any cycle.
- **Speed measured in WebAssembly** (`pnpm bench`): fast 279 M cycles/s
  against a budget of 5 M; gate, with the scalar evaluator, 250 k against
  50 k. The bit-parallel rewrite was deferred (ADR 0009).
- **The real CPU in the browser** (`crates/wasm`, `web/`): the worker runs
  the machine paced against real time at a nominal 4 MHz; the page shows
  the screen, the registers, the real speed, and every gate of the CPU lit
  by the wire it drives. Engines can be switched while the program runs.
- The gate view uses the M4 data path: one instanced quad per gate, wire
  levels in a texture looked up by wire index in the shader.
- Decisions: ADR 0008 (device reads are pure; device time is clock cycles),
  ADR 0009; `Bus::tick`; the screen is drawn from the framebuffer bytes as
  they are.

### M1 — ISA and hardware design (2026-11-13)

- **ISA** (`docs/ISA.md`, `crates/isa`): 8-bit data, 16-bit addresses, eight
  registers pairing into four 16-bit ones, flags `Z C N V`. 33 instruction
  forms cover all 256 opcodes, so there is no illegal opcode. 1–3 bytes,
  1–5 cycles. The opcode table in `ISA.md` is generated from
  `crates/isa/src/form.rs` and a test keeps them identical
  (`SILICON_BLESS=1 cargo test -p silicon-isa` rewrites it).
- **Component library** (`crates/hdl`): gates, multiplexers, decoder, ripple
  adder, register; re-enterable scopes, named probes, fan-in cone analysis.
- **The CPU in gates** (`crates/machine/src/hw`): control unit generated from
  a microcode table as a PLA, 8-bit ALU, 16-bit address unit, register file
  with three read ports. Only `nand` and `dff`. By block (NAND): regs 1,031 ·
  control 579 · agu 387 · alu 357 · dout 52 · t 50 · pc 49 · sp 49 · ir 25 ·
  flags 24.
- **Reference model** (`crates/sim-fast`): the CPU written again from the
  specification with plain integers, sharing no code with the microcode.
- **Lockstep harness** (`machine::lockstep`): reports the first diverging
  cycle and the instruction being executed.
- Every hardware block is compared with a plain Rust reference. Structural
  tests: every flip-flop maps to a bit of `ArchState`; the address bus never
  depends on `data_in`. Mutation check: four injected bugs, each caught by
  lockstep and named correctly.
- ADRs 0005 (ISA shape), 0006 (microcode table compiled to a PLA,
  independent reference model), 0007 (reset is all zeros; ROM at 0).

### M0 — plan and scaffold (2026-11-12)

- Monorepo (Cargo + pnpm workspaces), `docs/ARCHITECTURE.md`, ADRs 0001–0004,
  `CLAUDE.md`, CI workflow.
- Walking skeleton: `hdl` builder and netlist, scalar `sim-gate`, a four-NAND
  XOR simulated in WASM inside a worker and drawn with Three.js.

## Risks (biggest first)

1. **Seamless LOD across five orders of magnitude of scale** (M4). Precision,
   cross-fades, label legibility. Prototype with a synthetic hierarchy early,
   before polishing anything. The state path (instancing + wire texture) is
   already proven on the real CPU.
2. **What only a browser shows.** Twice the page had faults no test saw (M3: flicker, a squeezed panel). `pnpm check` now runs in CI; a fault it does not look for still needs `look.mjs` and an eye.
3. **Cross-browser WASM/worker behaviour** (Safari, Firefox). Untested.
4. **Engine switch correctness.** Covered by the no-hidden-state test,
   cycle-level lockstep on random programs and on every real one,
   swap-at-every-cycle and switching mid-game. Residual risk is in the
   worker's scheduling, not in the engines.

Retired: *gate engine speed in WASM* (M2: measured, 5× over budget); *scope
of the C-like compiler* (M3: built, and small); *maze3d on an 8-bit CPU with
no multiply* (M3: 45 frames/s, 4.8 KiB of ROM).
