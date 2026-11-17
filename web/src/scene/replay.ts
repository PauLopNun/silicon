/**
 * The playback of a single step. What a clock cycle changed is shown again
 * in the order the gate engine works it out: a gate changes one gate delay
 * after the deepest gate that feeds it (its logic depth).
 *
 * The whole CPU changes in that order, and the block on view may be thirty
 * gates deep: played from depth 0 it would stand still for seconds while
 * the change crossed blocks that are not in the picture. So the playback
 * begins one gate delay before the first change in the module the camera is
 * in. What is shallower is shown as already changed, which by then it is.
 */
import type { ModuleTree } from './tree';

/** The playback goes on this many gate delays past the deepest gate that changed, while the last pulses fade. */
export const REPLAY_TAIL = 6;

export interface ReplayPlan {
  /** The logic depth the playback begins at. */
  from: number;
  /** The logic depth at which it is over. */
  until: number;
}

/**
 * @param depthOfGate per gate, its logic depth
 * @param outputOfGate per gate, the wire it drives
 * @param activity per wire, how much it changed in the step; 0 if it did not
 * @param focus the module the camera is in
 */
export function planReplay(
  tree: Pick<ModuleTree, 'gateCount' | 'moduleOfGate' | 'parents'>,
  depthOfGate: ArrayLike<number>,
  outputOfGate: ArrayLike<number>,
  activity: ArrayLike<number>,
  focus: number,
): ReplayPlan {
  let first = Infinity;
  let firstInFocus = Infinity;
  let last = 0;
  for (let g = 0; g < tree.gateCount; g++) {
    if (activity[outputOfGate[g]!] === 0) continue;
    const depth = depthOfGate[g]!;
    first = Math.min(first, depth);
    last = Math.max(last, depth);
    if (depth >= firstInFocus) continue;
    // Parents come before their children: walking up stops at the focus or above it.
    let m = tree.moduleOfGate[g]!;
    while (m > focus) m = tree.parents[m]!;
    if (m === focus) firstInFocus = depth;
  }
  // Nothing changed where the camera is: the playback is of the rest.
  const from = Number.isFinite(firstInFocus) ? firstInFocus : Number.isFinite(first) ? first : 0;
  return { from: Math.max(0, from - 1), until: last + REPLAY_TAIL };
}
