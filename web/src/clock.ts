/**
 * Pacing the simulated clock against real time. Pure logic, used by the
 * worker's run loop.
 */

/**
 * Decides how many clock cycles are due so that simulated time follows real
 * time at a target rate.
 *
 * It keeps a debt of cycles owed. When the engine cannot keep up (the gate
 * engine at a multi-megahertz target), the debt is capped at a short window
 * instead of growing without bound, so that a later switch to a faster engine
 * does not trigger a long catch-up burst.
 */
export class Pacer {
  private debt = 0;

  /**
   * @param hz target rate in cycles per second, or `null` for "as fast as possible"
   * @param maxLagMs how much real time the simulation may fall behind before the rest is forgotten
   */
  constructor(
    public hz: number | null,
    private readonly maxLagMs = 100,
  ) {}

  /** Real time moved forward by `elapsedMs`; returns the cycles now due (possibly `Infinity`). */
  advance(elapsedMs: number): number {
    if (this.hz === null) return Infinity;
    // Never less than two cycles: a slow clock (10 Hz) owes less than one
    // cycle per window, and must keep its fractions rather than lose them.
    const cap = Math.max((this.hz * this.maxLagMs) / 1000, 2);
    this.debt = Math.min(this.debt + (this.hz * elapsedMs) / 1000, cap);
    return Math.floor(this.debt);
  }

  /** Report cycles actually run. */
  ran(cycles: number): void {
    if (this.hz !== null) this.debt -= cycles;
  }

  /** Forget what is owed (after a pause, a reset or a change of rate). */
  clear(): void {
    this.debt = 0;
  }
}

/** Smoothed measurement of a rate (cycles per second) from noisy samples. */
export class RateMeter {
  private value = 0;

  /** @param smoothing weight of each new sample, 0–1 */
  constructor(private readonly smoothing = 0.2) {}

  /** `count` events took `ms` milliseconds. */
  sample(count: number, ms: number): void {
    if (ms <= 0) return;
    const rate = (count * 1000) / ms;
    this.value = this.value === 0 ? rate : this.value + this.smoothing * (rate - this.value);
  }

  /** Current estimate; 0 before the first sample. */
  get rate(): number {
    return this.value;
  }

  reset(): void {
    this.value = 0;
  }
}

/**
 * A rate (per second) of a running total, over a sliding window of recent
 * readings. Steadier than per-frame differences when the total moves in rare
 * steps, like a count of finished frames.
 */
export class WindowRate {
  private readings: { time: number; total: number }[] = [];

  constructor(private readonly windowMs = 1000) {}

  /** The running total was `total` at `timeMs`. */
  record(timeMs: number, total: number): void {
    const newest = this.readings[this.readings.length - 1];
    // The total went backwards: the machine was reset. Start again.
    if (newest && (total < newest.total || timeMs < newest.time)) this.readings = [];
    this.readings.push({ time: timeMs, total });
    // Keep one reading at least a window old, and everything after it.
    while (this.readings.length > 2 && timeMs - (this.readings[1]?.time ?? timeMs) >= this.windowMs) {
      this.readings.shift();
    }
  }

  /** Units per second across the window; 0 until there are two readings. */
  get rate(): number {
    const oldest = this.readings[0];
    const newest = this.readings[this.readings.length - 1];
    if (!oldest || !newest || newest.time <= oldest.time) return 0;
    return ((newest.total - oldest.total) * 1000) / (newest.time - oldest.time);
  }
}

/**
 * How many cycles to run in one call so that it takes about `targetMs`,
 * given the engine's measured speed. Small until a measurement exists.
 */
export function chunkSize(rate: number, targetMs = 1, max = 1_000_000): number {
  if (rate <= 0) return 1000;
  return Math.max(1, Math.min(max, Math.floor((rate * targetMs) / 1000)));
}
