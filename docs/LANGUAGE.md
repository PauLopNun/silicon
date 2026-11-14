# The Silicon C-like language

A small language with the look of C for writing programs for the Silicon
computer. The compiler (`crates/cc`) turns it into the assembly of
`docs/ISA.md`, which the assembler (`crates/asm`) turns into a ROM image.

```c
#include "lib/silicon.h"

u8 total;

void main() {
    for (u8 i = 1; i <= 10; i++) total += i;
    *OUT = '0' + total / 10;
    *OUT = '0' + total % 10;
}
```

It is **not C**. It keeps what an 8-bit computer with 28 KiB of memory can
use and leaves the rest out, and in a few places it deliberately differs
(§9). The reasons are in [ADR 0012](adr/0012-a-language-that-looks-like-c.md);
how the code is generated is in
[ADR 0011](adr/0011-compiled-code-accumulator-and-sp-frames.md).

## 1. Types

| Type | Size | Values |
|---|---|---|
| `u8` | 1 byte | 0 … 255 |
| `i8` | 1 byte | −128 … 127 |
| `u16` | 2 bytes | 0 … 65,535 |
| `i16` | 2 bytes | −32,768 … 32,767 |
| `T*` | 2 bytes | the address of a `T` |
| `T name[n]` | n × size of `T` | an array of `n` values |
| `void` | — | "nothing": only as what a function returns |

There is nothing else: no `char`, `int`, `float`, `struct`, `enum`,
`typedef`, no pointers to functions, no arrays of arrays (write
`grid[y * WIDTH + x]`).

## 2. Numbers, characters and text

```c
42      0x2a      0b0010_1010      1_000        'A'      '\n'
```

Digits may be grouped with `_`. A character is the number of its ASCII code.
The escapes are `\n` `\r` `\t` `\0` `\\` `\'` `\"` and `\x41`.

`"text"` is the address (`u8*`) of those bytes in ROM, followed by a zero.
Two strings side by side are one string. Comments are `// …` and `/* … */`.

## 3. Variables

```c
u8 lives = 3;                   // outside any function: a global, in RAM
u16 score;                      // starts as 0
u8 board[64];                   // 64 bytes, all 0
u16 scores[] = { 100, 250 };    // the size is counted
u8 name[] = "Ada";              // 4 bytes: the text and its zero

const u8 SIZE = 8;              // a name for a value; takes no memory
const u8 shades[] = { 0, 0x55, 0xaa, 0xff };    // a table, kept in ROM
const u8* SCREEN = (u8*)0x8000;                 // a name for an address
```

- **Globals** live in RAM from `0x9000` up. One without a value starts as 0.
  A value must be known before the program runs: a number, the address of
  another global, or arithmetic on those.
- **`const` with one value** is a name for that value, like `#define` in C
  but with a type. It occupies nothing. **`const` arrays** are stored in
  ROM and cannot be written.
- **Locals** are declared anywhere inside a function and exist until the
  end of the block they are in. `u8 n = n + 1;` in an inner block means the
  outer `n` plus one. A local without a value holds whatever was there
  before. Arrays inside a function cannot be given values where they are
  declared.
- In `u8* a, b;` **both** are pointers: the `*` belongs to the type.

## 4. Arithmetic

Operators and their precedence are C's:

```text
()  []  x++  x--                         highest
-x  ~x  !x  *p  &x  ++x  --x  (type)x  sizeof
*  /  %
+  -
<<  >>
<  <=  >  >=
==  !=
&
^
|
&&
||
c ? a : b
=  +=  -=  *=  /=  %=  &=  |=  ^=  <<=  >>=    lowest
```

### The type of an operation

**An operation is done in the type of its operands**, and wraps around in
that type. There is no promotion to a larger "int":

```c
u8 a = 200, b = 100;
u16 w = a + b;          // 44: the sum is made in 8 bits, then stored
u16 v = (u16)a + b;     // 300
```

When the two operands have different types:

| Operands | Done in |
|---|---|
| same type | that type |
| `u8` and `i8` | `u8` |
| `u16` and `i16` | `u16` |
| 8 bits and 16 bits | the 16-bit type; the 8-bit operand is widened first, keeping its value (so an `i8` keeps its sign) |

**A number written in the program takes the type of what it is combined
with**, if it fits there: in `a + 1` the `1` is whatever `a` is. If it does
not fit in 8 bits the operation is done in 16 (`a + 300` is a 16-bit sum);
if it fits in nothing, that is an error. A number that stands alone takes
the type of the place it is going to (`u16 w = 5`), or else the smallest
type that holds it. Arithmetic between numbers (`60 * 1000 / 8`) is done
exactly, at compile time.

Assigning converts between integer types without asking: a wider value
loses its high byte, a narrower one is widened. Pointers are never
converted silently, except that `0` can be given to any pointer.

### What the operators do

- `/` rounds towards zero and `%` has the sign of its left operand, as in
  C. **Dividing by zero is defined**: the quotient is all ones for unsigned
  numbers (255 or 65,535), −1 or 1 for signed ones (−1 when the left
  operand is not negative), and the remainder is the left operand. Dividing
  by a zero written in the program is an error.
- `>>` brings in zeroes for unsigned numbers and copies of the sign for
  signed ones. A shift by the width of the type or more shifts everything
  out. The result has the type of the left operand; the count can be any
  integer, of which the low byte is used.
- Comparisons give a `u8`, 1 or 0. They compare as signed numbers only when
  both operands are signed: **`i8 < u8` compares the two as `u8`**.
