// Speed of both engines in WebAssembly, measured in Node (same V8 as Chrome).
//
//   pnpm wasm && pnpm bench
//
// Runs the built-in demo program and reports clock cycles per second, to be
// compared with the budgets in docs/ARCHITECTURE.md §7.
import { readFileSync } from 'node:fs';
import { initSync, Machine } from '../src/wasm-pkg/silicon_wasm.js';

const wasm = new URL('../src/wasm-pkg/silicon_wasm_bg.wasm', import.meta.url);
initSync({ module: readFileSync(wasm) });

/** Best of several runs, in cycles per second. */
function measure(machine, cyclesPerRun, runs) {
  let best = 0;
  for (let i = 0; i < runs; i++) {
    const start = performance.now();
    machine.run(cyclesPerRun);
    best = Math.max(best, cyclesPerRun / ((performance.now() - start) / 1000));
  }
  return best;
}

const machine = Machine.demo();
const fast = measure(machine, 20_000_000, 5);
machine.use_gate_engine(true);
const gate = measure(machine, 200_000, 5);
const gates = machine.gate_table().length / 5;

const budget = { fast: 5_000_000, gate: 50_000 };
const row = (name, rate, min) =>
  `${name}  ${(rate / 1e6).toFixed(2).padStart(8)} M cycles/s   budget ${(min / 1e6).toFixed(2)} M   ${
    rate >= min ? 'ok' : 'BELOW BUDGET'
  }`;
console.log(row('fast', fast, budget.fast));
console.log(row('gate', gate, budget.gate));
console.log(`(${gates} gates, ${machine.wires_len()} wires, node ${process.version})`);

// The game, built from its source by the toolchain inside the WASM core:
// what a frame costs while walking and turning, and so how many frames a
// second the player sees at the nominal clock and on the gates.
const NOMINAL_HZ = 4_000_000;
const MIN_FPS = 10;
const start = performance.now();
const game = Machine.running('maze3d');
const buildMs = performance.now() - start;
game.set_keys(0b1001); // up and right
game.run(1_000_000); // past the digging of the first maze
const [cycles0, frames0] = [game.cycles(), game.frames()];
game.run(20_000_000);
const perFrame = (game.cycles() - cycles0) / (game.frames() - frames0);
const nominalFps = NOMINAL_HZ / perFrame;
console.log(
  `maze3d  ${Math.round(perFrame).toLocaleString('en-US')} cycles per frame: ` +
    `${nominalFps.toFixed(1)} frames/s at 4 MHz   budget ${MIN_FPS}   ${nominalFps >= MIN_FPS ? 'ok' : 'BELOW BUDGET'}`,
);
console.log(
  `        ${(gate / perFrame).toFixed(1)} frames/s on the gate engine; built from source in ${buildMs.toFixed(0)} ms`,
);
if (fast < budget.fast || gate < budget.gate || nominalFps < MIN_FPS) process.exitCode = 1;
