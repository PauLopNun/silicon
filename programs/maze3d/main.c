// maze3d: find the way out of the maze.
//
// UP and DOWN walk, LEFT and RIGHT turn. The map in the corner shows where
// you are (the bright dot) and where the way out is (the dimmer one). Get
// there and a new maze is dug.
//
// The picture is drawn by render.asm, one ray per screen column, in
// assembly because it is where all the time goes. Everything else is here.

#include "../lib/console.h"
#include "walk.h"

// What the renderer needs to know (see render.asm).
u8 maze_map[256];           // 16 x 16 cells, row by row from the north; 0 is floor
u16 player_x;               // in cells: the high byte is the cell, the low
u16 player_y;               //   byte the place inside it, in 256ths
u8 player_angle;            // 256 to a full turn; 0 looks east, 64 south

void render_init();         // to be called when the map has changed
void render();              // draws what the player sees

asm {
        .include "../lib/silicon.inc"
        .include "render.asm"
}

// ------------------------------------------------------------- the maze
//
// A cell is a number: 16 * row + column. Rooms are the cells whose row and
// column are both odd, 1 to 13: seven by seven of them. Between two rooms
// there is one cell, wall or passage, and the map has walls all around.

const u8 START = 1 * 16 + 1;        // the room in the north-west corner
const u8 EXIT = 13 * 16 + 13;       // the room in the south-east corner

// One cell east, south, west, north.
const i8 beside[4] = { 1, 16, -1, -16 };

u8 level;                   // which maze this is: 1, 2, 3, ...
u16 seed;
u8 path[49];                // the rooms between the first and the one being dug

// The next of a sequence of numbers that looks random (a 16-bit xorshift:
// it goes through every value but 0 before repeating).
u8 random() {
    seed ^= seed << 7;
    seed ^= seed >> 9;
    seed ^= seed << 8;
    return (u8)seed;
}

// Dig a maze out of solid rock: from a room, break through to a neighbour
// that has not been reached yet, chosen at random, and go on from there;
// at a dead end, walk back to the last room that still has such a
// neighbour. Every room ends up reached, by exactly one way.
void dig() {
    for (u16 i = 0; i < 256; i++) maze_map[i] = 1;
    u8 cell = START;
    u8 depth = 0;
    maze_map[cell] = 0;
    for (;;) {
        u8 d = random();
        u8 next = 0;
        for (u8 tries = 0; tries < 4; tries++) {
            d = (d + 1) & 3;
            u8 room = cell + beside[d] * 2;
            // Outside the maze? (Going west or north from the edge wraps
            // round to column or row 15.)
            if ((room & 15) > 13 || room >= 14 * 16) continue;
            if (maze_map[room]) {
                next = room;
                break;
            }
        }
        if (next) {
            maze_map[cell + beside[d]] = 0;
            maze_map[next] = 0;
            path[depth++] = cell;
            cell = next;
        } else {
            if (depth == 0) break;
            cell = path[--depth];
        }
    }
}

// ------------------------------------------------------------- the map
//
// The maze as a picture, one pixel to a cell: 16 rows of 4 screen bytes.
// Walls are the dimmest shade that is not black (1), which the view behind
// has only on its floor, so the map stands out from it; the way out is
// shade 2 and the player, added when the map is shown, shade 3.

u8 chart[64];

void draw_chart() {
    u8 cell = 0;
    for (u8 i = 0; i < 64; i++) {
        u8 bits = 0;
        // Four cells to a byte; the first ends up in the lowest two bits.
        for (u8 k = 0; k < 4; k++) {
            u8 shade = 0;
            if (maze_map[cell]) shade = 1;
            if (cell == EXIT) shade = 2;
            bits = bits >> 2 | shade << 6;
            cell++;
        }
        chart[i] = bits;
    }
}

// The bits of each of the four pixels of a screen byte.
const u8 pixel[4] = { 0x03, 0x0c, 0x30, 0xc0 };

// Put the map in the corner of the screen, over the picture, and the
// player on it.
void show_chart() {
    u8* from = chart;
    u8* to = SCREEN;
    for (u8 row = 0; row < 16; row++) {
        for (u8 i = 0; i < 4; i++) to[i] = *from++;
        to += ROW_BYTES;
    }
    u8 column = player_x >> 8;
    u8 row = player_y >> 8;
    SCREEN[(u16)row * ROW_BYTES + (column >> 2)] |= pixel[column & 3];
}

// ------------------------------------------------------------ walking

const u8 TURN = 3;          // how far the view turns in a frame
const u8 MARGIN = 48;       // how near a wall the player can get, in 256ths of a cell

// The cell that holds the point (x, y).
u8 cell_at(u16 x, u16 y) {
    return (u8)(y >> 8) << 4 | (u8)(x >> 8);
}

// Is there no room for the player at (x, y)? The player is a square,
// MARGIN to each side, and none of its corners may be in a wall.
u8 blocked(u16 x, u16 y) {
    return maze_map[cell_at(x - MARGIN, y - MARGIN)]
        || maze_map[cell_at(x + MARGIN, y - MARGIN)]
        || maze_map[cell_at(x - MARGIN, y + MARGIN)]
        || maze_map[cell_at(x + MARGIN, y + MARGIN)];
}

// Do what the keys ask, as far as one frame goes.
void move() {
    u8 keys = *KEYS;
    if (keys & KEY_LEFT) player_angle -= TURN;
    if (keys & KEY_RIGHT) player_angle += TURN;
    if (!(keys & (KEY_UP | KEY_DOWN))) return;

    // One step along the direction of view (walk.h), or against it.
    i8 dx = walk[player_angle];
    i8 dy = walk[(u8)(player_angle - 64)];
    if (keys & KEY_DOWN) {
        dx = -dx;
        dy = -dy;
    }
    // Each direction on its own, so that walking into a wall at an angle
    // slides along it.
    u16 x = player_x + dx;
    if (!blocked(x, player_y)) player_x = x;
    u16 y = player_y + dy;
    if (!blocked(player_x, y)) player_y = y;
}

// ------------------------------------------------------------- the game

// Begin a maze: each level digs its own, the same one every time.
void begin() {
    print("Maze ");
    print_number(level);
    put('\n');
    seed = 0x5eed + level;
    for (u8 i = 0; i < 8; i++) random();
    dig();
    render_init();
    draw_chart();
    // In the middle of the first room, looking along the one passage that
    // leaves it.
    player_x = 0x0180;
    player_y = 0x0180;
    player_angle = maze_map[START + 1] ? 64 : 0;
}

// Close the picture like a shutter, from the top and from the bottom,
// while the view is still being drawn behind it.
void shutter() {
    for (u8 rows = 4; rows <= 32; rows += 4) {
        render();
        u8* top = SCREEN;
        u8* bottom = SCREEN + (u16)(SCREEN_HEIGHT - rows) * ROW_BYTES;
        for (u16 n = (u16)rows * ROW_BYTES; n; n--) {
            *top++ = 0xff;
            *bottom++ = 0xff;
        }
        *FRAME = 0;
    }
}

void main() {
    level = 1;
    begin();
    for (;;) {
        move();
        render();
        show_chart();
        *FRAME = 0;
        if (cell_at(player_x, player_y) == EXIT) {
            shutter();
            level++;
            begin();
        }
    }
}
