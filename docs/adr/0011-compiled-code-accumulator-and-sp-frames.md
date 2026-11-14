# ADR 0011 — Compiled code: one accumulator, frames reached from `sp`, arguments on the stack

**Status:** accepted (M3)

## Context
The compiler (`crates/cc`) has to turn expressions and functions into
instructions for a CPU with one accumulator, three more register pairs, and
no addressing mode with a displacement except from `sp`
(`ld r, [sp+n]`, `st [sp+n], r`; `docs/ISA.md` §5). Functions written in
assembly (the renderer of `maze3d`) must be callable from compiled code and
the other way round, so the rules have to be simple enough to follow by hand.

## Decision

**Values.** While an expression is computed its value is in `a` (8 bits) or
in `ba` (16 bits). The right operand of an operation is in `xl` or `x`. A
value that has to wait while something else is computed is pushed on the
stack. `y` and `z` are scratch. Nothing is kept in a register from one
statement to the next.

**Calls.**

- The caller pushes the arguments in the order they are written, two bytes
  each (an 8-bit argument is the low byte; the other byte is unspecified),
  calls, and takes them off again.
- On entry `[sp]` is the return address, the *last* argument is at `sp+2`
  and each earlier one two bytes higher.
- The result comes back in `a`, or in `ba` if it is 16 bits wide.
- A function may change every register. The caller assumes nothing survives.

**Variables.** A function makes room for its local variables with one
`lea sp, [sp-N]` and reaches them, and its parameters, as `[sp+n]`. There is
no frame pointer: the compiler knows at every instruction how far `sp` has
moved since the function began.

**Names.** A name written in C is that name with `_` in front in assembly
(`render` is `_render`). Labels the compiler invents are `_function.L<n>`,
its runtime routines `cc_…`, string literals `cc_str<n>`. None of these can
collide with a C name.

**Start.** The ROM begins with `call _main` and `hlt` (preceded by a call
that copies initial values into RAM, if any global has one). The CPU resets
with `sp` = 0, so the first push lands at the top of RAM (ADR 0007) and no
start-up code sets the stack.

## Why
- **`sp`-relative frames are the only cheap ones.** A frame pointer in a
  pair would need `[pp+d]` loads and stores, which the instruction set does
  not have; `[sp+n]` it has, at 3 cycles. The price is the reach of `n`:
  variables, parameters and whatever is pushed at that moment must lie
  within 255 bytes of `sp`. The compiler reports a function that exceeds it
  and says to make the large variables global. In a computer with 28 KiB of
  RAM and no recursion-heavy programs that is rarely met.
- **Arguments go on the stack** because the registers are too few to pass
  them in: with one accumulator, an argument that arrived in a register
  would have to be stored at once to compute anything.
- **Two bytes per argument** because `push pp` is the only push (3 cycles).
  Pushing one byte would be `lea sp, [sp-1]` and `st [sp+0], a`: twice the
  time to save a byte of stack.
- **The caller removes the arguments** because `ret` cannot.
- **Everything is caller-saved** because the generated code never has a
  value in a register across a call anyway; saving registers in the callee
  would be work for nobody.
- **Simple code on purpose.** No register allocation, no optimisation passes
  beyond folding constants and picking the short form for constants
  (`inc a`, shifts for powers of two, `lea ba, [ba+d]`). The result is
  several times slower than hand-written assembly (every use of a variable
  is a load from the stack) and anyone can follow it in the listing, which
  in this project matters more. The one program that needs speed keeps its
  inner loop in assembly (ADR 0010).

## Consequences
- A call costs about 14 cycles beyond the work of the function: 3 per
  argument pushed, 5 for `call`, 3 for `ret`, 3 to drop the arguments.
- Writing a function in assembly for C: read arguments from `[sp+2]` up,
  leave the result in `a` or `ba`, use any register, `ret`. Calling C from
  assembly: push, `call _name`, `lea sp, [sp+2·n]`.
- `asm { … }` inside a function must leave `sp` where it found it; while it
  runs, local variables are at the offsets the compiler chose, which the
  listing shows but the language does not promise. Assembly that needs
  values takes them from globals.
- Multiplication, division and shifts by a computed count are calls to
  runtime routines (`crates/cc/src/runtime.rs`), linked only when used.
  They are tested against Rust's arithmetic for every pair of bytes and for
  sampled 16-bit pairs.
- If compiled code ever needs to be faster, the first step is keeping
  loop counters in registers, and it can be taken without changing these
  rules.
