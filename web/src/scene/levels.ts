/**
 * The logic depth of the gates of a netlist, worked out from its gate
 * table: 0 for a flip-flop, and for a NAND one more than the deeper of the
 * gates that feed it (inputs and constants count as 0).
 *
 * The page gets the depths from the simulation core, which evaluates the
 * gates in this order. This is the same thing in a few lines, for netlists
 * made by hand in tests and on the laboratory page, and as an independent
 * check on the core's: a test compares the two on the real CPU.
 */
import { GATE_FIELDS } from '../protocol';

export function gateLevels(gates: Uint32Array, wireCount: number): Uint32Array {
  const count = gates.length / GATE_FIELDS;
  const levels = new Uint32Array(count);
  // The depth of the gate driving each wire; what no NAND drives is at 0.
  const ofWire = new Uint32Array(wireCount);
  // A NAND's output is a new wire, made when the gate is: its inputs come
  // from gates made before it or from flip-flops, so one pass in the order
  // of the table is enough. A further pass that changes nothing confirms it.
  for (let pass = 0; pass < 2; pass++) {
    let changed = false;
    for (let g = 0; g < count; g++) {
      if (gates[g * GATE_FIELDS] !== 0) continue;
      const level = 1 + Math.max(ofWire[gates[g * GATE_FIELDS + 1]!]!, ofWire[gates[g * GATE_FIELDS + 2]!]!);
      if (level !== levels[g]) {
        levels[g] = level;
        ofWire[gates[g * GATE_FIELDS + 3]!] = level;
        changed = true;
      }
    }
    if (pass === 1 && changed) throw new RangeError('the gates are not in an order in which each follows what feeds it');
  }
  return levels;
}
