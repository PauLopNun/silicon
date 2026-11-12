import { describe, expect, it } from 'vitest';
import { commandQueue } from './queue';

describe('commandQueue', () => {
  // Regression: inputs toggled while the WASM core was still loading were
  // lost, leaving the buttons and the simulated circuit out of sync.
  it('keeps commands sent before start and replays them in order', () => {
    const queue = commandQueue<string>();
    const seen: string[] = [];
    queue.push('a');
    queue.push('b');
    expect(seen).toEqual([]);
    queue.start((c) => seen.push(c));
    expect(seen).toEqual(['a', 'b']);
  });

  it('delivers immediately once started', () => {
    const queue = commandQueue<number>();
    const seen: number[] = [];
    queue.start((c) => seen.push(c));
    queue.push(1);
    queue.push(2);
    expect(seen).toEqual([1, 2]);
  });

  it('does not replay old commands twice', () => {
    const queue = commandQueue<string>();
    const seen: string[] = [];
    queue.push('early');
    queue.start((c) => seen.push(c));
    queue.push('late');
    expect(seen).toEqual(['early', 'late']);
  });
});
