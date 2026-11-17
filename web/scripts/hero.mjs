// The animation at the top of the README: from the whole CPU down to the
// gates of one full adder while the game runs, the way a person would get
// there (a click on a block flies into it), every frame a picture of the
// real page.
//
//   pnpm build && node web/scripts/hero.mjs docs/media/zoom.png
import { writeFileSync } from 'node:fs';
import { apng } from './apng.mjs';
import { launch, serve } from './browser.mjs';

const out = process.argv[2] ?? 'zoom.png';
const site = await serve(new URL('../dist/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));
const browser = await launch();
const page = await browser.page(site.url, { width: 1000, height: 600 });
const frames = [];
// Smaller than the window: the README shows it smaller anyway, and the file
// stays a few megabytes.
const shoot = async () => frames.push(await page.capture(undefined, 0.6));
const hover = () => page.evaluate(`document.getElementById('hover').textContent`);

/** Feel over the scene for a closed block with this path, and give its point on the picture. */
async function find(path) {
  const box = await page.evaluate(
    `(() => { const r = document.getElementById('scene').getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; })()`,
  );
  // From the middle outwards, so that the point found is well inside.
  const points = [];
  for (let fy = 0.1; fy <= 0.9; fy += 0.03) for (let fx = 0.05; fx <= 0.95; fx += 0.03) points.push([fx, fy]);
  points.sort((a, b) => Math.hypot(a[0] - 0.5, a[1] - 0.55) - Math.hypot(b[0] - 0.5, b[1] - 0.55));
  for (const [fx, fy] of points) {
    const x = box.x + fx * box.w;
    const y = box.y + fy * box.h;
    await page.move(x, y);
    const said = await hover();
    if (said.startsWith(`${path} `) && said.includes('click to go in')) return { x, y };
  }
  throw new Error(`${path} is not in the picture, closed`);
}

/** Click into a block and take pictures while the camera flies there. */
async function into(path) {
  const { x, y } = await find(path);
  await page.click(x, y);
  // Out of the way of the pictures.
  await page.move(2, 2);
  for (let i = 0; i < 11; i++) {
    await shoot();
    await page.sleep(60);
  }
}

try {
  await page.until('document.body?.dataset.ready', 20000);
  // Into the maze a little, so the screen shows a corridor and not the start.
  await page.sleep(1200);
  await page.key('ArrowUp');
  await page.sleep(600);
  await page.key('ArrowUp', 'ArrowUp', false);
  await page.sleep(300);

  for (let i = 0; i < 8; i++) {
    await shoot();
    await page.sleep(60);
  }
  await into('cpu/alu');
  await into('cpu/alu/adder8');
  await into('cpu/alu/adder8/fa3');
  // Stay a moment on the gates at work.
  for (let i = 0; i < 8; i++) {
    await page.sleep(90);
    await shoot();
  }
  writeFileSync(out, apng(frames, 80, 1800));
  console.log(`${out}: ${frames.length} frames`);
} finally {
  await page.close();
  await browser.close();
  site.close();
}
