/**
 * The words in the scene, as letters to draw: one small rectangle per
 * character, lying flat. Which letters, where and how large is decided
 * here, in plain arrays that can be tested; the renderer only draws them.
 *
 * A module has two labels. Closed, its name is written large across its
 * lid, with the number of gates inside. Open, its name is in the title
 * strip along its top edge. A gate's name is under the gate. Which of them
 * shows is the renderer's business: it follows the level of detail.
 */
import type { SceneLayout } from './layout';
import type { ModuleTree } from './tree';

/** Width of a character's cell over its height: the letters are of one width. */
export const ADVANCE = 0.625;

/** When a label shows. */
export const Show = {
  /** On the lid of a module, while the module is closed. */
  Closed: 0,
  /** On the floor of a module, while it is open. */
  Open: 1,
} as const;
export type Show = (typeof Show)[keyof typeof Show];

/** The colours a label can have; the renderer has the palette. */
export const Ink = {
  Bright: 0,
  Dim: 1,
  Hot: 2,
} as const;
export type Ink = (typeof Ink)[keyof typeof Ink];

/** Characters, each with its place, to be drawn as one batch. */
export class Glyphs {
  /** Per character: x, y of the top left corner of its cell, each as a whole part and the rest. */
  readonly at: Float32Array;
  /** Per character: height above the floor, height of the cell, character code, module. */
  readonly glyph: Float32Array;
  /** Per character: when it shows, and its colour. */
  readonly style: Float32Array;
  count = 0;

  constructor(readonly capacity: number) {
    this.at = new Float32Array(capacity * 4);
    this.glyph = new Float32Array(capacity * 4);
    this.style = new Float32Array(capacity * 2);
  }

  clear(): void {
    this.count = 0;
  }

  /**
   * Add `text` with the top left corner of its first character at (x, y),
   * `size` high. Spaces take room and are not drawn. Text that does not fit
   * in what is left of the batch is cut short.
   */
  write(text: string, x: number, y: number, z: number, size: number, module: number, show: Show, ink: Ink): void {
    for (let i = 0; i < text.length && this.count < this.capacity; i++) {
      const code = text.charCodeAt(i);
      if (code === 32) continue;
      const k = this.count++;
      const cx = x + i * ADVANCE * size;
      // Kept as a whole number and a small rest, so that the renderer can
      // subtract the camera exactly; see `view.ts`.
      this.at[k * 4] = Math.round(cx);
      this.at[k * 4 + 1] = Math.round(y);
      this.at[k * 4 + 2] = cx - Math.round(cx);
      this.at[k * 4 + 3] = y - Math.round(y);
      this.glyph[k * 4] = z;
      this.glyph[k * 4 + 1] = size;
      // Anything outside printable ASCII is drawn as a question mark.
      this.glyph[k * 4 + 2] = code > 32 && code < 127 ? code : 63;
      this.glyph[k * 4 + 3] = module;
      this.style[k * 2] = show;
      this.style[k * 2 + 1] = ink;
    }
  }
}

export function textWidth(text: string, size: number): number {
  return text.length * ADVANCE * size;
}

/** How high labels float above what they are written on, so that the two do not fight. */
const LIFT = 0.02;
/** A lid's label uses at most this much of the lid's width. */
const LID_FILL = 0.84;
/** Size of a gate's name, and how far below the middle of its cell it starts. */
const GATE_LABEL = 0.1;
const GATE_LABEL_Y = 0.27;

/** `1 gate`, `357 gates`. */
export function gateCountText(gates: number): string {
  return `${gates.toLocaleString('en-US')} gate${gates === 1 ? '' : 's'}`;
}

/** Where a module's two labels go. */
export interface LabelPlan {
  /** On the lid: the name, centred. */
  name: { x: number; y: number; size: number };
  /** On the lid: the line under the name, for a value, centred on `x`. */
  value: { x: number; y: number; size: number };
  /** On the lid: the number of gates, centred. */
  count: { x: number; y: number; size: number };
  /** In the title strip: the name at the left, and the right edge for a value. */
  title: { x: number; y: number; size: number; right: number };
}

