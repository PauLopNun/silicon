// The still pictures of the README, each one the real page doing what its
// caption says, on this machine's graphics card.
//
//   pnpm build && node web/scripts/shots.mjs docs/media
import { join } from 'node:path';
import { launch, serve } from './browser.mjs';

const out = process.argv[2] ?? 'docs/media';
const site = await serve(new URL('../dist/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));
const browser = await launch({ gpu: true });
const text = (id) => `document.getElementById(${JSON.stringify(id)}).textContent`;
const press = (page, selector) => page.evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);

/** Open the page at a link, do something there, and save a picture of it. */
async function shot(name, query, body, size = { width: 1280, height: 800 }) {
  const page = await browser.page(site.url + query, size);
  try {
    await page.until('document.body?.dataset.ready', 30000);
    await page.sleep(1200);
    await body(page);
    // Out of the way of the picture.
    await page.move(2, 2);
    await page.screenshot(join(out, name));
    console.log(join(out, name));
    for (const problem of page.problems) console.log('  !', problem);
  } finally {
    await page.close();
  }
}

try {
  await shot('m5-broken-gate.png', '?at=cpu/alu/adder8/fa3', async (page) => {
    const middle = await page.evaluate(
      `(() => { const r = document.getElementById('scene').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`,
    );
    let found;
    for (let dy = -120; dy <= 120 && !found; dy += 20) {
      for (let dx = -200; dx <= 200 && !found; dx += 20) {
        await page.move(middle.x + dx, middle.y + dy);
        await page.sleep(20);
        if (/\/nand\d+ /.test(await page.evaluate(text('hover')))) found = { x: middle.x + dx, y: middle.y + dy };
      }
    }
    if (!found) throw new Error('no gate near the middle of the scene');
    await page.click(found.x, found.y);
    await page.until(`!document.getElementById('gate-panel').hidden`, 5000);
    await press(page, '#stuck-1');
    // Long enough for the game to draw a frame or two with the gate stuck.
    await page.sleep(2500);
  });

  await shot('m5-ram-of-gates.png', '?view=ram', async (page) => {
    await press(page, '#ram-addr [data-addr="5"]');
    await press(page, '#ram-data [data-bit="3"]');
    await press(page, '#ram-data [data-bit="1"]');
    await press(page, '#ram-write');
    await press(page, '#ram-clock');
    await page.until(`${text('ram-out')} === '1010'`, 5000);
    await page.sleep(1500);
  });

  await shot('m5-tour.png', '', async (page) => {
    await press(page, '#learn');
    await press(page, '#lesson-next');
    await page.sleep(3000);
  });

  // A step being played back: the carry on its way through the adder that
  // works out the next address.
  await shot('m6-step.png', '?at=cpu/agu/adder16', async (page) => {
    await press(page, '#explain-start');
    await page.until(`${text('speed')} === 'paused' && document.body.dataset.engine === 'gate'`, 8000);
    await page.sleep(800);
    await press(page, '#step-insn');
    await page.until(`'replay' in document.body.dataset`, 4000);
    await page.sleep(350);
  });

  // The picture a link to the repository is shown with (Settings → Social
  // preview), at the size GitHub asks for: the game, and the CPU it runs on.
  await shot(
    'social.png',
    '?glow=on',
    async (page) => {
      await page.key('ArrowUp');
      await page.sleep(700);
      await page.key('ArrowUp', 'ArrowUp', false);
      await page.sleep(1200);
    },
    { width: 1280, height: 640 },
  );
} finally {
  await browser.close();
  site.close();
}
