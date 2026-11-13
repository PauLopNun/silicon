import { describe, expect, it } from 'vitest';
import {
  breadcrumb,
  decodeArch,
  flagsText,
  formatRate,
  hex,
  pairValue,
  parentPath,
  wireLevel,
} from './protocol';

describe('protocol helpers', () => {
  it('reads wire levels from a state buffer', () => {
    const wires = Uint8Array.of(0, 1, 0, 1);
    expect(wireLevel(wires, 1)).toBe(true);
    expect(wireLevel(wires, 2)).toBe(false);
  });

  it('rejects wires outside the buffer instead of reading garbage', () => {
    expect(() => wireLevel(Uint8Array.of(0, 1), 2)).toThrow(RangeError);
  });

  it('formats module paths as breadcrumbs', () => {
    expect(breadcrumb('computer/cpu/alu/adder8/fa3')).toBe('computer › cpu › alu › adder8 › fa3');
    expect(breadcrumb('')).toBe('');
  });

  it('finds the parent module of a path', () => {
    expect(parentPath('demo/xor/nand2')).toBe('demo/xor');
    expect(parentPath('demo')).toBe('');
  });
});

describe('register state', () => {
  // pc=0x1234 sp=0xfffe a..zh=1..8 flags=0b0101 ir=0x40 step=1 t=0xbeef
  const packed = Uint8Array.of(0x34, 0x12, 0xfe, 0xff, 1, 2, 3, 4, 5, 6, 7, 8, 0b0101, 0x40, 1, 0xef, 0xbe);

  it('unpacks the registers', () => {
    expect(decodeArch(packed)).toEqual({
      pc: 0x1234,
      sp: 0xfffe,
      regs: [1, 2, 3, 4, 5, 6, 7, 8],
      flags: 0b0101,
      ir: 0x40,
      step: 1,
      t: 0xbeef,
    });
  });

  it('rejects a buffer of the wrong length', () => {
    expect(() => decodeArch(new Uint8Array(16))).toThrow(RangeError);
  });

  it('reads pairs low byte first', () => {
    const { regs } = decodeArch(packed);
    expect(pairValue(regs, 0)).toBe(0x0201); // b:a
    expect(pairValue(regs, 3)).toBe(0x0807); // zh:zl
  });

  it('prints flags, upper case when set', () => {
    expect(flagsText(0)).toBe('zcnv');
    expect(flagsText(0b0101)).toBe('ZcNv');
    expect(flagsText(0b1111)).toBe('ZCNV');
  });

  it('formats numbers for the HUD', () => {
    expect(hex(0x2a, 4)).toBe('002a');
    expect(formatRate(4_000_000)).toBe('4.00 MHz');
    expect(formatRate(243_400)).toBe('243 kHz');
    expect(formatRate(12)).toBe('12 Hz');
  });
});
