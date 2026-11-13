# Silicon ISA

The instruction set of the Silicon CPU: what software sees. Three things
implement this document and are tested against each other:

| What | Where |
|---|---|
| The opcode map, as data | `crates/isa/src/form.rs` |
| The hardware (microcode table → NAND gates) | `crates/machine/src/hw/` |
| The reference model, in plain integer arithmetic | `crates/sim-fast/src/lib.rs` |

A change to the instruction set touches all three and this file; the lockstep
test fails until they agree again.

## 1. At a glance

- 8-bit data, 16-bit addresses, little-endian.
- Eight 8-bit registers that pair up into four 16-bit registers.
- Instructions are 1 to 3 bytes and take 1 to 5 clock cycles.
- **Every one of the 256 opcodes is an instruction.** There is no illegal
  opcode and no undefined behaviour: any byte sequence is a program.
- At most one memory access per clock cycle.
- No interrupts. Programs wait for events by reading device registers.

## 2. Registers

| Register | Bits | Number | Role |
|---|---|---|---|
| `a` | 8 | 0 | accumulator: left operand and result of every ALU instruction |
| `b` | 8 | 1 | general purpose |
| `xl`, `xh` | 8 | 2, 3 | general purpose |
| `yl`, `yh` | 8 | 4, 5 | general purpose |
| `zl`, `zh` | 8 | 6, 7 | general purpose |
| `pc` | 16 | — | address of the next byte to fetch |
| `sp` | 16 | — | address of the last byte pushed; the stack grows downwards |
| flags | 4 | — | `Z`, `C`, `N`, `V` |

Two neighbouring registers form a **pair**, low byte in the even-numbered one:

| Pair | Number | High : low |
|---|---|---|
| `ba` | 0 | `b` : `a` |
| `x` | 1 | `xh` : `xl` |
| `y` | 2 | `yh` : `yl` |
| `z` | 3 | `zh` : `zl` |

Pairs are how the CPU holds addresses and 16-bit numbers. They are not extra
storage: writing `xl` changes `x`.

Three more registers exist in the hardware and are part of the machine's state
(they are saved and restored when the simulation switches engines), but no
instruction names them:

| Register | Bits | Role |
|---|---|---|
| `ir` | 8 | the opcode being executed |
| `step` | 3 | position inside the instruction; 0 means "fetch an opcode" |
| `t` | 16 | holds an address or offset operand while it is being read |

That is all the state there is: 127 flip-flops.

## 3. Flags

| Flag | Bit | Meaning after an arithmetic instruction |
|---|---|---|
| `Z` | 0 | the result is zero |
| `C` | 1 | carry out of the adder |
| `N` | 2 | bit 7 of the result |
| `V` | 3 | signed overflow |

Subtraction is addition of the complement (`a + ~n + 1`), so after `sub` or
`cmp` **`C` = 1 means "no borrow"**, that is `a ≥ n` for unsigned values.

Which instructions write which flags:

| Instructions | `Z` | `C` | `N` | `V` |
|---|---|---|---|---|
| `add`, `sub`, `cmp` | ✓ | ✓ | ✓ | ✓ |
| `adc`, `sbc` | cleared only | ✓ | ✓ | ✓ |
| `and`, `or`, `xor` | ✓ | — | ✓ | — |
| `inc`, `dec` | ✓ | — | ✓ | ✓ |
| `shr`, `ror`, `asr` | ✓ | bit shifted out | ✓ | — |
| `add pd, ps` | ✓ (all 16 bits) | carry out of bit 15 | bit 15 | ✓ (16-bit) |
| everything else | — | — | — | — |

Two details make multi-byte arithmetic work:

- `adc` and `sbc` can clear `Z` but never set it. After `add` on the low bytes
  and `adc` on the high bytes, `Z` is set only if the whole 16-bit result is
  zero. The same holds for `cmp` followed by `sbc`: a 16-bit comparison.
- `inc` and `dec` leave `C` alone, so a loop counter can be updated in the
  middle of a carry chain.

Loads, stores, moves, stack operations, jumps and all address arithmetic
(`lea`) never touch the flags.

## 4. Memory

The address space is 64 KiB of bytes. 16-bit values are stored low byte first.

| Range | What |
|---|---|
| `0x0000`–`0x7FFF` | ROM, 32 KiB: code and tables. Execution starts at `0x0000` |
| `0x8000`–`0x87FF` | framebuffer: 128×64 pixels, 2 bits per pixel, 4 pixels per byte |
| `0x8800`–`0x88FF` | device registers: keyboard, frame timer, debug port |
| `0x8900`–`0x8FFF` | unused, reads as 0 |
| `0x9000`–`0xFFFF` | RAM, 28 KiB. The stack starts at the top |

ROM is at the bottom and RAM at the top because of how the CPU resets (§7):
`pc` = 0 and `sp` = 0. Execution begins at address 0, and the first push
wraps to `0xFFFF`, so a program has a working stack without setting `sp`.

The CPU itself knows nothing of this map; it sees 64 KiB of bytes. The devices
and the layout of the register page are implemented with the machine in M2.

