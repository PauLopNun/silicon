/**
 * Where everything is. Every module gets a rectangle, nested like the module
 * tree: a module's rectangle holds a title strip and, below it, the
 * rectangles of the modules inside it and a grid with one cell for each gate
 * of its own.
 *
 * Plan coordinates: x grows to the right, y grows downwards, one unit is one
 * gate cell. Everything is kept in doubles; the renderer subtracts the
 * camera before anything becomes a 32-bit float.
 *
 * Deterministic: the same tree and the same hints always give the same
 * layout. Nothing here depends on the simulation's state.
 */
import type { ModuleTree } from './tree';

/**
 * How to place the modules inside one module by hand: a module's name, or
 * several arrangements side by side (`row`) or one under another (`column`).
 */
export type Arrangement = string | { row: readonly Arrangement[] } | { column: readonly Arrangement[] };

/** Hand-made arrangements, by module path. Modules without one are packed. */
export type Hints = Readonly<Record<string, Arrangement>>;

export interface SceneLayout {
  /** Per module: left, top, width, height. */
  rects: Float64Array;
  /** Per module: height of the title strip at the top of its rectangle. */
  headers: Float32Array;
  /** Per gate: the centre of its cell. */
  gates: Float64Array;
  /** Per module: left, top and number of columns of the grid of its own gates (0 columns: none). */
  grids: Float64Array;
  /** Per module: height above the floor of its lid, the top of the block it is when closed. */
  lidZ: Float32Array;
  /** Per module: height of the wires that join its parts. Below its lid, above its parts' lids. */
  wireZ: Float32Array;
  width: number;
  height: number;
}

/** Height of a gate above the floor. */
export const GATE_TOP = 0.3;
/** Wires that turn the other way run this much above `wireZ`. */
export const BRANCH_RISE = 0.25;
/** From the top of what a module holds up to its own wires, and up to its lid. */
const WIRE_CLEARANCE = 0.35;
const LID_CLEARANCE = 1.0;

/** A grid of gates is about this many times wider than tall. */
const GRID_ASPECT = 1.6;
/** The packer aims for rectangles of about this width over height. */
const TARGET_ASPECT = 1.5;
/** Title strip, margin and gaps, as fractions of what the module holds, and their minimums. */
const HEADER_RATIO = 0.05;
const HEADER_MIN = 1.2;
const PAD_RATIO = 0.035;
const PAD_MIN = 0.6;
const GAP_RATIO = 0.08;
const GAP_MIN = 1;

interface Size {
  w: number;
  h: number;
}

/** Items placed inside a box of size `w` × `h`; `x[i]`, `y[i]` is the corner of item `i`. */
interface Placed extends Size {
  x: number[];
  y: number[];
}

/**
 * Put the items on shelves, in order, starting a new shelf when the next
 * item would pass `limit`.
 */
function shelves(sizes: readonly Size[], gap: number, limit: number): Placed {
  const x: number[] = [];
  const y: number[] = [];
  let cursorX = 0;
  let cursorY = 0;
  let shelf = 0;
  let w = 0;
  for (const size of sizes) {
    if (cursorX > 0 && cursorX + size.w > limit) {
      cursorX = 0;
      cursorY += shelf + gap;
      shelf = 0;
    }
    x.push(cursorX);
    y.push(cursorY);
    w = Math.max(w, cursorX + size.w);
    shelf = Math.max(shelf, size.h);
    cursorX += size.w + gap;
  }
  return { x, y, w, h: cursorY + shelf };
}

/**
 * Pack the items in their order into a rectangle close to the target shape.
 * The items go on shelves, either in rows (left to right, then down) or in
 * columns (top to bottom, then right); shelves of several lengths are tried
 * both ways and the arrangement that wastes least is kept.
 */
