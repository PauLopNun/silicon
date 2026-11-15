/**
 * Messages exchanged between the main thread (renderer, UI) and the
 * simulation worker. This file is the single definition of that contract;
 * see docs/ARCHITECTURE.md §5.
 */
import type { ProgramInfo } from './programs';

/** Which engine is running the CPU. */
export type EngineKind = 'gate' | 'fast';

/** Numbers per gate in {@link NetlistInfo.gates}. */
export const GATE_FIELDS = 5;

/** Static description of the CPU netlist, sent once when the worker is ready. */
export interface NetlistInfo {
  /** Per gate: kind (0 = NAND, 1 = DFF), input wire a, input wire b, output wire, module index. */
  gates: Uint32Array;
  /** Parent of each module; the top module's parent is -1. */
  moduleParents: Int32Array;
  moduleNames: string[];
  /** Length of the wire-level buffer. */
  wireCount: number;
}

export type ToWorker =
  | { type: 'run'; running: boolean }
  /** Run exactly this many clock cycles (used while paused). */
  | { type: 'step'; cycles: number }
  /** Run to the start of the next instruction (used while paused). */
  | { type: 'step-instruction' }
  | { type: 'engine'; engine: EngineKind }
  /** Target clock rate in cycles per second; `null` means as fast as possible. */
  | { type: 'clock'; hz: number | null }
  /** Keys held down, as the bits of the machine's KEYS register. */
  | { type: 'keys'; keys: number }
  | { type: 'reset' }
  /**
   * Put this program in ROM and start it from power-on. A name that is not in
   * the list of programs is ignored: the computer keeps the one it has.
   */
  | { type: 'program'; name: string };

/** Everything the page shows, sent at most once per display frame. */
export interface FrameState {
  type: 'frame';
  /** Name of the program in ROM. */
  program: string;
  /**
   * How many times the computer has been started: once at first, and once
   * more for every reset and every program loaded. When it changes, what was
   * shown of the run before (the console) is stale.
   */
  boot: number;
  engine: EngineKind;
  running: boolean;
  halted: boolean;
  /** Clock cycles since reset. */
  cycles: number;
  /** Frames the program has declared finished since reset. */
  frames: number;
  /** Screen memory: 2048 bytes, four 2-bit pixels each. */
  framebuffer: Uint8Array;
  /**
   * One byte per wire of the CPU, 0 or 1: its level at the instant of this
   * message. Live only on the gate engine; on the fast engine the wires
   * stay as the gate engine left them.
   */
  wires: Uint8Array;
  /**
   * One byte per wire: how busy it has been since the frame message before
   * this one. 0 if it did not change at all, otherwise 1 to 255 on a
   * square-root scale of changes per clock cycle (255: it changed in every
   * cycle). At hundreds of thousands of cycles a second the levels are a
   * sample; this is everything that happened in between. All zero on the
   * fast engine.
   */
  activity: Uint8Array;
  /** CPU registers, packed; see {@link decodeArch}. */
  arch: Uint8Array;
  /** Bytes the program wrote on the console since the frame message before this one. */
  console: Uint8Array;
  /**
   * While paused: what the CPU does in the cycle it is about to run, as the
   * core writes it (see `explain.ts`). Empty while running.
   */
  explain: string;
}

export type FromWorker =
  | ({ type: 'ready'; version: string; engine: EngineKind; programs: ProgramInfo[] } & NetlistInfo)
  | FrameState
  | { type: 'error'; message: string };

/** The CPU's registers. */
export interface ArchState {
  pc: number;
  sp: number;
  /** `a b xl xh yl yh zl zh` */
  regs: number[];
  flags: number;
  ir: number;
  step: number;
  t: number;
}

/** Length of the packed register state. */
export const ARCH_STATE_BYTES = 17;

/** Unpack the register state: pc, sp (16-bit, low byte first), 8 registers, flags, ir, step, t. */
export function decodeArch(bytes: Uint8Array): ArchState {
  if (bytes.length !== ARCH_STATE_BYTES) {
    throw new RangeError(`register state is ${bytes.length} bytes, expected ${ARCH_STATE_BYTES}`);
  }
  const b = (i: number) => bytes[i] ?? 0;
  const word = (i: number) => b(i) | (b(i + 1) << 8);
  return {
    pc: word(0),
    sp: word(2),
    regs: Array.from(bytes.subarray(4, 12)),
    flags: b(12),
    ir: b(13),
    step: b(14),
    t: word(15),
  };
}

/** A 16-bit register pair: 0 = `ba`, 1 = `x`, 2 = `y`, 3 = `z`. */
export function pairValue(regs: readonly number[], pair: number): number {
  return (regs[2 * pair] ?? 0) | ((regs[2 * pair + 1] ?? 0) << 8);
}

/** Flags as four letters, upper case when set: bit 0 = Z, 1 = C, 2 = N, 3 = V. */
export function flagsText(flags: number): string {
  return ['z', 'c', 'n', 'v'].map((f, i) => ((flags >> i) & 1 ? f.toUpperCase() : f)).join('');
}

/** `0x1f` → `"1f"`, padded to `digits`. */
export function hex(value: number, digits: number): string {
  return value.toString(16).padStart(digits, '0');
}

/** A clock rate for people: `4.00 MHz`, `243 kHz`, `12 Hz`. */
export function formatRate(hz: number): string {
  if (hz >= 1e6) return `${(hz / 1e6).toFixed(2)} MHz`;
  if (hz >= 1e3) return `${Math.round(hz / 1e3)} kHz`;
  return `${Math.round(hz)} Hz`;
}

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
