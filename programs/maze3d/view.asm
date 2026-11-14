; The view from inside a maze. Hold LEFT or RIGHT to look around.
;
; This is the renderer of maze3d on its own: a fixed place, no walking. The
; game is main.c; this is what the renderer is measured and tested with
; (crates/programs/tests/maze3d.rs).

        .include "../lib/silicon.inc"

        .ram
_player_x:      .res 2          ; cells, 8.8: high byte = cell
_player_y:      .res 2
_player_angle:  .res 1          ; 256 to a full turn; 0 is east, 64 south
        .rom

        ld   a, 0x80            ; the middle of cell (1, 1)
        st   [_player_x], a
        st   [_player_y], a
        ld   a, 1
        st   [_player_x+1], a
        st   [_player_y+1], a
        call _render_init

frame:  call _render
        st   [FRAME], a
        ld   a, [KEYS]
        mov  b, a
        and  a, KEY_LEFT
        bz   right
        ld   a, [_player_angle]
        sub  a, 2
        st   [_player_angle], a
right:  mov  a, b
        and  a, KEY_RIGHT
        bz   frame
        ld   a, [_player_angle]
        add  a, 2
        st   [_player_angle], a
        jmp  frame

; 16 x 16 cells, row by row from the north; 1 is wall.
_maze_map:
        .byte 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1
        .byte 1,0,0,0,0,0,0,0,1,0,0,0,0,0,0,1
        .byte 1,0,1,1,1,0,1,0,1,0,1,1,1,1,0,1
        .byte 1,0,1,0,0,0,1,0,0,0,1,0,0,0,0,1
        .byte 1,0,1,0,1,1,1,1,1,0,1,0,1,1,1,1
        .byte 1,0,0,0,1,0,0,0,0,0,1,0,0,0,0,1
        .byte 1,1,1,0,1,0,1,1,1,1,1,1,1,1,0,1
        .byte 1,0,0,0,1,0,0,0,0,0,0,0,0,1,0,1
        .byte 1,0,1,1,1,1,1,1,0,1,1,1,0,1,0,1
        .byte 1,0,0,0,0,0,0,1,0,1,0,0,0,1,0,1
        .byte 1,0,1,1,1,1,0,1,0,1,0,1,1,1,0,1
        .byte 1,0,1,0,0,0,0,1,0,1,0,0,0,0,0,1
        .byte 1,0,1,0,1,1,1,1,0,1,1,1,1,1,0,1
        .byte 1,0,1,0,0,0,0,0,0,0,0,0,0,1,0,1
        .byte 1,0,0,0,1,1,1,1,1,1,1,1,0,0,0,1
        .byte 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1

        .include "render.asm"
