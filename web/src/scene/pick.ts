/**
 * What is under the pointer. A closed module is a solid block and the ray
 * stops at it; an open one lets the ray through to what is inside, down to
 * the floor and the gates standing on it.
 */
import type { Vec3 } from './camera';
import { GATE_TOP, gateAt, type SceneLayout } from './layout';
import type { Lod } from './lod';
import type { ModuleTree } from './tree';

export interface Hit {
  /** The closed block that was hit, or the open module whose floor was. */
  module: number;
  /** The gate that was hit, or -1. */
  gate: number;
  /** Where on the floor plan the ray ended. */
  x: number;
  y: number;
}

/**
 * Where along the ray it enters the box `[x0, x1] × [y0, y1] × [z0, z1]`,
 * or `Infinity` if it misses. 0 if it starts inside.
 */
export function rayBox(
  origin: Vec3,
  direction: Vec3,
  min: Vec3,
  max: Vec3,
): number {
  let enter = 0;
  let leave = Infinity;
  for (let axis = 0; axis < 3; axis++) {
    const o = origin[axis]!;
    const d = direction[axis]!;
    if (Math.abs(d) < 1e-12) {
      if (o < min[axis]! || o > max[axis]!) return Infinity;
      continue;
    }
    const a = (min[axis]! - o) / d;
    const b = (max[axis]! - o) / d;
    enter = Math.max(enter, Math.min(a, b));
    leave = Math.min(leave, Math.max(a, b));
  }
  return enter <= leave ? enter : Infinity;
}

export function pick(
  tree: ModuleTree,
  layout: SceneLayout,
  lod: Lod,
  origin: Vec3,
  direction: Vec3,
): Hit | undefined {
  let best: Hit | undefined;
  let bestT = Infinity;
  const at = (t: number) => ({ x: origin[0] + t * direction[0], y: origin[1] + t * direction[1] });

  const visit = (m: number): void => {
    const i = m * 4;
    const [x, y, w, h] = [layout.rects[i]!, layout.rects[i + 1]!, layout.rects[i + 2]!, layout.rects[i + 3]!];
    const t = rayBox(origin, direction, [x, y, 0], [x + w, y + h, layout.lidZ[m]!]);
    // Everything of a module is inside its block: a ray that misses the
    // block misses all of it.
    if (t === Infinity) return;
    if (lod.open[m]! < 0.5) {
      if (t < bestT) {
        bestT = t;
        best = { module: m, gate: -1, ...at(t) };
      }
      return;
    }
    // Open: the ray goes on to the floor, unless something inside stops it.
    if (direction[2] < 0) {
      const floorT = -origin[2] / direction[2];
      const floor = at(floorT);
      const onFloor = floor.x >= x && floor.x <= x + w && floor.y >= y && floor.y <= y + h;
      // `<=`: the floor of a module replaces the same floor seen as its parent's.
      if (onFloor && floorT <= bestT) {
        // A gate is looked for where the ray crosses the middle of its height.
        const mid = at((GATE_TOP / 2 - origin[2]) / direction[2]);
        bestT = floorT;
        best = { module: m, gate: gateAt(tree, layout, m, mid.x, mid.y), ...floor };
      }
    }
    for (const child of tree.children[m]!) visit(child);
  };
  visit(0);
  return best;
}
