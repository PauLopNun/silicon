# Silicon architecture

> "A 3D game running on a CPU you can zoom into, in 3D, down to every single NAND gate."

This document is the map of the project: what the subsystems are, how they talk
to each other, and the budgets they must meet. Decisions with real alternatives
are recorded as ADRs in `docs/adr/`. Where something is marked *provisional*,
the milestone that settles it is named.

## 1. The stack at a glance

```
                programs/*.c, *.asm
                        │  cc → asm                       (crates/cc, crates/asm)
                        ▼
                    ROM image
                        │
┌───────────────────────▼─────────────────────────────────────────────┐
│ machine  (crates/machine)                                            │
│   memory map: ROM · RAM · framebuffer · keyboard · frame timer       │
│   CPU behind `trait Engine`  ──────────────┐                         │
│        ┌──────────────────┐   ArchState    │  ┌───────────────────┐  │
│        │ gate engine      │◄──────────────►│  │ fast engine       │  │
│        │ sim-gate + CPU   │  (transfer /   │  │ sim-fast          │  │
│        │ netlist from hdl │   lockstep)    │  │ arithmetic model  │  │
│        └──────────────────┘                   └───────────────────┘  │
└───────────────────────┬─────────────────────────────────────────────┘
                        │ wasm-bindgen                   (crates/wasm)
                ┌───────▼────────┐   state buffers   ┌──────────────────┐
                │ Web Worker     │──────────────────►│ main thread      │
                │ runs the clock │◄──────────────────│ Three.js, UI     │
                └────────────────┘   commands        └──────────────────┘
```

Dependency direction is strictly downward; nothing in `crates/` knows about the
browser except `crates/wasm`.

| Crate | Responsibility | Depends on |
|---|---|---|
| `isa` | The contract: opcode map as data, `Insn` (encode / decode / print), `ArchState`, `trait Bus`. No behaviour | — |
| `hdl` | DSL (`Builder`), `Netlist` with module hierarchy, validation, levelization, statistics, and the generic component library (gates, multiplexers, adders, registers) | — |
| `sim-gate` | Generic evaluator for any `Netlist`. Knows nothing about CPUs | `hdl` |
| `sim-fast` | Cycle-accurate arithmetic model of the Silicon CPU, written from `docs/ISA.md` | `isa` |
| `machine` | The CPU netlist itself (`machine::hw`), `trait Engine` + both adapters, lockstep harness. From M2: memory map, devices, engine switching | `isa`, `hdl`, `sim-gate`, `sim-fast` |
| `asm` | Assembler; owns the `Diagnostic` error type used by the whole toolchain | — |
| `cc` | C-like compiler that emits assembly (`docs/LANGUAGE.md`) | `asm` |
| `programs` | The sources in `programs/`, carried as text and built with the toolchain when asked for; the generator of maze3d's tables | `asm`, `cc` |
| `wasm` | Bindings: machine, programs and toolchain, netlist metadata for the renderer | all of the above |

The encoding exists in exactly one place, `crates/isa/src/form.rs`. The
decoder, the hardware's control unit and the opcode table of `docs/ISA.md` are
all checked against it. `sim-fast` deliberately does not share the hardware's
microcode ([ADR 0006](adr/0006-microcode-table-compiled-to-gates.md)): the two
are independent implementations of the same specification.

## 2. Hardware description (`hdl`)

- Two primitives only: 2-input `nand` and positive-edge `dff`. Constants 0 and 1
  are wires 0 and 1.
- Modules are Rust functions that call `Builder::scope("name", |b| …)`. The
  scope tree is stored in the netlist; a gate's path is
  `module/…/module/nand<N>`. Paths are the public identifier for gates in
  tests, the renderer, search, URLs and "break a gate" mode
  ([ADR 0001](adr/0001-embedded-rust-dsl.md)).
- `Builder::finish` rejects undriven wires, multiple drivers and combinational
  loops (any cycle must pass through a `dff`).
