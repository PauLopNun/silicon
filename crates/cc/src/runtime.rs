//! What the CPU cannot do in one instruction, written once in assembly:
//! multiplying, dividing, shifting by a count that is not known in advance.
//! A program gets only the routines it uses.
//!
//! They all take their left operand in `a` (8 bits) or `ba` (16 bits) and
//! the right one in `xl` or `x`, like the code the compiler generates
//! around them, and leave the result where the left operand was. Their
//! names begin with `cc_`, which no name written in C can (those begin
//! with `_`).

/// A routine: its name, the routines it calls, its text.
pub(crate) struct Routine {
    pub name: &'static str,
    pub needs: &'static [&'static str],
    pub text: &'static str,
}

pub(crate) const ROUTINES: &[Routine] = &[
    Routine {
        name: "cc_init_data",
        needs: &[],
        text: "
; Give the variables that start with a value their values: RAM is all
; zeroes when the computer is switched on, so they are copied from ROM.
cc_init_data:
        ld   x, cc_data_init
        ld   y, cc_data
        ld   z, cc_data_end - cc_data
.byte:  ld   a, [x+]
        st   [y+], a
        lea  z, [z-1]
        mov  a, zl
        or   a, zh
        bnz  .byte
        ret
",
    },
    Routine {
        name: "cc_mul8",
        needs: &[],
        text: "
; a <- a * xl (the low 8 bits). Shift and add: for each 1 bit of xl, from
; the lowest, add a to the product; double a at every step.
cc_mul8:
        mov  yl, a              ; yl = a, doubling
        ld   zl, 0              ; zl = the product so far
.bit:   mov  a, xl
        shr  a                  ; the next bit of xl falls into the carry
        mov  xl, a
        bnc  .skip
        mov  a, zl
        add  a, yl
        mov  zl, a
.skip:  mov  a, yl
        add  a, a
        mov  yl, a
        mov  a, xl
        or   a, a
        bnz  .bit               ; until no 1 bits are left
        mov  a, zl
        ret
",
    },
    Routine {
        name: "cc_mul16",
        needs: &[],
        text: "
; ba <- ba * x (the low 16 bits). As cc_mul8, with pairs.
cc_mul16:
        mov  yl, a
        mov  yh, b              ; y = ba, doubling
        ld   z, 0               ; z = the product so far
.bit:   mov  a, xh
        shr  a
        mov  xh, a
        mov  a, xl
        ror  a
        mov  xl, a              ; x is halved; its lowest bit is in the carry
        bnc  .skip
        add  z, y
.skip:  add  y, y
        mov  a, xl
        or   a, xh
        bnz  .bit
        mov  a, zl
        mov  b, zh
        ret
",
    },
    Routine {
        name: "cc_udivmod8",
        needs: &[],
        text: "
; a <- a / xl, and xh <- the remainder; unsigned. Long division: bring the
; bits of a down one at a time, from the top, into a remainder, and take xl
; out of it whenever it fits. Dividing by 0 gives 255, remainder a.
cc_udivmod8:
        mov  yl, a              ; yl: what is left of a, then the quotient
        ld   xh, 0              ; xh = the remainder
        ld   yh, 8
.bit:   mov  a, yl
        add  a, a               ; the top bit of a goes into the carry ...
        mov  yl, a
        mov  a, xh
        adc  a, a               ; ... and into the bottom of the remainder
        bc   .fits              ; nine bits: more than any divisor
        cmp  a, xl
        bnc  .next
.fits:  sub  a, xl
        inc  yl                 ; a 1 in the quotient
.next:  mov  xh, a
        dec  yh
        bnz  .bit
        mov  a, yl
        ret
",
    },
    Routine {
        name: "cc_udivmod16",
        needs: &[],
        text: "
; ba <- ba / x, and y <- the remainder; unsigned. As cc_udivmod8.
; Dividing by 0 gives 65535, remainder ba.
cc_udivmod16:
        ld   y, 0               ; y = the remainder
        ld   zl, 16
.bit:   add  ba, ba             ; the top bit of ba goes into the carry ...
        mov  zh, a              ; (a is needed; its bottom bit is now free)
        mov  a, yl
        adc  a, a
        mov  yl, a
        mov  a, yh
        adc  a, a
        mov  yh, a              ; ... and into the bottom of the remainder
        bc   .fits              ; seventeen bits: more than any divisor
        mov  a, yl
        cmp  a, xl
        mov  a, yh
        sbc  a, xh
        bnc  .next              ; smaller than the divisor
.fits:  mov  a, yl
        sub  a, xl
        mov  yl, a
        mov  a, yh
        sbc  a, xh
        mov  yh, a
        inc  zh                 ; a 1 in the quotient
.next:  mov  a, zh
        dec  zl
        bnz  .bit
        ret
",
    },
    Routine {
        name: "cc_sdivmod8",
        needs: &["cc_udivmod8"],
        text: "
; a <- a / xl, and xh <- the remainder; signed. The quotient is rounded
; towards zero and the remainder has the sign of a, as in C. Done on the
; sizes of the two numbers; the signs are put back afterwards.
cc_sdivmod8:
        mov  zl, a              ; zl: its top bit is the sign of a
        xor  a, xl
        mov  zh, a              ; zh: its top bit is the sign of the quotient
        mov  a, xl
        or   a, a
        bp   .right
        xor  a, 0xff
        inc  a
        mov  xl, a              ; xl = its size
.right: mov  a, zl
        or   a, a
        bp   .left
        xor  a, 0xff
        inc  a                  ; a = its size
.left:  call cc_udivmod8        ; (changes neither zl nor zh)
        mov  yl, a
        mov  a, zl
        or   a, a
        bp   .rem
        mov  a, xh
        xor  a, 0xff
        inc  a
        mov  xh, a
.rem:   mov  a, zh
        or   a, a
        mov  a, yl              ; (a mov keeps the flags)
        bp   .done
        xor  a, 0xff
        inc  a
.done:  ret
",
    },
    Routine {
        name: "cc_neg16",
        needs: &[],
        text: "
; ba <- -ba. Changes xl.
cc_neg16:
        xor  a, 0xff
        add  a, 1
        mov  xl, a
        mov  a, b
        xor  a, 0xff            ; (xor leaves the carry alone)
        adc  a, 0
        mov  b, a
        mov  a, xl
        ret
",
    },
    Routine {
        name: "cc_sdivmod16",
        needs: &["cc_udivmod16", "cc_neg16"],
        text: "
; ba <- ba / x, and y <- the remainder; signed. As cc_sdivmod8.
cc_sdivmod16:
        mov  zl, a              ; (keep the low byte while a is busy)
        mov  a, b
        mov  yl, a              ; yl: its top bit is the sign of ba
        xor  a, xh
        mov  yh, a              ; yh: its top bit is the sign of the quotient
        push y
        mov  a, xh
        or   a, a
        bp   .right
        mov  a, xl              ; x = -x
        xor  a, 0xff
        add  a, 1
        mov  xl, a
        mov  a, xh
        xor  a, 0xff
        adc  a, 0
        mov  xh, a
.right: mov  a, b
        or   a, a
        mov  a, zl              ; ba is whole again (a mov keeps the flags)
        bp   .left
        mov  zl, xl             ; (cc_neg16 changes xl)
        call cc_neg16
        mov  xl, zl
.left:  call cc_udivmod16       ; ba = quotient, y = remainder
        pop  z                  ; zl, zh = the two signs
        mov  xl, a
        mov  a, zh
        or   a, a
        mov  a, xl
        bp   .quot
        call cc_neg16
.quot:  mov  xl, a
        mov  a, zl
        or   a, a
        bp   .done
        mov  a, yl              ; y = -y
        xor  a, 0xff
        add  a, 1
        mov  yl, a
        mov  a, yh
        xor  a, 0xff
        adc  a, 0
        mov  yh, a
.done:  mov  a, xl
        ret
",
    },
    Routine {
        name: "cc_shl8",
        needs: &[],
        text: "
; a <- a << xl
cc_shl8:
        inc  xl
        jmp  .test
.bit:   add  a, a
.test:  dec  xl
        bnz  .bit
        ret
",
    },
    Routine {
        name: "cc_shr8",
        needs: &[],
        text: "
; a <- a >> xl, zeroes coming in
cc_shr8:
        inc  xl
        jmp  .test
.bit:   shr  a
.test:  dec  xl
        bnz  .bit
        ret
",
    },
    Routine {
        name: "cc_sar8",
        needs: &[],
        text: "
; a <- a >> xl, keeping the sign
cc_sar8:
        inc  xl
        jmp  .test
.bit:   asr  a
.test:  dec  xl
        bnz  .bit
        ret
",
    },
    Routine {
        name: "cc_shl16",
        needs: &[],
        text: "
; ba <- ba << xl
cc_shl16:
        inc  xl
        jmp  .test
.bit:   add  ba, ba
.test:  dec  xl
        bnz  .bit
        ret
",
    },
    Routine {
        name: "cc_shr16",
        needs: &[],
        text: "
; ba <- ba >> xl, zeroes coming in
cc_shr16:
        inc  xl
        jmp  .test
.bit:   mov  xh, a
        mov  a, b
        shr  a                  ; the bit that leaves b ...
        mov  b, a
        mov  a, xh
        ror  a                  ; ... enters a at the top
.test:  dec  xl
        bnz  .bit
        ret
",
    },
    Routine {
        name: "cc_sar16",
        needs: &[],
        text: "
; ba <- ba >> xl, keeping the sign
cc_sar16:
        inc  xl
        jmp  .test
.bit:   mov  xh, a
        mov  a, b
        asr  a
        mov  b, a
        mov  a, xh
        ror  a
.test:  dec  xl
        bnz  .bit
        ret
",
    },
];
