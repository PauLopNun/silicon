// What a picture of the scene costs, in a real Chrome on this machine's
// graphics card, and what the simulation gets done meanwhile.
//
//   pnpm build && node web/scripts/frametime.mjs
//
// For each of the views that cost most, with and without the glow:
//
// - the main thread's share: working out the levels of detail, filling the
//   state textures and handing the draw calls to WebGL (the median of the
//   page's last 120 pictures, `data-render-ms`);
// - the graphics card's share, asked of the card itself with a timer query
//   (the median of the last 60 answers, `data-gpu-ms`). Timing frames from
//   outside would not do: a browser without a window does not wait for the
//   card, so that only measures the browser's own pace;
// - the clock rate the simulation reaches while the page draws.
//
// The numbers are this machine's. The renderer is printed with them.
import { launch, serve } from './browser.mjs';

const site = await serve(new URL('../dist/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));
const browser = await launch();
/** A whole frame at 60 frames a second. */
const BUDGET_MS = 16.6;

const views = [
  ['the whole CPU', ''],
  ['inside the registers', 'at=cpu/regs'],
  ['close over gates', 'at=cpu/alu/adder8/fa3'],
];
let worst = 0;
let renderer = '';
try {
  for (const glow of [true, false]) {
    console.log(glow ? 'with the glow' : 'without it (?glow=off)');
    for (const [name, at] of views) {
      const query = [at, glow ? '' : 'glow=off'].filter(Boolean).join('&');
      const page = await browser.page(`${site.url}?${query}`, { width: 1280, height: 800 });
      await page.until('document.body?.dataset.ready', 30000);
      // Long enough for the medians to be of this view and for the clock
      // rate to settle; the scene is drawn all the while (the program runs).
      await page.sleep(4000);
      const told = await page.evaluate(
        `({ render: document.body.dataset.renderMs, gpu: document.body.dataset.gpuMs, speed: document.getElementById('speed').textContent, engine: document.body.dataset.engine, wires: document.body.dataset.wires })`,
      );
      renderer ||= await page.evaluate(
        `(() => { const gl = document.createElement('canvas').getContext('webgl2'); const e = gl.getExtension('WEBGL_debug_renderer_info'); return e ? gl.getParameter(e.UNMASKED_RENDERER_WEBGL) : 'unknown'; })()`,
      );
      const gpu = told.gpu === undefined ? 'not measurable here' : `${told.gpu} ms`;
      if (told.gpu !== undefined) worst = Math.max(worst, Number(told.gpu) + Number(told.render));
      console.log(
        `  ${name.padEnd(22)} main thread ${told.render} ms   graphics card ${gpu}   ` +
          `${told.engine} engine at ${told.speed} (wires: ${told.wires})`,
      );
      for (const problem of page.problems) console.log('    !', problem);
      await page.close();
    }
  }
} finally {
  await browser.close();
  site.close();
}
console.log(`at 1280×800 on ${renderer}`);
console.log(`frame budget ${BUDGET_MS} ms: the costliest view takes ${worst.toFixed(1)} ms of it (${((100 * worst) / BUDGET_MS).toFixed(0)}%)`);
