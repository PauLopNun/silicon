/**
 * Which engine runs the CPU. Left to itself the page uses the gate engine
 * exactly while there is something in sight that only it computes: the
 * wires and gates inside the CPU. With the CPU closed, or out of sight
 * altogether, nothing shown depends on them and the fast engine runs the
 * program at its proper speed.
 *
 * The decision is a function of what is in the picture and of nothing else,
 * so a camera that stands still never makes the engines flap.
 */
import type { EngineKind } from '../protocol';

/** What the person asked for: one engine always, or whichever the view needs. */
export type EngineMode = 'auto' | EngineKind;

/**
 * The engine that should be running.
 *
 * @param inside how much of the gates of the CPU is in sight, from 0 (every
 *   block of the CPU is shut, or hidden) to 1
 */
export function engineFor(mode: EngineMode, inside: number): EngineKind {
  if (mode !== 'auto') return mode;
  // Half open: gates glimpsed through a lid that is still fading out are
  // shown as they were last computed, not worth slowing the program for.
  return inside >= 0.5 ? 'gate' : 'fast';
}

/** What a link asks for with `?engine=…`; anything else, or nothing, is `auto`. */
export function parseEngineMode(text: string | null): EngineMode {
  return text === 'gate' || text === 'fast' ? text : 'auto';
}

/** The words for the page: what is running and why. */
export function engineNote(mode: EngineMode, engine: EngineKind, inside: number): string {
  if (engine === 'gate') {
    return mode === 'auto'
      ? 'You are looking inside the CPU, so every gate is being simulated and the program runs slower.'
      : 'Every gate is being simulated, so the program runs slower.';
  }
  if (mode === 'auto') {
    return 'No gate is plainly in sight, so the fast engine runs the program at full speed. Zoom into a block to see its gates work.';
  }
  if (inside > 0) return 'The fast engine does not compute individual gates: these are as the gate engine left them.';
  return 'The fast engine runs the program at full speed.';
}
