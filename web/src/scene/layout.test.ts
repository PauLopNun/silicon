import { describe, expect, it } from 'vitest';
import { expectProperLayout } from '../testing/checks';
import { smallNetlist, treeNetlist } from '../testing/nets';
import { gateAt, gridColumns, layoutScene, pack, rectOf } from './layout';
import { syntheticNetlist } from './synthetic';
import { buildTree, moduleAt } from './tree';

describe('layoutScene', () => {
  const tree = buildTree(smallNetlist());
  const layout = layoutScene(tree);

  it('nests the modules like the tree and gives every gate a cell', () => {
    expectProperLayout(tree, layout);
  });

  it('is the same every time', () => {
    expect(layoutScene(tree)).toEqual(layout);
  });

  it('raises the lid of a module above the lids inside it', () => {
    const z = (path: string) => layout.lidZ[moduleAt(tree, path)]!;
    expect(z('cpu')).toBeGreaterThan(z('cpu/alu'));
    expect(z('cpu/alu')).toBeGreaterThan(z('cpu/alu/adder'));
    expect(z('cpu/alu/adder')).toBeGreaterThan(z('cpu/alu/adder/fa0'));
    // Modules with nothing but gates inside are all as tall as each other.
    expect(z('cpu/regs')).toBe(z('cpu/alu/adder/fa1'));
  });

  it('lays a module with gates of its own beside the modules inside it', () => {
    // `top` holds a module and five gates of its own.
    const mixed = buildTree(treeNetlist([-1, 0], ['top', 'm'], [5, 7]));
    expectProperLayout(mixed, layoutScene(mixed));
  });

  it('finds the gate under a point, and none between or beyond the gates', () => {
    const regs = moduleAt(tree, 'cpu/regs');
    const gates = tree.gates[regs]!;
    const columns = gridColumns(gates.length);
    const [left, top] = [layout.grids[regs * 3]!, layout.grids[regs * 3 + 1]!];
    expect(layout.grids[regs * 3 + 2]).toBe(columns);
    expect(gateAt(tree, layout, regs, left + 0.01, top + 0.01)).toBe(gates[0]);
    expect(gateAt(tree, layout, regs, left + columns - 0.01, top + 0.99)).toBe(gates[columns - 1]);
    expect(gateAt(tree, layout, regs, left - 0.01, top + 0.5)).toBe(-1);
    expect(gateAt(tree, layout, regs, left + columns + 0.01, top + 0.5)).toBe(-1);
    expect(gateAt(tree, layout, regs, left + 0.5, top - 0.01)).toBe(-1);
    // The last row is not full: 61 gates do not fill a rectangle.
    const rows = Math.ceil(gates.length / columns);
    expect(gates.length % columns).not.toBe(0);
    expect(gateAt(tree, layout, regs, left + columns - 0.5, top + rows - 0.5)).toBe(-1);
    // A module with no gates of its own has none to find.
    expect(gateAt(tree, layout, 0, 1, 1)).toBe(-1);
  });

  it('holds for a tree far deeper and larger than the CPU', () => {
    const big = buildTree(syntheticNetlist({ depth: 5, fanout: 4, gatesPerLeaf: 9 }));
    expect(big.count).toBe(1 + 4 + 16 + 64 + 256 + 1024);
    const bigLayout = layoutScene(big);
    expectProperLayout(big, bigLayout);
    // Each level is a few times the size of the one below: no level is
    // lost in the margins of the one above.
    let module = 0;
    while (big.children[module]!.length > 0) {
      const child = big.children[module]![0]!;
      const ratio = rectOf(bigLayout, module).w / rectOf(bigLayout, child).w;
      expect(ratio).toBeGreaterThan(1.5);
      expect(ratio).toBeLessThan(6);
      module = child;
    }
  });
});

