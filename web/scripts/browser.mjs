// A real browser for the page checks, with nothing to install: the Chrome
// (or Chromium, or Edge) already on the machine, run without a window and
// driven through its DevTools protocol over a WebSocket, both of which Node
// has built in.
//
// Set CHROME to the browser's path if it is not found by itself.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join, normalize, resolve, sep } from 'node:path';

const CANDIDATES = [
  process.env.CHROME,
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/google-chrome-stable',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
];

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
  '.map': 'application/json',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
};

/** Serve a directory of built files on a free port of this machine. */
export function serve(root) {
  const base = resolve(root);
  const server = createServer((request, response) => {
    const path = decodeURIComponent(new URL(request.url, 'http://x').pathname);
    let file = normalize(join(base, path));
    if (file !== base && !file.startsWith(base + sep)) {
      response.writeHead(403).end();
      return;
    }
    if (existsSync(file) && statSync(file).isDirectory()) file = join(file, 'index.html');
    if (!existsSync(file)) {
      response.writeHead(404).end('not found');
      return;
    }
    response.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' });
    response.end(readFileSync(file));
  });
  return new Promise((done) => {
    server.listen(0, '127.0.0.1', () => {
      done({ url: `http://127.0.0.1:${server.address().port}/`, close: () => server.close() });
    });
  });
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

/** Start the browser. `close()` stops it and removes its temporary profile. */
export async function launch() {
  const binary = CANDIDATES.find((path) => path && existsSync(path));
  if (!binary) throw new Error('no Chrome found; set CHROME to the path of a Chromium-based browser');
  const profile = mkdtempSync(join(tmpdir(), 'silicon-check-'));
  const child = spawn(
    binary,
    [
      '--headless=new',
      '--remote-debugging-port=0',
      `--user-data-dir=${profile}`,
      '--no-first-run',
      '--no-default-browser-check',
      '--hide-scrollbars',
      '--mute-audio',
      // WebGL without a graphics card (a CI runner): draw in software.
      '--enable-unsafe-swiftshader',
      'about:blank',
    ],
    { stdio: 'ignore' },
  );
  // The browser writes the port it chose into its profile.
  const portFile = join(profile, 'DevToolsActivePort');
  for (let i = 0; i < 200 && !existsSync(portFile); i++) await sleep(50);
  if (!existsSync(portFile)) {
    child.kill();
    throw new Error(`${binary} did not start`);
  }
  const port = Number(readFileSync(portFile, 'utf8').split('\n')[0]);

  return {
    binary,
    /** Open a page of the given size (CSS pixels) and wait until it has loaded. */
    async page(url, { width = 1280, height = 800, mobile = false } = {}) {
      const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
      const page = await connect(target.webSocketDebuggerUrl);
      await page.send('Page.enable');
      await page.send('Runtime.enable');
      await page.send('Log.enable');
      await page.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile });
      if (mobile) await page.send('Emulation.setTouchEmulationEnabled', { enabled: true });
      await page.send('Page.navigate', { url });
      return page;
    },
    async close() {
      child.kill();
      await sleep(200);
      try {
        rmSync(profile, { recursive: true, force: true });
      } catch {
        // The browser may still hold a file for a moment; the profile is in the temporary directory anyway.
      }
    },
  };
}

/** One page, spoken to over the DevTools protocol. */
async function connect(address) {
  const socket = new WebSocket(address);
  await new Promise((done, fail) => {
    socket.onopen = done;
    socket.onerror = () => fail(new Error('could not connect to the page'));
  });
  let nextId = 1;
  const waiting = new Map();
  /** Errors and warnings from the page: the console, uncaught exceptions, failed requests. */
  const problems = [];
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.id) {
      const { done, fail } = waiting.get(message.id);
      waiting.delete(message.id);
      if (message.error) fail(new Error(`${message.error.message}`));
      else done(message.result);
    } else if (message.method === 'Runtime.consoleAPICalled') {
      const { type, args } = message.params;
      if (type === 'error' || type === 'warning' || type === 'assert') {
        problems.push(`console.${type}: ${args.map((a) => a.value ?? a.description ?? a.type).join(' ')}`);
      }
    } else if (message.method === 'Runtime.exceptionThrown') {
      const details = message.params.exceptionDetails;
      problems.push(`uncaught: ${details.exception?.description ?? details.text}`);
    } else if (message.method === 'Log.entryAdded') {
      const { level, text, url } = message.params.entry;
      if (level === 'error' || level === 'warning') problems.push(`${level}: ${text} ${url ?? ''}`);
    }
  };
  const send = (method, params = {}) =>
    new Promise((done, fail) => {
      const id = nextId++;
      waiting.set(id, { done, fail });
      socket.send(JSON.stringify({ id, method, params }));
    });

  /** The value of an expression in the page. A promise is waited for. */
  const evaluate = async (expression) => {
    const { result, exceptionDetails } = await send('Runtime.evaluate', {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
    return result.value;
  };

  const mouse = (type, x, y, options = {}) =>
    send('Input.dispatchMouseEvent', { type, x, y, button: 'none', ...options });

  return {
    send,
    evaluate,
    problems,
    /** Wait until an expression is true in the page. */
    async until(expression, timeout = 15000, what = expression) {
      const start = Date.now();
      for (;;) {
        if (await evaluate(`Boolean(${expression})`)) return;
        if (Date.now() - start > timeout) throw new Error(`timed out waiting for: ${what}`);
        await sleep(50);
      }
    },
    sleep,
    /** Save a picture of the page. */
    async screenshot(file) {
      const { data } = await send('Page.captureScreenshot', { format: 'png' });
      writeFileSync(file, Buffer.from(data, 'base64'));
    },
    /** The pixels of a rectangle of the page as PNG bytes. */
    async capture(clip) {
      const { data } = await send('Page.captureScreenshot', { format: 'png', clip: clip && { ...clip, scale: 1 } });
      return Buffer.from(data, 'base64');
    },
    move: (x, y) => mouse('mouseMoved', x, y),
    async click(x, y) {
      await mouse('mouseMoved', x, y);
      await mouse('mousePressed', x, y, { button: 'left', buttons: 1, clickCount: 1 });
      await mouse('mouseReleased', x, y, { button: 'left', clickCount: 1 });
    },
    /** Press at one place, move to another in steps, release. */
    async drag(x0, y0, x1, y1, { button = 'left', steps = 8 } = {}) {
      const buttons = button === 'right' ? 2 : 1;
      await mouse('mouseMoved', x0, y0);
      await mouse('mousePressed', x0, y0, { button, buttons, clickCount: 1 });
      for (let i = 1; i <= steps; i++) {
        await mouse('mouseMoved', x0 + ((x1 - x0) * i) / steps, y0 + ((y1 - y0) * i) / steps, { button, buttons });
      }
      await mouse('mouseReleased', x1, y1, { button, clickCount: 1 });
    },
    /** Turn the wheel at a place; a negative amount zooms in. */
    wheel: (x, y, deltaY) => mouse('mouseWheel', x, y, { deltaX: 0, deltaY }),
    async key(code, key = code, down = true) {
      await send('Input.dispatchKeyEvent', { type: down ? 'rawKeyDown' : 'keyUp', code, key });
    },
    close: () => socket.close(),
  };
}
