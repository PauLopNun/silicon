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
  levels: Uint32Array.from([1, 2, 0, 2]),
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
      // The trunk is one straight line, in one piece or in two that meet
      // where the driver joins it.
      const trunk: [number, number][] = [];
      const rows = new Set<number>();
      for (const s of segmentsOf(wire)) {
        const [x0, y0, z0, x1, y1, z1] = routing.ends.subarray(s * 6, s * 6 + 6);
        if (x0 !== x1) {
          trunk.push([Math.min(x0!, x1!), Math.max(x0!, x1!)]);
          rows.add(y0!);
          expect([y0, z0, z1]).toEqual([y1, trunkZ, trunkZ]);
        } else if (y0 !== y1) {
          expect([z0, z1]).toEqual([trunkZ + BRANCH_RISE, trunkZ + BRANCH_RISE]);
        } else {
          // A via: straight up from a pin, or between the two layers.
          expect(Math.min(z0!, z1!)).toBeGreaterThanOrEqual(GATE_TOP);
        }
      }
      expect(rows.size).toBe(1);
      trunk.sort((p, q) => p[0] - q[0]);
      expect([1, 2]).toContain(trunk.length);
      if (trunk.length === 2) expect(trunk[0]![1]).toBe(trunk[1]![0]);
    }
    // Wires between the modules pass above the lids of both.
    expect(layout.wireZ[0]!).toBeGreaterThan(Math.max(layout.lidZ[1]!, layout.lidZ[2]!));
  });

  it('comes down on the right pins: output on the right of a gate, inputs on its left', () => {
    // Where the wire touches the gates: either end of a segment, since a
    // segment runs the way the signal goes (out of a pin, or into one).
    const feet = (wire: number) =>
      segmentsOf(wire)
        .map((s) => routing.ends.subarray(s * 6, s * 6 + 6))
        .flatMap((e) => [
          [e[0]!, e[1]!, e[2]!],
          [e[3]!, e[4]!, e[5]!],
        ])
        .filter(([, , z]) => z === GATE_TOP)
        .map(([x, y]) => [x!, y!] as const);
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

  it('knows the way along a wire: out of the gate that drives it, into those that read it', () => {
    // w4 goes from the flip-flop g2 to the first input of g3, in the same module.
    const segments = segmentsOf(4);
    const distances = segments.flatMap((s) => [routing.along[s * 2]!, routing.along[s * 2 + 1]!]);
    expect(Math.min(...distances)).toBe(0);
    // The farthest point is the pin of g3, and the way there is the sum of the segments.
    const total = segments.reduce((sum, s) => {
      const e = routing.ends.subarray(s * 6, s * 6 + 6);
      return sum + Math.abs(e[0]! - e[3]!) + Math.abs(e[1]! - e[4]!) + Math.abs(e[2]! - e[5]!);
    }, 0);
    expect(Math.max(...distances)).toBeCloseTo(total, 4);
    for (const s of segments) expect(routing.reach[s]).toBeCloseTo(total, 4);
    // With several readers the wire branches: the way to the farthest is
    // shorter than all the wire there is.
    const fan = segmentsOf(2);
    const length = fan.reduce((sum, s) => {
      const e = routing.ends.subarray(s * 6, s * 6 + 6);
      return sum + Math.abs(e[0]! - e[3]!) + Math.abs(e[1]! - e[4]!) + Math.abs(e[2]! - e[5]!);
    }, 0);
    expect(routing.reach[fan[0]!]!).toBeLessThan(length);
    expect(routing.reach[fan[0]!]!).toBeGreaterThan(0);
  });

  it('gives no way along a wire that no gate drives', () => {
    // An input of the circuit, read by two gates.
    const withInput: NetlistInfo = {
      gates: Uint32Array.from([0, 2, 1, 3, 0, 0, 2, 3, 4, 0]),
      levels: Uint32Array.from([1, 2]),
      moduleParents: Int32Array.from([-1]),
      moduleNames: ['top'],
      wireCount: 5,
    };
    const inputTree = buildTree(withInput);
    const inputLayout = layoutScene(inputTree);
    const routed = routeWires(withInput, inputTree, inputLayout);
    expectProperRouting(withInput, inputTree, inputLayout, routed);
    const ofInput = Array.from({ length: routed.count }, (_, s) => s).filter((s) => routed.wire[s] === 2);
    expect(ofInput.length).toBeGreaterThan(0);
    for (const s of ofInput) expect([routed.along[s * 2], routed.along[s * 2 + 1], routed.reach[s]]).toEqual([-1, -1, -1]);
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
