/**
 * The real simulation core, for tests that must be about the real CPU and
 * not about a stand-in: its module tree, its gates, its wires.
 *
 * Only tests import this. On the page the core lives in the worker and
 * nothing else touches it. Needs `pnpm wasm` to have been run.
 */
import { readFileSync } from 'node:fs';
import type { NetlistInfo } from '../protocol';
import { initSync, Machine, RamExhibit } from '../wasm-pkg/silicon_wasm.js';

let loaded = false;

/** A computer running the built-in demo program. */
export function demoMachine(): Machine {
  if (!loaded) {
    initSync({ module: readFileSync(new URL('../wasm-pkg/silicon_wasm_bg.wasm', import.meta.url)) });
    loaded = true;
  }
  return Machine.demo();
}

/** The memory made of gates' netlist, as the worker sends it to the page. */
export function ramNetlist(): NetlistInfo {
  demoMachine();
  const ram = new RamExhibit();
  return {
    gates: ram.gate_table(),
    levels: ram.gate_levels(),
    moduleParents: ram.module_parents(),
    moduleNames: ram.module_names().split('\n'),
    wireCount: ram.wires_len(),
  };
}

/** The CPU's netlist, as the worker sends it to the page. */
export function netlistOf(machine: Machine): NetlistInfo {
  return {
    gates: machine.gate_table(),
    levels: machine.gate_levels(),
    moduleParents: machine.module_parents(),
    moduleNames: machine.module_names().split('\n'),
    wireCount: machine.wires_len(),
  };
}