export function pack(sizes: readonly Size[], gap: number): Placed {
  if (sizes.length === 0) return { x: [], y: [], w: 0, h: 0 };
  let area = 0;
  for (const size of sizes) area += (size.w + gap) * (size.h + gap);
  const turned = sizes.map(({ w, h }) => ({ w: h, h: w }));
  let best: Placed | undefined;
  let bestCost = Infinity;
  const keep = (placed: Placed) => {
    const cost = placed.w * placed.h * (1 + Math.abs(Math.log(placed.w / placed.h / TARGET_ASPECT)));
    // Strictly less: of two arrangements as good as each other the first tried wins.
    if (cost < bestCost) {
      best = placed;
      bestCost = cost;
    }
  };
  // Shelves from short to long, and last a single one.
  const CANDIDATES = 24;
  const limit = (i: number) => (i === CANDIDATES ? Infinity : Math.sqrt(area * 0.4 * 20 ** (i / (CANDIDATES - 1))));
  // All the rows first: where columns are no better, rows read better.
  for (let i = 0; i <= CANDIDATES; i++) keep(shelves(sizes, gap, limit(i)));
  for (let i = 0; i <= CANDIDATES; i++) {
    // Columns are rows of the same items turned on their side.
    const columns = shelves(turned, gap, limit(i));
    keep({ x: columns.y, y: columns.x, w: columns.h, h: columns.w });
  }
  return best!;
}

/** Columns of the grid for `n` gates. */
export function gridColumns(n: number): number {
  return Math.max(1, Math.ceil(Math.sqrt(n * GRID_ASPECT)));
}

