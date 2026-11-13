import { describe, expect, it } from 'vitest';
import { flatLayout, HEADER, topBlocks, type Block } from './layout';
import { GATE_FIELDS, type NetlistInfo } from './protocol';

/** A netlist with the given module tree and `gatesIn[m]` gates directly in module m. */
function netlist(parents: number[], names: string[], gatesIn: number[]): NetlistInfo {
  const gates: number[] = [];
  let wire = 2;
  // Interleave modules so that grouping is really by module, not by order.
  const left = [...gatesIn];
  while (left.some((n) => n > 0)) {
    left.forEach((n, module) => {
      if (n > 0) {
        gates.push(0, 0, 1, wire++, module);
        left[module] = n - 1;
      }
    });
  }
  return {
    gates: Uint32Array.from(gates),
    moduleParents: Int32Array.from(parents),
    moduleNames: names,
    wireCount: wire,
  };
}

// cpu ─┬─ alu ── adder ── fa0
//      └─ regs
const parents = [-1, 0, 1, 2, 0];
const names = ['cpu', 'alu', 'adder', 'fa0', 'regs'];

describe('topBlocks', () => {
  it('maps every module to its ancestor just below the top', () => {
    expect(Array.from(topBlocks(Int32Array.from(parents)))).toEqual([0, 1, 1, 1, 4]);
  });
});

describe('flatLayout', () => {
  const net = netlist(parents, names, [0, 3, 0, 9, 40]);
  const layout = flatLayout(net);
  const block = (i: number): Block => {
    const b = layout.blocks[i];
    if (!b) throw new Error(`no block ${i}`);
    return b;
  };

  it('makes one block per top-level module, largest first', () => {
    expect(layout.blocks.map((b) => [b.name, b.gates])).toEqual([
      ['regs', 40],
      ['alu', 12],
    ]);
  });

  it('gives every gate its own cell inside its block', () => {
    const count = net.gates.length / GATE_FIELDS;
    const seen = new Set<string>();
    for (let g = 0; g < count; g++) {
      const x = layout.positions[g * 2] ?? NaN;
      const y = layout.positions[g * 2 + 1] ?? NaN;
      seen.add(`${x},${y}`);
      const module = net.gates[g * GATE_FIELDS + 4] ?? -1;
      const home = block([1, 2, 3].includes(module) ? 1 : 0);
      expect(x).toBeGreaterThan(home.x);
      expect(x).toBeLessThan(home.x + home.width);
      expect(y).toBeGreaterThan(home.y + HEADER);
      expect(y).toBeLessThan(home.y + home.height);
    }
    expect(seen.size).toBe(count);
  });

  it('keeps blocks apart and inside the reported size', () => {
    const [a, b] = [block(0), block(1)];
    const apart =
      a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y;
    expect(apart).toBe(true);
    for (const each of layout.blocks) {
      expect(each.x + each.width).toBeLessThanOrEqual(layout.width);
      expect(each.y + each.height).toBeLessThanOrEqual(layout.height);
    }
  });

  it('wraps to a new shelf when a row is full', () => {
    const narrow = flatLayout(net, 10);
    expect(narrow.blocks[1]?.y).toBeGreaterThan(narrow.blocks[0]?.y ?? Infinity);
    expect(narrow.blocks[1]?.x).toBe(0);
  });

  it('is deterministic', () => {
    expect(flatLayout(net)).toEqual(layout);
  });
});
