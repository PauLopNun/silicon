# ADR 0004 — How simulation state reaches the renderer

**Status:** accepted (M0)

## Context
The simulation runs in a Web Worker; the renderer needs the framebuffer and
all wire states every frame. Truly shared WASM memory needs a threads-enabled
WASM build and cross-origin isolation headers, which GitHub Pages cannot set
directly.

## Decision
- WASM memory is private to the worker. The worker reads state through typed
  array views into it (no per-wire calls).
- Per frame, state crosses to the main thread in bulk:
  - through a JS-allocated `SharedArrayBuffer` when `crossOriginIsolated` is
    true (enabled on Pages with a small service worker that adds COOP/COEP);
  - otherwise as a single transferred `ArrayBuffer` copy.
- The renderer consumes the buffers as data textures, so cost does not depend
  on the number of gates drawn.

## Why
- Total state is ~25 kB per frame; one copy is far below the frame budget, so
  the fallback is not a degraded mode, only a slightly less elegant one.
- Avoids a nightly/threads Rust toolchain and keeps the site working in every
  browser and inside iframes.

## Consequences
- Both paths are covered by tests; the debug overlay shows which one is active.
