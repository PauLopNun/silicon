# ADR 0016 — Light in the scene is a fact about the simulation

**Status:** accepted (M6)

## Context
The scene of M4 was correct and flat: a wire at 1 was amber, a busy one a
little brighter. From afar nothing moved, because on `auto` the fast engine
runs the program there and no gate is computed. A single step changed the
picture all at once, which shows *what* changed and nothing of *how*.

Making it livelier is easy to do by cheating: sparkles on a timer, a pulse
that runs along every wire whether or not it switched. Rule 1 forbids it.
Every decision here is about which true thing each light stands for, and
how to keep the picture readable once there is light in it.

## Decisions

### 1. Glow is the excess over white, and nothing else
The scene is drawn into a picture of 16-bit floats. A shader that wants
something to give off light makes it brighter than 1; `bloom.ts` blurs
whatever is over 1 into a halo (a six-level pyramid, dual filtering) and
adds it back. Nothing is chosen to glow by its kind or its colour. Where the
driver has no float pictures, or with `?glow=off`, the scene is drawn
straight to the screen as before.

How bright a wire's light is drawn falls with how wide the wire is on
screen: light is given off per length of wire, not per pixel, or a wire
seen from close would be a white bar. The halo's strength is one number
(`GLOW`), set by eye so that letters stay readable beside a busy wire.

The grain that hides the steps of dark gradients in eight bits is ordered
(Bayer, four by four), not random: it hides them as well, and pictures of
the page are about a quarter smaller as files (642 kB to 496 kB for the
whole CPU at 1280×800).

### 2. From afar, a sample of the cycles is computed on the gates
On `auto` with the fast engine running, 256 of the clock cycles of every
frame are run on the gate engine instead (`Machine::sample_on_gates`): the
last cycles before the frame is sent, taken out of the cycles that were due,
never added to them, so the clock keeps its rate. The test for it checks
that a computer sampled this way is, cycle for cycle, the one that was not.
The page says that what it shows is a sample. With the fast engine asked
for by name nothing is sampled, and the wires are drawn grey.

Rejected: keeping the gate engine running in the background (it would run
at 5% of the clock and show wires from another moment of the program);
showing remembered activity (not live, so not shown as live).

### 3. Dashes move the way the signal goes
While the program runs, a busy wire carries dashes that move away from the
gate that drives it. `route.ts` knows the distance of every point of a wire
from its driver's pin. Their brightness is the wire's activity; their
direction is the signal's; their speed is the picture's and says nothing,
which the comment in `view.ts` states. A wire driven from outside the
netlist has no direction and carries none.

### 4. A step is played back in order of logic depth, from where the camera is
After a single step, each gate takes its new level one gate delay after the
deepest gate that feeds it, which is the order the gate engine evaluates
them in; a pulse runs along each wire that changed, from its driver to the
farthest gate it feeds. Wires that did not change show nothing. Under a
closed lid the gates' lights flare in the same order, so a closed block
shows the change passing through it.

The playback begins one gate delay before the first change in the module
the camera is in (`replay.ts`), not at depth 0. The first version began at
0: the address adder is some thirty gates deep, so looking at it the
picture stood still for two seconds and a half after every step, and the
feature looked broken. What is shallower than the starting depth is drawn
as already changed, which at that moment of the playback it is.

Rejected: playing every block at once from its own first change (blocks
would no longer be in order with each other, and a carry would seem to
arrive before the bit that causes it); compressing the depths that have no
change on view (the pace would differ from step to step for no reason a
person could see).

### 5. What would be in the way is taken away, not dimmed
- A module's wires fall back to a faint trace once the camera goes into one
  of its parts (ADR 0014). The nearest of those, between the eye and the
  part, would be translucent bars across the picture: they fade out
  altogether as they grow wider than a few pixels.
- Letters carry a dark shadow, taken from a smaller copy of the glyph sheet,
  so that a name stays readable where bright wires pass under it.
- A wire thinner than the pixel-wide ribbon drawn for it covers only its
  share of that ribbon, and its light likewise (the shader's colour is
  multiplied by the cover, and blended as such). From afar, where a
  thousand wires cross the picture, they are faint traces and what is seen
  is what moves along them; from close they are solid. Drawn at full
  strength, the whole CPU was a tangle that hid its own blocks, worst on a
  phone.
- The scene darkens towards its top and bottom edges under the words that
  lie over it there.

### 6. The glow goes where it is too heavy
The page watches the time between its pictures (`pace.ts`). If half of 45
in a row take more than 40 ms, the glow is turned off for good and the page
says so; a gap of more than 400 ms is the page having had nothing to draw,
and is not counted. A link settles it either way: `?glow=off`, `?glow=on`.

Rejected: asking the graphics card how long it took (the timer query exists
in Chrome only, and what matters is the pace the person sees, whatever the
cause); lowering the resolution first (a second thing to tune, for a
decoration).

## Consequences
- Every light can be traced to a number from the simulation: a level, an
  activity, a logic depth, a distance along a wire.
- The glow costs the graphics card 2.3 to 2.7 ms a picture at 1280×800
  (`frametime.mjs`, RTX 5060 Ti, 17% of a frame at 60 a second);
  `?glow=off` is there for a card that finds it heavy.
- Page checks cover what only a browser shows: that the sample moves on the
  wires from afar, and that a step is played back at once where the camera
  is and comes to an end. `replay.test.ts` holds the rule itself.
