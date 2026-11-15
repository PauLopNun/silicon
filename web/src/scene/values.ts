/**
 * What is written on the blocks that hold a value: the registers. The
 * values are the CPU's registers as the simulation reports them, which it
 * does on both engines; nothing here is worked out by the page.
 */
import { flagsText, hex, pairValue, type ArchState } from '../protocol';

/** For each block of the CPU that holds a value, by path: how to write it. */
export const VALUES: Readonly<Record<string, (s: ArchState) => string>> = {
  'cpu/pc': (s) => hex(s.pc, 4),
  'cpu/sp': (s) => hex(s.sp, 4),
  'cpu/t': (s) => hex(s.t, 4),
  'cpu/ir': (s) => hex(s.ir, 2),
  'cpu/step': (s) => String(s.step),
  'cpu/flags': (s) => flagsText(s.flags),
  'cpu/regs/a': (s) => hex(s.regs[0] ?? 0, 2),
  'cpu/regs/b': (s) => hex(s.regs[1] ?? 0, 2),
  'cpu/regs/xl': (s) => hex(s.regs[2] ?? 0, 2),
  'cpu/regs/xh': (s) => hex(s.regs[3] ?? 0, 2),
  'cpu/regs/yl': (s) => hex(s.regs[4] ?? 0, 2),
  'cpu/regs/yh': (s) => hex(s.regs[5] ?? 0, 2),
  'cpu/regs/zl': (s) => hex(s.regs[6] ?? 0, 2),
  'cpu/regs/zh': (s) => hex(s.regs[7] ?? 0, 2),
  // The register file as a whole shows its three 16-bit pairs.
  'cpu/regs': (s) => `x ${hex(pairValue(s.regs, 1), 4)} y ${hex(pairValue(s.regs, 2), 4)} z ${hex(pairValue(s.regs, 3), 4)}`,
};