- `Netlist::levelize` gives a topological order and the logic depth (critical
  path, in gate delays).

### Clocking model ([ADR 0003](adr/0003-single-clock-and-memory-timing.md))

One global clock; every `dff` latches on the same edge. **One CPU cycle = one
clock cycle = one full evaluation of the netlist.** Memory read is combinational
within the cycle, so the netlist is evaluated in two cones:

1. *address cone* — the gates that compute the address bus and read/write
   strobes (a function of flip-flop outputs only);
2. the machine performs the bus read and feeds `data_in`;
3. *main cone* — everything else, ending at the `d` inputs of the flip-flops;
4. clock edge: flip-flops latch, and a pending write commits to memory.

The cone split is computed by the netlist compiler, not written by hand.

### What is and is not made of gates ([ADR 0002](adr/0002-memory-is-behavioural.md))

The **CPU is 100% NAND + DFF**: ALU, registers, flags, PC, SP, IR, control.
**Memory and devices are behavioural** (arrays behind the bus): 64 KiB of RAM
as flip-flops would be ~half a million DFFs plus decoders, which would bury the
CPU visually and make the gate engine unusably slow. This is stated openly in
the README and in the UI. A small gate-level RAM is built as a lesson exhibit so
that "how memory works" is still shown with real gates.

Measured size (M1): 2,603 NANDs, 127 DFFs, critical path 67 gate delays.

### Inside the CPU (`machine::hw`)

| Block | What it is | NANDs |
|---|---|---|
| `control` | microcode table compiled to a PLA ([ADR 0006](adr/0006-microcode-table-compiled-to-gates.md)); outputs the control word, ~50 signals | 579 |
| `regs` | eight 8-bit registers, two 8-bit read ports and one 16-bit pair port | 1,031 |
| `alu` | 8-bit adder, AND/OR/XOR, right shift; every register write passes through it | 357 |
| `agu` | 16-bit adder for all address arithmetic: `pc` + 1, stack, pointers, branches | 387 |
| `pc`, `sp`, `t`, `ir`, `step`, `flags` | the remaining registers | 197 |
| `dout` | what a write puts on the data bus | 52 |

## 3. The machine

Memory map (specified in `docs/ISA.md` §4; devices are implemented in M2):

| Range | What |
|---|---|
| `0x0000–0x7FFF` | ROM (32 KiB): code, trig and reciprocal tables. Execution starts at 0 |
| `0x8000–0x87FF` | Framebuffer, 128×64, 2 bits/pixel, 4 pixels per byte (2048 bytes) |
| `0x8800–0x88FF` | I/O: keyboard register, frame-sync/timer, (debug port for tests) |
| `0x9000–0xFFFF` | RAM (28 KiB); the stack starts at the top |

ROM is low and RAM high because reset is simply "every flip-flop at 0"
([ADR 0007](adr/0007-reset-is-all-zeros.md)).

`machine::Board` is the memory and the devices; `machine::Machine` is a board
plus both engines. Device reads have no side effects and device time is the
clock-cycle count, so runs are reproducible on either engine at any speed
([ADR 0008](adr/0008-devices-are-pure-and-count-cycles.md)).

128×64 at 4 shades is the smallest screen on which a raycaster reads well and
distance shading is visible, while a full redraw is only 2 KiB of writes.
The screen is on view all the time: the page shows the framebuffer as it is,
and there is no page flip, so programs draw in place
([ADR 0013](adr/0013-the-screen-is-on-view-all-the-time.md)).

### Programs and the toolchain

```
programs/*.c ──cc──► assembly ──┐
programs/*.asm ─────────────────┴─asm──► ROM image + symbols + line map
```

- **`asm`** turns the assembly of `docs/ISA.md` into a ROM image. Every byte
  comes out of `isa::Insn::encode`. Conditional jumps of any distance are
  chosen by repeating the layout until nothing grows, which is what lets a
  compiler emit jumps without knowing distances.
