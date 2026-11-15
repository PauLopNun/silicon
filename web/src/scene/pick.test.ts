import { describe, expect, it } from 'vitest';
import { smallNetlist } from '../testing/nets';
import type { Vec3 } from './camera';
import { layoutScene, rectOf } from './layout';
import { createLod } from './lod';
import { pick, rayBox } from './pick';
import { buildTree, moduleAt } from './tree';

const tree = buildTree(smallNetlist());
const layout = layoutScene(tree);
const DOWN: Vec3 = [0, 0, -1];

/** Levels of detail with exactly these modules open. */
function opened(...paths: string[]) {
  const lod = createLod(tree);
  lod.seen.fill(1);
  for (const path of paths) lod.open[moduleAt(tree, path)] = 1;
  return lod;
}

const centre = (path: string): [number, number] => {
  const r = rectOf(layout, moduleAt(tree, path));
  return [r.x + r.w / 2, r.y + r.h / 2];
};

describe('pick', () => {
  it('stops at a closed block', () => {
    const [x, y] = centre('cpu/regs');
    expect(pick(tree, layout, opened(), [x, y, 500], DOWN)).toMatchObject({ module: 0, gate: -1 });
    expect(pick(tree, layout, opened('cpu'), [x, y, 500], DOWN)).toMatchObject({
      module: moduleAt(tree, 'cpu/regs'),
      gate: -1,
    });
  });

  it('goes through open modules down to the gate under the pointer', () => {
    const lod = opened('cpu', 'cpu/alu', 'cpu/alu/adder', 'cpu/alu/adder/fa1', 'cpu/regs');
    for (const path of ['cpu/alu/adder/fa1', 'cpu/regs']) {
      const module = moduleAt(tree, path);
      for (const gate of tree.gates[module]!) {
        const origin: Vec3 = [layout.gates[gate * 2]!, layout.gates[gate * 2 + 1]!, 50];
        expect(pick(tree, layout, lod, origin, DOWN)).toMatchObject({ module, gate });
      }
    }
    // The other full adder is still closed: its gates are not to be had.
    const fa0 = moduleAt(tree, 'cpu/alu/adder/fa0');
    const gate = tree.gates[fa0]![0]!;
    expect(pick(tree, layout, lod, [layout.gates[gate * 2]!, layout.gates[gate * 2 + 1]!, 50], DOWN)).toMatchObject({
      module: fa0,
      gate: -1,
    });
  });

  it('finds the floor of the deepest open module where there is no gate', () => {
    const lod = opened('cpu', 'cpu/alu');
    const alu = rectOf(layout, moduleAt(tree, 'cpu/alu'));
    // The title strip of the ALU: inside it, above everything it holds.
    const hit = pick(tree, layout, lod, [alu.x + alu.w / 2, alu.y + layout.headers[1]! / 2, 50], DOWN);
    expect(hit).toMatchObject({ module: moduleAt(tree, 'cpu/alu'), gate: -1 });
    expect(hit!.x).toBeCloseTo(alu.x + alu.w / 2, 9);
  });

  it('sees the side of a closed block before the floor behind it', () => {
    const lod = opened('cpu');
    const regs = moduleAt(tree, 'cpu/regs');
    const r = rectOf(layout, regs);
    // From the west, low, towards a point of the floor east of the block.
    const origin: Vec3 = [r.x - 20, r.y + r.h / 2, 2];
    const target: Vec3 = [r.x + r.w + 3, r.y + r.h / 2, 0];
    const direction: Vec3 = [target[0] - origin[0], 0, target[2] - origin[2]];
    const hit = pick(tree, layout, lod, origin, direction);
    expect(hit).toMatchObject({ module: regs, gate: -1 });
    // It stopped at the wall, not at the floor.
    expect(hit!.x).toBeCloseTo(r.x, 6);
  });

  it('finds nothing beside the scene or looking at the sky', () => {
    const lod = opened('cpu');
    expect(pick(tree, layout, lod, [-10, -10, 50], DOWN)).toBeUndefined();
    const [x, y] = centre('cpu');
    expect(pick(tree, layout, lod, [x, y, 50], [0, 0, 1])).toBeUndefined();
  });
});

describe('rayBox', () => {
  const min: Vec3 = [0, 0, 0];
  const max: Vec3 = [10, 10, 2];

  it('says where the ray enters', () => {
    expect(rayBox([5, 5, 12], [0, 0, -1], min, max)).toBe(10);
    expect(rayBox([-5, 5, 1], [1, 0, 0], min, max)).toBe(5);
    expect(rayBox([-5, 5, 1], [2, 0, 0], min, max)).toBe(2.5);
  });

  it('misses beside it, behind it and along it', () => {
    expect(rayBox([11, 5, 12], [0, 0, -1], min, max)).toBe(Infinity);
    expect(rayBox([5, 5, 12], [0, 0, 1], min, max)).toBe(Infinity);
    expect(rayBox([-5, 11, 1], [1, 0, 0], min, max)).toBe(Infinity);
  });

  it('is zero from inside', () => {
    expect(rayBox([5, 5, 1], [1, 1, 0], min, max)).toBe(0);
  });
});
