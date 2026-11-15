/**
 * Level of detail. A module far away is a closed block; as it grows in the
 * picture it opens and shows what is inside. How open a module is depends
 * only on how large it looks from where the eye is, and it changes smoothly
 * with it, so that nothing ever pops: move the camera a little and the
 * picture changes a little.
 *
 * Nothing here remembers anything between frames.
 */
import type { Vec3 } from './camera';
import type { SceneLayout } from './layout';
import type { ModuleTree } from './tree';

export interface Lod {
  /** Per module: 0 while it is a closed block, 1 once its lid is gone. */
  open: Float32Array;
  /**
   * Per module: how much of it shows through the lids of the modules around
   * it: the product of `open` over everything above it. 0 means it is
   * entirely hidden and need not be drawn.
   */
  seen: Float32Array;
}

/** A module starts to open when it looks this large, in pixels, and is fully open at the second size. */
export const OPENS_AT = 230;
export const OPEN_AT = 460;

/** The sizes, in pixels, between which a module opens. */
export interface LodRange {
  opensAt: number;
  openAt: number;
}

/**
 * The range for a picture of this size. In a small picture (a phone held
 * upright) modules open sooner, in proportion: otherwise a module that
 * fills the picture would still not be large enough to be open.
 */
export function lodRange(viewport: { width: number; height: number }): LodRange {
  const scale = Math.min(1, Math.max(0.4, Math.min(viewport.width, viewport.height) / 720));
  return { opensAt: OPENS_AT * scale, openAt: OPEN_AT * scale };
}

export function createLod(tree: ModuleTree): Lod {
  return { open: new Float32Array(tree.count), seen: new Float32Array(tree.count) };
}

/** 0 below `lo`, 1 above `hi`, and a smooth S in between. */
export function smoothstep(lo: number, hi: number, v: number): number {
  const t = Math.min(1, Math.max(0, (v - lo) / (hi - lo)));
  return t * t * (3 - 2 * t);
}

/**
 * How large a module looks, in pixels: the side of the square with its
 * area, at the distance of the point of its block nearest to the eye.
 * Infinite when the eye is inside the block.
 */
export function apparentSize(layout: SceneLayout, module: number, eye: Vec3, focal: number): number {
  const i = module * 4;
  const [x, y, w, h] = [layout.rects[i]!, layout.rects[i + 1]!, layout.rects[i + 2]!, layout.rects[i + 3]!];
  const dx = Math.max(x - eye[0], 0, eye[0] - (x + w));
  const dy = Math.max(y - eye[1], 0, eye[1] - (y + h));
  const dz = Math.max(eye[2] - layout.lidZ[module]!, 0);
  const distance = Math.hypot(dx, dy, dz);
  return distance === 0 ? Infinity : (focal * Math.sqrt(w * h)) / distance;
}

/**
 * Fill `lod` for an eye at `eye`. `focal` is the number of pixels a unit
 * covers at a distance of one unit (`focalLength` of the viewport).
 */
export function updateLod(
  tree: ModuleTree,
  layout: SceneLayout,
  eye: Vec3,
  focal: number,
  lod: Lod,
  range: LodRange = { opensAt: OPENS_AT, openAt: OPEN_AT },
): void {
  const { open, seen } = lod;
  // Parents come before their children, so one pass in order is enough.
  for (let m = 0; m < tree.count; m++) {
    const parent = tree.parents[m]!;
    const visible = parent < 0 ? 1 : seen[parent]! * open[parent]!;
    seen[m] = visible;
    // What cannot be seen is not measured: it stays closed, and so does
    // everything inside it.
    open[m] = visible === 0 ? 0 : smoothstep(range.opensAt, range.openAt, apparentSize(layout, m, eye, focal));
  }
}

/**
 * The module the camera is in: the deepest module that holds the point of
 * the floor at (x, y) and is more open than closed, or the top module.
 */
export function focusModule(tree: ModuleTree, layout: SceneLayout, lod: Lod, x: number, y: number): number {
  let focus = 0;
  for (;;) {
    let next = -1;
    for (const child of tree.children[focus]!) {
      const i = child * 4;
      const inside =
        x >= layout.rects[i]! &&
        x <= layout.rects[i]! + layout.rects[i + 2]! &&
        y >= layout.rects[i + 1]! &&
        y <= layout.rects[i + 1]! + layout.rects[i + 3]!;
      if (inside && lod.open[child]! * lod.seen[child]! >= 0.5) {
        next = child;
        break;
      }
    }
    if (next < 0) return focus;
    focus = next;
  }
}
