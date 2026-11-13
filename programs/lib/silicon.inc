; The Silicon computer as a program sees it: where things are in memory.
; (docs/ISA.md, section 4.)

SCREEN        = 0x8000      ; 128 x 64 pixels, 2 bits each, 4 to a byte
SCREEN_WIDTH  = 128
SCREEN_HEIGHT = 64
ROW_BYTES     = 32          ; row y starts at SCREEN + 32 * y

KEYS          = 0x8800      ; read: the keys held down, one bit each
FRAME         = 0x8801      ; write: "this frame is finished"; read: frames so far
TIMER_LO      = 0x8802      ; read: clock cycles since reset, bits 15..8
TIMER_HI      = 0x8803      ; read: clock cycles since reset, bits 23..16
OUT           = 0x8804      ; write: a character for the console

KEY_UP        = 0x01
KEY_DOWN      = 0x02
KEY_LEFT      = 0x04
KEY_RIGHT     = 0x08
KEY_A         = 0x10
KEY_B         = 0x20
KEY_START     = 0x40
KEY_SELECT    = 0x80
