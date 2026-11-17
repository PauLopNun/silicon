/**
 * The page: the computer's screen, keys and console, its registers, and the
 * CPU as a scene to move about in, from the whole of it down to one gate.
 * Everything shown comes from the simulation in the worker, down to which
 * program is running; this file only draws it and passes on what is asked.
 */
import { WindowRate } from './clock';
import { blocksAtWork, headline, memoryText, parseExplanation } from './explain';
import { LESSON, type LessonContext } from './lessons';
import { searchWith } from './link';
import { appendConsole, consoleText, searchFor, type ProgramInfo } from './programs';
import {
  decodeArch,
  flagsText,
  formatRate,
  GATE_FIELDS,
  hex,
  pairValue,
  type EngineKind,
  faultMap,
  type FrameState,
  type FromWorker,
  type NetlistInfo,
  type ToWorker,
} from './protocol';
import { FLOORPLAN } from './scene/floorplan';
import { gateCountText } from './scene/labels';
import { GATE_TOP, gateRect, rectOf } from './scene/layout';
import { focusModule } from './scene/lod';
import { Navigator } from './scene/navigator';
import { Pace } from './scene/pace';
import type { Hit } from './scene/pick';
import { engineFor, engineNote, parseEngineMode, samplingFor, type EngineMode } from './scene/policy';
import { gatePath, moduleAt, resolvePath } from './scene/tree';
import { VALUES } from './scene/values';
import { SceneView } from './scene/view';
import {
  AMBER,
  controlUsesKey,
  decodeFramebuffer,
  keysRegister,
  KEY_BITS,
  SCREEN_HEIGHT,
  SCREEN_WIDTH,
} from './screen';

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

// ───────────────────────────── the screen ─────────────────────────────

const screenCanvas = el<HTMLCanvasElement>('screen');
screenCanvas.width = SCREEN_WIDTH;
screenCanvas.height = SCREEN_HEIGHT;
const screenContext = screenCanvas.getContext('2d')!;
const screenPixels = screenContext.createImageData(SCREEN_WIDTH, SCREEN_HEIGHT);

// ───────────────────────────── the worker ─────────────────────────────

const worker = new Worker(new URL('./sim.worker.ts', import.meta.url), { type: 'module' });
const send = (msg: ToWorker) => worker.postMessage(msg);
/** The last state the simulation sent. */
let state: FrameState | undefined;

// ───────────────────────────── the scene ──────────────────────────────

/** A link can say where to look and which engine to use: ?at=cpu/alu/adder8&engine=gate */
const link = new URLSearchParams(location.search);

interface Scene {
  view: SceneView;
  nav: Navigator;
  /** The blocks that show a register, and how to write its value. */
  values: { module: number; text: (typeof VALUES)[string] }[];
  net: NetlistInfo;
  /** The levels of its wires, as last reported. */
  levels: () => Uint8Array | undefined;
}
/**
 * What gives off light is drawn with a glow around it. A graphics card that
 * finds it heavy is spared it: the page watches the pace of its pictures
 * and turns the glow off if they come slowly. A link can settle it either
 * way: `?glow=off`, `?glow=on`.
 */
const GLOW = link.get('glow') !== 'off';
const pace = link.get('glow') === 'on' || !GLOW ? undefined : new Pace();
document.body.dataset.glow = GLOW ? 'on' : 'off';
/** The CPU. */
let scene: Scene | undefined;
/** The memory made of gates. */
let ramScene: Scene | undefined;
/** Which of the two is on view. A link can ask for the memory: ?view=ram */
let showing: 'cpu' | 'ram' = link.get('view') === 'ram' ? 'ram' : 'cpu';
/** The scene on view. */
const active = (): Scene | undefined => (showing === 'ram' ? ramScene : scene);
/** Something changed that the picture does not show yet. */
let stale = true;
/** What the person asked for: an engine, or whichever the view needs. */
let mode: EngineMode = parseEngineMode(link.get('engine'));
/** The engine last asked of the worker, so that it is asked once and not at every frame. */
let asked: EngineKind | undefined;
/** What the pointer is on. */
let pointed: Hit | undefined;
/** The module the camera is in, as shown in the trail and in the address bar. */
let focus = -1;
/** The gate chosen with a click, to be broken or mended; -1 for none. */
let chosenGate = -1;
/** The broken gates, as the simulation last reported them: gate → level it is stuck at. */
let faults = new Map<number, 0 | 1>();

