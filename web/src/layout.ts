/**
 * Where each gate is drawn. For now a flat floorplan: one rectangle per
 * top-level block of the CPU, gates in netlist order inside it. The
 * hierarchical 3D layout replaces this in M4; the data path (one instance per
 * gate, state looked up by wire index) stays.
 *
 * Deterministic: the same netlist always gives the same layout.
 */
import { GATE_FIELDS, type NetlistInfo } from './protocol';

/** One top-level block of the CPU. Units are gate cells; y grows downwards. */
export interface Block {
  /** Module name, e.g. `alu`. */
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** Gates inside. */
  gates: number;
}

export interface FlatLayout {
  /** Centre of each gate's cell: x, y per gate. */
  positions: Float32Array;
  blocks: Block[];
  width: number;
  height: number;
}

/** Rows reserved above each block's gates for its label. */
export const HEADER = 2;
/** Empty cells between blocks. */
export const GAP = 2;

/**
 * For each module, the top-level block it belongs to: its ancestor that is a
 * direct child of the top module (or the top module itself).
 */
export function topBlocks(moduleParents: Int32Array): Int32Array {
  const block = new Int32Array(moduleParents.length);
  // Parents always come before their children, so one forward pass suffices.
  for (let m = 0; m < moduleParents.length; m++) {
    const parent = moduleParents[m] ?? -1;
    block[m] = parent <= 0 ? m : (block[parent] ?? m);
  }
  return block;
}

export function flatLayout(net: NetlistInfo, maxRowWidth = 100): FlatLayout {
  const gateCount = net.gates.length / GATE_FIELDS;
  const blockOfModule = topBlocks(net.moduleParents);
  const blockOfGate = (g: number) => blockOfModule[net.gates[g * GATE_FIELDS + 4] ?? 0] ?? 0;

  const counts = new Map<number, number>();
  for (let g = 0; g < gateCount; g++) counts.set(blockOfGate(g), (counts.get(blockOfGate(g)) ?? 0) + 1);

  // Largest first, then shelf-pack left to right.
  const order = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0] - b[0]);
  const blocks: Block[] = [];
  const columnsOf = new Map<number, number>();
  const indexOf = new Map<number, number>();
  let x = 0;
  let y = 0;
  let shelfHeight = 0;
  let width = 0;
  for (const [module, gates] of order) {
    // Twice as wide as tall.
    const columns = Math.max(1, Math.ceil(Math.sqrt(gates * 2)));
    const rows = Math.ceil(gates / columns);
    if (x > 0 && x + columns > maxRowWidth) {
      x = 0;
      y += shelfHeight + GAP;
      shelfHeight = 0;
    }
    indexOf.set(module, blocks.length);
    columnsOf.set(module, columns);
    blocks.push({ name: net.moduleNames[module] ?? '?', x, y, width: columns, height: rows + HEADER, gates });
    width = Math.max(width, x + columns);
    shelfHeight = Math.max(shelfHeight, rows + HEADER);
    x += columns + GAP;
  }

  const positions = new Float32Array(gateCount * 2);
  const placed = new Map<number, number>();
  for (let g = 0; g < gateCount; g++) {
    const module = blockOfGate(g);
    const block = blocks[indexOf.get(module) ?? 0];
    const columns = columnsOf.get(module) ?? 1;
    const k = placed.get(module) ?? 0;
    placed.set(module, k + 1);
    if (!block) continue;
    positions[g * 2] = block.x + (k % columns) + 0.5;
    positions[g * 2 + 1] = block.y + HEADER + Math.floor(k / columns) + 0.5;
  }
  return { positions, blocks, width, height: y + shelfHeight };
}
