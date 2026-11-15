/**
 * The scene of the real CPU: these tests load the simulation core and work
 * on the netlist the page is given, not on a stand-in.
 */
import { describe, expect, it } from 'vitest';
import { expectProperLayout, expectProperRouting } from '../testing/checks';
import { demoMachine, netlistOf } from '../testing/core';
import { FLOORPLAN } from './floorplan';
import { layoutScene, rectOf, type Arrangement } from './layout';
import { routeWires } from './route';
import { buildTree, gatePath, moduleAt, resolvePath } from './tree';

const machine = demoMachine();
const net = netlistOf(machine);
const tree = buildTree(net);
const layout = layoutScene(tree, FLOORPLAN);

describe('the CPU as a scene', () => {
  it('is the CPU that was measured', () => {
    expect(tree.names[0]).toBe('cpu');
    expect(tree.gateCount).toBe(2603 + 127);
    expect(Array.from(tree.kindOfGate).filter((k) => k === 1).length).toBe(127);
  });

  it('names every gate as the core does', () => {
    for (let g = 0; g < tree.gateCount; g++) {
      const path = machine.gate_path(g);
      expect(gatePath(tree, g)).toBe(path);
      expect(resolvePath(tree, path)).toEqual({ module: tree.moduleOfGate[g], gate: g });
    }
    expect(gatePath(tree, tree.gates[moduleAt(tree, 'cpu/alu/adder8/fa3')]![2]!)).toBe('cpu/alu/adder8/fa3/nand2');
  });

  it('has a proper layout', () => {
    expectProperLayout(tree, layout);
  });

  it('has a floor plan that names every block it arranges, and only blocks that exist', () => {
    const named = (a: Arrangement): string[] =>
      typeof a === 'string' ? [a] : ('row' in a ? a.row : a.column).flatMap(named);
    for (const [path, arrangement] of Object.entries(FLOORPLAN)) {
      const module = moduleAt(tree, path);
      expect(module, path).toBeGreaterThanOrEqual(0);
      const children = tree.children[module]!.map((c) => tree.names[c]!);
      expect(named(arrangement).sort()).toEqual([...children].sort());
    }
  });

  it('is a compact rectangle, not a strip', () => {
    const cpu = rectOf(layout, 0);
    expect(cpu.w / cpu.h).toBeGreaterThan(0.8);
    expect(cpu.w / cpu.h).toBeLessThan(2.4);
    // Not much more floor than the gates need: a cell each, and as much
    // again several times over for margins, titles and the gaps between blocks.
    expect(cpu.w * cpu.h).toBeLessThan(tree.gateCount * 8);
  });

  it('has every wire drawn, connected, down to every pin', () => {
    const routing = routeWires(net, tree, layout);
    expectProperRouting(net, tree, layout, routing);
    expect(routing.count).toBeGreaterThan(10_000);
    expect(routing.count).toBeLessThan(60_000);
  });
});
