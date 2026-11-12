# Progress

Read this and `CLAUDE.md` at the start of every session.

## Status

| Milestone | State |
|---|---|
| M0 — Plan and scaffold | **done** (2026-11-12) |
| M1 — ISA and hardware design | next |
| M2 — Simulation engines | — |
| M3 — Toolchain and programs | — |
| M4 — 3D visualization | — |
| M5 — Learning tools | — |
| M6 — Polish and launch | — |

## M0 — what was done

- Monorepo: Cargo workspace (7 crates) + pnpm workspace (`web`).
- `docs/ARCHITECTURE.md`, four ADRs, `CLAUDE.md`.
- Walking skeleton, real end to end rather than empty stubs:
  - `hdl`: `Builder` DSL (nand, dff, scopes, feedback wires), `Netlist` with
    hierarchy and gate paths, validation (undriven / multiple drivers /
    combinational loop), levelization, stats.
  - `sim-gate`: scalar levelized reference evaluator with a dense state buffer.
  - `machine`: `Engine` / `Bus` traits, `EngineKind`. `sim-fast`: `ArchState`.
  - `asm` / `cc`: shared `Diagnostic` type only.
  - `wasm`: `XorDemo` binding exposing the state buffer by pointer.
  - `web`: worker runs the WASM XOR; main thread draws it with Three.js; every
    colour comes from the simulated wire state; engine badge and breadcrumb.
- CI workflow: fmt, clippy, Rust tests, WASM build, typecheck, Vitest, web build.

### Verification
- `cargo test --workspace`: 16 tests pass (incl. one proptest and one doctest). Clippy clean with `-D warnings`.
- `pnpm test`: 4 Vitest tests pass. `pnpm build` succeeds.
- Production build loaded in headless Chrome: worker boots WASM, page reaches
  ready state, XOR renders with correct levels for A=0, B=0.

### Metrics
- XOR demo: 4 NAND, depth 3. WASM 38 kB raw. JS bundle 132 kB gzipped (Three.js, not yet tree-shaken by usage).

### Decisions
- ADR 0001 embedded Rust DSL · ADR 0002 memory is behavioural · ADR 0003 single
  clock and memory timing · ADR 0004 worker state transport.
- Layout is computed on the frontend from exported hierarchy (testable in
  Vitest, no WASM rebuild to iterate on visuals).
- M0 was done without subagents: it is small and defines the interfaces the
  delegated work will be written against.

### Known limitations
- No Playwright test yet (arrives with the real scene in M4; checked by hand
  with headless Chrome for now).
- CI has not run on GitHub yet: the repository has no remote.
- `ArchState` fields and the memory map are provisional until `docs/ISA.md`.
- Deploy workflow is M6.

## Risks (biggest first)

1. **Gate engine speed.** 50 k cycles/s × ~6 k gates = 300 M gate evaluations/s
   in WASM. Needs bit-parallel evaluation that actually packs well; gates of one
   level must be laid out contiguously. Attack first in M2, with a benchmark
   before the CPU is finished (synthetic netlist of similar size/depth).
2. **maze3d frame rate on an 8-bit CPU.** ≥ 10 FPS means the ISA needs cheap
   16-bit add/shift and table lookups. Design the ISA in M1 by writing the
   raycaster inner loop on paper first and counting cycles.
3. **Seamless LOD across five orders of magnitude of scale.** Precision,
   cross-fades, label legibility. Prototype with a synthetic hierarchy early in
   M4 before polishing anything.
4. **Engine switch correctness.** Mitigated by the no-hidden-state test and
   cycle-level lockstep; switch under randomised timing in tests.
5. **Cross-browser WASM/worker/SAB behaviour** (Safari). The copy path of
   ADR 0004 is the safety net; test both.
6. **Scope of the C-like compiler.** Keep the language small; maze3d may use
   inline asm for its inner loop.
