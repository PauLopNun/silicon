// The Silicon computer as a program sees it: where things are in memory.
// (docs/ISA.md, section 4.) The same facts as silicon.inc, for C.

const u8* SCREEN = (u8*)0x8000;     // 128 x 64 pixels, 2 bits each, 4 to a byte
const u8 SCREEN_WIDTH = 128;
const u8 SCREEN_HEIGHT = 64;
const u8 ROW_BYTES = 32;            // row y starts at SCREEN + 32 * y

const u8* KEYS = (u8*)0x8800;       // read: the keys held down, one bit each
const u8* FRAME = (u8*)0x8801;      // write: "this frame is finished"; read: frames so far
const u8* TIMER_LO = (u8*)0x8802;   // read: clock cycles since reset, bits 15..8
const u8* TIMER_HI = (u8*)0x8803;   // read: clock cycles since reset, bits 23..16
const u8* OUT = (u8*)0x8804;        // write: a character for the console

const u8 KEY_UP = 0x01;
const u8 KEY_DOWN = 0x02;
const u8 KEY_LEFT = 0x04;
const u8 KEY_RIGHT = 0x08;
const u8 KEY_A = 0x10;
const u8 KEY_B = 0x20;
const u8 KEY_START = 0x40;
const u8 KEY_SELECT = 0x80;