describe('layout hints', () => {
  const tree = buildTree(smallNetlist());
  const at = (layout: ReturnType<typeof layoutScene>, path: string) => rectOf(layout, moduleAt(tree, path));

  it('put modules side by side in a row and one under another in a column', () => {
    const layout = layoutScene(tree, { cpu: { row: ['pc', { column: ['regs', 'alu'] }] } });
    expectProperLayout(tree, layout);
    const [pc, regs, alu] = [at(layout, 'cpu/pc'), at(layout, 'cpu/regs'), at(layout, 'cpu/alu')];
    expect(pc.x + pc.w).toBeLessThan(regs.x);
    expect(pc.y).toBe(regs.y);
    expect(alu.x).toBe(regs.x);
    expect(alu.y).toBeGreaterThan(regs.y + regs.h);
  });

  it('never lose a module: what a hint leaves out goes underneath', () => {
    const layout = layoutScene(tree, { cpu: { row: ['regs', 'pc'] } });
    expectProperLayout(tree, layout);
    const [regs, pc, alu] = [at(layout, 'cpu/regs'), at(layout, 'cpu/pc'), at(layout, 'cpu/alu')];
    expect(alu.y).toBeGreaterThan(Math.max(regs.y + regs.h, pc.y + pc.h));
  });

  it('must name modules that are there, once', () => {
    expect(() => layoutScene(tree, { cpu: { row: ['regs', 'nope'] } })).toThrow(/names nope/);
    expect(() => layoutScene(tree, { cpu: { row: ['regs', 'regs'] } })).toThrow(/twice/);
  });

  it('for a module that is not in the tree change nothing', () => {
    expect(layoutScene(tree, { 'cpu/absent': 'x' })).toEqual(layoutScene(tree));
  });
});

describe('pack', () => {
  it('keeps equal items in reading order, in a rectangle that is not a strip', () => {
    for (const [count, columns, rows] of [
      [16, 4, 4],
      [15, 5, 3],
      [8, 4, 2],
      [3, 3, 1],
    ] as const) {
      const sizes = Array.from({ length: count }, () => ({ w: 4, h: 4 }));
      const placed = pack(sizes, 1);
      // Left to right, then down.
      sizes.forEach((_, i) => {
        expect([placed.x[i], placed.y[i]]).toEqual([(i % columns) * 5, Math.floor(i / columns) * 5]);
      });
      expect([placed.w, placed.h]).toEqual([columns * 5 - 1, rows * 5 - 1]);
    }
  });

  it('stands small items in a column beside a tall one instead of leaving a hole', () => {
    // Three small blocks and one as tall as the three together.
    const sizes = [
      { w: 10, h: 6 },
      { w: 10, h: 6 },
      { w: 10, h: 6 },
      { w: 12, h: 20 },
    ];
    const placed = pack(sizes, 1);
    expect(placed.x).toEqual([0, 0, 0, 11]);
    expect(placed.y).toEqual([0, 7, 14, 0]);
    expect([placed.w, placed.h]).toEqual([23, 20]);
  });

  it('never puts one item on another', () => {
    const sizes = [3, 9, 4, 14, 2, 6, 6, 11, 5].map((w, i) => ({ w, h: ((i * 7) % 5) + 2 }));
    const placed = pack(sizes, 1);
    sizes.forEach((a, i) => {
      expect(placed.x[i]! + a.w).toBeLessThanOrEqual(placed.w);
      expect(placed.y[i]! + a.h).toBeLessThanOrEqual(placed.h);
      sizes.slice(i + 1).forEach((b, k) => {
        const j = i + 1 + k;
        const clear =
          placed.x[i]! + a.w + 1 <= placed.x[j]! ||
          placed.x[j]! + b.w + 1 <= placed.x[i]! ||
          placed.y[i]! + a.h + 1 <= placed.y[j]! ||
          placed.y[j]! + b.h + 1 <= placed.y[i]!;
        expect(clear).toBe(true);
      });
    });
  });

  it('places a single item at the corner and nothing at all in no space', () => {
    expect(pack([{ w: 3, h: 2 }], 1)).toEqual({ x: [0], y: [0], w: 3, h: 2 });
    expect(pack([], 1)).toEqual({ x: [], y: [], w: 0, h: 0 });
  });
});
