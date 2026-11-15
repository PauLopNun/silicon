// Five balls bouncing around the screen, one step per tick of the timer.
// Hold A and they leave trails.

#include "lib/silicon.h"

const u8 BALLS = 5;
const u8 SIZE = 3;                  // a ball is a square this many pixels wide

u8 x[BALLS] = { 10, 40, 70, 100, 120 };
u8 y[BALLS] = { 5, 50, 20, 40, 10 };
i8 dx[BALLS] = { 1, -1, 2, -2, 1 };     // pixels per step
i8 dy[BALLS] = { 1, 1, -1, 1, -2 };
const u8 shade[BALLS] = { 3, 2, 3, 1, 2 };

// A screen byte holds four pixels: the bits of each, and a byte of four
// pixels of each shade.
const u8 pixel[4] = { 0x03, 0x0c, 0x30, 0xc0 };
const u8 solid[4] = { 0x00, 0x55, 0xaa, 0xff };

// Give the pixel at (px, py) a shade.
void plot(u8 px, u8 py, u8 shade) {
    u8* at = SCREEN + (u16)py * ROW_BYTES + (px >> 2);
    u8 bits = pixel[px & 3];
    *at = *at & ~bits | solid[shade] & bits;
}

// Draw ball n where it is.
void ball(u8 n) {
    for (u8 j = 0; j < SIZE; j++) {
        for (u8 i = 0; i < SIZE; i++) plot(x[n] + i, y[n] + j, shade[n]);
    }
}

// Is the pixel (px, py) part of a ball, where the balls are now?
u8 covered(u8 px, u8 py) {
    for (u8 n = 0; n < BALLS; n++) {
        // (To the left of a ball or above it, the difference would be
        // negative: it wraps round to a large number.)
        if (px - x[n] < SIZE && py - y[n] < SIZE) return 1;
    }
    return 0;
}

// Move one step along one axis, turning round at 0 and at `last`.
u8 step(u8 at, i8* speed, u8 last) {
    i16 next = (i16)at + *speed;
    if (next < 0 || next > last) {
        *speed = -*speed;
        next = (i16)at + *speed;
    }
    return (u8)next;
}

u8 old_x[BALLS];            // where the balls were before they moved
u8 old_y[BALLS];

void main() {
    for (;;) {
        // The screen is on view all the time, not only when a frame is
        // finished. A ball that was rubbed out and then drawn in its new
        // place would be gone for a moment, and would be seen to blink. So
        // the balls are first drawn where they now are ...
        for (u8 n = 0; n < BALLS; n++) {
            old_x[n] = x[n];
            old_y[n] = y[n];
            x[n] = step(x[n], &dx[n], SCREEN_WIDTH - SIZE);
            y[n] = step(y[n], &dy[n], SCREEN_HEIGHT - SIZE);
            ball(n);
        }
        // ... and then what is left of them where they were is taken away,
        // unless they are to leave trails.
        if (!(*KEYS & KEY_A)) {
            for (u8 n = 0; n < BALLS; n++) {
                for (u8 j = 0; j < SIZE; j++) {
                    for (u8 i = 0; i < SIZE; i++) {
                        u8 px = old_x[n] + i;
                        u8 py = old_y[n] + j;
                        if (!covered(px, py)) plot(px, py, 0);
                    }
                }
            }
        }
        *FRAME = 0;

        // Wait for the timer's next tick, so that the balls move at the
        // speed of the clock and not of this program.
        u8 tick = *TIMER_HI;
        while (*TIMER_HI == tick) { }
    }
}
