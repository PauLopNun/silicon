# Progress

Read this and `CLAUDE.md` at the start of every session.

## Status

| Milestone | State |
|---|---|
| M0 — Plan and scaffold | **done** (2026-11-12) |
| M1 — ISA and hardware design | **done** (2026-11-13), on branch `m1-isa-and-cpu`, not yet merged to `main` |
| M2 — Simulation engines | next |
| M3 — Toolchain and programs | — |
| M4 — 3D visualization | — |
| M5 — Learning tools | — |
| M6 — Polish and launch | — |

## M1 — what was done

- **ISA** (`docs/ISA.md`, `crates/isa`): 8-bit data, 16-bit addresses, eight
  registers pairing into four 16-bit ones, flags `Z C N V`. 33 instruction
  forms cover all 256 opcodes, so there is no illegal opcode. 1–3 bytes,
  1–5 cycles. The opcode table in `ISA.md` is generated from
  `crates/isa/src/form.rs` and a test keeps them identical
  (`SILICON_BLESS=1 cargo test -p silicon-isa` rewrites it).
- **Component library** (`crates/hdl`): gates, multiplexers, decoder, ripple
  adder, register. `Builder::scope` can now be re-entered (a block is built in
  two visits: read side first, flip-flops last), plus named probes and fan-in
  cone analysis.
- **The CPU in gates** (`crates/machine/src/hw`): control unit generated from
  a microcode table as a PLA, 8-bit ALU, 16-bit address unit, register file
  with three read ports. Only `nand` and `dff`.
- **Reference model** (`crates/sim-fast`): the CPU written again from the
  specification with plain integers, sharing no code with the microcode.
  This was pulled forward from M2 because the hardware needed something
  independent to be checked against.
- **Both engines behind `Engine`** (`GateCpu`, `FastCpu`) and the **lockstep
  harness** (`machine::lockstep`), which reports the first diverging cycle and
  the instruction being executed.

### Verification

- `cargo test --workspace`: 101 tests pass. `cargo fmt --check` and
  `cargo clippy -D warnings` are clean.
- Every hardware block is compared with a plain Rust reference: adder,
  multiplexers and decoder exhaustively; the ALU over all 65,536 operand
  pairs for each of the 16 ways the microcode drives it; the control unit
  for every opcode × step × flag value (32,768 inputs).
- Lockstep, gate netlist against reference model, comparing the whole
  `ArchState` and the bus access after every cycle: random memory from random
  states (120 k cycles), every opcode from random register contents (614 k
  cycles), a 200 k-cycle soak, and hand-written programs. About 0.9 M cycles
  in 2.5 s.
- Engines swapped at every cycle of a program, in both directions, give the
  same final state and memory.
- Structural tests: every flip-flop maps to a bit of `ArchState` (rule 5);
  the address bus and strobes never depend on `data_in` (ADR 0003).
- Does lockstep actually catch bugs? Four were injected one at a time (`inc`
  clobbering `C` in the model; sticky `Z` removed from the 16-bit add
  microcode; the increment winning in `ld a, [ba+]`; the wrong carry source
  for shifts in the hardware). Each failed lockstep and was named correctly.
  All four were reverted; the seeds proptest found are kept in
  `crates/machine/tests/lockstep.proptest-regressions`.
- `silicon-wasm` still compiles for `wasm32-unknown-unknown`.

**Not run this session:** `pnpm test`, `pnpm typecheck`, `pnpm build` and
`pnpm wasm`. The machine used had no pnpm or wasm-pack. Nothing under `web/`
was changed. CI has not run on the branch either (it triggers on pull requests
and on pushes to `main`).

### Metrics

| | |
|---|---|
| CPU size | 2,603 NAND, 127 DFF, 2,740 wires, 70 modules |
| Critical path | 67 gate delays |
| By block (NAND) | regs 1,031 · control 579 · agu 387 · alu 357 · dout 52 · t 50 · pc 49 · sp 49 · ir 25 · flags 24 |
| Fast engine, native release build | 297 M cycles/s |
| Gate engine, native release build, scalar evaluator | 450 k cycles/s |

