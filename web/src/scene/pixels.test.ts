import { describe, expect, it } from 'vitest';
import { MAX_PIXELS, MAX_RATIO, pixelRatioFor } from './pixels';

describe('device pixels for the scene', () => {
  it('uses the density the screen has when the picture is small', () => {
    expect(pixelRatioFor(860, 800, 1)).toBe(1);
    expect(pixelRatioFor(390, 390, 2)).toBe(2);
    expect(pixelRatioFor(860, 800, 1.5)).toBe(1.5);
  });

  it('never uses more than two each way', () => {
    expect(pixelRatioFor(390, 390, 3)).toBe(MAX_RATIO);
    expect(pixelRatioFor(390, 390, 3.5)).toBe(MAX_RATIO);
  });

  it('gives up density on a large dense screen, to about three million pixels', () => {
    // A 4K screen at twice the density: 3,000 × 2,160 device pixels of scene asked for.
    const ratio = pixelRatioFor(1500, 1080, 2);
    expect(ratio).toBeLessThan(2);
    expect(ratio).toBeGreaterThan(1);
    expect(1500 * 1080 * ratio * ratio).toBeCloseTo(MAX_PIXELS, -3);
  });

  it('never goes below one device pixel per CSS pixel, however large the picture', () => {
    expect(pixelRatioFor(3400, 2160, 1)).toBe(1);
    expect(pixelRatioFor(3400, 2160, 2)).toBe(1);
    // (A browser zoomed out reports a ratio under 1; the scene is still drawn at 1.)
    expect(pixelRatioFor(800, 600, 0.5)).toBe(1);
  });

  it('changes smoothly as the picture grows', () => {
    let last = pixelRatioFor(100, 100, 2);
    for (let side = 200; side <= 4000; side += 100) {
      const now = pixelRatioFor(side, side, 2);
      expect(now).toBeLessThanOrEqual(last);
      last = now;
    }
  });
});
