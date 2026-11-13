import { describe, expect, it } from 'vitest';
import { chunkSize, Pacer, RateMeter, WindowRate } from './clock';

describe('Pacer', () => {
  it('owes cycles in proportion to elapsed time', () => {
    const pacer = new Pacer(4_000_000);
    expect(pacer.advance(10)).toBe(40_000);
    pacer.ran(40_000);
    expect(pacer.advance(0)).toBe(0);
  });

  it('carries over what was not run', () => {
    const pacer = new Pacer(1000);
    expect(pacer.advance(10)).toBe(10);
    pacer.ran(4);
    expect(pacer.advance(10)).toBe(16);
  });

  it('keeps fractions of a cycle for slow clocks', () => {
    const pacer = new Pacer(10); // one cycle every 100 ms
    let total = 0;
    for (let i = 0; i < 100; i++) {
      const due = pacer.advance(16);
      pacer.ran(due);
      total += due;
    }
    expect(total).toBe(16); // 1.6 s at 10 Hz
  });

  it('forgets debt beyond the lag window when the engine cannot keep up', () => {
    const pacer = new Pacer(4_000_000, 100);
    for (let i = 0; i < 50; i++) {
      pacer.advance(10);
      pacer.ran(2_000); // far slower than the target
    }
    expect(pacer.advance(0)).toBeLessThanOrEqual(400_000);
  });

  it('runs flat out when there is no target', () => {
    const pacer = new Pacer(null);
    expect(pacer.advance(5)).toBe(Infinity);
    pacer.ran(1_000_000);
    expect(pacer.advance(5)).toBe(Infinity);
  });

  it('can be cleared after a pause', () => {
    const pacer = new Pacer(1000);
    pacer.advance(50);
    pacer.clear();
    expect(pacer.advance(1)).toBe(1);
  });
});

describe('RateMeter', () => {
  it('starts at the first sample and then smooths', () => {
    const meter = new RateMeter(0.5);
    expect(meter.rate).toBe(0);
    meter.sample(1000, 1000);
    expect(meter.rate).toBe(1000);
    meter.sample(3000, 1000);
    expect(meter.rate).toBe(2000);
    meter.reset();
    expect(meter.rate).toBe(0);
  });

  it('ignores samples with no duration', () => {
    const meter = new RateMeter();
    meter.sample(5, 0);
    expect(meter.rate).toBe(0);
  });
});

describe('WindowRate', () => {
  it('measures a running total over the window', () => {
    const rate = new WindowRate(1000);
    expect(rate.rate).toBe(0);
    rate.record(0, 0);
    expect(rate.rate).toBe(0);
    for (let t = 16; t <= 2000; t += 16) rate.record(t, t * 4000); // 4 MHz
    expect(rate.rate).toBeCloseTo(4_000_000, 0);
  });

  it('is steady for totals that move in rare steps', () => {
    // A frame counter that ticks 4 times a second, sampled 60 times a second.
    const rate = new WindowRate(2000);
    for (let t = 0; t <= 6000; t += 16) rate.record(t, Math.floor(t / 250));
    expect(rate.rate).toBeGreaterThan(3.4);
    expect(rate.rate).toBeLessThan(4.6);
  });

  it('follows a change of speed within one window', () => {
    const rate = new WindowRate(1000);
    let total = 0;
    for (let t = 0; t <= 2000; t += 10) rate.record(t, (total += 1000));
    for (let t = 2010; t <= 3100; t += 10) rate.record(t, (total += 10));
    expect(rate.rate).toBeCloseTo(1000, -1); // 10 per 10 ms
  });

  it('starts again when the total goes backwards', () => {
    const rate = new WindowRate(1000);
    rate.record(0, 500);
    rate.record(100, 900);
    rate.record(200, 0); // reset
    expect(rate.rate).toBe(0);
    rate.record(300, 50);
    expect(rate.rate).toBe(500);
  });
});

describe('chunkSize', () => {
  it('aims for about a millisecond of work', () => {
    expect(chunkSize(250_000)).toBe(250);
    expect(chunkSize(4_000_000, 2)).toBe(8000);
  });

  it('is cautious before any measurement and bounded after', () => {
    expect(chunkSize(0)).toBe(1000);
    expect(chunkSize(5e9)).toBe(1_000_000);
    expect(chunkSize(10)).toBe(1);
  });
});