export function layoutScene(tree: ModuleTree, hints: Hints = {}): SceneLayout {
  const { count } = tree;
  const rects = new Float64Array(count * 4);
  const headers = new Float32Array(count);
  const grids = new Float64Array(count * 3);
  const lidZ = new Float32Array(count);
  const wireZ = new Float32Array(count);
  // Corner of each module, and of its grid, from its parent's corner.
  const relX = new Float64Array(count);
  const relY = new Float64Array(count);
  const gridRelX = new Float64Array(count);
  const gridRelY = new Float64Array(count);

  // Sizes from the leaves up: children have larger indices than their parent.
  for (let m = count - 1; m >= 0; m--) {
    const children = tree.children[m]!;
    const own = tree.gates[m]!.length;
    const columns = own > 0 ? gridColumns(own) : 0;
    grids[m * 3 + 2] = columns;

    // What the module holds: its modules, then the grid of its own gates.
    const GRID = -1;
    const items: number[] = [...children];
    if (own > 0) items.push(GRID);
    const sizeOf = (item: number): Size =>
      item === GRID
        ? { w: columns, h: Math.ceil(own / columns) }
        : { w: rects[item * 4 + 2]!, h: rects[item * 4 + 3]! };
    let largest = 0;
    for (const item of items) largest = Math.max(largest, sizeOf(item).w, sizeOf(item).h);
    const gap = Math.max(GAP_MIN, GAP_RATIO * largest);

    const x = new Map<number, number>();
    const y = new Map<number, number>();
    const packInto = (list: readonly number[], left: number, top: number): Size => {
      const placed = pack(list.map(sizeOf), gap);
      list.forEach((item, i) => {
        x.set(item, left + placed.x[i]!);
        y.set(item, top + placed.y[i]!);
      });
      return placed;
    };

    let content: Size;
    const hint = hints[tree.paths[m]!];
    if (hint === undefined) {
      content = packInto(items, 0, 0);
    } else {
      const byName = new Map(children.map((c) => [tree.names[c]!, c]));
      const place = (a: Arrangement, left: number, top: number): Size => {
        if (typeof a === 'string') {
          const child = byName.get(a);
          if (child === undefined) throw new RangeError(`layout hint for ${tree.paths[m]!} names ${a}, which is not in it`);
          if (x.has(child)) throw new RangeError(`layout hint for ${tree.paths[m]!} names ${a} twice`);
          x.set(child, left);
          y.set(child, top);
          return sizeOf(child);
        }
        let w = 0;
        let h = 0;
        if ('row' in a) {
          for (const part of a.row) {
            const size = place(part, left + (w > 0 ? w + gap : 0), top);
            w = (w > 0 ? w + gap : 0) + size.w;
            h = Math.max(h, size.h);
          }
        } else {
          for (const part of a.column) {
            const size = place(part, left, top + (h > 0 ? h + gap : 0));
            h = (h > 0 ? h + gap : 0) + size.h;
            w = Math.max(w, size.w);
          }
        }
        return { w, h };
      };
      content = place(hint, 0, 0);
      // Whatever the hint does not mention goes underneath: a hint can
      // never lose a module.
      const rest = items.filter((item) => !x.has(item));
      if (rest.length > 0) {
        const below = packInto(rest, 0, content.h + gap);
        content = { w: Math.max(content.w, below.w), h: content.h + gap + below.h };
      }
    }

    const extent = Math.max(content.w, content.h);
    const header = Math.max(HEADER_MIN, HEADER_RATIO * extent);
    const pad = Math.max(PAD_MIN, PAD_RATIO * extent);
    headers[m] = header;
    rects[m * 4 + 2] = content.w + 2 * pad;
    rects[m * 4 + 3] = content.h + 2 * pad + header;
    for (const child of children) {
      relX[child] = pad + x.get(child)!;
      relY[child] = header + pad + y.get(child)!;
    }
    if (own > 0) {
      gridRelX[m] = pad + x.get(GRID)!;
      gridRelY[m] = header + pad + y.get(GRID)!;
    }

    let base = own > 0 ? GATE_TOP : 0;
    for (const child of children) base = Math.max(base, lidZ[child]!);
    wireZ[m] = base + WIRE_CLEARANCE;
    lidZ[m] = base + LID_CLEARANCE;
  }

  // Positions from the top down.
  const gates = new Float64Array(tree.gateCount * 2);
  for (let m = 0; m < count; m++) {
    const parent = tree.parents[m]!;
    if (parent >= 0) {
      rects[m * 4] = rects[parent * 4]! + relX[m]!;
      rects[m * 4 + 1] = rects[parent * 4 + 1]! + relY[m]!;
    }
    const columns = grids[m * 3 + 2]!;
    if (columns === 0) continue;
    const left = rects[m * 4]! + gridRelX[m]!;
    const top = rects[m * 4 + 1]! + gridRelY[m]!;
    grids[m * 3] = left;
    grids[m * 3 + 1] = top;
    tree.gates[m]!.forEach((gate, k) => {
      gates[gate * 2] = left + (k % columns) + 0.5;
      gates[gate * 2 + 1] = top + Math.floor(k / columns) + 0.5;
    });
  }

  return { rects, headers, gates, grids, lidZ, wireZ, width: rects[2]!, height: rects[3]! };
}

/** A rectangle in plan coordinates. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function rectOf(layout: SceneLayout, module: number): Rect {
  const i = module * 4;
  return { x: layout.rects[i]!, y: layout.rects[i + 1]!, w: layout.rects[i + 2]!, h: layout.rects[i + 3]! };
}

/** The cell of a gate: a unit square around its centre. */
export function gateRect(layout: SceneLayout, gate: number): Rect {
  return { x: layout.gates[gate * 2]! - 0.5, y: layout.gates[gate * 2 + 1]! - 0.5, w: 1, h: 1 };
}

/** The gate of `module`'s own grid whose cell holds the point, or -1. */
export function gateAt(tree: ModuleTree, layout: SceneLayout, module: number, x: number, y: number): number {
  const columns = layout.grids[module * 3 + 2]!;
  if (columns === 0) return -1;
  const column = Math.floor(x - layout.grids[module * 3]!);
  const row = Math.floor(y - layout.grids[module * 3 + 1]!);
  if (column < 0 || column >= columns || row < 0) return -1;
  return tree.gates[module]![row * columns + column] ?? -1;
}