- **`cc`** compiles the language of `docs/LANGUAGE.md`: it looks like C, has
  four integer types, pointers and arrays, and leaves nothing undefined
  ([ADR 0012](adr/0012-a-language-that-looks-like-c.md)). The generated code
  keeps values in `a`/`ba`, reaches variables from `sp` and passes arguments
  on the stack ([ADR 0011](adr/0011-compiled-code-accumulator-and-sp-frames.md)).
  There is no optimiser; `asm { … }` is for what must be fast.
- **`programs`** carries the sources as text. Nothing is built ahead of
  time: the worker compiles the chosen program inside the WASM core (the
  largest takes about 13 ms). The instruction set and the 4 MHz clock were
  settled by writing the game's renderer first
  ([ADR 0010](adr/0010-isa-and-clock-after-the-raycaster.md)).

| Program | Written in | What |
|---|---|---|
| `maze3d` | C, renderer in assembly | the game: a maze dug anew for each level, seen from inside, with a map |
| `bounce` | C | balls on the screen, paced by the timer |
| `stripes` | assembly | the M2 demo |
| `primes`, `hello` | C, assembly | write on the console and stop |

`trait Engine` (in `crates/machine/src/lib.rs`) is the seam: `reset`,
`step_cycle(&mut dyn Bus)`, `state() -> ArchState`, `load_state(&ArchState)`.
Memory and devices belong to the machine, not to an engine, so switching
engines never copies memory.

## 4. Two engines, proven equivalent

| | `gate` | `fast` |
|---|---|---|
| Computes | every NAND and DFF of the real netlist | registers with integer arithmetic |
| Role | source of truth; drives everything you see inside the CPU | runs programs at playable speed |
| Target (WASM) | ≥ 50,000 cycles/s | ≥ 5,000,000 cycles/s |

**Equivalence.** `ArchState` (PC, SP, registers, flags, IR, micro-step,
operand latch, cycle count) is the complete architectural state. The lockstep harness steps both
engines one cycle at a time on the same program and compares `ArchState`, every
bus access (address, data, direction) and periodically all memory. It runs
generated random programs plus everything in `programs/`
(`crates/programs/tests/lockstep.rs`), millions of cycles in CI. Any
divergence fails CI and is minimised into a regression test.

**No hidden state.** A test asserts that every `dff` in the CPU netlist is
mapped to a bit of `ArchState`. That makes `ArchState` sufficient for a transfer
by construction: if someone adds a flip-flop without declaring it, the build
goes red.

**Switching.** Both engines are cycle-accurate, so a switch can happen at any
cycle boundary:

```
fast → gate:  s = fast.state();  gate.load_state(&s);  gate.settle()
gate → fast:  s = gate.state();  fast.load_state(&s)
```

`load_state` on the gate engine writes the flip-flop outputs through the
`ArchState`↔`dff` map and re-settles the combinational logic. The worker
switches to `gate` while one of the CPU's blocks is at least half open in the
scene, which is when gates are plainly in view, and back to `fast` otherwise;
or the user picks an engine explicitly
([ADR 0014](adr/0014-the-scene-opens-by-size-and-chooses-the-engine.md)). The active engine and its real speed are always shown in the
UI. In `gate` mode the game runs slower than in `fast` mode; we show that
honestly rather than hide it.

**Gate engine design.** Levelized, compiled to flat arrays, one word per wire
(its level, and a count of its changes for the wire-activity buffer), one
NAND at a time. Measured in WASM it runs the 2,603-NAND CPU at about
205 k cycles/s, four times the budget, so the bit-parallel evaluator that was
planned here has been deferred
([ADR 0009](adr/0009-scalar-gate-engine-is-enough.md)). The fast engine
measures 279 M cycles/s. `pnpm bench` reproduces both numbers.

## 5. Worker ↔ renderer data flow

The simulation never runs on the main thread, and the renderer never calls
into WASM.

