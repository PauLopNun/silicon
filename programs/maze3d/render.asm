; maze3d: the renderer.
;
; Draws what the player sees: for each of the 128 columns of the screen it
; sends a ray through the map until it meets a wall, and draws that wall as
; a vertical line whose height says how far away it is.
;
; It is written to be called from C, which spells its names with a leading
; underscore:
;
;       _render_init    call once: prepares the map
;       _render         draws one frame
;
; and it expects the caller to define
;
;       _maze_map       16 x 16 bytes (in ROM or in RAM), row by row; 0 is floor
;       _player_x       2 bytes: where the player is, in cells; the high
;       _player_y         byte is the cell, the low byte the place inside it
;       _player_angle   1 byte: direction of view, 256 to a full turn;
;                         0 looks east (+x) and 64 south (+y)
;       _render_corner  1 byte: how many rows (0...63) to leave alone at
;                         the top of the first four screen bytes: a corner
;                         16 pixels wide for the caller to draw in. With 0
;                         the whole screen is drawn
;
; The map must have walls all around: a ray stops only at a wall.
;
; There is no multiply instruction and no divide, so everything that needs
; one is a table (maze3d/tables.inc) and 16-bit additions.

        .include "tables.inc"

        .ram
        .align 256
maps:   .res 1024               ; the map and three mirror images of it
delta:  .res 64                 ; see "drawing" below
        .rom

; ---------------------------------------------------------------- the map
;
; A ray moves through the map one cell at a time: one step along x or one
; step along y. Rather than have four versions of that loop, one for each
; combination of directions, there are four copies of the map, mirrored so
; that in copy q a ray of quadrant q always moves towards higher x and
; higher y. In a mirror image, cell number c is cell number c XOR mask:
; XOR with 0x0f turns column x into 15 - x, and 0xf0 does it to the row.

_render_init:
        ld   x, _maze_map
        ld   zl, 0              ; cell number
.cell:  ld   a, [x+]
        mov  b, a               ; b = what is in this cell
        ld   yh, hi(maps)       ; quadrant 0: as it is
        mov  yl, zl
        st   [y], a
        inc  yh                 ; quadrant 1: x mirrored
        mov  a, zl
        xor  a, 0x0f
        mov  yl, a
        mov  a, b
        st   [y], a
        inc  yh                 ; quadrant 2: both mirrored
        mov  a, zl
        xor  a, 0xff
        mov  yl, a
        mov  a, b
        st   [y], a
        inc  yh                 ; quadrant 3: y mirrored
        mov  a, zl
        xor  a, 0xf0
        mov  yl, a
        mov  a, b
        st   [y], a
        inc  zl
        bnz  .cell
        ret

; What changes from one quadrant to the next. Quadrant q covers the
; directions q * 90 to q * 90 + 90 degrees.
quadrants:
        ;     swap  x     y     cell
        .byte 0x00, 0xff, 0xff, 0x00    ; 0: east and south
        .byte 0xff, 0x00, 0xff, 0x0f    ; 1: west and south
        .byte 0x00, 0x00, 0x00, 0xff    ; 2: west and north
        .byte 0xff, 0xff, 0x00, 0xf0    ; 3: east and north
; swap: in odd quadrants the angle inside the quadrant is measured from the
;       y axis, so the two directions read the 1/cos table the other way.
; x, y: the distance to the first grid line is the rest of the cell when
;       moving towards higher coordinates (255 - fraction), else the fraction.
; cell: the mask of the mirror image.

; ------------------------------------------------------------- the frame
;
; Local variables of _render, as offsets from sp.

DX      = 0             ; 2 bytes: length of ray between two vertical grid lines
DY      = 2             ; 2 bytes: ... between two horizontal ones
COL     = 4             ; screen column of the ray being cast, 0...127
FIRST   = 5             ; top-row value of the screen byte being built
QUAD    = 6             ; quadrant the next five are set up for; 0xff: none
SWAP    = 7             ; the four settings of that quadrant, applied:
FRAC_X  = 8             ;   distance to the first vertical grid line, in cells/256
FRAC_Y  = 9             ;   ... horizontal
START   = 10            ;   cell the player is in (low byte of its address)
PAGE    = 11            ;   which image of the map (high byte)
ANG_LO  = 12            ; direction of view * 4 - 128: position in the quadrant
ANG_HI  = 13            ;   ... and the quadrant, in bits 1..0
POS_X   = 14            ; where in its cell the player is
POS_Y   = 15
CELL    = 16            ; number of that cell: row * 16 + column
FRAME_SIZE = 17

