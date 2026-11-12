import { describe, expect, it } from 'vitest';
import { breadcrumb, parentPath, wireLevel } from './protocol';

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
