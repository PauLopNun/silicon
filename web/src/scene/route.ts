/**
 * Where the wires run. Every wire of the netlist that joins two or more
 * gates becomes a few straight segments, laid out the way a chip's metal
 * layers are: a trunk running east–west on one layer, branches running
 * north–south on the layer above it, and vertical vias down to the gates.
 *
 * A wire belongs to the lowest module that holds every gate on it, and runs
 * at that module's height: wires inside an adder stay low over its gates,
 * wires between the blocks of the CPU pass above those blocks' lids. A wire
 * is shown when the module it belongs to is open.
 *
 * Every point of a wire also knows how far it is, along the wire, from the
 * gate that drives it. That is what lets a change be drawn travelling the
 * way it really goes: out of the driving gate and into the gates that read
 * it.
 *
 * This is a drawing of the connections, exact about what is joined to what.
 * It is not a routed chip: wires of the same layer may cross and overlap.
 */
import { GATE_FIELDS, type NetlistInfo } from '../protocol';
import { BRANCH_RISE, GATE_TOP, type SceneLayout } from './layout';
import { commonAncestor, type ModuleTree } from './tree';

export interface Routing {
  /** Number of segments. */
  count: number;
  /** Per segment: x, y, z of one end, then of the other. */
  ends: Float64Array;
  /** Per segment: the wire it carries. */
  wire: Uint32Array;
  /** Per segment: the module the wire belongs to. */
  module: Uint32Array;
  /**
   * Per segment: how far each of its two ends is from the output pin of the
   * gate that drives the wire, measured along the wire. Both -1 for a wire
   * that no gate drives (it comes from outside: an input).
   */
  along: Float32Array;
  /** Per segment: the same distance at the pin of its wire that is farthest from the driver. */
  reach: Float32Array;
}

/** A gate's pins, from the centre of its cell: the output on the right, the inputs on the left. */
export const PIN_X = 0.3;
export const PIN_Y = 0.13;
/** How far a trunk may sit from its driver's row, and a branch from its pin, so that wires do not all coincide. */
const TRUNK_SPREAD = 0.3;
const BRANCH_SPREAD = 0.06;

/** A number in [-1, 1] that depends only on the wire: the same wire always takes the same track. */
function spread(wire: number, salt: number): number {
  let h = Math.imul(wire ^ salt, 0x9e3779b1);
  h ^= h >>> 15;
  h = Math.imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  return ((h >>> 0) / 0xffffffff) * 2 - 1;
}

