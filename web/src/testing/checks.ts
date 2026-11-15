/**
 * What must be true of any layout and any routing, whatever the tree. Used
 * by the tests on small hand-made trees, on large made-up ones and on the
 * real CPU alike.
 */
import { expect } from 'vitest';
import { GATE_FIELDS, type NetlistInfo } from '../protocol';
import { BRANCH_RISE, GATE_TOP, gateAt, rectOf, type Rect, type SceneLayout } from '../scene/layout';
import { PIN_X, PIN_Y, type Routing } from '../scene/route';
import { commonAncestor, type ModuleTree } from '../scene/tree';

const EPS = 1e-9;

const contains = (outer: Rect, inner: Rect) =>
  inner.x >= outer.x - EPS &&
  inner.y >= outer.y - EPS &&
  inner.x + inner.w <= outer.x + outer.w + EPS &&
  inner.y + inner.h <= outer.y + outer.h + EPS;

const apart = (a: Rect, b: Rect) =>
  a.x + a.w <= b.x + EPS || b.x + b.w <= a.x + EPS || a.y + a.h <= b.y + EPS || b.y + b.h <= a.y + EPS;

/** Modules nest, siblings do not touch, every gate has a cell of its own in its own module. */
export function expectProperLayout(tree: ModuleTree, layout: SceneLayout): void {
  expect(rectOf(layout, 0)).toEqual({ x: 0, y: 0, w: layout.width, h: layout.height });
  for (let m = 0; m < tree.count; m++) {
    const rect = rectOf(layout, m);
    expect(rect.w).toBeGreaterThan(0);
    expect(rect.h).toBeGreaterThan(0);
    const header = layout.headers[m]!;
    expect(header).toBeGreaterThan(0);
    // What a module holds lies below its title strip.
    const inside: Rect = { x: rect.x, y: rect.y + header, w: rect.w, h: rect.h - header };
    const children = tree.children[m]!;
    children.forEach((child, i) => {
      const c = rectOf(layout, child);
      if (!contains(inside, c)) throw new Error(`${tree.paths[child]!} is not inside ${tree.paths[m]!}`);
      for (const other of children.slice(i + 1)) {
        if (!apart(c, rectOf(layout, other))) {
          throw new Error(`${tree.paths[child]!} overlaps ${tree.paths[other]!}`);
        }
      }
    });
    // A module's lid is above everything inside it, and its wires in between.
    const below = Math.max(tree.gates[m]!.length > 0 ? GATE_TOP : 0, ...children.map((c) => layout.lidZ[c]!));
    expect(layout.wireZ[m]!).toBeGreaterThan(below);
    expect(layout.wireZ[m]! + BRANCH_RISE).toBeLessThan(layout.lidZ[m]!);

    for (const gate of tree.gates[m]!) {
      const cell: Rect = { x: layout.gates[gate * 2]! - 0.5, y: layout.gates[gate * 2 + 1]! - 0.5, w: 1, h: 1 };
      if (!contains(inside, cell)) throw new Error(`gate ${gate} is not inside ${tree.paths[m]!}`);
      for (const child of children) {
        if (!apart(cell, rectOf(layout, child))) throw new Error(`gate ${gate} is under ${tree.paths[child]!}`);
      }
      // The cell leads back to the gate, so no two gates share one.
      expect(gateAt(tree, layout, m, cell.x + 0.5, cell.y + 0.5)).toBe(gate);
    }
  }
}

/**
 * Every wire that joins gates is drawn as one connected piece that reaches
 * every pin on it and nothing else, in axis-aligned segments at the heights
 * of the module it belongs to.
 */