**Commands** (main → worker, `postMessage`, small JSON): load program, key
state, run/pause/step (a cycle or an instruction), set speed, set engine,
break or mend a gate, drive the memory made of gates. Types live in `web/src/protocol.ts`.

**State** (worker → main), once per rendered frame
([ADR 0004](adr/0004-worker-state-transport.md)):

| Buffer | Size | Use |
|---|---|---|
| framebuffer | 2048 B | uploaded as the screen texture |
| wire levels | 1 B per wire (~10–20 kB) | uploaded as a data texture; shaders look up each wire's state by index |
| wire activity | 1 B per wire | changes since the last frame, on a square-root scale of changes per cycle, counted by `sim-gate` itself (ADR 0014); drives the glow of busy wires and blocks |
| arch state | ~32 B | live values on block labels, HUD |
| console | a few bytes | what the program wrote since the frame before |

Each frame also names the program in ROM and counts how many times the
computer has been started, so that the page shows what the simulation is
doing and not what it was last asked to do.

Inside the worker these are typed-array views straight into WASM memory. To
the main thread they travel through a `SharedArrayBuffer` when the page is
cross-origin isolated, and as one transferred copy per frame otherwise
(~25 kB, well under 0.1 ms). M0 implements the copy path.

**Static metadata** (once, at load): module tree, gate list (kind, module,
input/output wire indices) as flat typed arrays. The layout is computed on the
frontend from this, deterministically.

**Clock scheduling.** The worker runs cycles in time-sliced batches so it stays
responsive to commands: `cycles_this_slice = clock_rate × elapsed`, capped by a
time budget. Step, slow motion and "explain" modes are the same loop with a
tiny rate. The nominal clock is 4 MHz; when the active engine cannot reach it
(the gate engine), the simulation runs as fast as it can and the page shows
the real rate (`web/src/clock.ts`).

**As of M4** the copy path is implemented, with the framebuffer, wire levels
and activity, registers and console. A link can name a program, an engine
and a place: `?program=bounce&engine=gate&at=cpu/alu/adder8`. The
`SharedArrayBuffer` path is still to come.

## 6. Rendering (`web/src/scene/`)

Everything that decides what the picture is lives in plain modules, tested
without a browser (some on the real netlist, loaded from the WASM core);
only `view.ts` touches WebGL.

| Module | What |
|---|---|
| `tree.ts` | the module tree from the netlist; gate paths, as the core names them |
| `layout.ts` | a rectangle per module, nested like the tree, and a cell per gate. Packed in declaration order on shelves in rows or columns, whichever wastes least; hand arrangements for `cpu` and `cpu/regs` in `floorplan.ts`. Each module's lid stands above everything inside it |
| `route.ts` | each wire as a trunk east–west and branches north–south one layer up, with vias to its pins, at the height of the lowest module holding all its gates |
| `lod.ts` | how open each module is, from its size on screen; how much of it is seen; how far the camera has gone into its parts |
| `camera.ts` | the camera as numbers: projection, rays, fit, zoom about a point, smooth flights (van Wijk and Nuij) |
| `pick.ts` | what is under the pointer: a closed block, or through open ones down to a gate |
| `labels.ts` | names, gate counts and register values as glyph instances |
| `navigator.ts` | pointer, wheel, touch and keys to camera movements |
| `policy.ts` | which engine the view asks for |
| `replay.ts` | where the playback of a single step begins and ends, in logic depth |
| `bloom.ts` | the glow: whatever a shader drew brighter than white, blurred into a halo |
| `pace.ts` | whether pictures are coming too slowly for the glow |
| `pixels.ts` | how many device pixels the scene is drawn with: the screen's density, held to about three million pixels |
| `view.ts` | ten draw calls and two state textures |

- **Level of detail by projected size**, continuous: a module's lid fades as
  it grows on screen and its parts show through; nothing pops
  ([ADR 0014](adr/0014-the-scene-opens-by-size-and-chooses-the-engine.md)).
