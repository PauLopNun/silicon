# ADR 0015 — The learning tools use the real hardware, not a model of it

**Status:** accepted (M5)

## Context
M5 adds four ways to learn from the computer: following it a cycle at a
time (explain), breaking gates, a memory made of gates, and a guided tour.
Each could have been written as a separate illustration: a hand-made
animation of an instruction, a picture of a broken adder, a diagram of a
memory. That would have been easier and would have drifted from the
hardware the first time the hardware changed. Rule 1 of the project is
that nothing shown about the machine is made up.

## Decisions

### Explain reads the microcode table the gates are built from
`Machine::explain` lists, for the cycle about to run, the control signals
of `control::control_word(ir, step, flags)`: the plain-Rust reading of the
microcode table that the control unit's PLA is generated from and tested
against for every opcode, step and flag value. The signal descriptions are
the table's own. Which instruction is running comes from
`Machine::opcode_at`, observed on both engines when they read an opcode
and tested to agree with each other, cycle by cycle, and with the
assembler.

### A broken gate is rewired, and only the gate engine can run it
A stuck-at fault is made by rewiring the gate in the simulator's compiled
arrays: a NAND stuck at 1 takes both inputs from the constant 0, one stuck
at 0 from the constant 1; a stuck flip-flop latches a constant. The inner
loop does not change, so faults cost nothing. Mending restores the wiring
from the netlist.

The fast engine has no gates to break. With a fault present it would run
a different CPU from the one on screen, so `Machine::set_engine` refuses
it until every gate is mended, and the page says why. Faults survive reset
and program loads: the hardware is broken, not the program.

Rejected: masking outputs in the inner loop (a branch or a mask per gate
per cycle, for a feature used rarely); modelling faults in the fast
engine (it would need the netlist, which is what it exists to avoid).

### The memory made of gates is a netlist of its own
ADR 0002 keeps the computer's 64 KiB behavioural. To still show how memory
works, `machine::exhibit` builds 8 words of 4 bits with an address
decoder, a write enable per word and a read multiplexer, from `nand` and
`dff` only, as a separate netlist with its own simulator. The CPU netlist
is untouched, so the CPU's size, speed and lockstep are unaffected. It is
tested against a plain-Rust reference like every other block.

Rejected: putting it on the computer's bus (it would have to be the
computer's memory, which is ADR 0002's whole problem), or making it out of
cross-coupled NANDs (the simulator rejects combinational loops by design,
and the CPU's registers are flip-flops too; showing a different kind of
storage from the one the CPU uses would teach the wrong thing).

### The tour is data, checked against the netlists
The steps of the tour name places by their public paths and state facts
about them. A test resolves every path in the real netlists and checks
every fact stated (nine NANDs to a full adder, eight flip-flops to a
register, 127 flip-flops in the CPU…), so a change to the hardware that
makes a lesson wrong fails the build.

## Consequences
- The learning tools cannot disagree with the machine: they read it.
- A change to the microcode changes the explanations with it; a change to
  the structure of the CPU may fail the tour's test, on purpose.
