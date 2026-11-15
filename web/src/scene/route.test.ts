import { describe, expect, it } from 'vitest';
import { expectProperRouting } from '../testing/checks';
import type { NetlistInfo } from '../protocol';
import { BRANCH_RISE, GATE_TOP, layoutScene } from './layout';
import { PIN_X, PIN_Y, routeWires } from './route';
import { syntheticNetlist } from './synthetic';
import { buildTree } from './tree';

/**
 * ```
 * top ─┬─ a:  g0 = nand(0, 1) → w2     g1 = nand(w2, w2) → w3
 *      └─ b:  g2 = dff(w3) → w4        g3 = nand(w4, w2) → w5
 * ```
 * w2 joins three gates across both modules; w3 crosses from a to b; w4
 * stays inside b; w5 goes nowhere.
 */
const net: NetlistInfo = {
  gates: Uint32Array.from([0, 0, 1, 2, 1, 0, 2, 2, 3, 1, 1, 3, 0, 4, 2, 0, 4, 2, 5, 2]),
  moduleParents: Int32Array.from([-1, 0, 0]),
  moduleNames: ['top', 'a', 'b'],
  wireCount: 6,
};
const tree = buildTree(net);
const layout = layoutScene(tree);
const routing = routeWires(net, tree, layout);

const segmentsOf = (wire: number) =>
  Array.from({ length: routing.count }, (_, s) => s).filter((s) => routing.wire[s] === wire);

describe('routeWires', () => {
  it('draws every wire that joins gates, connected, down to every pin', () => {
    expectProperRouting(net, tree, layout, routing);
  });

  it('draws nothing for the constants or for a wire that goes nowhere', () => {
    for (const wire of [0, 1, 5]) expect(segmentsOf(wire)).toEqual([]);
  });

  it('gives a wire to the lowest module that holds all its gates', () => {
    const home = (wire: number) => new Set(segmentsOf(wire).map((s) => routing.module[s]));
    expect(home(2)).toEqual(new Set([0]));
    expect(home(3)).toEqual(new Set([0]));
    expect(home(4)).toEqual(new Set([2]));
  });

  it('runs a wire at the height of its module: trunk east–west, branches above it north–south', () => {
    for (const wire of [2, 3, 4]) {
      const module = routing.module[segmentsOf(wire)[0]!]!;
      const trunkZ = layout.wireZ[module]!;
      let trunks = 0;
      for (const s of segmentsOf(wire)) {
        const [x0, y0, z0, x1, y1, z1] = routing.ends.subarray(s * 6, s * 6 + 6);
        if (x0 !== x1) {
          trunks++;
          expect([z0, z1]).toEqual([trunkZ, trunkZ]);
        } else if (y0 !== y1) {
          expect([z0, z1]).toEqual([trunkZ + BRANCH_RISE, trunkZ + BRANCH_RISE]);
        } else {
          // A via: straight up from a pin, or between the two layers.
          expect(Math.min(z0!, z1!)).toBeGreaterThanOrEqual(GATE_TOP);
        }
      }
      expect(trunks).toBe(1);
    }
    // Wires between the modules pass above the lids of both.
    expect(layout.wireZ[0]!).toBeGreaterThan(Math.max(layout.lidZ[1]!, layout.lidZ[2]!));
  });

  it('comes down on the right pins: output on the right of a gate, inputs on its left', () => {
    const feet = (wire: number) =>
      segmentsOf(wire)
        .map((s) => routing.ends.subarray(s * 6, s * 6 + 6))
        .filter((e) => e[2] === GATE_TOP)
        .map((e) => [e[0]!, e[1]!] as const);
    const gate = (g: number) => [layout.gates[g * 2]!, layout.gates[g * 2 + 1]!] as const;
    // w2: out of g0, into both inputs of g1 and the second input of g3.
    const w2 = feet(2);
    expect(w2.length).toBe(4);
    const near = (x: number, y: number) => w2.some(([fx, fy]) => Math.abs(fx - x) < 0.1 && Math.abs(fy - y) < 1e-9);
    expect(near(gate(0)[0] + PIN_X, gate(0)[1])).toBe(true);
    expect(near(gate(1)[0] - PIN_X, gate(1)[1] - PIN_Y)).toBe(true);
    expect(near(gate(1)[0] - PIN_X, gate(1)[1] + PIN_Y)).toBe(true);
    expect(near(gate(3)[0] - PIN_X, gate(3)[1] + PIN_Y)).toBe(true);
    // w3 goes into the flip-flop, which has one input in the middle.
    const w3 = feet(3);
    expect(w3.some(([fx, fy]) => Math.abs(fx - (gate(2)[0] - PIN_X)) < 0.1 && fy === gate(2)[1])).toBe(true);
  });

  it('is the same every time', () => {
    expect(routeWires(net, tree, layout)).toEqual(routing);
  });

  it('holds for thousands of gates with wires at every level of a deep tree', () => {
    const big = syntheticNetlist({ depth: 4, fanout: 3, gatesPerLeaf: 12, seed: 7 });
    const bigTree = buildTree(big);
    const bigLayout = layoutScene(bigTree);
    const bigRouting = routeWires(big, bigTree, bigLayout);
    expectProperRouting(big, bigTree, bigLayout, bigRouting);
    // There are wires that belong to modules of every height.
    const heights = new Set(Array.from(bigRouting.module, (m) => bigTree.height[m]));
    expect(heights).toEqual(new Set([0, 1, 2, 3, 4]));
  });
});
