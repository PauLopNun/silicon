/**
 * The computer's screen and keys, as the page sees them. Mirrors
 * docs/ISA.md §4.
 */

export const SCREEN_WIDTH = 128;
export const SCREEN_HEIGHT = 64;
/** Bytes of screen memory: four 2-bit pixels per byte. */
export const FRAMEBUFFER_BYTES = (SCREEN_WIDTH * SCREEN_HEIGHT) / 4;

/** RGB of the four shades, darkest first. */
export type Palette = readonly [Rgb, Rgb, Rgb, Rgb];
export type Rgb = readonly [number, number, number];

export const AMBER: Palette = [
  [10, 12, 18],
  [92, 58, 22],
  [186, 120, 44],
  [255, 190, 96],
];

/**
 * Turn screen memory into RGBA pixels. Pixel 0 of each byte is its two least
 * significant bits.
 */
export function decodeFramebuffer(framebuffer: Uint8Array, rgba: Uint8ClampedArray, palette: Palette): void {
  if (framebuffer.length !== FRAMEBUFFER_BYTES || rgba.length !== SCREEN_WIDTH * SCREEN_HEIGHT * 4) {
    throw new RangeError('framebuffer or pixel buffer has the wrong size');
  }
  let out = 0;
  for (const byte of framebuffer) {
    for (let pixel = 0; pixel < 4; pixel++) {
      const [r, g, b] = palette[((byte >> (2 * pixel)) & 3) as 0 | 1 | 2 | 3];
      rgba[out++] = r;
      rgba[out++] = g;
      rgba[out++] = b;
      rgba[out++] = 255;
    }
  }
}

/** Keyboard keys (`KeyboardEvent.code`) and the bit each sets in the KEYS register. */
export const KEY_BITS: Readonly<Record<string, number>> = {
  ArrowUp: 1 << 0,
  ArrowDown: 1 << 1,
  ArrowLeft: 1 << 2,
  ArrowRight: 1 << 3,
  KeyZ: 1 << 4, // A
  KeyX: 1 << 5, // B
  Enter: 1 << 6, // start
  ShiftRight: 1 << 7, // select
};

/** The KEYS register for a set of held keyboard keys. */
export function keysRegister(held: Iterable<string>): number {
  let keys = 0;
  for (const code of held) keys |= KEY_BITS[code] ?? 0;
  return keys;
}

/**
 * Does a focused control of this kind use the key itself? Then the key is
 * left to it and not passed on to the computer: arrows move through a list,
 * Enter presses a button, and a text field takes everything.
 */
export function controlUsesKey(tagName: string, code: string): boolean {
  switch (tagName.toUpperCase()) {
    case 'SELECT':
      return code.startsWith('Arrow') || code === 'Enter';
    case 'BUTTON':
    case 'A':
      return code === 'Enter';
    case 'INPUT':
    case 'TEXTAREA':
      return true;
    default:
      return false;
  }
}