_render:
        lea  sp, [sp-FRAME_SIZE]

        ; Directions are measured in 1024ths of a turn: 256 to a quadrant.
        ; Column c looks at  angle * 4 + col_angle[c] - 128.
        ld   a, [_player_angle]
        mov  b, a
        shr  a
        shr  a
        shr  a
        shr  a
        shr  a
        shr  a
        mov  xl, a              ; the two top bits: the quadrant
        mov  a, b
        add  a, a
        add  a, a               ; the other six, times 4
        sub  a, 128
        st   [sp+ANG_LO], a
        mov  a, xl
        sbc  a, 0               ; the borrow goes into the quadrant
        st   [sp+ANG_HI], a

        ld   a, [_player_x]
        st   [sp+POS_X], a
        ld   a, [_player_y]
        st   [sp+POS_Y], a
        ld   a, [_player_y+1]
        add  a, a
        add  a, a
        add  a, a
        add  a, a               ; row * 16
        mov  b, a
        ld   a, [_player_x+1]
        or   a, b
        st   [sp+CELL], a

        ld   a, 0xff
        st   [sp+QUAD], a
        xor  a, a
        st   [sp+COL], a
        st   [sp+FIRST], a

; ---------------------------------------------------------------- one ray

.column:
        ld   x, col_angle
        ld   a, [sp+COL]
        ld   a, [x+a]
        ld   b, [sp+ANG_LO]
        add  a, b
        mov  yl, a              ; yl = angle inside the quadrant
        ld   a, [sp+ANG_HI]
        adc  a, 0
        and  a, 3               ; a = quadrant
        ld   b, [sp+QUAD]
        cmp  a, b
        jnz  .new_quadrant      ; happens at most twice in a frame
.quadrant_set:

        ; DX, DY: how much longer the ray gets from one grid line to the
        ; next. 1 / cos of the angle for x, and the same table read
        ; backwards (1 / sin) for y.
        ld   a, [sp+SWAP]
        xor  a, yl
        mov  yl, a              ; yl = index for x
        ld   z, inv_cos_lo
        ld   a, [z+a]
        st   [sp+DX], a
        ld   z, inv_cos_hi
        mov  a, yl
        ld   a, [z+a]
        st   [sp+DX+1], a
        mov  a, yl
        xor  a, 0xff
        mov  yl, a              ; yl = index for y
        ld   z, inv_cos_lo
        ld   a, [z+a]
        st   [sp+DY], a
        mov  xl, a
        ld   z, inv_cos_hi
        mov  a, yl
        ld   a, [z+a]
        st   [sp+DY+1], a
        mov  xh, a              ; x = DY

        ; The ray starts inside a cell. Its length at the first grid line
        ; it meets in each direction is that fraction of DX or DY.
        ld   b, [sp+FRAC_Y]
        call mulhi
        push x                  ; (sp moves: offsets are 2 more until the pop)
        ld   xl, [sp+DX+2]
        ld   xh, [sp+DX+3]
        ld   b, [sp+FRAC_X+2]
        call mulhi              ; x = length of the ray at the next vertical line
        pop  y                  ; y = ... at the next horizontal line
        ld   zl, [sp+START]
        ld   zh, [sp+PAGE]      ; z -> the cell the ray is in

        ; Walk from grid line to grid line, always taking the nearer one,
        ; until the cell entered is a wall. x and y are the length of the
        ; ray at the next line of each kind.
.step:  mov  a, xl
        cmp  a, yl
        mov  a, xh
        sbc  a, yh              ; x - y
        bc   .row               ; no borrow: the horizontal line is not farther
        inc  zl                 ; cross a vertical line: one cell along x
        ld   a, [z]
        or   a, a
        bnz  .hit_x
        ld   a, [sp+DX]
        add  a, xl
        mov  xl, a
        ld   a, [sp+DX+1]
        adc  a, xh
        mov  xh, a
        jmp  .step
.row:   lea  z, [z+16]          ; cross a horizontal line: one cell along y
        ld   a, [z]
        or   a, a
        bnz  .hit_y
        ld   a, [sp+DY]
        add  a, yl
        mov  yl, a
        ld   a, [sp+DY+1]
        adc  a, yh
        mov  yh, a
        jmp  .step