/** The panel for the chosen gate, and for mending whatever is broken. */
function showGatePanel(): void {
  const panel = el('gate-panel');
  panel.hidden = chosenGate < 0 && faults.size === 0;
  if (panel.hidden || !scene) return;
  const stuck = faults.get(chosenGate);
  el('gate-name').textContent =
    chosenGate < 0
      ? `${faults.size} broken`
      : `${gatePath(scene.view.tree, chosenGate)}${stuck === undefined ? '' : `  stuck at ${stuck}`}`;
  if (stuck === undefined) delete panel.dataset.broken;
  else panel.dataset.broken = '';
  for (const id of ['stuck-0', 'stuck-1', 'mend']) el(id).hidden = chosenGate < 0;
  el<HTMLButtonElement>('stuck-0').disabled = stuck === 0;
  el<HTMLButtonElement>('stuck-1').disabled = stuck === 1;
  el<HTMLButtonElement>('mend').disabled = stuck === undefined;
  el('mend-all').hidden = faults.size === 0;
  el('mend-all').textContent = faults.size === 1 ? 'mend all' : `mend all ${faults.size}`;
}

/** The address bar says `text`, without adding to the history. */
function replaceSearch(search: string): void {
  if (search === location.search) return;
  try {
    history.replaceState(null, '', `${location.pathname}${search}${location.hash}`);
  } catch {
    // Some places (a page opened from a file) do not allow it. The link is
    // a convenience; everything works all the same.
  }
}

function buildScene(net: NetlistInfo): Scene {
  const view = new SceneView(el<HTMLCanvasElement>('scene'), net, FLOORPLAN, GLOW);
  const nav = new Navigator(view, {
    invalidate: () => (stale = true),
    hover: (hit) => {
      pointed = hit;
      showPointed();
      stale = true;
    },
    // A click on a gate chooses it; a click on the floor lets it go.
    select: (hit) => {
      chosenGate = hit.gate;
      showGatePanel();
    },
  });
  view.resize();
  // Start where the link says, or with everything in view.
  const wanted = resolvePath(view.tree, link.get('at') ?? '');
  nav.jumpTo(nav.home());
  if (wanted && wanted.gate >= 0) nav.jumpTo(nav.framing({ ...gateRect(view.layout, wanted.gate), w: 1, h: 1 }, 0.42));
  else if (wanted && wanted.module > 0) nav.jumpTo(nav.framing(rectOf(view.layout, wanted.module)));

  const values = Object.entries(VALUES).flatMap(([path, text]) => {
    const module = moduleAt(view.tree, path);
    return module < 0 ? [] : [{ module, text }];
  });
  return { view, nav, values, net, levels: () => state?.wires };
}

/** The memory made of gates, on a canvas of its own. */
function buildRamScene(net: NetlistInfo): Scene {
  const view = new SceneView(el<HTMLCanvasElement>('ram-scene'), net, FLOORPLAN, GLOW);
  const nav = new Navigator(view, {
    invalidate: () => (stale = true),
    hover: (hit) => {
      pointed = hit;
      showPointed();
      stale = true;
    },
  });
  return { view, nav, values: [], net, levels: () => state?.ram.wires };
}

/** The memory's inputs as set on the page. What it holds comes from the simulation. */
const ramInputs = { addr: 0, data: 0, write: false };
let ramBits = 4;

const bits = (value: number, n: number) => value.toString(2).padStart(n, '0');

function sendRamInputs(): void {
  send({ type: 'ram', ...ramInputs });
  el('ram-addr-value').textContent = String(ramInputs.addr);
  el('ram-data-value').textContent = bits(ramInputs.data, ramBits);
  for (const button of el('ram-data').children) {
    const bit = Number((button as HTMLElement).dataset.bit);
    button.setAttribute('aria-pressed', String(((ramInputs.data >> bit) & 1) === 1));
    button.textContent = String((ramInputs.data >> bit) & 1);
  }
  for (const button of el('ram-addr').children) {
    button.setAttribute('aria-pressed', String(Number((button as HTMLElement).dataset.addr) === ramInputs.addr));
  }
  el('ram-write').setAttribute('aria-pressed', String(ramInputs.write));
}

