import { describe, expect, it } from 'vitest';
import { Pace, PACE_WINDOW, PAUSE_MS, SLOW_MS } from './pace';

/** Draw `count` pictures `gap` ms apart, starting at `from`; the times at which the pace said "slow". */
function draw(pace: Pace, from: number, gap: number, count: number): number[] {
  const told: number[] = [];
  for (let i = 0; i < count; i++) if (pace.drew(from + i * gap)) told.push(i);
  return told;
}

describe('the pace of the pictures', () => {
  it('is never slow at sixty or at thirty pictures a second', () => {
    expect(draw(new Pace(), 0, 16.7, 1000)).toEqual([]);
    expect(draw(new Pace(), 0, 33.4, 1000)).toEqual([]);
  });

  it('is slow, once, after a window of slow pictures', () => {
    // The first picture has none before it: the window is full one later.
    expect(draw(new Pace(), 0, SLOW_MS + 20, 500)).toEqual([PACE_WINDOW]);
  });

  it('is not slow for a few hitches among quick pictures', () => {
    const pace = new Pace();
    let now = 0;
    let told = false;
    for (let i = 0; i < 600; i++) {
      now += i % 5 === 0 ? 120 : 16.7;
      told ||= pace.drew(now);
    }
    expect(told).toBe(false);
  });

  it('takes a long gap for a pause, and starts again after it', () => {
    const pace = new Pace();
    // Slow pictures, but never a whole window of them between two pauses.
    let now = 0;
    let told = false;
    for (let burst = 0; burst < 20; burst++) {
      for (let i = 0; i < PACE_WINDOW - 1; i++) told ||= pace.drew((now += SLOW_MS + 30));
      now += PAUSE_MS + 1;
    }
    expect(told).toBe(false);
    // Left to draw without a pause, the same pictures are found slow.
    expect(draw(pace, now, SLOW_MS + 30, 200).length).toBe(1);
  });

  it('becomes slow when quick pictures turn slow', () => {
    const pace = new Pace();
    expect(draw(pace, 0, 16.7, 300)).toEqual([]);
    const told = draw(pace, 300 * 16.7, 80, 200);
    expect(told.length).toBe(1);
    // Once more than half the window is slow.
    expect(told[0]).toBeGreaterThan(PACE_WINDOW / 2 - 2);
    expect(told[0]).toBeLessThan(PACE_WINDOW);
  });
});
