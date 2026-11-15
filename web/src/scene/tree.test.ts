import { describe, expect, it } from 'vitest';
import { smallNetlist, treeNetlist } from '../testing/nets';
import { buildTree, commonAncestor, gatePath, isWithin, moduleAt, resolvePath } from './tree';

describe('buildTree', () => {
  const tree = buildTree(smallNetlist());

  it('knows who is inside whom, in the order of declaration', () => {
    expect(tree.count).toBe(8);
    expect(tree.children).toEqual([[1, 6, 7], [2, 5], [3, 4], [], [], [], [], []]);
    expect(Array.from(tree.depth)).toEqual([0, 1, 2, 3, 3, 2, 1, 1]);
    expect(Array.from(tree.height)).toEqual([3, 2, 1, 0, 0, 0, 0, 0]);
  });

  it('gives every module its path', () => {
    expect(tree.paths).toEqual([
      'cpu',
      'cpu/alu',
      'cpu/alu/adder',
      'cpu/alu/adder/fa0',
      'cpu/alu/adder/fa1',
      'cpu/alu/logic',
      'cpu/regs',
      'cpu/pc',
    ]);
    expect(moduleAt(tree, 'cpu/alu/logic')).toBe(5);
    expect(moduleAt(tree, 'cpu/nothing')).toBe(-1);
  });

  it('groups the gates by module and counts those below', () => {
    expect(tree.gateCount).toBe(119);
    expect(tree.gates.map((g) => g.length)).toEqual([0, 0, 0, 9, 9, 24, 61, 16]);
    expect(Array.from(tree.totalGates)).toEqual([119, 42, 18, 9, 9, 24, 61, 16]);
    tree.gates.forEach((gates, module) => {
      for (const gate of gates) expect(tree.moduleOfGate[gate]).toBe(module);
      // In netlist order.
      expect(Array.from(gates)).toEqual(Array.from(gates).sort((a, b) => a - b));
    });
  });

  it('refuses a tree whose modules do not hang from earlier ones', () => {
    expect(() => buildTree(treeNetlist([-1, 2, 0], ['a', 'b', 'c'], [0, 0, 0]))).toThrow(/earlier module/);
    expect(() => buildTree(treeNetlist([0, 0], ['a', 'b'], [0, 0]))).toThrow(/earlier module/);
  });
});

describe('gate paths', () => {
  // Two NANDs, a flip-flop and another NAND in `top/m`, one flip-flop in `top`.
  const tree = buildTree({
    gates: Uint32Array.from([0, 0, 1, 2, 1, 0, 0, 1, 3, 1, 1, 2, 0, 4, 1, 0, 3, 4, 5, 1, 1, 5, 0, 6, 0]),
    moduleParents: Int32Array.from([-1, 0]),
    moduleNames: ['top', 'm'],
    wireCount: 7,
  });

  it('number the gates of each kind within their module', () => {
    expect([0, 1, 2, 3, 4].map((g) => gatePath(tree, g))).toEqual([
      'top/m/nand0',
      'top/m/nand1',
      'top/m/dff0',
      'top/m/nand2',
      'top/dff0',
    ]);
  });

  it('lead back to the gate or the module they name', () => {
    for (let g = 0; g < tree.gateCount; g++) {
      expect(resolvePath(tree, gatePath(tree, g))).toEqual({ module: tree.moduleOfGate[g], gate: g });
    }
    expect(resolvePath(tree, 'top/m')).toEqual({ module: 1, gate: -1 });
    expect(resolvePath(tree, 'top/m/nand3')).toBeUndefined();
    expect(resolvePath(tree, 'top/x/nand0')).toBeUndefined();
    expect(resolvePath(tree, '')).toBeUndefined();
  });
});

describe('ancestors', () => {
  const tree = buildTree(smallNetlist());

  it('finds the lowest module that holds two others', () => {
    expect(commonAncestor(tree, 3, 4)).toBe(2);
    expect(commonAncestor(tree, 3, 5)).toBe(1);
    expect(commonAncestor(tree, 4, 6)).toBe(0);
    expect(commonAncestor(tree, 2, 3)).toBe(2);
    expect(commonAncestor(tree, 7, 7)).toBe(7);
  });

  it('tells whether a module is inside another', () => {
    expect(isWithin(tree, 4, 1)).toBe(true);
    expect(isWithin(tree, 4, 4)).toBe(true);
    expect(isWithin(tree, 4, 0)).toBe(true);
    expect(isWithin(tree, 5, 2)).toBe(false);
    expect(isWithin(tree, 1, 4)).toBe(false);
  });
});