## 5. Encoding

The top two bits of the opcode pick one of four groups:

```
  00 ...... pointer, stack and 16-bit instructions    bits 1..0 = pair
  01 ddd sss   mov rd, rs
  10 ooo sss   op a, rs
  11 ggg nnn   group g; n is a register, an ALU operation or a condition
```

Fields are always in the same place, which is what keeps the decoder small:

| Field | Bits | Values |
|---|---|---|
| `r`, `rs` | 2..0 | register number |
| `rd` | 5..3 | register number |
| `pp`, `ps` | 1..0 | pair number |
| `pd` | 3..2 | pair number |
| `op` | 5..3 (`op a, rs`) or 2..0 (`op a, n`) | `add`=0 `adc`=1 `sub`=2 `sbc`=3 `and`=4 `or`=5 `xor`=6 `cmp`=7 |
| `cc` | 2..0 | condition, see below |

Operands follow the opcode: `n` is one unsigned byte, `d` one signed byte,
`nn` two bytes, low byte first.

### Opcode table

Generated from `crates/isa/src/form.rs`; a test fails if the two differ.
"Cycles" is the number of clock cycles the instruction adds to a program (§6).

<!-- opcode-table:begin -->
| Opcode | Instruction | Bytes | Cycles | Flags | Effect |
|---|---|---|---|---|---|
| `00` | `hlt` | 1 | — | — | stop until reset |
| `01` | `mov sp, x` | 1 | 2 | — | sp ← x |
| `02` | `ret` | 1 | 3 | — | pc ← mem16[sp]; sp ← sp + 2 |
| `03` | `jmp z` | 1 | 1 | — | pc ← z |
| `04`–`07` | `ld a, [pp]` | 1 | 2 | — | a ← mem[pp] |
| `08`–`0B` | `ld a, [pp+]` | 1 | 2 | — | a ← mem[pp]; pp ← pp + 1 |
| `0C`–`0F` | `st [pp], a` | 1 | 2 | — | mem[pp] ← a |
| `10`–`13` | `st [pp+], a` | 1 | 2 | — | mem[pp] ← a; pp ← pp + 1 |
| `14`–`17` | `ld a, [pp+a]` | 1 | 2 | — | a ← mem[pp + a] |
| `18`–`1B` | `ld pp, nn` | 3 | 3 | — | pp ← nn |
| `1C`–`1F` | `push pp` | 1 | 3 | — | sp ← sp − 2; mem16[sp] ← pp |
| `20`–`23` | `pop pp` | 1 | 3 | — | pp ← mem16[sp]; sp ← sp + 2 |
| `24`–`27` | `lea pp, [sp+n]` | 2 | 3 | — | pp ← sp + n |
| `28`–`2B` | `lea pp, [pp+d]` | 2 | 3 | — | pp ← pp + d |
| `2C`–`2F` | `lea pp, [pp+a]` | 1 | 2 | — | pp ← pp + a |
| `30`–`3F` | `add pd, ps` | 1 | 2 | ZCNV | pd ← pd + ps |
| `40`–`7F` | `mov rd, rs` | 1 | 1 | — | rd ← rs |
| `80`–`BF` | `op a, rs` | 1 | 1 | by op | a ← a op rs |
| `C0`–`C7` | `inc r` | 1 | 1 | ZNV | r ← r + 1 |
| `C8`–`CF` | `dec r` | 1 | 1 | ZNV | r ← r − 1 |
| `D0`–`D7` | `ld r, n` | 2 | 2 | — | r ← n |
| `D8`–`DF` | `ld r, [sp+n]` | 2 | 3 | — | r ← mem[sp + n] |
| `E0`–`E7` | `st [sp+n], r` | 2 | 3 | — | mem[sp + n] ← r |
| `E8`–`EF` | `op a, n` | 2 | 2 | by op | a ← a op n |
| `F0`–`F7` | `bcc d` | 2 | 2 / 3 taken | — | if cc: pc ← pc + d |
| `F8` | `jmp nn` | 3 | 3 | — | pc ← nn |
| `F9` | `call nn` | 3 | 5 | — | sp ← sp − 2; mem16[sp] ← pc; pc ← nn |
| `FA` | `ld a, [nn]` | 3 | 4 | — | a ← mem[nn] |
| `FB` | `st [nn], a` | 3 | 4 | — | mem[nn] ← a |
| `FC` | `lea sp, [sp+d]` | 2 | 3 | — | sp ← sp + d |
| `FD` | `shr a` | 1 | 1 | ZCN | a ← a >> 1; bit 7 ← 0; C ← old a₀ |
| `FE` | `ror a` | 1 | 1 | ZCN | a ← a >> 1; bit 7 ← old C; C ← old a₀ |
| `FF` | `asr a` | 1 | 1 | ZCN | a ← a >> 1; bit 7 kept; C ← old a₀ |
<!-- opcode-table:end -->

### Conditions

