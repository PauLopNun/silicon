// Take pictures of the built page, to look at by eye.
//
//   pnpm build && node web/scripts/look.mjs <directory> [query …]
//
// Each query (`?at=cpu/alu`, `?program=bounce&engine=gate`…) is opened in a
// real browser and saved as a PNG in the directory, with what the page said
// on its console. Add `@390x780` to a query for a window of that size.
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { launch, serve } from './browser.mjs';

const [directory, ...queries] = process.argv.slice(2);
if (!directory) {
  console.error('usage: node web/scripts/look.mjs <directory> [query …]');
  process.exit(2);
}
mkdirSync(directory, { recursive: true });
const site = await serve(new URL('../dist/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));
const browser = await launch({ gpu: true });
try {
  let n = 0;
  for (const spec of queries.length > 0 ? queries : ['']) {
    const [query, size] = spec.split('@');
    const [width, height] = (size ?? '1280x800').split('x').map(Number);
    const page = await browser.page(site.url + query, { width, height });
    await page.until('document.body?.dataset.ready', 20000);
    // Let the program draw and the engines settle into the view.
    await page.sleep(1500);
    const file = join(directory, `${String(n++).padStart(2, '0')}${query.replace(/[^a-z0-9]+/gi, '-') || '-home'}.png`);
    await page.screenshot(file);
    const told = await page.evaluate(
      `JSON.stringify({ engine: document.body.dataset.engine, focus: document.body.dataset.focus, speed: document.getElementById('speed').textContent, note: document.getElementById('engine-note').textContent })`,
    );
    console.log(file, told);
    for (const problem of page.problems) console.log('  !', problem);
    await page.close();
  }
} finally {
  await browser.close();
  site.close();
}
