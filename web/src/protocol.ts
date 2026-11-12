/**
 * Messages exchanged between the main thread (renderer, UI) and the
 * simulation worker. This file is the single definition of that contract;
 * see docs/ARCHITECTURE.md §5.
 */

/** Which engine produced the state being shown. */
export type EngineKind = 'gate' | 'fast';

/** Static description of one gate, sent once when the worker is ready. */
export interface GateInfo {
  /** Hierarchical path, e.g. `demo/xor/nand2`. */
  path: string;
  /** Wire indices into the state buffer. */
  a: number;
  b: number;
  out: number;
}

export type ToWorker = { type: 'setInputs'; a: boolean; b: boolean };

export type FromWorker =
  | { type: 'ready'; version: string; engine: EngineKind; gates: GateInfo[] }
  | { type: 'state'; engine: EngineKind; wires: Uint8Array }
  | { type: 'error'; message: string };

/** Logic level of wire `w` in a state buffer (one byte per wire). */
export function wireLevel(wires: Uint8Array, w: number): boolean {
  const v = wires[w];
  if (v === undefined) throw new RangeError(`wire ${w} out of range (${wires.length} wires)`);
  return v !== 0;
}

/** `cpu/alu/adder8` → `cpu › alu › adder8`, as shown in the breadcrumb bar. */
export function breadcrumb(path: string): string {
  return path.split('/').filter(Boolean).join(' › ');
}

/** Parent module of a gate or module path (`a/b/nand0` → `a/b`). */
export function parentPath(path: string): string {
  const i = path.lastIndexOf('/');
  return i < 0 ? '' : path.slice(0, i);
}