/** Buttons for the address, a toggle per bit of the data, writing and the clock. */
function buildRamPanel(shape: Uint32Array): void {
  const [words = 8, width = 4] = shape;
  ramBits = width;
  el('ram-addr').replaceChildren(
    ...Array.from({ length: words }, (_, addr) => {
      const button = document.createElement('button');
      button.textContent = String(addr);
      button.dataset.addr = String(addr);
      button.title = `Word ${addr}`;
      button.addEventListener('click', () => {
        ramInputs.addr = addr;
        sendRamInputs();
      });
      return button;
    }),
  );
  // Most significant bit first, as numbers are written.
  el('ram-data').replaceChildren(
    ...Array.from({ length: width }, (_, i) => {
      const bit = width - 1 - i;
      const button = document.createElement('button');
      button.dataset.bit = String(bit);
      button.title = `Bit ${bit} of the word to write`;
      button.addEventListener('click', () => {
        ramInputs.data ^= 1 << bit;
        sendRamInputs();
      });
      return button;
    }),
  );
  sendRamInputs();
}

/** What the memory holds and reads out, on its blocks and in the panel. */
function showRam(words: Uint8Array, out: number): void {
  if (!ramScene) return;
  const { view } = ramScene;
  words.forEach((word, i) => view.setValue(moduleAt(view.tree, `ram/word${i}`), bits(word, ramBits)));
  view.setValue(moduleAt(view.tree, 'ram/read'), bits(out, ramBits));
  el('ram-out').textContent = bits(out, ramBits);
}

/** Show the CPU or the memory made of gates. */
function setShowing(next: 'cpu' | 'ram'): void {
  showing = next;
  el('scene').hidden = next !== 'cpu';
  el('ram-scene').hidden = next !== 'ram';
  el('ram-panel').hidden = next !== 'ram';
  el('gate-panel').hidden = true;
  if (next === 'cpu') showGatePanel();
  el('show-cpu').setAttribute('aria-pressed', String(next === 'cpu'));
  el('show-ram').setAttribute('aria-pressed', String(next === 'ram'));
  document.body.dataset.showing = next;
  pointed = undefined;
  showPointed();
  focus = -1;
  const current = active();
  if (current) {
    current.view.resize();
    if (next === 'ram' && current.view.camera.distance === 1) current.nav.jumpTo(current.nav.home());
  }
  replaceSearch(searchWith(location.search, 'view', next === 'ram' ? 'ram' : undefined));
  sendSampling();
  stale = true;
}

/** Say what the pointer is on, and put a mark around it. */
function showPointed(): void {
  const hover = el('hover');
  const current = active();
  if (!current || !pointed) {
    hover.hidden = true;
    current?.view.setMark(undefined);
    return;
  }
  const { view, net } = current;
  const { tree, layout } = view;
  hover.hidden = false;
  if (pointed.gate >= 0) {
    const g = pointed.gate;
    view.setMark(gateRect(layout, g), GATE_TOP);
    let text = gatePath(tree, g);
    const levels = current.levels();
    if (state && levels) {
      const wire = (field: number) => levels[net.gates[g * GATE_FIELDS + field] ?? 0] ?? 0;
      const isNand = tree.kindOfGate[g] === 0;
      text += isNand ? `   ${wire(1)} nand ${wire(2)} = ${wire(3)}` : `   d ${wire(1)}   q ${wire(3)}`;
      // The memory always runs on its gates; the CPU only on the gate engine.
      if (current === scene && state.engine !== 'gate') text += '   (not live)';
    }
    hover.textContent = text;
  } else {
    const m = pointed.module;
    const closed = view.lod.open[m]! < 0.5;
    view.setMark(rectOf(layout, m), closed ? layout.lidZ[m]! : 0);
    hover.textContent = `${tree.paths[m]!}   ${gateCountText(tree.totalGates[m]!)}${closed ? '   click to go in' : ''}`;
  }
}

