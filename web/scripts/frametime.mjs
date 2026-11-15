// What a displayed frame costs the page's main thread, in a real Chrome.
//
//   pnpm build && node web/scripts/frametime.mjs [--gpu]
//
// The page records how long each picture takes on the main thread (levels
// of detail, state textures, handing the draw calls to WebGL) and keeps the
// median of the last 120 in `data-render-ms`. This keeps the camera moving
// for two seconds at each of the views that cost most, so that every frame
// is drawn, and reads it.
//
// What the graphics card then takes is not measured: a browser without a
// window does not wait for it, so timing the frames from outside would only
// measure the browser's own pace. With --gpu the machine's graphics card is
// used instead of drawing in software.
import { launch, serve } from './browser.mjs';

const site = await serve(new URL('../dist/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));
const browser = await launch({ gpu: process.argv.includes('--gpu') });
/** The whole frame's budget, of which the main thread may use about half. */
const BUDGET_MS = 16.6;

const views = [
  ['the whole CPU', ''],
  ['inside the registers', '?at=cpu/regs'],
  ['close over gates', '?at=cpu/alu/adder8/fa3'],
];
let worst = 0;
try {
  for (const [name, query] of views) {
    const page = await browser.page(site.url + query, { width: 1280, height: 800 });
    await page.until('document.body?.dataset.ready', 30000);
    await page.sleep(1500);
    await page.evaluate(`new Promise((done) => {
      const canvas = document.getElementById('scene');
      const r = canvas.getBoundingClientRect();
      const start = performance.now();
      let n = 0;
      const step = (now) => {
        // A tiny zoom in and out keeps the camera moving, so every frame is drawn.
        canvas.dispatchEvent(new WheelEvent('wheel', { deltaY: n++ % 2 ? 1 : -1, clientX: r.x + r.width / 2, clientY: r.y + r.height / 2, bubbles: true, cancelable: true }));
        if (now - start < 2000) requestAnimationFrame(step);
        else done();
      };
      requestAnimationFrame(step);
    })`);
    const ms = Number(await page.evaluate('document.body.dataset.renderMs'));
    worst = Math.max(worst, ms);
    console.log(`${name.padEnd(22)} ${ms.toFixed(2)} ms on the main thread per picture (median of 120)`);
    for (const problem of page.problems) console.log('  !', problem);
    await page.close();
  }
} finally {
  await browser.close();
  site.close();
}
console.log(`frame budget ${BUDGET_MS} ms: main thread uses ${((100 * worst) / BUDGET_MS).toFixed(0)}% at worst`);
