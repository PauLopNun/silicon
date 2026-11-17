import { describe, expect, it } from 'vitest';
import { planReplay, REPLAY_TAIL } from './replay';

// Modules: 0 the whole; 1 and 2 inside it; 3 inside 2.
const parents = Int32Array.from([-1, 0, 0, 2]);
// Six gates, each driving the wire of its own number.
const tree = { gateCount: 6, parents, moduleOfGate: Uint32Array.from([1, 1, 2, 3, 3, 0]) };
const depths = [0, 1, 12, 30, 31, 40];
const wires = [0, 1, 2, 3, 4, 5];
const changed = (...gates: number[]) => wires.map((w) => (gates.includes(w) ? 255 : 0));

describe('the playback of a step', () => {
  it('begins one gate delay before the first change where the camera is', () => {
    // The regression: with the camera in module 3, thirty gates deep, the
    // playback began at depth 0 and the picture stood still for seconds.
    expect(planReplay(tree, depths, wires, changed(0, 1, 2, 3, 4, 5), 3).from).toBe(29);
    expect(planReplay(tree, depths, wires, changed(0, 1, 2, 4, 5), 3).from).toBe(30);
  });

  it('counts what is inside the parts of the module the camera is in', () => {
    expect(planReplay(tree, depths, wires, changed(0, 3, 4), 2).from).toBe(29);
    expect(planReplay(tree, depths, wires, changed(0, 2, 3, 4), 2).from).toBe(11);
  });

  it('begins with the first change of all from the whole, and never before depth 0', () => {
    expect(planReplay(tree, depths, wires, changed(1, 2, 3), 0).from).toBe(0);
    expect(planReplay(tree, depths, wires, changed(0, 5), 0).from).toBe(0);
    expect(planReplay(tree, depths, wires, changed(2, 3), 0).from).toBe(11);
  });

  it('plays the rest when nothing changed where the camera is', () => {
    expect(planReplay(tree, depths, wires, changed(2, 5), 1).from).toBe(11);
  });

  it('ends a little after the deepest change anywhere', () => {
    expect(planReplay(tree, depths, wires, changed(0, 3), 3).until).toBe(30 + REPLAY_TAIL);
    expect(planReplay(tree, depths, wires, changed(0, 5), 1).until).toBe(40 + REPLAY_TAIL);
  });

  it('is nothing at all when nothing changed', () => {
    expect(planReplay(tree, depths, wires, changed(), 3)).toEqual({ from: 0, until: REPLAY_TAIL });
  });
});
