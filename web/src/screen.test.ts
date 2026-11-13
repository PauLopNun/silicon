import { describe, expect, it } from 'vitest';
import { AMBER, decodeFramebuffer, FRAMEBUFFER_BYTES, keysRegister, SCREEN_HEIGHT, SCREEN_WIDTH } from './screen';

const PIXEL_BYTES = SCREEN_WIDTH * SCREEN_HEIGHT * 4;

describe('decodeFramebuffer', () => {
  it('unpacks four pixels per byte, least significant bits first', () => {
    const fb = new Uint8Array(FRAMEBUFFER_BYTES);
    fb[0] = 0b11_10_01_00; // shades 0, 1, 2, 3 from left to right
    const rgba = new Uint8ClampedArray(PIXEL_BYTES);
    decodeFramebuffer(fb, rgba, AMBER);
    AMBER.forEach((shade, pixel) => {
      expect(Array.from(rgba.subarray(pixel * 4, pixel * 4 + 4))).toEqual([...shade, 255]);
    });
    // The fifth pixel belongs to the next byte, which is 0.
    expect(Array.from(rgba.subarray(16, 20))).toEqual([...AMBER[0], 255]);
  });

  it('puts the second row 32 bytes in', () => {
    const fb = new Uint8Array(FRAMEBUFFER_BYTES);
    fb[32] = 0b11;
    const rgba = new Uint8ClampedArray(PIXEL_BYTES);
    decodeFramebuffer(fb, rgba, AMBER);
    const firstOfRow1 = SCREEN_WIDTH * 4;
    expect(Array.from(rgba.subarray(firstOfRow1, firstOfRow1 + 3))).toEqual([...AMBER[3]]);
  });

  it('refuses buffers of the wrong size', () => {
    const rgba = new Uint8ClampedArray(PIXEL_BYTES);
    expect(() => decodeFramebuffer(new Uint8Array(10), rgba, AMBER)).toThrow(RangeError);
    const small = new Uint8ClampedArray(4);
    expect(() => decodeFramebuffer(new Uint8Array(FRAMEBUFFER_BYTES), small, AMBER)).toThrow(RangeError);
  });
});

describe('keysRegister', () => {
  it('maps keyboard keys to the bits of the KEYS register', () => {
    expect(keysRegister([])).toBe(0);
    expect(keysRegister(['ArrowLeft'])).toBe(0b0000_0100);
    expect(keysRegister(['ArrowUp', 'ArrowRight', 'KeyZ'])).toBe(0b0001_1001);
    expect(keysRegister(['KeyQ', 'Enter'])).toBe(0b0100_0000);
  });
});
