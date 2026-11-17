/**
 * How many device pixels the scene is drawn with for each CSS pixel.
 *
 * A screen with small pixels (a phone, a laptop with a dense display) gets
 * as many as it has, up to two each way, for sharp lines and letters. But
 * the scene is drawn with four samples a pixel into a picture of 16-bit
 * floats, for the glow, and on a very large dense screen that is hundreds
 * of megabytes and a graphics card's whole frame. So the picture is held
 * to about three million pixels, by giving up density down to one device
 * pixel per CSS pixel and never below it.
 */

/** Device pixels in the picture, at most, where there is density to give up. */
export const MAX_PIXELS = 3_200_000;
/** Device pixels per CSS pixel each way, at most. */
export const MAX_RATIO = 2;

/**
 * @param width the picture's width in CSS pixels
 * @param height its height in CSS pixels
 * @param devicePixelRatio the screen's device pixels per CSS pixel
 */
export function pixelRatioFor(width: number, height: number, devicePixelRatio: number): number {
  const wanted = Math.min(Math.max(devicePixelRatio, 1), MAX_RATIO);
  const fits = Math.sqrt(MAX_PIXELS / Math.max(1, width * height));
  return Math.max(1, Math.min(wanted, fits));
}