export function routeWires(net: NetlistInfo, tree: ModuleTree, layout: SceneLayout): Routing {
  const gateCount = tree.gateCount;
  const field = (g: number, f: number) => net.gates[g * GATE_FIELDS + f]!;

  // The gate that drives each wire, and the pins it feeds, as one list per wire.
  const driver = new Int32Array(net.wireCount).fill(-1);
  const fanout = new Uint32Array(net.wireCount + 1);
  const eachInput = (visit: (gate: number, wire: number, pin: number) => void) => {
    for (let g = 0; g < gateCount; g++) {
      visit(g, field(g, 1), 0);
      // A flip-flop has one input.
      if (field(g, 0) === 0) visit(g, field(g, 2), 1);
    }
  };
  for (let g = 0; g < gateCount; g++) driver[field(g, 3)] = g;
  eachInput((_, wire) => fanout[wire + 1]!++);
  for (let w = 0; w < net.wireCount; w++) fanout[w + 1]! += fanout[w]!;
  // Each sink is a gate and which of its inputs: gate * 2 + pin.
  const sinks = new Uint32Array(fanout[net.wireCount]!);
  const filled = Uint32Array.from(fanout.subarray(0, net.wireCount));
  eachInput((gate, wire, pin) => (sinks[filled[wire]!++] = gate * 2 + pin));

  const ends: number[] = [];
  const wires: number[] = [];
  const modules: number[] = [];
  const along: number[] = [];
  const reach: number[] = [];

  // Wires 0 and 1 are the constants: they are the supply rails, not signals.
  for (let w = 2; w < net.wireCount; w++) {
    const from = fanout[w]!;
    const to = fanout[w + 1]!;
    const source = driver[w]!;
    if (to - from + (source >= 0 ? 1 : 0) < 2) continue;

    // Pins as x, y pairs: the driver's output first, if there is one.
    const pins: number[] = [];
    let home = -1;
    const join = (gate: number) => {
      const module = tree.moduleOfGate[gate]!;
      home = home < 0 ? module : commonAncestor(tree, home, module);
    };
    if (source >= 0) {
      pins.push(layout.gates[source * 2]! + PIN_X, layout.gates[source * 2 + 1]!);
      join(source);
    }
    for (let s = from; s < to; s++) {
      const gate = sinks[s]! >>> 1;
      const second = (sinks[s]! & 1) === 1;
      const isNand = field(gate, 0) === 0;
      const dy = isNand ? (second ? PIN_Y : -PIN_Y) : 0;
      pins.push(layout.gates[gate * 2]! - PIN_X, layout.gates[gate * 2 + 1]! + dy);
      join(gate);
    }

    const trunkZ = layout.wireZ[home]!;
    const branchZ = trunkZ + BRANCH_RISE;
    const rise = branchZ - GATE_TOP;
    // The trunk runs along the first pin's row: the driver's, when there is one.
    const trunkY = pins[1]! + TRUNK_SPREAD * spread(w, 1);
    const shift = BRANCH_SPREAD * spread(w, 2);
    const driven = source >= 0;
    // Where the driver's branch meets the trunk, and how far along the wire that is.
    const sourceX = pins[0]! + shift;
    const toTrunk = rise + Math.abs(pins[1]! - trunkY) + BRANCH_RISE;

    const first = wires.length;
    let farthest = 0;
    /** A segment between two points, each with its distance from the driver's pin. */
    const segment = (
      x0: number,
      y0: number,
      z0: number,
      s0: number,
      x1: number,
      y1: number,
      z1: number,
      s1: number,
    ) => {
      // A pin exactly on the trunk's track needs no branch: nothing to draw.
      if (x0 === x1 && y0 === y1 && z0 === z1) return;
      ends.push(x0, y0, z0, x1, y1, z1);
      wires.push(w);
      modules.push(home);
      along.push(driven ? s0 : -1, driven ? s1 : -1);
      farthest = Math.max(farthest, s0, s1);
    };

    let west = Infinity;
    let east = -Infinity;
    for (let p = 0; p < pins.length; p += 2) {
      const x = pins[p]! + shift;
      const y = pins[p + 1]!;
      west = Math.min(west, x);
      east = Math.max(east, x);
      const across = Math.abs(y - trunkY);
      if (driven && p === 0) {
        // Out of the driver: up from its pin, across to the trunk, down onto it.
        segment(x, y, GATE_TOP, 0, x, y, branchZ, rise);
        segment(x, y, branchZ, rise, x, trunkY, branchZ, rise + across);
        segment(x, trunkY, branchZ, rise + across, x, trunkY, trunkZ, toTrunk);
      } else {
        // Into a gate that reads it: off the trunk, across, and down to the pin.
        const atTrunk = toTrunk + Math.abs(x - sourceX);
        segment(x, trunkY, trunkZ, atTrunk, x, trunkY, branchZ, atTrunk + BRANCH_RISE);
        segment(x, trunkY, branchZ, atTrunk + BRANCH_RISE, x, y, branchZ, atTrunk + BRANCH_RISE + across);
        segment(x, y, branchZ, atTrunk + BRANCH_RISE + across, x, y, GATE_TOP, atTrunk + BRANCH_RISE + across + rise);
      }
    }
    // The trunk, in two pieces if the driver joins it in the middle: along
    // each piece the distance from the driver only grows.
    if (east > west) {
      const joint = Math.min(east, Math.max(west, sourceX));
      if (joint > west) segment(joint, trunkY, trunkZ, toTrunk, west, trunkY, trunkZ, toTrunk + joint - west);
      if (east > joint) segment(joint, trunkY, trunkZ, toTrunk, east, trunkY, trunkZ, toTrunk + east - joint);
    }
    for (let s = first; s < wires.length; s++) reach.push(driven ? farthest : -1);
  }

  return {
    count: wires.length,
    ends: Float64Array.from(ends),
    wire: Uint32Array.from(wires),
    module: Uint32Array.from(modules),
    along: Float32Array.from(along),
    reach: Float32Array.from(reach),
  };
}