.hit_y: mov  xl, yl
        mov  xh, yh
        ld   b, 0               ; faces looking north or south are darker
        jmp  .distance
.hit_x: ld   b, 1

; -------------------------------------------- from distance to wall height
;
; x = length of the ray where it met the wall (8.8). The height of the wall
; on the screen is proportional to 1 / distance, where the distance is
; measured straight ahead, not along the ray: shorter by the cosine of the
; angle between the two. Division and multiplication become subtraction of
; logarithms.

.distance:
        mov  a, xh
        cmp  a, 4
        bc   .far
        shr  a                  ; near: index = distance * 64
        ror  a
        ror  a                  ; the cells (0...3) are now in bits 7..6
        mov  zl, a
        mov  a, xl
        shr  a
        shr  a
        or   a, zl
        ld   y, log_near
        jmp  .log
.far:   add  a, a               ; far: index = distance * 4
        add  a, a
        mov  zl, a
        mov  a, xl
        add  a, a               ; the two top bits of the fraction go round
        adc  a, a               ; through the carry to bits 1..0
        adc  a, a
        and  a, 3
        or   a, zl
        ld   y, log_far
.log:   ld   a, [y+a]
        mov  zl, a              ; zl = log of the length of the ray
        ld   y, log_cos
        ld   a, [sp+COL]
        ld   a, [y+a]
        mov  zh, a
        mov  a, zl
        sub  a, zh              ; log of the distance straight ahead
        bc   .tall
        xor  a, a               ; closer than the tables go: as tall as can be
.tall:  ld   y, height
        ld   a, [y+a]
        mov  zl, a              ; zl = half the height of the wall, 1...32

; ---------------------------------------------------------------- drawing
;
; A screen byte holds four pixels, so four neighbouring columns share the
; bytes of one "byte column". Going down such a column, a byte only changes
; in the rows where one of its four walls begins or ends. So each wall just
; notes, in the list `delta`, which bits change in which row (as an XOR),
; and when all four have been cast one pass down the screen writes the 64
; bytes, XORing in the changes as it meets them. Above a wall is ceiling
; (shade 0), below it floor (shade 1).
;
; The screen is on view all the time, not only when a frame is finished,
; so every byte is written once, with what it is to hold. For the same
; reason a caller that wants something over the picture (a map) cannot
; draw it afterwards: it would be painted over in the next frame and
; flicker. It asks for a corner to be left alone instead (_render_corner).

CORNER_BYTES = 4                ; the corner is this many screen bytes wide

        ld   a, [sp+COL]
        and  a, 3
        add  a, a
        add  a, b
        mov  b, a               ; b = 2 * pixel within the byte + face
        ld   y, wall_bits
        ld   a, [y+a]
        mov  zh, a              ; zh = the wall's shade, in this pixel's bits
        mov  a, zl
        cmp  a, 32
        bz   .full
        ld   a, 32
        sub  a, zl
        ld   x, delta
        lea  x, [x+a]           ; x -> the row where the wall begins
        ld   a, [x]
        xor  a, zh              ; ceiling (0) becomes wall
        st   [x], a
        mov  a, zl
        add  a, a
        lea  x, [x+a]           ; x -> the row after the wall ends
        ld   y, floor_bits
        mov  a, b
        ld   a, [y+a]
        mov  zh, a
        ld   a, [x]
        xor  a, zh              ; wall becomes floor
        st   [x], a
        jmp  .next
.full:  ld   a, [sp+FIRST]      ; the wall is taller than the screen:
        or   a, zh              ; it is there from the first row on
        st   [sp+FIRST], a

.next:  ld   a, [sp+COL]
        inc  a
        st   [sp+COL], a
        and  a, 3
        jnz  .column

        ; Four columns are cast: write their byte in every row.
        ld   a, [sp+COL]
        shr  a
        shr  a
        dec  a
        mov  xl, a
        ld   xh, hi(SCREEN)     ; x -> the byte in the top row
        ld   z, delta
        ld   b, [sp+FIRST]      ; b = the byte in the row above
        ld   yl, SCREEN_HEIGHT
        mov  a, xl
        cmp  a, CORNER_BYTES
        bc   .fill              ; to the right of the corner
        ld   a, [_render_corner]
        or   a, a
        bz   .fill              ; no corner is asked for
        mov  yh, a              ; rows of the corner still to pass