- **State by texture.** Every gate, wire segment, lid and letter is an
  instance carrying the index of its wire and of its module. The state of
  the whole circuit is one texel per wire (level, activity) and one per
  module (open, seen, busy, entered). Neither the simulation nor the camera
  ever rebuilds geometry.
- **Camera-relative rendering.** Positions are stored as whole numbers and
  small rests, both exact in 32-bit floats, and so is the eye; the vertex
  shader subtracts whole from whole and rest from rest. Near and far planes
  follow the camera's distance.
- **Order instead of sorting.** Floors (parents first), gates, lids (lowest
  first, blended), wires (blended; a module's wires run above every lid
  still visible inside it, and fade once the camera goes into one of its
  parts), letters. The camera never goes below the floor, so lower is
  always behind.
- **Light is a fact about the simulation**
  ([ADR 0016](adr/0016-light-is-a-fact-about-the-simulation.md)). What
  gives off light is drawn brighter than white and glows by that excess
  alone. A busy wire carries dashes that move away from the gate driving
  it; a closed block lets the lights of its gates through. From afar, on
  `auto`, 256 clock cycles of every frame are computed on the gates so that
  those lights are live. A single step is played back in order of logic
  depth, beginning where the camera is.
- Not built: signed-distance text (a glyph sheet with mipmaps, a sharpening
  step and a shadow is used instead).

## 6½. Learning tools

All four read the real hardware rather than a model of it
([ADR 0015](adr/0015-learning-tools-use-the-real-hardware.md)).

| Tool | Where | What it reads |
|---|---|---|
| **Explain**: one cycle or one instruction at a time | `Machine::explain`, `step_instruction`; `web/src/explain.ts` | the microcode table through `control_word`; the opcode's address seen on the bus (`opcode_at`) |
| **Break a gate**: stuck at 0 or 1 | `GateSim::force`, `Machine::break_gate` | rewires the gate in the compiled arrays, at no cost per cycle; only the gate engine runs while one is broken |
| **Memory made of gates**: 8 × 4 bits | `machine::exhibit`, `wasm::RamExhibit`; `?view=ram` | its own netlist and simulator beside the CPU's |
| **Guided tour**: nine steps | `web/src/lessons.ts` | places by path and facts about them, checked against the netlists by a test |

## 7. Performance budgets

| Item | Budget |
|---|---|
| Frame time, main thread | ≤ 16.6 ms (60 FPS) on a mid-range GPU, whole design visible |
| State upload per frame | ≤ 0.5 ms |
| Draw calls | ≤ 50 |
| `gate` engine | ≥ 50 k cycles/s (WASM) |
| `fast` engine | ≥ 5 M cycles/s (WASM) |
| `maze3d` | ≥ 10 FPS on `fast` |
| Lockstep in CI | ≥ 5 M cycles, ≤ 5 min |
| WASM size | ≤ 400 kB gzipped |
| Initial JS | ≤ 300 kB gzipped |
| Load to playable | ≤ 2 s on broadband |

## 8. Testing strategy

- `hdl` components: exhaustive where the input space allows (≤ 2²⁰ vectors),
  `proptest` otherwise; each compared to a plain Rust reference function.
- Engines: lockstep equivalence (§4), no-hidden-state test, transfer round-trip.
- Toolchain: assembler encoding tests against `isa` tables. The compiler is
  compared with a reference in plain Rust: random programs must print what
  the reference computes, and multiplication, division and shifts are
  checked for every pair of bytes.
- Programs: each is compared with its rules written in plain Rust, frame by
  frame with scripted keys, on both engines; pictures a person can check are
  kept as text. A program that animates is also looked at between frames
  (ADR 0013).
- Frontend: Vitest for pure logic (layout, LOD, camera, picking, routing,
  protocol, URL state), some of it on the real netlist from the WASM core;
  `pnpm check` drives the built page in a real Chrome (programs, keys, the
  engine following the view, zooming, links, phone layout, no console errors).
- Rule: a failing test is fixed at the cause, never weakened; every bug found
  gets a regression test.
