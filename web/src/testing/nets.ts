/** Small netlists made by hand, for tests. */
import type { NetlistInfo } from '../protocol';

/**
 * A netlist with the given module tree and `gatesIn[m]` gates directly in
 * module `m`. The gates of different modules are interleaved, so that
 * anything that groups them does it by module and not by order. Every gate
 * is a NAND of the two constants.
 */
export function treeNetlist(parents: number[], names: string[], gatesIn: number[]): NetlistInfo {
  const gates: number[] = [];
  let wire = 2;
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

/**
 * ```
 * cpu ─┬─ alu ─┬─ adder ─┬─ fa0
 *      │       │         └─ fa1
 *      │       └─ logic
 *      ├─ regs
 *      └─ pc
 * ```
 */
export const SMALL = {
  parents: [-1, 0, 1, 2, 2, 1, 0, 0],
  names: ['cpu', 'alu', 'adder', 'fa0', 'fa1', 'logic', 'regs', 'pc'],
  gatesIn: [0, 0, 0, 9, 9, 24, 61, 16],
};

export function smallNetlist(): NetlistInfo {
  return treeNetlist(SMALL.parents, SMALL.names, SMALL.gatesIn);
}
