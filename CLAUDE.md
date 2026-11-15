# Silicon — project conventions

An 8-bit computer built from NAND gates and D flip-flops, simulated at gate
level and rendered in 3D in the browser. Start every session by reading
`docs/PROGRESS.md` (status, risks) and skimming `docs/ARCHITECTURE.md`.

## Commands

Rust and pnpm are installed per-user; if they are not found, add
`~/.cargo/bin` and `~/.local/bin` to `PATH`.

| Task | Command |
|---|---|
| Rust tests | `cargo test --workspace` |
| Rust lint | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` |
| Build WASM into `web/src/wasm-pkg` | `pnpm wasm` |
| Dev server (builds WASM first) | `pnpm dev` |
| Frontend tests | `pnpm test` |
| Typecheck (needs `pnpm wasm` once) | `pnpm typecheck` |
| Production build | `pnpm build` |
| Everything | `pnpm lint && pnpm test:all && pnpm build` |

## Rules that do not bend

1. **No fakery.** Anything shown about the machine comes from the simulation.
2. **Only `nand` and `dff`** in the CPU netlist. Memory/devices are behavioural (ADR 0002).
3. **Never weaken a test to make it pass.** Fix the cause; add a regression test for every bug.
4. **The two engines must stay equivalent.** A change to CPU behaviour touches
   the netlist, `sim-fast` and `docs/ISA.md` together, and lockstep must pass.
5. **Every `dff` in the CPU maps to a bit of `ArchState`** (no hidden state).
6. **Original work only.** No copied netlists, games, names or art.
7. Ambiguity → decide, write an ADR in `docs/adr/NNNN-title.md`, continue.

## Conventions

- Crates are `silicon-<dir>`; dependency direction is in ARCHITECTURE §1. Only `crates/wasm` knows about the browser.
- Module names in the HDL are lowercase, no slashes, indexed instances get a
  numeric suffix (`fa3`). Gate paths (`cpu/alu/adder8/fa3/nand2`) are public
  API: tests, URLs and the UI refer to them.
- Every hardware module has a plain Rust reference function and a test
  comparing them (exhaustive if ≤ 2²⁰ input vectors, else `proptest`).
- `unsafe` is denied workspace-wide; if a hot path needs it, allow it locally
  with a comment explaining the invariant.
- TypeScript is strict; the main thread never imports the WASM package, only
  the worker does. All worker messages are typed in `web/src/protocol.ts`.
- User-facing code and docs favour clarity; hot paths may be clever but must be commented.
- Programs live as source in `programs/` (assembly, or the C-like language of
  `docs/LANGUAGE.md`) and each is tested against its rules written in plain
  Rust. The screen is on view all the time (ADR 0013): programs draw in
  place, and tests of anything that moves also look between frames.
- Commit messages: imperative summary line, body says why. One milestone ends
  with tests green, `docs/PROGRESS.md` updated, and a commit.