export function expectProperRouting(net: NetlistInfo, tree: ModuleTree, layout: SceneLayout, routing: Routing): void {
  const field = (g: number, f: number) => net.gates[g * GATE_FIELDS + f]!;
  // The pins on each wire, as "x,y" at the height of the gates, and the gates on it.
  const pins = new Map<number, Set<string>>();
  const gatesOn = new Map<number, number[]>();
  const key = (x: number, y: number, z: number) => `${x.toFixed(6)},${y.toFixed(6)},${z.toFixed(6)}`;
  const add = (wire: number, gate: number, dx: number, dy: number) => {
    if (wire < 2) return;
    if (!pins.has(wire)) pins.set(wire, new Set());
    if (!gatesOn.has(wire)) gatesOn.set(wire, []);
    pins.get(wire)!.add(`${gate}:${dx},${dy}`);
    gatesOn.get(wire)!.push(gate);
  };
  for (let g = 0; g < tree.gateCount; g++) {
    add(field(g, 3), g, PIN_X, 0);
    if (field(g, 0) === 0) {
      add(field(g, 1), g, -PIN_X, -PIN_Y);
      add(field(g, 2), g, -PIN_X, PIN_Y);
    } else {
      add(field(g, 1), g, -PIN_X, 0);
    }
  }

  // Segments by wire.
  const segments = new Map<number, number[]>();
  for (let s = 0; s < routing.count; s++) {
    const wire = routing.wire[s]!;
    if (!segments.has(wire)) segments.set(wire, []);
    segments.get(wire)!.push(s);
    const e = routing.ends.subarray(s * 6, s * 6 + 6);
    const moved = [0, 1, 2].filter((axis) => Math.abs(e[axis]! - e[axis + 3]!) > EPS);
    expect(moved.length).toBeLessThanOrEqual(1);
  }

  for (const [wire, pinSet] of pins) {
    const mine = segments.get(wire);
    if (pinSet.size < 2) {
      // A wire that joins nothing to nothing is not drawn.
      expect(mine).toBeUndefined();
      continue;
    }
    if (!mine) throw new Error(`wire ${wire} joins ${pinSet.size} pins and is not drawn`);

    const home = gatesOn.get(wire)!.map((g) => tree.moduleOfGate[g]!).reduce((a, b) => commonAncestor(tree, a, b));
    const trunkZ = layout.wireZ[home]!;
    // Joined ends: every end of every segment, merged where they coincide.
    const parent = new Map<string, string>();
    const find = (k: string): string => {
      let root = k;
      while (parent.get(root) !== root) root = parent.get(root)!;
      parent.set(k, root);
      return root;
    };
    const points: [number, number, number][] = [];
    for (const s of mine) {
      expect(routing.module[s]).toBe(home);
      const e = routing.ends.subarray(s * 6, s * 6 + 6);
      const a = key(e[0]!, e[1]!, e[2]!);
      const b = key(e[3]!, e[4]!, e[5]!);
      for (const k of [a, b]) if (!parent.has(k)) parent.set(k, k);
      parent.set(find(a), find(b));
      points.push([e[0]!, e[1]!, e[2]!], [e[3]!, e[4]!, e[5]!]);
      for (const z of [e[2]!, e[5]!]) {
        expect(z).toBeGreaterThanOrEqual(GATE_TOP - EPS);
        expect(z).toBeLessThanOrEqual(trunkZ + BRANCH_RISE + EPS);
      }
    }
    // A point in the middle of a segment (a branch meeting the trunk) joins it too.
    for (const s of mine) {
      const e = routing.ends.subarray(s * 6, s * 6 + 6);
      for (const [x, y, z] of points) {
        const on = [0, 1, 2].every((axis) => {
          const v = [x, y, z][axis]!;
          return v >= Math.min(e[axis]!, e[axis + 3]!) - 1e-6 && v <= Math.max(e[axis]!, e[axis + 3]!) + 1e-6;
        });
        if (on) parent.set(find(key(x, y, z)), find(key(e[0]!, e[1]!, e[2]!)));
      }
    }
    const roots = new Set([...parent.keys()].map(find));
    if (roots.size !== 1) throw new Error(`wire ${wire} is drawn in ${roots.size} pieces`);

    // It comes down to the floor at its pins and nowhere else. (A wire's
    // segments are all shifted sideways by the same small amount.)
    const feet = points.filter(([, , z]) => Math.abs(z - GATE_TOP) < EPS);
    expect(feet.length).toBe(pinSet.size);
    for (const pin of pinSet) {
      const [gate, offset] = pin.split(':') as [string, string];
      const [dx, dy] = offset.split(',').map(Number) as [number, number];
      const x = layout.gates[Number(gate) * 2]! + dx;
      const y = layout.gates[Number(gate) * 2 + 1]! + dy;
      const reached = feet.some(([fx, fy]) => Math.abs(fx - x) < 0.1 && Math.abs(fy - y) < EPS);
      if (!reached) throw new Error(`wire ${wire} does not reach gate ${gate}`);
    }

    // Every point knows how far it is from the driver, along the wire.
    const driverGate = gatesOn.get(wire)!.find((g) => field(g, 3) === wire);
    // The distance at each point, as every segment that touches it tells it.
    const told = new Map<string, number[]>();
    let farthest = 0;
    for (const s of mine) {
      const e = routing.ends.subarray(s * 6, s * 6 + 6);
      const [s0, s1] = [routing.along[s * 2]!, routing.along[s * 2 + 1]!];
      if (driverGate === undefined) {
        // Nothing drives it from inside: there is no way along it.
        expect([s0, s1, routing.reach[s]]).toEqual([-1, -1, -1]);
        continue;
      }
      // Along a straight segment the distance grows by the segment's length.
      const length = Math.abs(e[0]! - e[3]!) + Math.abs(e[1]! - e[4]!) + Math.abs(e[2]! - e[5]!);
      expect(Math.abs(s1 - s0)).toBeCloseTo(length, 4);
      expect(Math.min(s0, s1)).toBeGreaterThanOrEqual(0);
      farthest = Math.max(farthest, s0, s1);
      for (const [k, v] of [
        [key(e[0]!, e[1]!, e[2]!), s0],
        [key(e[3]!, e[4]!, e[5]!), s1],
      ] as const) {
        if (!told.has(k)) told.set(k, []);
        told.get(k)!.push(v);
      }
    }
    if (driverGate !== undefined) {
      // Segments that meet agree on the distance where they meet.
      for (const [k, values] of told) {
        if (Math.max(...values) - Math.min(...values) > 1e-4) {
          throw new Error(`wire ${wire}: segments disagree at ${k}: ${values.join(', ')}`);
        }
      }
      // It is 0 at the driver's output pin and nowhere else…
      const zeros = [...told].filter(([, values]) => values[0]! < 1e-6).map(([k]) => k);
      const out = key(
        zeros.length === 1 ? Number(zeros[0]!.split(',')[0]) : NaN,
        layout.gates[driverGate * 2 + 1]!,
        GATE_TOP,
      );
      expect(zeros).toEqual([out]);
      expect(Math.abs(Number(out.split(',')[0]) - (layout.gates[driverGate * 2]! + PIN_X))).toBeLessThan(0.1);
      // …and largest at a pin of a gate that reads the wire, as `reach` says.
      for (const s of mine) expect(routing.reach[s]).toBeCloseTo(farthest, 4);
      const ends = [...told].filter(([, values]) => Math.abs(values[0]! - farthest) < 1e-4).map(([k]) => k);
      expect(ends.some((k) => Math.abs(Number(k.split(',')[2]) - GATE_TOP) < EPS)).toBe(true);
    }
  }
  // Nothing is drawn for a wire that is not in the netlist.
  for (const wire of segments.keys()) expect(pins.has(wire)).toBe(true);
}