| `cc` | Branch | Taken when | After `cmp a, n` it means |
|---|---|---|---|
| 0 | `bz` | `Z` = 1 | `a` = `n` |
| 1 | `bnz` | `Z` = 0 | `a` ≠ `n` |
| 2 | `bc` | `C` = 1 | `a` ≥ `n`, unsigned |
| 3 | `bnc` | `C` = 0 | `a` < `n`, unsigned |
| 4 | `bn` | `N` = 1 | result negative |
| 5 | `bp` | `N` = 0 | result positive or zero |
| 6 | `blt` | `N` ≠ `V` | `a` < `n`, signed |
| 7 | `bge` | `N` = `V` | `a` ≥ `n`, signed |

The offset `d` is relative to the instruction after the branch, so `bz -2`
branches to itself and the range is −128…+127 bytes from there.

### Notes on individual instructions

- **`ld a, [pp+a]`** reads a table: the address is the pair plus `a`, with `a`
  taken as unsigned. One byte, two cycles.
- **`ld a, [pp+]`** and **`st [pp+], a`** use the pair as the address and then
  add 1 to it. With `pp` = `ba` the load writes `a` twice; the byte read from
  memory wins, and `b` gets the high byte of `ba + 1`.
- **`lea`** is address arithmetic: it computes an address and stores it in a
  pair (or in `sp`) without touching memory or flags. `n` is unsigned
  (0…255), `d` is signed (−128…+127). `lea sp, [sp+d]` reserves and releases
  stack space; `lea pp, [sp+0]` reads `sp`.
- **`add pd, ps`** is the 8-bit adder used twice: low bytes, then high bytes
  with the carry. `add x, x` doubles `x`.
- **`push pp`** stores the high byte first, so the pair lies in memory low
  byte first, at `sp` and `sp + 1`.
- **`call nn`** pushes the address of the instruction after it, then jumps.
  **`ret`** pops that address into `pc`.
- **`jmp z`** jumps to the address in `z`: jump tables and function pointers.
- **`hlt`** stops the CPU until reset. Its opcode is `00`, so running into
  zeroed memory halts instead of running away.

### Things spelled with other instructions

| To do this | Write |
|---|---|
| nothing (`nop`) | `mov a, a` |
| clear `a` | `xor a, a` |
| shift `a` left | `add a, a` |
| rotate `a` left through `C` | `adc a, a` |
| test `a` for zero or sign | `or a, a` |
| negate `a` | `xor a, 0xff` then `inc a` |
| compare two 16-bit values | `cmp` the low bytes, then `sbc` the high bytes |
| shift a pair left | `add pp, pp` |

There is no multiply or divide. Programs use shift-and-add loops or tables.

## 6. Timing

One clock for everything; all flip-flops change on the same edge. In each
cycle the CPU does at most one memory access.

An instruction is a sequence of **steps**, one per clock cycle:

- step 0 reads the opcode at `pc` into `ir`;
- steps 1, 2, … do the work, reading operands as they go.

Instructions whose last step leaves the memory bus free use it to fetch the
next opcode in that same cycle. That is why `mov`, the register ALU
instructions, `inc`, `dec` and the shifts cost a single cycle each, and why
jumps cost no more than reading their operand.

So the "Cycles" column is the number of clock cycles from the fetch of one
opcode to the fetch of the next: exactly what the instruction adds to the
running time of a program. The cycle count of a straight-line program is the
sum of its instructions' cycles, plus one for the very first fetch after reset.

What the multi-step stack instructions do in each step:

| Instruction | Step 1 | Step 2 | Step 3 | Step 4 | Step 5 |
|---|---|---|---|---|---|
| `push pp` | `sp`−1; store high | `sp`−1; store low | | | |
| `pop pp` | load low; `sp`+1 | load high; `sp`+1 | | | |
| `call nn` | read `nn` low | read `nn` high | `sp`−1; store `pc` high | `sp`−1; store `pc` low | fetch at `nn` |
| `ret` | load low; `sp`+1 | load high; `sp`+1 | fetch at that address | | |

The complete step-by-step description of every instruction is the microcode
table in `crates/machine/src/hw/control.rs`: one line per instruction and
step, listing the control signals raised in that cycle.

### The bus, cycle by cycle

1. The address bus and the read/write strobes depend only on flip-flops, so
   they are valid at the start of the cycle.
2. On a read, memory answers within the cycle and the byte is used at once.
3. On a write, memory changes at the clock edge that ends the cycle.

## 7. Reset and halting

Reset sets every flip-flop to 0: all registers, `pc`, `sp`, the flags, `ir`,
`step` and `t`. With `step` = 0 the first cycle fetches the opcode at address
`0x0000`.

`hlt` holds the CPU at step 1 of itself: no memory access, nothing changes.
Only reset leaves that state.

## 8. Nothing is undefined

- Every opcode is an instruction (the 64 `mov` opcodes include eight
  that move a register to itself; they do nothing, in one cycle).
- Arithmetic on `pc`, `sp` and pairs wraps around at 16 bits.
- The hardware can be placed in states that no program reaches from reset:
  a step number larger than the instruction has steps. Such a cycle does
  nothing and returns to step 0. This matters only to the simulation, which
  may be handed any state, and both engines implement it identically.
