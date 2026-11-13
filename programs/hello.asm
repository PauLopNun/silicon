; Writes a line on the console and stops.

        .include "lib/silicon.inc"

        ld   x, message
next:   ld   a, [x+]
        or   a, a               ; a zero ends the text
        bz   done
        st   [OUT], a
        jmp  next
done:   hlt

message:
        .byte "Hello from Silicon!\n", 0
