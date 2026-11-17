/**
 * Whether pictures are coming too slowly. The glow costs the graphics card
 * a few milliseconds a picture on a card made for it, and far more on one
 * that is not; where it makes the scene stutter, the scene is better
 * without it. This watches the time between pictures and says, once, that
 * they are slow.
 *
 * The page only draws when there is something new to show, so a long gap
 * between two pictures is a pause, not a slow picture, and is left out.
 */

/** Pictures looked at before deciding. */
export const PACE_WINDOW = 45;
/** Slow: half of them took longer than this, in milliseconds (25 pictures a second). */
export const SLOW_MS = 40;
/** A gap longer than this, in milliseconds, is the page having had nothing to draw. */
export const PAUSE_MS = 400;

export class Pace {
  private readonly gaps: number[] = [];
  private last = -1;
  private told = false;

  /**
   * A picture was drawn at `now` (milliseconds). True the one time the last
   * `PACE_WINDOW` pictures in a row are found slow.
   */
  drew(now: number): boolean {
    const gap = this.last < 0 ? -1 : now - this.last;
    this.last = now;
    if (this.told || gap < 0) return false;
    if (gap > PAUSE_MS) {
      // What came before the pause says nothing about what comes after it.
      this.gaps.length = 0;
      return false;
    }
    this.gaps.push(gap);
    if (this.gaps.length < PACE_WINDOW) return false;
    const median = [...this.gaps].sort((a, b) => a - b)[PACE_WINDOW >> 1]!;
    this.gaps.shift();
    this.told = median > SLOW_MS;
    return this.told;
  }
}
