# ADR 0013 — The screen is on view all the time: no page flip, programs draw in place

**Status:** accepted (M3)

## Context
The screen is ordinary memory (`docs/ISA.md` §4) and the page shows it as it
is at the moment of each display frame. A write to `FRAME` only counts
frames; nothing waits for it and nothing is latched by it.

When the first programs with moving pictures ran in the browser, two of them
flickered although every test passed:

- `maze3d` drew its map over the finished view. The renderer painted over it
  again at the start of the next frame, so the map was on the screen about an
  eighth of the time.
- `bounce` rubbed out all its balls and then drew them in their new places.
  Each ball was missing for a quarter of every tick.

The tests looked at the screen when a frame was declared finished, and there
it was always right. The check of the real page in a browser found it: the
picture changed while no key was held.

## Alternatives
1. **A page flip.** The board keeps a second copy of the screen, taken at
   every write to `FRAME`, and the page shows the copy.
2. **A register to wait on**, as home computers had for the vertical blank,
   so that programs draw while nobody is looking.
3. **Leave the computer as it is.** Programs draw so that the screen is right
   at every moment.

## Decision
The third. The computer is not changed; the two programs are.

- The renderer of `maze3d` can be told to leave a corner of the screen alone
  (`_render_corner`), and the map lives there. The player's dot is put in its
  new place before it is taken from the old one.
- `bounce` draws the balls where they now are and then takes away what is
  left of where they were.
- A program that animates is tested between frames too, not only at their
  ends.

## Why
- **It is what the hardware is.** There is one screen memory. A latched copy
  would be 2 KiB of state that nobody designed, added to hide what programs
  do; the rule that everything shown comes from the simulation would be kept
  in letter only. On the gate engine, at two or three frames a second,
  watching a frame being painted column by column is what there is to see.
- **Waiting for the display would bring the timer back.** ADR 0010 made game
  time CPU time so that the gate engine never sits idle, and ADR 0008 counts
  device time in clock cycles: the computer has no display rate to wait for.
  The display belongs to the host.
- **Drawing in place costs almost nothing.** The renderer already wrote each
  byte once, with its final value. Leaving the corner alone is six cycles
  per byte column, 220 in a frame of 86,000. `bounce` got a little longer.
- **It is how such machines were programmed.** The discipline is part of
  what the project shows.

## Consequences
- Rules for programs (`docs/LANGUAGE.md` §10): write each byte once, with
  what it is finally to hold; put the new thing before taking the old one
  away; whatever goes over a picture needs room left for it, because drawing
  over the finished picture lasts until the next frame begins.
- Movement still tears: a frame in the making is part new and part old. At
  4 MHz that is a few milliseconds and not seen; on the gate engine it is a
  wipe from left to right, shown as it happens.
- The level-end shutter of `maze3d` is drawn over the last view without
  redrawing it, for the same reason, and paced by counting.
- Regression tests: `the_picture_does_not_flicker` (the whole screen is
  unchanged while nothing moves; the map is whole at every sampled cycle of a
  walk), `bounce_never_blinks`, `the_corner_that_is_asked_for_is_left_alone`.
- The test suite did not find this; a browser did. The page check planned
  for M4 should include "the screen is still while nothing is held", and it
  should run in CI.
