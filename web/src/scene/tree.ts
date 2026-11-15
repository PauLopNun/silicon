/**
 * The module tree of a netlist in the shape the scene needs: who contains
 * whom, which gates sit directly in each module, and the public paths
 * (`cpu/alu/adder8/fa3/nand2`) that tests, links and the page refer to.
 */
import { GATE_FIELDS, type NetlistInfo } from '../protocol';

export interface ModuleTree {
  /** Number of modules. Module 0 is the top. */
  count: number;
  names: readonly string[];
  /** Parent of each module; -1 for the top. A parent always comes before its children. */
  parents: Int32Array;
  /** Modules directly inside each module, in the order they were declared. */
  children: readonly (readonly number[])[];
  /** Gates directly inside each module, in netlist order. */
  gates: readonly Uint32Array[];
  /** Gates inside each module and everything below it. */
  totalGates: Uint32Array;
  /** 0 for the top module, 1 for its children… */
  depth: Uint8Array;
  /** 0 for a module with no modules inside, otherwise one more than its tallest child. */
  height: Uint8Array;
  /** `cpu/alu/adder8` */
  paths: readonly string[];
  gateCount: number;
  /** The module each gate sits in. */
  moduleOfGate: Uint32Array;
  /** Kind of each gate: 0 = NAND, 1 = flip-flop. */
  kindOfGate: Uint8Array;
  /**
   * Position of each gate among the gates of its kind in its module: the
   * number at the end of its path.
   */
  localOfGate: Uint32Array;
}

export function buildTree(net: NetlistInfo): ModuleTree {
  const count = net.moduleParents.length;
  const gateCount = net.gates.length / GATE_FIELDS;
  if (net.moduleNames.length !== count) throw new RangeError('module names and parents differ in length');

  const children: number[][] = Array.from({ length: count }, () => []);
  const depth = new Uint8Array(count);
  const height = new Uint8Array(count);
  const paths: string[] = [];
  for (let m = 0; m < count; m++) {
    const parent = net.moduleParents[m]!;
    const name = net.moduleNames[m]!;
    if (m === 0 ? parent !== -1 : parent < 0 || parent >= m) {
      throw new RangeError(`module ${m} (${name}) does not hang from an earlier module`);
    }
    if (parent >= 0) {
      children[parent]!.push(m);
      depth[m] = depth[parent]! + 1;
    }
    paths.push(parent >= 0 ? `${paths[parent]!}/${name}` : name);
  }
  for (let m = count - 1; m > 0; m--) {
    const parent = net.moduleParents[m]!;
    height[parent] = Math.max(height[parent]!, height[m]! + 1);
  }

  const moduleOfGate = new Uint32Array(gateCount);
  const kindOfGate = new Uint8Array(gateCount);
  const localOfGate = new Uint32Array(gateCount);
  const direct = new Uint32Array(count);
  // Gates of each kind seen so far in each module.
  const seen = [new Uint32Array(count), new Uint32Array(count)] as const;
  for (let g = 0; g < gateCount; g++) {
    const kind = net.gates[g * GATE_FIELDS]! === 0 ? 0 : 1;
    const module = net.gates[g * GATE_FIELDS + 4]!;
    if (module >= count) throw new RangeError(`gate ${g} is in module ${module}, which does not exist`);
    moduleOfGate[g] = module;
    kindOfGate[g] = kind;
    localOfGate[g] = seen[kind][module]!++;
    direct[module]!++;
  }
  const gates = Array.from(direct, (n) => new Uint32Array(n));
  const filled = new Uint32Array(count);
  for (let g = 0; g < gateCount; g++) {
    const module = moduleOfGate[g]!;
    gates[module]![filled[module]!++] = g;
  }
  const totalGates = Uint32Array.from(direct);
  for (let m = count - 1; m > 0; m--) totalGates[net.moduleParents[m]!]! += totalGates[m]!;

  return {
    count,
    names: net.moduleNames,
    parents: net.moduleParents,
    children,
    gates,
    totalGates,
    depth,
    height,
    paths,
    gateCount,
    moduleOfGate,
    kindOfGate,
    localOfGate,
  };
}

/** `cpu/alu/adder8/fa3/nand2`: the public name of a gate. */
export function gatePath(tree: ModuleTree, gate: number): string {
  const kind = tree.kindOfGate[gate] === 0 ? 'nand' : 'dff';
  return `${tree.paths[tree.moduleOfGate[gate]!]!}/${kind}${tree.localOfGate[gate]!}`;
}

/** The module with this path, or -1. */
export function moduleAt(tree: ModuleTree, path: string): number {
  return tree.paths.indexOf(path);
}

/**
 * What a path names: a module, a gate, or nothing. A gate's path is its
 * module's path, a slash, and `nand<N>` or `dff<N>`.
 */
export function resolvePath(tree: ModuleTree, path: string): { module: number; gate: number } | undefined {
  const module = moduleAt(tree, path);
  if (module >= 0) return { module, gate: -1 };
  const match = /^(.*)\/(nand|dff)(\d+)$/.exec(path);
  if (!match) return undefined;
  const home = moduleAt(tree, match[1]!);
  if (home < 0) return undefined;
  const kind = match[2] === 'nand' ? 0 : 1;
  const local = Number(match[3]);
  for (const gate of tree.gates[home]!) {
    if (tree.kindOfGate[gate] === kind && tree.localOfGate[gate] === local) return { module: home, gate };
  }
  return undefined;
}

/** The lowest module that contains both `a` and `b` (either may be it). */
export function commonAncestor(tree: ModuleTree, a: number, b: number): number {
  // A parent's index is always smaller than its children's, so the larger
  // index is never above the smaller one: step the larger one up.
  while (a !== b) {
    if (a > b) a = tree.parents[a]!;
    else b = tree.parents[b]!;
  }
  return a;
}

/** Is `module` inside `ancestor`, or the same? */
export function isWithin(tree: ModuleTree, module: number, ancestor: number): boolean {
  while (module > ancestor) module = tree.parents[module]!;
  return module === ancestor;
}