export function planLabels(tree: ModuleTree, layout: SceneLayout, module: number): LabelPlan {
  const [x, y, w, h] = [
    layout.rects[module * 4]!,
    layout.rects[module * 4 + 1]!,
    layout.rects[module * 4 + 2]!,
    layout.rects[module * 4 + 3]!,
  ];
  const name = tree.names[module]!;
  const count = gateCountText(tree.totalGates[module]!);
  const room = LID_FILL * w;
  const size = Math.min(0.24 * h, room / (name.length * ADVANCE));
  const valueSize = 0.62 * size;
  const countSize = Math.min(0.4 * size, room / (count.length * ADVANCE));
  // The three lines as a block in the middle of the lid.
  const block = size + 1.15 * valueSize + 1.25 * countSize;
  const top = y + (h - block) / 2;
  const middle = x + w / 2;

  const header = layout.headers[module]!;
  const pad = 0.35 * header;
  const titleSize = Math.min(0.6 * header, (w - 2 * pad) / ((name.length + 1) * ADVANCE));
  return {
    name: { x: middle - textWidth(name, size) / 2, y: top, size },
    value: { x: middle, y: top + size + 0.15 * valueSize, size: valueSize },
    count: { x: middle - textWidth(count, countSize) / 2, y: top + size + 1.15 * valueSize + 0.25 * countSize, size: countSize },
    title: { x: x + pad, y: y + (header - titleSize) / 2, size: titleSize, right: x + w - pad },
  };
}

/** The labels that never change: names of modules and of gates, and gate counts. */
export function buildLabels(tree: ModuleTree, layout: SceneLayout): Glyphs {
  let letters = 0;
  for (let m = 0; m < tree.count; m++) {
    letters += 2 * tree.names[m]!.length + gateCountText(tree.totalGates[m]!).length;
  }
  // `nand` or `dff` and up to ten digits.
  letters += tree.gateCount * 14;
  const glyphs = new Glyphs(letters);

  for (let m = 0; m < tree.count; m++) {
    const plan = planLabels(tree, layout, m);
    const name = tree.names[m]!;
    const lid = layout.lidZ[m]! + LIFT;
    glyphs.write(name, plan.name.x, plan.name.y, lid, plan.name.size, m, Show.Closed, Ink.Bright);
    glyphs.write(gateCountText(tree.totalGates[m]!), plan.count.x, plan.count.y, lid, plan.count.size, m, Show.Closed, Ink.Dim);
    glyphs.write(name, plan.title.x, plan.title.y, LIFT, plan.title.size, m, Show.Open, Ink.Dim);
  }
  for (let g = 0; g < tree.gateCount; g++) {
    const text = `${tree.kindOfGate[g] === 0 ? 'nand' : 'dff'}${tree.localOfGate[g]!}`;
    const size = Math.min(GATE_LABEL, 0.9 / (text.length * ADVANCE));
    const x = layout.gates[g * 2]! - textWidth(text, size) / 2;
    const y = layout.gates[g * 2 + 1]! + GATE_LABEL_Y;
    glyphs.write(text, x, y, LIFT, size, tree.moduleOfGate[g]!, Show.Open, Ink.Dim);
  }
  return glyphs;
}

/**
 * A value to show on a module: on its lid under the name while it is
 * closed, at the right of its title strip while it is open.
 */
export function writeValue(glyphs: Glyphs, tree: ModuleTree, layout: SceneLayout, module: number, text: string): void {
  const plan = planLabels(tree, layout, module);
  const w = layout.rects[module * 4 + 2]!;
  const size = Math.min(plan.value.size, (LID_FILL * w) / (text.length * ADVANCE));
  glyphs.write(text, plan.value.x - textWidth(text, size) / 2, plan.value.y, layout.lidZ[module]! + LIFT, size, module, Show.Closed, Ink.Hot);
  // In the title strip the value keeps clear of the name.
  const nameEnd = plan.title.x + textWidth(tree.names[module]! + ' ', plan.title.size);
  const titleSize = Math.min(plan.title.size, (plan.title.right - nameEnd) / (text.length * ADVANCE));
  if (titleSize > 0) {
    const y = plan.title.y + (plan.title.size - titleSize) / 2;
    glyphs.write(text, plan.title.right - textWidth(text, titleSize), y, LIFT, titleSize, module, Show.Open, Ink.Hot);
  }
}