Speeds are from `cargo run --release -p silicon-machine --example speed` on
the development machine. **WASM speed has not been measured**; the budgets in
ARCHITECTURE §7 are for WASM.

### Decisions

- ADR 0005 — shape of the ISA: accumulator plus register pairs, every opcode
  defined.
- ADR 0006 — control unit is a microcode table compiled to a PLA; the
  reference model is written independently of it.
- ADR 0007 — reset is "every flip-flop 0", hence ROM at `0x0000` and RAM at
  the top (the memory map in ARCHITECTURE §3 was swapped accordingly).
- `ArchState` and `Bus` moved to `crates/isa`, so both engines depend on the
  contract rather than on each other. `ArchState` gained `t` (operand latch).
- Tests are built with `opt-level = 2`.

### Known limitations

- **The raycaster inner loop was not written out and cycle-counted** before
  fixing the ISA, as risk 2 below asked. The ISA has what that loop should
  need (2-cycle table lookup, 2-cycle 16-bit add, 1-cycle shifts) but this is
  untested. Do it first thing in M3, before the assembler hardens the
  encoding. Changing an instruction means changing `form.rs`, the microcode,
  the fast model and `ISA.md` together; lockstep catches any mismatch.
- No memory map or devices yet: tests use `Ram`, 64 KiB of plain memory.
- The gate engine is still the scalar evaluator from M0 and runs the whole
  netlist twice per cycle. The cone split of ADR 0003 is not implemented; the
  result is the same because the address cone depends only on flip-flops.
- The WASM binding still exposes the XOR demo, not the CPU.
- No calling convention yet; it is defined with the compiler in M3.

## Next: M2

1. `Machine`: memory map of `ISA.md` §4, framebuffer, keyboard, frame timer,
   debug port; lockstep on the whole machine.
2. Bit-parallel gate engine with the address/main cone split; benchmark it in
   WASM first (risk 1).
3. Raise lockstep in CI to ≥ 5 M cycles (about 15 s at the current speed).
4. WASM binding for the real machine; the worker runs the CPU instead of the
   XOR demo and can switch engines.

## M0 — what was done

- Monorepo: Cargo workspace + pnpm workspace (`web`).
- `docs/ARCHITECTURE.md`, four ADRs, `CLAUDE.md`.
- Walking skeleton, real end to end: `hdl` builder and netlist, scalar
  `sim-gate`, `Engine` / `Bus` traits, `wasm` XOR demo, worker and Three.js
  view where every colour comes from the simulated wire state.
- CI workflow: fmt, clippy, Rust tests, WASM build, typecheck, Vitest, web build.
- Verified then: 16 Rust tests, 4 Vitest tests, production build loaded in
  headless Chrome. XOR demo: 4 NAND, depth 3; WASM 38 kB; JS 132 kB gzipped.
- No Playwright test yet (arrives with the real scene in M4).

## Risks (biggest first)

1. **Gate engine speed in WASM.** Budget: 50 k cycles/s. The CPU came out at
   2.6 k NANDs, less than half of what was assumed, and the unoptimised scalar
   evaluator already does 450 k cycles/s natively. Encouraging, but WASM is
   unmeasured. Measure before optimising.
2. **maze3d frame rate on an 8-bit CPU.** ≥ 10 FPS on `fast`. Not yet
   validated against the ISA; see "Known limitations".
3. **Seamless LOD across five orders of magnitude of scale.** Precision,
   cross-fades, label legibility. Prototype with a synthetic hierarchy early in
   M4 before polishing anything.
4. **Engine switch correctness.** Now covered by the no-hidden-state test,
   cycle-level lockstep and the swap-at-every-cycle test. Still to do: switch
   under randomised timing with the full machine (M2).
5. **Cross-browser WASM/worker/SAB behaviour** (Safari). The copy path of
   ADR 0004 is the safety net; test both.
6. **Scope of the C-like compiler.** Keep the language small; maze3d may use
   inline asm for its inner loop.
