# Silicon

**A 3D game running on a CPU you can zoom into, in 3D, down to every single NAND gate.**

![The CPU running a program in the browser: the screen it draws, its registers, and every one of its gates lit by the wire it drives](docs/media/m2-cpu-live.png)

> Work in progress. The computer exists and runs in the browser: a CPU of
> 2,603 NAND gates and 127 flip-flops, checked cycle by cycle against a
> reference model, drawing to a screen while you watch every gate. The 3D
> view and the game do not exist yet. See [docs/PROGRESS.md](docs/PROGRESS.md)
> for status, [docs/ISA.md](docs/ISA.md) for the instruction set and
> [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the design.

Silicon is a complete 8-bit computer designed from scratch out of NAND gates and
D flip-flops, simulated at gate level, and rendered in real-time 3D in the
browser.

## Build it locally

Requirements: Rust (stable, with the `wasm32-unknown-unknown` target),
`wasm-pack`, Node 20+, pnpm.

```sh
pnpm install
pnpm dev          # builds the WASM core, then starts the dev server
cargo test --workspace
pnpm test
pnpm bench        # speed of both engines in WebAssembly
```

## License

MIT