.keep:  ld   a, [z]             ; they change b like any other row ...
        xor  a, b
        mov  b, a
        xor  a, a
        st   [z+], a
        lea  x, [x+ROW_BYTES]   ; ... but the screen keeps what it has
        dec  yl
        dec  yh
        bnz  .keep
.fill:  ld   a, [z]
        xor  a, b               ; what changes in this row
        mov  b, a
        st   [x], a
        xor  a, a
        st   [z+], a            ; leave the list empty for the next four
        lea  x, [x+ROW_BYTES]
        dec  yl
        bnz  .fill
        st   [sp+FIRST], a      ; (a is 0)

        ld   a, [sp+COL]
        cmp  a, SCREEN_WIDTH
        jnz  .column
        lea  sp, [sp+FRAME_SIZE]
        ret

; The ray has turned into another quadrant: a = the new one, yl must be
; kept.
.new_quadrant:
        st   [sp+QUAD], a
        add  a, hi(maps)
        st   [sp+PAGE], a
        ld   a, [sp+QUAD]
        add  a, a
        add  a, a
        ld   x, quadrants
        lea  x, [x+a]           ; x -> the four settings of this quadrant
        ld   a, [x+]
        st   [sp+SWAP], a
        ld   a, [x+]
        mov  b, a
        ld   a, [sp+POS_X]
        xor  a, b
        st   [sp+FRAC_X], a
        ld   a, [x+]
        mov  b, a
        ld   a, [sp+POS_Y]
        xor  a, b
        st   [sp+FRAC_Y], a
        ld   a, [x]
        mov  b, a
        ld   a, [sp+CELL]
        xor  a, b
        st   [sp+START], a
        jmp  .quadrant_set

; The shade of a wall (2: faces north or south, 3: faces east or west),
; moved to each of the four pixel positions of a byte, and what to XOR to
; turn that into floor (shade 1).
wall_bits:
        .byte 0x02, 0x03, 0x08, 0x0c, 0x20, 0x30, 0x80, 0xc0
floor_bits:
        .byte 0x03, 0x02, 0x0c, 0x08, 0x30, 0x20, 0xc0, 0x80

; ------------------------------------------------------- multiplication
;
; mulhi: x <- (b * x) / 256, that is, the fraction b/256 of x.
; Changes a, y and z; keeps b.
;
; The CPU cannot multiply, but a product is a difference of two squares:
;       a * b = (a + b)^2 / 4 - (a - b)^2 / 4
; and squares can be looked up. `squares` holds n * n / 4 for n = 0...511,
; low bytes in its first two pages and high bytes in the next two.
;
; The 16-bit x is multiplied a byte at a time:
;       b * x = b * low(x) + 256 * b * high(x)
; and of the first product only the high byte survives the division by 256.

mulhi:  mov  a, xl
        add  a, b               ; b + low(x): nine bits, the ninth in the carry
        ld   zh, hi(squares)
        bnc  .s1
        inc  zh                 ; 256 and up: the second page
.s1:    mov  zl, a
        ld   a, [z]
        mov  yl, a
        inc  zh
        inc  zh                 ; same place among the high bytes
        ld   a, [z]
        mov  yh, a              ; y = square of the sum
        mov  a, xl
        sub  a, b
        bc   .d1
        xor  a, 0xff            ; negative: change its sign
        inc  a
.d1:    mov  zl, a
        ld   zh, hi(squares)
        ld   a, [z]
        mov  xl, a
        mov  a, yl
        sub  a, xl              ; low bytes: only the borrow is wanted
        inc  zh
        inc  zh
        ld   a, [z]
        mov  xl, a
        mov  a, yh
        sbc  a, xl
        mov  yl, a              ; yl = high byte of b * low(x)

        mov  a, xh
        add  a, b               ; the same for high(x) ...
        ld   zh, hi(squares)
        bnc  .s2
        inc  zh
.s2:    mov  zl, a
        ld   a, [z]
        add  a, yl              ; ... adding in what the first product left
        mov  yl, a
        inc  zh
        inc  zh
        ld   a, [z]
        adc  a, 0
        mov  yh, a
        mov  a, xh
        sub  a, b
        bc   .d2
        xor  a, 0xff
        inc  a
.d2:    mov  zl, a
        ld   zh, hi(squares)
        ld   a, [z]
        mov  xl, a
        mov  a, yl
        sub  a, xl
        mov  xl, a
        inc  zh
        inc  zh
        ld   a, [z]
        mov  xh, a
        mov  a, yh
        sbc  a, xh
        mov  xh, a
        ret
