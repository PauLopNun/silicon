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
if (fast < budget.fast || gate < budget.gate) process.exitCode = 1;