- `&&` and `||` evaluate their right side only if needed, and give 1 or 0.
  Anything that is not zero is true; a pointer can be a condition.
- `sizeof(type)` and `sizeof x` are sizes in bytes; of an array, the whole
  array.
- `(type)x` converts: integers as assignment does, pointers to and from
  integers and each other.

The CPU has no instruction to multiply, divide or shift by a variable
count; these call small routines that work a bit at a time and take tens
to hundreds of clock cycles. Multiplying or
dividing an unsigned number by a constant power of two is a shift, and
shifts by a constant are done in place.

### Order

Arguments of a call are evaluated from left to right, and `&&`, `||` and
`?:` evaluate what C says they do. **The two operands of any other operator
may be evaluated in either order**, so do not change a variable and use it
in the same expression (`a + (a = 5)`, `t[i] = i++`).

## 5. Pointers and arrays

```c
u8* p = board;          // an array is used as the address of its first element
p[3] = 7;               // the same as *(p + 3) = 7
p += 2;   p++;          // moves by whole elements: 2 bytes each for a u16*
u16* q = &scores[1];
if (p == board + 3 && q != 0) …

*(u8*)0x8801 = 0;       // any address can be read or written
```

A pointer can be moved by a number, compared with a pointer of the same
type or with 0, and followed with `*` or `[ ]`. Two pointers cannot be
subtracted (cast them to `u16` first). Nothing checks that an index is
inside its array.

`a[i] += 1`, `(*p)++` and the like are allowed when finding the place has no
side effects; `*p++ += 1` is refused (the place would be found twice).

## 6. Statements

```c
if (c) … else …
while (c) …
do … while (c);
for (u8 i = 0; i < n; i++) …      // i exists only inside the loop
break;   continue;   return;   return value;
{ … }                             // a block, with its own variables
```

There is no `switch` and no `goto`.

## 7. Functions

```c
u16 area(u8 width, u8 height) {
    return (u16)width * height;
}

void main() {
    u16 a = area(12, 30);
}
```

- A program starts at `main`, which takes no arguments. When `main`
  returns, the computer halts.
- Functions can be called before the place they are written; no prototypes
  are needed. A prototype (`u8 twice(u8 n);`) is for a function written in
  assembly.
- Arguments are passed by value and converted like assignments. Arrays are
  passed as pointers (`void clear(u8* cells, u8 count)`).
- A function that returns a value must end every path with a `return`; the
  compiler checks this by the shape of the code.
- Recursion works. The stack grows down from the top of RAM, and nothing
  checks that it does not run into the globals.
- The local variables and parameters of a function, plus whatever it has
  pushed at the moment, must fit in 255 bytes. A larger array belongs
  outside the function.

## 8. Files and assembly

`#include "file"` reads another file in place. Paths are relative to the
file that includes; a file is read only once however often it is included,
so headers need no guards. There is no other `#` directive.

`asm { … }` holds assembly, as a statement or between functions:

```c
u8 twice(u8 n);                 // written below, in assembly

asm {
_twice: ld   a, [sp+2]          ; the last argument is at sp+2,
        add  a, a               ; earlier ones two bytes higher each
        ret                     ; the result is in a (or in ba)
}

void beep() {
    asm { st [0x8804], a }
}
```

- A name in C is the same name with `_` in front in assembly.
- On one line the block ends at the last `}` of the line; otherwise at the
  first `}` that begins a line.
- Local labels (`.loop`) inside a function belong to that function.
- Assembly inside a function may use every register but must leave `sp` as
  it found it. It reaches variables as globals (`[_name]`).
- A function with a result may end in an `asm` block that leaves the result
  in `a` or `ba`.
- `.include "file.asm"` inside `asm` brings in a whole assembly file;
  `maze3d` includes its renderer that way.

## 9. Where it differs from C

| | C | Here |
|---|---|---|
| `u8 + u8` | computed as `int` | computed in 8 bits, wraps |
| `u8* a, b;` | `b` is a `u8` | both are pointers |
| `const` | a variable that cannot be assigned | a value with a name, or a table in ROM |
| dividing by zero | undefined | defined (§4) |
| shifting by the width or more | undefined | shifts everything out |
| signed overflow | undefined | wraps |
| a local used before it is set | undefined | holds what the stack held |
| falling off the end of a function that returns a value | undefined | does not compile |
| `#define`, `struct`, `switch`, `goto`, `,` operator, function pointers | yes | no |

## 10. The computer from C

`programs/lib/silicon.h` names the screen, the keys and the other devices
(`docs/ISA.md` §4):

```c
#include "lib/silicon.h"

void main() {
    for (;;) {
        if (*KEYS & KEY_A) SCREEN[0] = 0xff;    // four bright pixels
        *FRAME = 0;                             // "this frame is finished"
    }
}
```

## 11. Building

```text
cargo run -p silicon-cc --example cc -- programs maze3d/main.c          the assembly
cargo run -p silicon-cc --example cc -- --rom programs maze3d/main.c    the size of the ROM
```

In Rust, `silicon_cc::build(main, &files)` gives the ROM image, the symbols
and the map from addresses to source lines. Errors name the file, line and
column of the C source, also when it is the assembler that finds them.

The compiler is tested against a reference written in plain Rust: random
programs must print what the reference computes
(`crates/cc/tests/random.rs`), and multiplication, division and shifts are
compared with Rust's for every pair of bytes
(`crates/cc/tests/arithmetic.rs`).