/** The trail of modules from the whole CPU to where the camera is; each leads back to itself. */
function showFocus(module: number): void {
  const current = active();
  if (!current || module === focus) return;
  focus = module;
  const { tree } = current.view;
  const trail: number[] = [];
  for (let m = module; m >= 0; m = tree.parents[m]!) trail.unshift(m);
  el('crumbs').replaceChildren(
    ...trail.map((m, i) => {
      const button = document.createElement('button');
      button.textContent = tree.names[m]!;
      button.title = `Go to ${tree.paths[m]!}`;
      if (i === trail.length - 1) button.setAttribute('aria-current', 'location');
      button.addEventListener('click', () => (m === 0 ? current.nav.flyTo(current.nav.home()) : current.nav.flyToModule(m)));
      return button;
    }),
  );
  document.body.dataset.focus = tree.paths[module]!;
}

/** After a picture: what follows from where the camera is. */
function afterRender(): void {
  const current = active();
  if (!current || !scene) return;
  const { view, nav } = current;
  showFocus(focusModule(view.tree, view.layout, view.lod, view.camera.x, view.camera.y));
  // Once the camera is at rest the address bar leads back to this place in the CPU.
  if (current === scene && !nav.moving) {
    replaceSearch(searchWith(location.search, 'at', focus > 0 ? view.tree.paths[focus] : undefined));
  }

  if (!state) return;
  // Left to itself, the gate engine runs while gates are in sight (one of
  // the CPU's blocks is open) or the clock is stopped; see `policy.ts`.
  // Looking at the memory, no gate of the CPU is in sight.
  const needs = {
    inside: current === scene ? scene.view.lod.entered[0]! : 0,
    broken: faults.size,
    paused: !state.running,
  };
  const engine = engineFor(mode, needs);
  if (engine !== state.engine && engine !== asked) {
    asked = engine;
    send({ type: 'engine', engine });
  }
  el('engine-note').textContent = engineNote(mode, state.engine, state.wiresFrom, needs);
}

/**
 * Main-thread milliseconds of the last pictures drawn: working out the
 * levels of detail, filling the state textures and handing the draw calls
 * to WebGL. What the graphics card then takes is asked of the card itself
 * (`SceneView.gpuMs`). Both are read by `web/scripts/frametime.mjs`, from
 * `data-render-ms` and `data-gpu-ms`.
 */
const renderTimes: number[] = [];

function frame(now: number): void {
  const current = active();
  if (current) {
    const moved = current.nav.tick(now);
    // Drawn again when the camera moved, when the simulation said something
    // new, and while something in the picture moves by itself.
    if (current.view.resize() && (moved || stale || current.view.animating)) {
      stale = false;
      const start = performance.now();
      current.view.render(now);
      afterRender();
      renderTimes.push(performance.now() - start);
      if (renderTimes.length > 120) renderTimes.shift();
      const sorted = [...renderTimes].sort((a, b) => a - b);
      document.body.dataset.renderMs = (sorted[Math.floor(sorted.length / 2)] ?? 0).toFixed(2);
      const gpu = current.view.gpuMs;
      if (gpu !== undefined) document.body.dataset.gpuMs = gpu.toFixed(2);
      // For the page checks: whether a step is being played back.
      if (current.view.replaying) document.body.dataset.replay = '';
      else delete document.body.dataset.replay;
      if (current.view.glows && pace?.drew(now)) {
        // Too slow with the glow: without it from here on, and say so.
        for (const each of [scene, ramScene]) if (each) each.view.glow = false;
        document.body.dataset.glow = 'off';
        el('status').textContent += ' · glow off: the pictures were coming slowly (?glow=on keeps it)';
        stale = true;
      }
    }
  }
  requestAnimationFrame(frame);
}

// ───────────────────────────── programs ───────────────────────────────

