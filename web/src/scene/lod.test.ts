import { describe, expect, it } from 'vitest';
import { smallNetlist } from '../testing/nets';
import { eyeOf, focalLength, type Camera } from './camera';
import { layoutScene, rectOf } from './layout';
import { apparentSize, createLod, focusModule, OPEN_AT, OPENS_AT, smoothstep, updateLod } from './lod';
import { syntheticNetlist } from './synthetic';
import { buildTree, moduleAt } from './tree';

const viewport = { width: 1200, height: 800 };
const focal = focalLength(viewport);

/** A camera straight above the middle of a module. */
function above(layout: ReturnType<typeof layoutScene>, module: number, distance: number): Camera {
  const r = rectOf(layout, module);
  return { x: r.x + r.w / 2, y: r.y + r.h / 2, distance, azimuth: 0, elevation: Math.PI / 2 };
}

describe('updateLod', () => {
  const tree = buildTree(smallNetlist());
  const layout = layoutScene(tree);
  const lod = createLod(tree);
  const look = (camera: Camera) => {
    updateLod(tree, layout, eyeOf(camera), focal, lod);
    return lod;
  };

  it('shows one closed block from far away', () => {
    look(above(layout, 0, 1e6));
    expect(lod.open[0]).toBe(0);
    expect(lod.seen[0]).toBe(1);
    // Nothing inside a closed block is seen, and nothing in there is open.
    for (let m = 1; m < tree.count; m++) expect([lod.open[m], lod.seen[m]]).toEqual([0, 0]);
  });

  it('opens everything on the way down to a gate', () => {
    const fa0 = moduleAt(tree, 'cpu/alu/adder/fa0');
    look(above(layout, fa0, 3));
    for (const path of ['cpu', 'cpu/alu', 'cpu/alu/adder', 'cpu/alu/adder/fa0']) {
      expect(lod.open[moduleAt(tree, path)]).toBe(1);
      expect(lod.seen[moduleAt(tree, path)]).toBe(1);
    }
  });

  it('opens a module by its size in the picture', () => {
    const regs = moduleAt(tree, 'cpu/regs');
    const r = rectOf(layout, regs);
    const side = Math.sqrt(r.w * r.h);
    // Straight above the middle the nearest point of the block is its lid.
    const distanceFor = (pixels: number) => (focal * side) / pixels + layout.lidZ[regs]!;
    look(above(layout, regs, distanceFor(OPENS_AT * 0.99)));
    expect(lod.open[regs]).toBe(0);
    look(above(layout, regs, distanceFor((OPENS_AT + OPEN_AT) / 2)));
    expect(lod.open[regs]).toBeCloseTo(0.5, 6);
    look(above(layout, regs, distanceFor(OPEN_AT * 1.01)));
    expect(lod.open[regs]).toBe(1);
  });

  it('never shows more of a module than its parents let through', () => {
    for (const distance of [5, 20, 60, 150, 400, 1000, 5000]) {
      look(above(layout, moduleAt(tree, 'cpu/alu'), distance));
      for (let m = 1; m < tree.count; m++) {
        const parent = tree.parents[m]!;
        expect(lod.seen[m]).toBeCloseTo(lod.seen[parent]! * lod.open[parent]!, 6);
        expect(lod.seen[m]!).toBeLessThanOrEqual(lod.seen[parent]!);
      }
    }
  });

  it('is open when the eye is inside the block', () => {
    const r = rectOf(layout, 0);
    updateLod(tree, layout, [r.x + r.w / 2, r.y + r.h / 2, layout.lidZ[0]! / 2], focal, lod);
    expect(lod.open[0]).toBe(1);
    expect(apparentSize(layout, 0, [r.x + 1, r.y + 1, 0.1], focal)).toBe(Infinity);
  });
});

