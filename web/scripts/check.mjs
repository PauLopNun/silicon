// The page checks: what only a real browser shows. Twice the page had
// faults no test saw (a flickering screen, a squeezed panel), so these run
// on the built page in a real Chrome, here and in CI.
//
//   pnpm build && pnpm check
//
// Each check prints one line; the exit code is the number that failed.
import { launch, serve } from './browser.mjs';

const site = await serve(new URL('../dist/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));
const browser = await launch();
let failed = 0;
const results = [];

/** Run one check on a fresh page; any problem on the page's console fails it too. */
async function check(name, query, body, size) {
  const page = await browser.page(site.url + query, size);
  const started = Date.now();
  try {
    await page.until('document.body?.dataset.ready', 30000, 'the simulation to be ready');
    await page.until(`document.getElementById('speed').textContent !== '…'`, 10000, 'the first frame');
    await body(page);
    if (page.problems.length > 0) throw new Error(`the page reported:\n      ${page.problems.join('\n      ')}`);
    results.push(`ok    ${name}  (${((Date.now() - started) / 1000).toFixed(1)} s)`);
  } catch (error) {
    failed++;
    results.push(`FAIL  ${name}\n      ${error.message}`);
  } finally {
    console.log(results[results.length - 1]);
    await page.close();
  }
}

const text = (id) => `document.getElementById(${JSON.stringify(id)}).textContent`;
/** The computer's screen as the page shows it, as a string to compare. */
const screen = `document.getElementById('screen').toDataURL()`;
const engine = `document.body.dataset.engine`;
const expect = (condition, message) => {
  if (!condition) throw new Error(message);
};

/** The middle of the scene, in page pixels. */
async function sceneMiddle(page) {
  return page.evaluate(
    `(() => { const r = document.getElementById('scene').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`,
  );
}

// ── the computer ──

await check('every program in the menu runs and is the one the menu says', '', async (page) => {
  const names = await page.evaluate(`[...document.querySelectorAll('#programs button')].map((b) => b.dataset.program)`);
  expect(names.length >= 5, `only ${names.length} programs in the menu`);
  for (const name of names) {
    await page.evaluate(`document.querySelector('#programs [data-program=${name}]').click()`);
    await page.until(`document.querySelector('#programs [aria-pressed=true]')?.dataset.program === '${name}'`, 10000, `${name} to be shown as running`);
    const before = await page.evaluate(`Number(${text('cycles')}.replace(/\\D/g, ''))`);
    await page.sleep(400);
    const after = await page.evaluate(`Number(${text('cycles')}.replace(/\\D/g, ''))`);
    const halted = await page.evaluate(`${text('speed')} === 'halted'`);
    expect(after > before || halted, `${name} does not run (cycle ${before} → ${after})`);
  }
});

await check('primes writes the primes on the console', '?program=primes', async (page) => {
  await page.until(`${text('console')}.includes('97')`, 15000, 'the primes up to 97');
  const shown = await page.evaluate(text('console'));
  expect(/\b2\b.*\b3\b.*\b5\b.*\b7\b/s.test(shown), `console says ${JSON.stringify(shown.slice(0, 80))}`);
});

await check('the maze stands still while no key is held, and turns with a key', '', async (page) => {
  // Past the digging of the maze and the shutter.
  await page.until(`Number(${text('fps')}.split(' ')[0]) > 5`, 15000, 'the game to be running');
  await page.sleep(1500);
  // Sampled at many moments, not only when a frame ends (ADR 0013).
  const first = await page.evaluate(screen);
  for (let i = 0; i < 10; i++) {
    await page.sleep(70);
    expect((await page.evaluate(screen)) === first, 'the picture changed while no key was held');
  }
  await page.key('ArrowRight');
  await page.sleep(500);
  await page.key('ArrowRight', 'ArrowRight', false);
  expect((await page.evaluate(screen)) !== first, 'holding → did not turn the player');
});

await check('bounce moves by itself', '?program=bounce', async (page) => {
  await page.sleep(500);
  const first = await page.evaluate(screen);
  await page.sleep(300);
  expect((await page.evaluate(screen)) !== first, 'the balls did not move');
});

await check('pause stops the clock and step runs one cycle', '', async (page) => {
  await page.evaluate(`document.getElementById('run').click()`);
  await page.until(`${text('speed')} === 'paused'`, 5000, 'the page to say paused');
  const cycle = async () => page.evaluate(`Number(${text('cycles')}.replace(/\\D/g, ''))`);
  const paused = await cycle();
  await page.sleep(300);
  expect((await cycle()) === paused, 'the clock ran while paused');
  await page.evaluate(`document.getElementById('step').click()`);
  await page.until(`Number(${text('cycles')}.replace(/\\D/g, '')) === ${paused + 1}`, 5000, 'one cycle more');
});

await check('explain pauses on the gates and follows the CPU cycle by cycle', '', async (page) => {
  await page.evaluate(`document.getElementById('explain-start').click()`);
  await page.until(`${text('speed')} === 'paused' && ${engine} === 'gate'`, 8000, 'paused on the gate engine');
  await page.until(`!document.getElementById('explain').hidden && ${text('explain-head')} !== ''`, 5000, 'the explanation');
  const cycle = async () => page.evaluate(`Number(${text('cycles')}.replace(/\\D/g, ''))`);
  // A cycle at a time: the clock moves by one, and every signal has its meaning.
  const before = await cycle();
  await page.evaluate(`document.getElementById('step').click()`);
  await page.until(`Number(${text('cycles')}.replace(/\\D/g, '')) === ${before + 1}`, 5000, 'one cycle more');
  const signals = await page.evaluate(`[...document.querySelectorAll('#explain-signals li')].map((li) => li.textContent)`);
  expect(signals.length > 0 && signals.every((s) => /^[A-Z_0-9]+\S*.{5,}/.test(s)), `signals: ${JSON.stringify(signals)}`);
  // An instruction at a time: each is shown at the step just after its opcode was read.
  for (let i = 0; i < 3; i++) {
    const head = await page.evaluate(text('explain-head'));
    await page.evaluate(`document.getElementById('step-insn').click()`);
    await page.until(`${text('explain-head')} !== ${JSON.stringify(head)}`, 5000, 'the next instruction');
    await page.until(`/^[0-9a-f]{4}  \\S.*step 1 \\(/.test(${text('explain-head')})`, 5000, 'an instruction at step 1');
  }
  // A block at work leads to it.
  await page.evaluate(`document.querySelector('#explain-blocks button').click()`);
  await page.until(`document.body.dataset.focus !== 'cpu'`, 8000, 'the camera to go to a block at work');
});

// ── the scene ──

await check('the page opens on the whole CPU, on the fast engine at full speed', '', async (page) => {
  await page.until(`${engine} === 'fast'`, 5000, 'the fast engine');
  // And it stays: the first frame comes before the view has had its say.
  await page.sleep(800);
  expect((await page.evaluate(engine)) === 'fast', 'the engine left the fast one with nothing open');
  expect((await page.evaluate('document.body.dataset.focus')) === 'cpu', 'the camera is not on the whole CPU');
  await page.until(`${text('speed')}.endsWith('MHz')`, 5000, 'a clock in megahertz');
});

await check('zooming into a block brings the gate engine; zooming out the fast one', '', async (page) => {
  const { x, y } = await sceneMiddle(page);
  for (let i = 0; i < 12; i++) {
    await page.wheel(x, y, -120);
    await page.sleep(40);
  }
  await page.until(`${engine} === 'gate'`, 8000, 'the gate engine after zooming in');
  await page.until(`document.body.dataset.focus !== 'cpu'`, 5000, 'the camera to be inside a block');
  await page.key('Digit0', '0');
  await page.until(`${engine} === 'fast'`, 8000, 'the fast engine after going back to the whole CPU');
});

await check('clicking a block flies into it and the trail and the link follow', '', async (page) => {
  // The register file, found by asking the page where it is drawn.
  const clicked = await page.evaluate(`(async () => {
    const canvas = document.getElementById('scene');
    const r = canvas.getBoundingClientRect();
    return { x: r.x + r.width * 0.55, y: r.y + r.height * 0.42 };
  })()`);
  await page.click(clicked.x, clicked.y);
  await page.until(`document.body.dataset.focus.split('/').length >= 2`, 8000, 'the camera to go into a block');
  const focus = await page.evaluate('document.body.dataset.focus');
  await page.until(`new URLSearchParams(location.search).get('at') === ${JSON.stringify(focus)}`, 5000, 'the link to say where');
  const trail = await page.evaluate(`[...document.querySelectorAll('#crumbs button')].map((b) => b.textContent).join('/')`);
  expect(trail === focus, `the trail says ${trail}, the camera is in ${focus}`);
});

await check('a link leads to a gate, lit by the gate engine', '?at=cpu/alu/adder8/fa3', async (page) => {
  await page.until(`document.body.dataset.focus === 'cpu/alu/adder8/fa3'`, 8000, 'the camera at fa3');
  await page.until(`${engine} === 'gate'`, 8000, 'the gate engine');
  // Point at the middle of the scene: it names a gate or a block of fa3.
  const { x, y } = await sceneMiddle(page);
  await page.move(x, y);
  await page.until(`${text('hover')}.startsWith('cpu/alu/adder8/fa3')`, 5000, 'the pointer to name what is under it');
});

await check('a link can ask for an engine, and gets it whatever the view', '?engine=gate', async (page) => {
  await page.until(`${engine} === 'gate'`, 8000, 'the gate engine');
  await page.sleep(500);
  expect((await page.evaluate(engine)) === 'gate', 'the engine changed by itself');
  await page.evaluate(`document.getElementById('use-auto').click()`);
  await page.until(`${engine} === 'fast'`, 8000, 'the fast engine back on auto');
  expect((await page.evaluate('location.search')) === '', 'the link still names an engine');
});

await check('a phone-sized window has everything and nothing sideways', '', async (page) => {
  const overflow = await page.evaluate(`document.documentElement.scrollWidth - document.documentElement.clientWidth`);
  expect(overflow <= 0, `the page is ${overflow} px wider than the window`);
  const sizes = await page.evaluate(`['screen', 'scene', 'keys'].map((id) => { const r = document.getElementById(id).getBoundingClientRect(); return [id, Math.round(r.width), Math.round(r.height)]; })`);
  for (const [id, w, h] of sizes) expect(w > 100 && h > 40, `#${id} is ${w}×${h}`);
}, { width: 390, height: 780, mobile: true });

await browser.close();
site.close();
console.log(failed === 0 ? `\nall ${results.length} page checks passed` : `\n${failed} of ${results.length} page checks failed`);
process.exitCode = failed;