/** Lines of console output kept on the page. */
const CONSOLE_LINES = 12;
const consoleView = el('console');
let programs: ProgramInfo[] = [];
// What the page shows of the program and of the run. Both follow the
// simulation: a button only asks, and the next frame says what happened.
let shownProgram = '';
let shownBoot = 0;

function buildProgramButtons(list: ProgramInfo[]): void {
  programs = list;
  el('programs').replaceChildren(
    ...list.map((program) => {
      const button = document.createElement('button');
      button.textContent = program.title;
      button.title = program.about;
      button.dataset.program = program.name;
      button.setAttribute('aria-pressed', 'false');
      button.addEventListener('click', () => send({ type: 'program', name: program.name }));
      return button;
    }),
  );
}

function showProgram(name: string): void {
  shownProgram = name;
  for (const button of el('programs').children) {
    button.setAttribute('aria-pressed', String((button as HTMLElement).dataset.program === name));
  }
  el('about').textContent = programs.find((p) => p.name === name)?.about ?? '';
  // The address bar leads back to this program.
  replaceSearch(searchFor(location.search, name, programs));
}

/** What the program wrote since the last frame, added to the console. */
function showConsole(msg: FrameState): void {
  if (msg.boot !== shownBoot) {
    shownBoot = msg.boot;
    consoleView.textContent = '';
  }
  if (msg.console.length > 0) {
    consoleView.textContent = appendConsole(consoleView.textContent ?? '', consoleText(msg.console), CONSOLE_LINES);
    consoleView.scrollTop = consoleView.scrollHeight;
  }
  consoleView.hidden = consoleView.textContent === '';
}

// ───────────────────────────── each frame ─────────────────────────────

// Real speeds: simulated clock cycles, and frames the program finishes, per
// second of wall-clock time.
const speed = new WindowRate(1000);
const fps = new WindowRate(2000);

/** The explanation shown last, so that the panel is rebuilt only when it changes. */
let shownExplanation = '';