describe('the way in', () => {
  // The risk this retires: a tree several times deeper than the CPU's,
  // travelled from the whole thing down to one gate.
  const tree = buildTree(syntheticNetlist({ depth: 6, fanout: 4, gatesPerLeaf: 9 }));
  const layout = layoutScene(tree);
  const lod = createLod(tree);
  // The deepest first module: top/m0/m0/m0/m0/m0/m0.
  let target = 0;
  while (tree.children[target]!.length > 0) target = tree.children[target]![0]!;
  const top = rectOf(layout, 0);
  const far = Math.max(top.w, top.h) * 20;

  it('covers a wide range of scales', () => {
    const leaf = rectOf(layout, target);
    expect(tree.depth[target]).toBe(6);
    // A gate is one unit and its pins a tenth of that; the whole is
    // hundreds of gates across, six times the CPU.
    expect(Math.max(top.w, top.h)).toBeGreaterThan(900);
    expect(leaf.w).toBeLessThan(10);
  });

  it('changes a little when the camera moves a little: nothing pops', () => {
    const previous = createLod(tree);
    let largest = 0;
    // From far away to two units above a gate, 1% closer at each step.
    for (let distance = far; distance > 2; distance *= 0.99) {
      updateLod(tree, layout, eyeOf(above(layout, target, distance)), focal, lod);
      if (distance < far) {
        for (let m = 0; m < tree.count; m++) {
          largest = Math.max(largest, Math.abs(lod.open[m]! - previous.open[m]!));
          largest = Math.max(largest, Math.abs(lod.seen[m]! - previous.seen[m]!));
        }
      }
      previous.open.set(lod.open);
      previous.seen.set(lod.seen);
    }
    expect(largest).toBeLessThan(0.03);
    // And at the end of the way everything above the gate is open.
    for (let m = target; m >= 0; m = tree.parents[m]!) expect(lod.open[m]).toBe(1);
  });

  it('only ever opens modules on the way in, never closes one', () => {
    const previous = new Float32Array(tree.count);
    for (let distance = far; distance > 2; distance *= 0.9) {
      updateLod(tree, layout, eyeOf(above(layout, target, distance)), focal, lod);
      for (let m = 0; m < tree.count; m++) {
        // Straight down onto the target, every block on the way only gets nearer.
        if (m === target || isAbove(tree.parents, m, target)) {
          expect(lod.open[m]!).toBeGreaterThanOrEqual(previous[m]!);
        }
      }
      previous.set(lod.open);
    }
  });

  it('always has something to read: some closed block or open leaf in sight', () => {
    for (let distance = far; distance > 2; distance *= 0.8) {
      updateLod(tree, layout, eyeOf(above(layout, target, distance)), focal, lod);
      let readable = 0;
      for (let m = 0; m < tree.count; m++) {
        const closedBlock = lod.seen[m]! > 0.5 && lod.open[m]! < 0.5;
        const openLeaf = lod.seen[m]! > 0.5 && lod.open[m]! >= 0.5 && tree.children[m]!.length === 0;
        if (closedBlock || openLeaf) readable++;
      }
      expect(readable).toBeGreaterThan(0);
    }
  });

  it('names the module the camera is in', () => {
    const camera = above(layout, target, 4);
    updateLod(tree, layout, eyeOf(camera), focal, lod);
    expect(focusModule(tree, layout, lod, camera.x, camera.y)).toBe(target);
    updateLod(tree, layout, eyeOf(above(layout, target, far)), focal, lod);
    expect(focusModule(tree, layout, lod, camera.x, camera.y)).toBe(0);
    // Half way in, it is one of the modules on the way.
    updateLod(tree, layout, eyeOf(above(layout, target, 60)), focal, lod);
    const focus = focusModule(tree, layout, lod, camera.x, camera.y);
    expect(focus).not.toBe(0);
    expect(focus === target || isAbove(tree.parents, focus, target)).toBe(true);
  });
});

function isAbove(parents: Int32Array, ancestor: number, module: number): boolean {
  for (let m = parents[module]!; m >= 0; m = parents[m]!) if (m === ancestor) return true;
  return false;
}

describe('smoothstep', () => {
  it('goes from 0 to 1 without a jump at either end', () => {
    expect(smoothstep(10, 20, 5)).toBe(0);
    expect(smoothstep(10, 20, 10)).toBe(0);
    expect(smoothstep(10, 20, 15)).toBe(0.5);
    expect(smoothstep(10, 20, 20)).toBe(1);
    expect(smoothstep(10, 20, 99)).toBe(1);
    expect(smoothstep(10, 20, 10.01)).toBeLessThan(1e-5);
  });
});
