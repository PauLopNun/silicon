; Diagonal stripes that scroll one step per timer tick. Holding LEFT
; reverses them.
;
; It uses the whole machine: a table in ROM, a variable in RAM, the screen,
; the keys, the frame marker and the timer.

        .include "lib/silicon.inc"

        .ram
offset: .res 1                  ; how far the stripes have scrolled
        .rom

        ld   y, shades
frame:  ld   x, SCREEN
        ld   zh, SCREEN_HEIGHT  ; rows left
row:    ld   a, [offset]
        add  a, zh
        mov  zl, a              ; phase of the first byte of this row
        ld   b, ROW_BYTES       ; bytes left in the row
byte:   mov  a, zl
        and  a, 7
        ld   a, [y+a]           ; shade from the table
        st   [x+], a
        inc  zl
        dec  b
        bnz  byte
        dec  zh
        bnz  row

        st   [FRAME], a         ; frame finished
        ld   a, [KEYS]
        and  a, KEY_LEFT
        ld   a, [offset]        ; (loads leave the flags alone)
        bnz  back
        inc  a
        jmp  store
back:   dec  a
store:  st   [offset], a

        ld   a, [TIMER_HI]      ; wait for the next tick
        mov  b, a
wait:   ld   a, [TIMER_HI]
        cmp  a, b
        bz   wait
        jmp  frame

; One period of the pattern. Each byte is four pixels of the same shade.
shades: .byte 0x00, 0x55, 0xaa, 0xff, 0xff, 0xaa, 0x55, 0x00
