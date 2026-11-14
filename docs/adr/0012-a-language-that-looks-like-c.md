# ADR 0012 — A language that looks like C and is not C

**Status:** accepted (M3)

## Context
Programs for the computer need a language above assembly: the game logic of
`maze3d` is a page of `if`s and arithmetic that nobody should have to read
as register moves. The plan called it "a C-like compiler" and named its
scope the second-largest risk of the project. The questions were how much
of C to take, and what to do where C's rules are a poor fit for an 8-bit
CPU.

## Decision
The language of `docs/LANGUAGE.md`: four integer types (`u8`, `i8`, `u16`,
`i16`), pointers and one-dimensional arrays of them, functions, `if`,
`while`, `do`, `for`, C's operators with C's precedence, `#include`, and
`asm { … }`. It departs from C in four places on purpose:

1. **No integer promotion.** `u8 + u8` is computed in 8 bits and wraps.
   Operands of different types are brought to the wider one, unsigned if
   they differ only in sign.
2. **Nothing is undefined.** Overflow wraps; shifts by the width or more
   shift everything out; division by zero has a stated result; a function
   that must return a value and might not does not compile.
3. **`const` means "known before the program runs".** A `const` scalar is a
   typed name for a value and occupies nothing; a `const` array is a table
   in ROM.
4. **The `*` of a pointer belongs to the type**: `u8* a, b;` declares two
   pointers.

Left out: `struct`, `union`, `enum`, `typedef`, `switch`, `goto`, the comma
operator, function pointers, multi-dimensional arrays, the preprocessor
other than `#include`, floating point.

## Why
- **Promotion to `int` is the wrong default here.** In C every `u8 + u8` is
  a 16-bit addition. On this CPU, whose accumulator is 8 bits wide, 16-bit
  values cost twice as much to load and to store (two instructions each),
  three instructions to compare instead of one, and six to subtract or to
  combine bit by bit. A compiler without an optimiser to narrow things back
  would make every program slow and large for the sake of a rule most code
  does not want. The cost is one surprise, `u16 w = a * b;` wrapping at
  8 bits, which the manual shows first and a cast fixes.
- **Defined behaviour is the project's habit.** The CPU has no illegal
  opcode and no undefined state (`docs/ISA.md` §8), and the simulation is
  only reproducible because of it. A language on top that reintroduced
  "anything may happen" would be out of character, and each definition
  costs nothing: they are what the generated code does anyway.
- **`const` as a name for a value replaces `#define`** and removes the need
  for a preprocessor. In a machine whose ROM and RAM are different address
  ranges, "this table is constant" has to mean "this table is in ROM".
- **What is left out is what the programs do not need.** The largest
  program is a maze game of a few hundred lines with a handful of globals.
  `struct` would be the first thing to add if that changed; the type
  representation (`ir::Ty`) has room for it.
- **It still looks like C** because that is the notation readers know. A
  line of this language reads the same to someone who has only seen C.
- **`asm { … }` is the escape hatch** that keeps the language small: the
  renderer's inner loops stay in assembly (ADR 0010), and anything the
  language cannot say can be said there.

## Consequences
- C code cannot be pasted in unchanged; arithmetic that relies on promotion
  needs casts. `docs/LANGUAGE.md` §9 lists every difference in one table.
- The compiler is four small passes (`lex`, `parse`, `sema`, `gen`, about
  4,500 lines with their tests) and has no optimiser. Its correctness is checked by
  comparing compiled programs with a reference in plain Rust
  (`crates/cc/tests/random.rs`) rather than by reading the output.
- The order of evaluation of two operands is not fixed, as in C; unlike C,
  everything else is.
