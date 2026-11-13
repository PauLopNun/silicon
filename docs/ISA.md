# Silicon ISA

The instruction set of the Silicon CPU. The opcode table below is generated from
`crates/isa/src/form.rs` and kept in sync by a test; the full prose specification
(registers, flags, timing, calling convention) is being written as part of M1.

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
