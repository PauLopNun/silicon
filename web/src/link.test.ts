import { describe, expect, it } from 'vitest';
import { searchWith } from './link';

describe('searchWith', () => {
  it('sets, replaces and removes one thing and keeps the rest', () => {
    expect(searchWith('', 'at', 'cpu/alu')).toBe('?at=cpu/alu');
    expect(searchWith('?program=bounce', 'at', 'cpu/alu/adder8/fa3')).toBe('?program=bounce&at=cpu/alu/adder8/fa3');
    expect(searchWith('?at=cpu&engine=gate', 'at', 'cpu/regs')).toBe('?at=cpu/regs&engine=gate');
    expect(searchWith('?at=cpu&engine=gate', 'at', undefined)).toBe('?engine=gate');
    expect(searchWith('?at=cpu', 'at', undefined)).toBe('');
    expect(searchWith('', 'at', undefined)).toBe('');
  });

  it('reads back what it wrote', () => {
    const search = searchWith('?program=bounce', 'at', 'cpu/alu/adder8/fa3/nand2');
    expect(new URLSearchParams(search).get('at')).toBe('cpu/alu/adder8/fa3/nand2');
    expect(new URLSearchParams(search).get('program')).toBe('bounce');
  });
});
