/**
 * A made-up netlist with a module tree as deep and as wide as asked for.
 *
 * It is not the computer and computes nothing. It exists to try the scene
 * (the layout, the levels of detail, the precision of the camera) on trees
 * much deeper and larger than the CPU's, in tests and on the laboratory
 * page. The page that shows the computer never uses it.
 */
import { GATE_FIELDS, type NetlistInfo } from '../protocol';

/** A small generator of repeatable pseudo-random numbers in [0, 1). */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export interface SyntheticShape {
  /** Levels of modules below the top one. */
  depth: number;
  /** Modules inside each module. */
  fanout: number;
  /** Gates in each module of the lowest level. */
  gatesPerLeaf: number;
  seed?: number;
}

/**
 * A tree of `fanout ** depth` leaves. Every gate takes its inputs from
 * gates made before it, mostly from its own module and now and then from
 * anywhere, so that there are wires at every level of the tree. One gate in
 * eight is a flip-flop.
 */
export function syntheticNetlist(shape: SyntheticShape): NetlistInfo {
  const random = mulberry32(shape.seed ?? 1);
  const parents: number[] = [-1];
  const names: string[] = ['top'];
  const leaves: number[] = [];
  const grow = (module: number, level: number) => {
    if (level === shape.depth) {
      leaves.push(module);
      return;
    }
    for (let i = 0; i < shape.fanout; i++) {
      const child = parents.length;
      parents.push(module);
      names.push(`m${i}`);
      grow(child, level + 1);
    }
  };
  grow(0, 0);

  const gates: number[] = [];
  // Wires 0 and 1 are the constants; every gate drives the next free wire.
  let wire = 2;
  for (const leaf of leaves) {
    const first = wire;
    for (let g = 0; g < shape.gatesPerLeaf; g++) {
      const input = () => {
        if (wire === 2) return 0;
        // Usually a neighbour, sometimes any earlier gate at all.
        const local = wire > first && random() < 0.9;
        const from = local ? first : 2;
        return from + Math.floor(random() * (wire - from));
      };
      const kind = random() < 0.125 ? 1 : 0;
      gates.push(kind, input(), kind === 0 ? input() : 0, wire++, leaf);
    }
  }
  if (gates.length % GATE_FIELDS !== 0) throw new Error('unreachable');
  return {
    gates: Uint32Array.from(gates),
    moduleParents: Int32Array.from(parents),
    moduleNames: names,
    wireCount: wire,
  };
}
