# Progress

Read this and `CLAUDE.md` at the start of every session.

## Status

| Milestone | State |
|---|---|
| M0 — Plan and scaffold | **done** (2026-11-12) |
| M1 — ISA and hardware design | **done** (2026-11-13), branch `m1-isa-and-cpu` |
| M2 — Simulation engines | **done** (2026-11-13), branch `m2-machine` (on top of M1) |
| M3 — Toolchain and programs | next |
| M4 — 3D visualization | — |
| M5 — Learning tools | — |
| M6 — Polish and launch | — |

Neither branch is merged to `main` yet, so CI (which runs on pull requests and
on pushes to `main`) has not run on any of this. Everything below was verified
locally on Windows.

## M2 — what was done

- **The whole computer** (`crates/machine`): `Board` implements the memory map
  of `docs/ISA.md` §4 (ROM at 0, 128×64 framebuffer, device page, RAM on top)
  with keys, a frame marker, a cycle-counting timer and a console. `Machine`
  is a board plus both engines; `set_engine` hands the CPU over at any cycle.
- **Speed measured in WebAssembly** (`pnpm bench`, Node 22 / V8), which is
  what the budgets are about:

  | Engine | Measured | Budget |
  |---|---|---|
  | fast | 279 M cycles/s | 5 M |
  | gate (scalar evaluator) | 250 k cycles/s | 50 k |

  The gate engine clears its budget five times over without the planned
  bit-parallel rewrite, so that rewrite is deferred (ADR 0009).
- **The real CPU in the browser** (`crates/wasm`, `web/`): the worker runs the
  machine, paced against real time at a nominal 4 MHz clock; the page shows
  the computer's screen, the registers, the real speed, and every gate of the
  CPU lit by the wire it drives. Engines can be switched while the program
  runs; pause, single-cycle step, reset and clock rate are controls.
- **A demo program** (`machine::demo::stripes`): scrolling stripes that
  reverse while the left key is held. Built with `silicon_isa::Program`, a
  small label-resolving builder, because the assembler is M3.
- The gate view already uses the M4 data path: one instanced quad per gate,
  wire levels in a texture looked up by wire index in the shader. Only the
  layout is provisional (a flat floorplan by block).

### Verification

- `cargo test --workspace`: 131 tests. `cargo fmt --check`, `cargo clippy -D
  warnings` clean.
- `pnpm typecheck`, `pnpm test` (36 Vitest tests), `pnpm build`: pass.
- Lockstep now compares over 5 M cycles (4.5 M-cycle soak plus the rest), in
  about 10 s. Random ROMs also run on the full memory map with keys changing
  mid-run, and a machine that switches engines at random moments is compared
  with one that never does.
- The demo's framebuffer is checked byte for byte against a picture computed
  in plain Rust, on both engines, with and without a key held.
- The production build was loaded in headless Chrome and driven through the
  DevTools protocol: page reaches ready, no console errors or warnings; fast
  engine shows 4.00 MHz and 61 frames/s (4 MHz ÷ 65,536 cycles per frame =
  61.0); gate engine shows ~269 kHz and 4.0 frames/s (269 k ÷ 65,536 = 4.1);
  Step while paused advances the cycle counter by exactly one. Screenshot:
  `docs/media/m2-cpu-live.png`.

**Not verified:** any browser other than Chrome; the page on a phone-sized
screen; CI on Linux.

### Metrics

| | |
|---|---|
| CPU | 2,603 NAND, 127 DFF, 2,740 wires, critical path 67 |
| WASM | 144 kB raw, 56 kB gzipped (budget 400 kB gzipped) |
| JS | 134 kB gzipped (budget 300 kB); almost all Three.js |
| Demo program | 25 k cycles per frame, 1 frame per 65,536-cycle timer tick |

### Decisions

- ADR 0008 — device reads are pure; device time is counted in clock cycles.
- ADR 0009 — the bit-parallel gate engine is deferred; the scalar one stays.
- `Bus::tick`: whoever drives the clock tells devices a cycle has passed; CPU
  engines know nothing about device time.
- The screen is drawn from the framebuffer bytes as they are, 4 pixels per
  byte, least significant bits first.

### Known limitations

- **The raycaster inner loop has still not been cycle-counted against the
  ISA** (carried over from M1). Do it first in M3, before the assembler
  hardens the encoding. With the fast engine at 279 M cycles/s there is far
  more headroom than the 5 M budget assumed, so the question has changed
  from "can it reach 10 FPS" to "which nominal clock rate do we pick": the
  game must also stay watchable on the gate engine, which is ~1000× slower.
- State reaches the page by copying (ADR 0004's fallback path); the
  `SharedArrayBuffer` path is not implemented.
- No wire-activity buffer yet (toggles between frames): the gate view shows
  the level at the instant of each frame, which at 269 kHz is a sample, not
  everything that happened.
- The gate view is a flat grid. No zoom, no wires drawn, no hierarchy: M4.
- Engine switching is manual. Switching by camera distance arrives with M4.
- The browser check was an ad-hoc script, not a test in the repository.
  Playwright is planned for M4.
- No disassembly on the page (the HUD shows `ir` in hex).

## Next: M3

1. Write the raycaster's inner loop by hand, count cycles, adjust the ISA if
   it hurts. Decide the nominal clock rate.
2. Assembler (`crates/asm`): labels, expressions, data directives, includes,
   listings, precise diagnostics. Its output must match `Insn::encode`.
3. Calling convention, then the C-like compiler (`crates/cc`).
4. Programs in `programs/`: a few small ones, then `maze3d`. Golden
   framebuffer tests with scripted keys on both engines.
5. The page loads a chosen program instead of the built-in demo.

## M1 — what was done

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
- Every hardware block is compared with a plain Rust reference (ALU over all
  operand pairs for each microcoded operation; control unit for every opcode ×
  step × flags). Structural tests: every flip-flop maps to a bit of
  `ArchState`; the address bus never depends on `data_in`.
- Mutation check: four bugs injected one at a time (two in the model, two in
  the hardware) were each caught by lockstep and named correctly.
- ADRs 0005 (ISA shape), 0006 (microcode table compiled to a PLA, independent
  reference model), 0007 (reset is all zeros; ROM at 0).

## M0 — what was done

- Monorepo (Cargo + pnpm workspaces), `docs/ARCHITECTURE.md`, ADRs 0001–0004,
  `CLAUDE.md`, CI workflow.
- Walking skeleton: `hdl` builder and netlist, scalar `sim-gate`, a four-NAND
  XOR simulated in WASM inside a worker and drawn with Three.js.

## Risks (biggest first)

1. **Seamless LOD across five orders of magnitude of scale** (M4). Precision,
   cross-fades, label legibility. Prototype with a synthetic hierarchy early,
   before polishing anything. The state path (instancing + wire texture) is
   already proven on the real CPU.
2. **Scope of the C-like compiler** (M3). Keep the language small; maze3d may
   use inline asm for its inner loop.
3. **maze3d on an 8-bit CPU with no multiply.** Speed is no longer the worry
   (see above); code size in a 32 KiB ROM and the amount of table data are.
4. **Cross-browser WASM/worker behaviour** (Safari, Firefox). Untested.
5. **Engine switch correctness.** Covered by the no-hidden-state test,
   cycle-level lockstep, swap-at-every-cycle and random switching on the full
   machine. Residual risk is in the worker's scheduling, not in the engines.

Retired: *gate engine speed in WASM* (measured, 5× over budget).