/** While paused: what the CPU does in the cycle it is about to run. */
function showExplanation(msg: FrameState): void {
  const panel = el('explain');
  panel.hidden = msg.running;
  if (msg.running || msg.explain === shownExplanation) return;
  shownExplanation = msg.explain;
  lessonStepped++;
  const e = parseExplanation(msg.explain);
  el('explain-head').textContent = headline(e);
  el('explain-memory').textContent = `This cycle ${memoryText(e.signals)}.`;
  // The blocks at work, each a way to go and look at it.
  el('explain-blocks').replaceChildren(
    ...blocksAtWork(e.signals).map((path) => {
      const button = document.createElement('button');
      button.textContent = path.replace(/^cpu\//, '');
      button.title = `Go to ${path}`;
      button.addEventListener('click', () => {
        const module = scene ? moduleAt(scene.view.tree, path) : -1;
        if (module > 0) scene!.nav.flyToModule(module);
      });
      return button;
    }),
  );
  el('explain-signals').replaceChildren(
    ...e.signals.map(({ name, doc }) => {
      const item = document.createElement('li');
      const label = document.createElement('b');
      label.textContent = name;
      item.append(label, doc);
      return item;
    }),
  );
}

/** The clock of the last state shown, to tell a single step from running. */
let shown: { cycles: number; boot: number; running: boolean } | undefined;

function show(msg: FrameState): void {
  // A step: the clock was stopped before and is stopped now, and has moved.
  const stepped =
    shown !== undefined && !shown.running && !msg.running && shown.boot === msg.boot && msg.cycles > shown.cycles;
  shown = { cycles: msg.cycles, boot: msg.boot, running: msg.running };
  state = msg;
  if (msg.engine === asked) asked = undefined;
  decodeFramebuffer(msg.framebuffer, screenPixels.data, AMBER);
  screenContext.putImageData(screenPixels, 0, 0);
  if (msg.program !== shownProgram) showProgram(msg.program);
  showConsole(msg);

  const s = decodeArch(msg.arch);
  if (scene) {
    const reported = faultMap(msg.faults);
    const changed = reported.size !== faults.size || [...reported].some(([g, l]) => faults.get(g) !== l);
    faults = reported;
    if (changed) showGatePanel();
    // The wires are live when the gates computed them for this state: all
    // of its cycles on the gate engine, a sample of them on the fast one.
    const live = msg.wiresFrom !== 'old';
    scene.view.setWires(msg.wires, msg.activity, {
      live,
      streaming: msg.running && !msg.halted,
      broken: faults.keys(),
      step: stepped && live,
    });
    for (const { module, text } of scene.values) scene.view.setValue(module, text(s));
  }
  if (ramScene) {
    // The memory changes only when its inputs or its clock are worked by
    // hand: whatever changed is a step.
    ramScene.view.setWires(msg.ram.wires, msg.ram.activity, {
      live: true,
      streaming: false,
      step: msg.ram.activity.some((a) => a > 0),
    });
    showRam(msg.ram.words, msg.ram.out);
  }
  if (pointed) showPointed();
  stale = true;

  const now = performance.now();
  speed.record(now, msg.cycles);
  fps.record(now, msg.frames);

  el('engine').textContent = `${msg.engine} engine`;
  el('engine').dataset.engine = msg.engine;
  document.body.dataset.engine = msg.engine;
  document.body.dataset.wires = msg.wiresFrom;
  el('speed').textContent = msg.running && !msg.halted ? formatRate(speed.rate) : msg.halted ? 'halted' : 'paused';
  el('fps').textContent = `${fps.rate.toFixed(1)} frames/s`;
  el('cycles').textContent = `cycle ${msg.cycles.toLocaleString('en-US')}`;
  el('registers').textContent =
    `pc ${hex(s.pc, 4)}  sp ${hex(s.sp, 4)}  ir ${hex(s.ir, 2)}  step ${s.step}  flags ${flagsText(s.flags)}\n` +
    `a ${hex(s.regs[0] ?? 0, 2)}  b ${hex(s.regs[1] ?? 0, 2)}  ` +
    `x ${hex(pairValue(s.regs, 1), 4)}  y ${hex(pairValue(s.regs, 2), 4)}  z ${hex(pairValue(s.regs, 3), 4)}`;
  el('run').textContent = msg.running ? 'Pause' : 'Run';
  el<HTMLButtonElement>('step').disabled = msg.running;
  showExplanation(msg);
  showLessonTask();
}

worker.onmessage = (e: MessageEvent<FromWorker>) => {
  const msg = e.data;
  if (msg.type === 'ready') {
    buildProgramButtons(msg.programs);
    scene = buildScene(msg);
    ramScene = buildRamScene(msg.ram);
    buildRamPanel(msg.ram.shape);
    setShowing(showing);
    const nands = msg.gates.filter((_, i) => i % GATE_FIELDS === 0 && msg.gates[i] === 0).length;
    const dffs = msg.gates.length / GATE_FIELDS - nands;
    el('status').textContent =
      `core v${msg.version} · ${nands.toLocaleString('en-US')} NAND gates · ${dffs} flip-flops · ` +
      `${msg.wireCount.toLocaleString('en-US')} wires`;
    document.body.dataset.ready = 'true';
  } else if (msg.type === 'frame') {
    show(msg);
  } else {
    el('status').textContent = `simulation failed: ${msg.message}`;
  }
};
worker.onerror = (e) => {
  el('status').textContent = `simulation failed: ${e.message}`;
};

// ───────────────────────────── controls ───────────────────────────────

/** Ask for an engine, or leave it to the view. The buttons and the address bar say which. */
function setMode(next: EngineMode): void {
  mode = next;
  asked = undefined;
  for (const each of ['auto', 'fast', 'gate'] as const) {
    el(`use-${each}`).setAttribute('aria-pressed', String(mode === each));
  }
  replaceSearch(searchWith(location.search, 'engine', mode === 'auto' ? undefined : mode));
  sendSampling();
  stale = true;
}

/** How much of the program to compute on the gates while the fast engine runs it. */
function sendSampling(): void {
  send({ type: 'sampling', cycles: samplingFor(mode, showing === 'cpu') });
}
el('use-auto').addEventListener('click', () => setMode('auto'));
el('use-gate').addEventListener('click', () => setMode('gate'));
el('use-fast').addEventListener('click', () => setMode('fast'));
el('engine').addEventListener('click', () => setMode(state?.engine === 'gate' ? 'fast' : 'gate'));
el('run').addEventListener('click', () => send({ type: 'run', running: !(state?.running ?? true) }));
el('step').addEventListener('click', () => send({ type: 'step', cycles: 1 }));
el('step-insn').addEventListener('click', () => send({ type: 'step-instruction' }));
// Explaining: stop the clock, which brings the gate engine unless the fast
// one alone was asked for, so that each step is played back in the scene.
function explain(): void {
  send({ type: 'run', running: false });
  if (mode === 'fast') setMode('auto');
}
el('explain-start').addEventListener('click', explain);
el('reset').addEventListener('click', () => send({ type: 'reset' }));
el<HTMLSelectElement>('clock').addEventListener('change', (e) => {
  const value = (e.target as HTMLSelectElement).value;
  send({ type: 'clock', hz: value === 'max' ? null : Number(value) });
});
el('whole').addEventListener('click', () => scene?.nav.flyTo(scene.nav.home()));
// Breaking gates. The panel shows what the simulation reports, not what was asked.
el('stuck-0').addEventListener('click', () => send({ type: 'break', gate: chosenGate, level: 0 }));
el('stuck-1').addEventListener('click', () => send({ type: 'break', gate: chosenGate, level: 1 }));
el('mend').addEventListener('click', () => send({ type: 'break', gate: chosenGate, level: null }));
el('mend-all').addEventListener('click', () => send({ type: 'mend-all' }));

// The computer's keys follow the keyboard and the keys drawn on the page.
const typed = new Set<string>(); // held on the keyboard
const touched = new Set<string>(); // held on the page
const keyButtons = Array.from(document.querySelectorAll<HTMLElement>('[data-key]'));
const sendKeys = () => {
  const held = new Set([...typed, ...touched]);
  send({ type: 'keys', keys: keysRegister(held) });
  // The drawn keys light up for the keyboard too: they show the KEYS register.
  for (const button of keyButtons) button.setAttribute('aria-pressed', String(held.has(button.dataset.key ?? '')));
};
window.addEventListener('keydown', (e) => {
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  // A list or a button that has the focus uses some keys itself.
  if (e.target instanceof Element && controlUsesKey(e.target.tagName, e.code)) return;
  if (!(e.code in KEY_BITS)) {
    // Not one of the computer's keys: perhaps one of the camera's.
    if (scene?.nav.key(e.code)) e.preventDefault();
    return;
  }
  e.preventDefault();
  if (!typed.has(e.code)) {
    typed.add(e.code);
    sendKeys();
  }
});
window.addEventListener('keyup', (e) => {
  if (typed.delete(e.code)) sendKeys();
});
window.addEventListener('blur', () => {
  if (typed.size + touched.size > 0) {
    typed.clear();
    touched.clear();
    sendKeys();
  }
});
for (const button of keyButtons) {
  const code = button.dataset.key ?? '';
  const press = (down: boolean) => {
    if (down === touched.has(code)) return;
    if (down) touched.add(code);
    else touched.delete(code);
    sendKeys();
  };
  button.addEventListener('pointerdown', () => press(true));
  for (const type of ['pointerup', 'pointerleave', 'pointercancel']) button.addEventListener(type, () => press(false));
  // A long touch is holding the key, not asking for a menu.
  button.addEventListener('contextmenu', (e) => e.preventDefault());
}

// A list keeps the arrow keys while it has the focus. One that was chosen
// from with the mouse gives the focus back at once, so that the arrows go on
// to the computer; one being worked with the keyboard keeps it.
for (const select of document.querySelectorAll('select')) {
  let byPointer = false;
  select.addEventListener('pointerdown', () => (byPointer = true));
  select.addEventListener('keydown', () => (byPointer = false));
  select.addEventListener('change', () => {
    if (byPointer) select.blur();
  });
}

const resized = new ResizeObserver(() => (stale = true));
resized.observe(el('scene'));
resized.observe(el('ram-scene'));
el('show-cpu').addEventListener('click', () => setShowing('cpu'));
el('show-ram').addEventListener('click', () => setShowing('ram'));
el('ram-write').addEventListener('click', () => {
  ramInputs.write = !ramInputs.write;
  sendRamInputs();
});
el('ram-clock').addEventListener('click', () => send({ type: 'ram-clock' }));

// ───────────────────────────── the tour ───────────────────────────────

/** The step of the guided tour on view, or -1 when there is none. */
let lessonAt = -1;
/** How many times the explanation moved on since the step began. */
let lessonStepped = 0;

function lessonContext(): LessonContext {
  return {
    focus: document.body.dataset.focus ?? '',
    showing,
    paused: state ? !state.running : false,
    engine: state?.engine ?? 'fast',
    chosen: scene && chosenGate >= 0 ? gatePath(scene.view.tree, chosenGate) : '',
    broken: faults.size,
    ramOut: state?.ram.out ?? 0,
    ramWords: state ? Array.from(state.ram.words) : [],
    stepped: lessonStepped,
  };
}

/** Go to a step of the tour: the scene, the place, the words. */
function enterLesson(index: number): void {
  const step = LESSON[index];
  if (!step) return closeLesson();
  lessonAt = index;
  lessonStepped = 0;
  document.body.dataset.lesson = String(index);
  el('lesson').hidden = false;
  el('lesson-count').textContent = `${index + 1} / ${LESSON.length}`;
  el('lesson-title').textContent = step.title;
  el('lesson-text').replaceChildren(
    ...step.text.split('\n\n').map((paragraph) => {
      const p = document.createElement('p');
      p.textContent = paragraph;
      return p;
    }),
  );
  el<HTMLButtonElement>('lesson-back').disabled = index === 0;
  el('lesson-next').textContent = index === LESSON.length - 1 ? 'finish' : 'next';
  if (step.view !== showing) setShowing(step.view);
  const current = active();
  if (current) {
    // Frame what the step is about out from under the panel: beside it or
    // below it, whichever leaves the larger part of the picture free.
    const panel = el('lesson').getBoundingClientRect();
    const canvas = current.view.canvas.getBoundingClientRect();
    const beside = panel.right - canvas.left;
    const below = panel.bottom - canvas.top;
    const besideArea = (canvas.width - beside) * canvas.height;
    const belowArea = canvas.width * (canvas.height - below);
    current.nav.inset = besideArea > belowArea ? { left: beside, top: 0 } : { left: 0, top: below };
    if (scene && current !== scene) scene.nav.inset = { left: 0, top: 0 };
    const place = resolvePath(current.view.tree, step.at);
    if (place && place.gate >= 0) current.nav.flyToGate(place.gate);
    else if (place && place.module > 0) current.nav.flyToModule(place.module);
    else current.nav.flyTo(current.nav.home());
  }
  if (step.explain) {
    explain();
  } else if (state && !state.running) {
    // A step that is not about explaining runs the computer again.
    send({ type: 'run', running: true });
  }
  showLessonTask();
}

/** The step's task, ticked once the page sees it done. */
function showLessonTask(): void {
  const step = LESSON[lessonAt];
  const task = el('lesson-task');
  if (!step?.task) {
    task.textContent = '';
    return;
  }
  const done = step.task.done(lessonContext());
  task.textContent = (done ? '✓ ' : '') + step.task.text;
  if (done) task.dataset.done = '';
  else delete task.dataset.done;
}

function closeLesson(): void {
  lessonAt = -1;
  el('lesson').hidden = true;
  delete document.body.dataset.lesson;
  for (const each of [scene, ramScene]) if (each) each.nav.inset = { left: 0, top: 0 };
}

el('learn').addEventListener('click', () => enterLesson(lessonAt >= 0 ? lessonAt : 0));
el('lesson-next').addEventListener('click', () => enterLesson(lessonAt + 1));
el('lesson-back').addEventListener('click', () => enterLesson(lessonAt - 1));
el('lesson-close').addEventListener('click', closeLesson);

// A link can choose the program: ?program=bounce
const wantedProgram = link.get('program');
if (wantedProgram) send({ type: 'program', name: wantedProgram });
setMode(mode);

requestAnimationFrame(frame);
