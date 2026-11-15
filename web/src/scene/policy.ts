/**
 * Which engine runs the CPU, and how much of the program is computed on the
 * gates. Left to itself (`auto`) the page shows the gates at work without
 * making the program pay for it more than it must:
 *
 * - While gates are plainly in sight, or the clock is stopped, the gate
 *   engine runs the CPU: every gate is computed for every clock cycle.
 * - Otherwise the fast engine runs the program at full speed, and a few
 *   hundred clock cycles of every frame are computed on the gates as well:
 *   a sample, enough to show which wires are busy from afar.
 *
 * The decision is a function of what is in the picture and of the state of
 * the clock, and of nothing else, so a camera that stands still never makes
 * the engines flap.
 */
import type { EngineKind } from '../protocol';

/** What the person asked for: one engine always, or whichever the view needs. */
export type EngineMode = 'auto' | EngineKind;

/**
 * Clock cycles of every frame computed on the gates while the fast engine
 * runs the program in `auto`. At the gate engine's 200,000 cycles a second
 * that is about a millisecond of the worker's time in each frame; the
 * program, at 4 MHz, runs 66,667 cycles in the same frame.
 */
export const SAMPLE_CYCLES = 256;

/** What the choice of engine depends on. */
export interface EngineNeeds {
  /**
   * How much of the gates of the CPU is in sight, from 0 (every block of
   * the CPU is shut, or hidden) to 1.
   */
  inside: number;
  /** Number of broken gates. */
  broken: number;
  /** The clock is stopped. */
  paused: boolean;
}

/** The engine that should be running. */
export function engineFor(mode: EngineMode, needs: EngineNeeds): EngineKind {
  // The fast engine has no gates to break: with one broken, it would not be
  // running this CPU. (The core refuses it too.)
  if (needs.broken > 0) return 'gate';
  if (mode !== 'auto') return mode;
  // With the clock stopped the gates cost nothing, and a step taken on them
  // shows what it did.
  if (needs.paused) return 'gate';
  // Half open: gates glimpsed through a lid that is still fading out are
  // not worth slowing the program for.
  return needs.inside >= 0.5 ? 'gate' : 'fast';
}

/**
 * Clock cycles of every frame to compute on the gates while the fast engine
 * runs the program: a sample in `auto` with the CPU on view, none when the
 * fast engine alone was asked for or nothing of the CPU is shown.
 */
export function samplingFor(mode: EngineMode, cpuOnView: boolean): number {
  return mode === 'auto' && cpuOnView ? SAMPLE_CYCLES : 0;
}

/** What a link asks for with `?engine=…`; anything else, or nothing, is `auto`. */
export function parseEngineMode(text: string | null): EngineMode {
  return text === 'gate' || text === 'fast' ? text : 'auto';
}

/** Where the wires on view come from; see `FrameState.wiresFrom`. */
export type WiresFrom = 'gate' | 'sample' | 'old';

/** The words for the page: what is running and why. */
export function engineNote(mode: EngineMode, engine: EngineKind, wires: WiresFrom, needs: EngineNeeds): string {
  if (needs.broken > 0) {
    const gates = needs.broken === 1 ? 'A gate is' : `${needs.broken} gates are`;
    return `${gates} broken, so only the gate engine can run this CPU: the fast one has no gates to break.`;
  }
  if (engine === 'gate') {
    if (mode !== 'auto') return 'Every gate is computed for every clock cycle, so the program runs slower.';
    return needs.paused
      ? 'The clock is stopped, so every gate is computed: step, and watch what each step does.'
      : 'You are looking at gates, so every one is computed for every clock cycle and the program runs slower.';
  }
  if (wires === 'sample') {
    return (
      `The fast engine runs the program at full speed. ${SAMPLE_CYCLES} clock cycles of every frame are also ` +
      'computed on the gates, to show which are busy: a sample. Zoom into a block and every cycle is.'
    );
  }
  return 'Only the fast engine runs: the gates are not computed, and are shown as they were last.';
}
