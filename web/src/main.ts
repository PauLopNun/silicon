/**
 * The page: the computer's screen, keys and console, its registers, and
 * every gate of the CPU lit by the wire it drives. Everything shown comes
 * from the simulation in the worker, down to which program is running; this
 * file only draws it.
 *
 * The gate view is the M4 data path in miniature: one instanced quad per
 * gate, and a texture with one texel per wire that the shader looks up by
 * index. Updating the whole CPU is a single small texture upload per frame.
 */
import * as THREE from 'three';
import { WindowRate } from './clock';
import { flatLayout, type FlatLayout } from './layout';
import { appendConsole, consoleText, searchFor, type ProgramInfo } from './programs';
import {
  decodeArch,
  flagsText,
  formatRate,
  GATE_FIELDS,
  hex,
  pairValue,
  type EngineKind,
  type FrameState,
  type FromWorker,
  type NetlistInfo,
  type ToWorker,
} from './protocol';
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

// ───────────────────────────── the gates ──────────────────────────────

const gateCanvas = el<HTMLCanvasElement>('gates');
const labels = el('labels');
const renderer = new THREE.WebGLRenderer({ canvas: gateCanvas, antialias: true });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
const scene = new THREE.Scene();
scene.background = new THREE.Color(0x05070c);
const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, -1, 1);

const VERTEX = /* glsl */ `
  attribute vec2 aCell;
  attribute float aWire;
  attribute float aKind;
  uniform sampler2D uWires;
  uniform float uWireCount;
  varying float vLevel;
  varying float vKind;
  void main() {
    // The wire texture holds 0 or 1 in a byte, which arrives as 0 or 1/255.
    vLevel = texture2D(uWires, vec2((aWire + 0.5) / uWireCount, 0.5)).r * 255.0;
    vKind = aKind;
    // Layout y grows downwards.
    vec2 p = vec2(aCell.x + position.x, -aCell.y + position.y);
    gl_Position = projectionMatrix * modelViewMatrix * vec4(p, 0.0, 1.0);
  }
`;
const FRAGMENT = /* glsl */ `
  uniform float uLive;
  varying float vLevel;
  varying float vKind;
  void main() {
    vec3 off = vec3(0.075, 0.10, 0.155);
    // NAND outputs glow amber, flip-flops (the state) cyan.
    vec3 on = mix(vec3(1.0, 0.70, 0.28), vec3(0.35, 0.86, 1.0), vKind);
    vec3 color = mix(off, on, step(0.5, vLevel));
    // Greyed out when the wires are not being simulated.
    float grey = dot(color, vec3(0.3, 0.5, 0.2));
    gl_FragColor = vec4(mix(vec3(grey) * 0.55, color, uLive), 1.0);
  }
`;

interface GateView {
  layout: FlatLayout;
  wires: Uint8Array;
  texture: THREE.DataTexture;
  material: THREE.ShaderMaterial;
}
let view: GateView | undefined;

function buildGateView(net: NetlistInfo): GateView {
  const layout = flatLayout(net);
  const count = net.gates.length / GATE_FIELDS;
  const wire = new Float32Array(count);
  const kind = new Float32Array(count);
  for (let g = 0; g < count; g++) {
    kind[g] = net.gates[g * GATE_FIELDS] ?? 0;
    wire[g] = net.gates[g * GATE_FIELDS + 3] ?? 0;
  }

  const quad = new THREE.PlaneGeometry(0.8, 0.8);
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.index = quad.index;
  geometry.setAttribute('position', quad.getAttribute('position'));
  geometry.setAttribute('aCell', new THREE.InstancedBufferAttribute(layout.positions, 2));
  geometry.setAttribute('aWire', new THREE.InstancedBufferAttribute(wire, 1));
  geometry.setAttribute('aKind', new THREE.InstancedBufferAttribute(kind, 1));
  geometry.instanceCount = count;

  const wires = new Uint8Array(net.wireCount);
  const texture = new THREE.DataTexture(wires, net.wireCount, 1, THREE.RedFormat, THREE.UnsignedByteType);
  texture.needsUpdate = true;
  const material = new THREE.ShaderMaterial({
    uniforms: { uWires: { value: texture }, uWireCount: { value: net.wireCount }, uLive: { value: 0 } },
    vertexShader: VERTEX,
    fragmentShader: FRAGMENT,
  });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.frustumCulled = false; // instances are placed by the shader
  scene.add(mesh);

  labels.replaceChildren(
    ...layout.blocks.map((block) => {
      const label = document.createElement('span');
      label.textContent = `${block.name} · ${block.gates}`;
      return label;
    }),
  );
  return { layout, wires, texture, material };
}

/** Fit the floorplan into the canvas and put the block labels over it. */
function fit(): void {
  const { clientWidth: w, clientHeight: h } = gateCanvas;
  if (w === 0 || h === 0) return;
  renderer.setSize(w, h, false);
  if (!view) return;
  const { layout } = view;
  const margin = 2;
  const scale = Math.min(w / (layout.width + 2 * margin), h / (layout.height + 2 * margin));
  const halfW = w / (2 * scale);
  const halfH = h / (2 * scale);
  const centreX = layout.width / 2;
  const centreY = layout.height / 2;
  camera.left = centreX - halfW;
  camera.right = centreX + halfW;
  camera.top = -centreY + halfH;
  camera.bottom = -centreY - halfH;
  camera.updateProjectionMatrix();
  layout.blocks.forEach((block, i) => {
    const label = labels.children[i] as HTMLElement | undefined;
    if (!label) return;
    label.style.left = `${(block.x - (centreX - halfW)) * scale}px`;
    label.style.top = `${(block.y - (centreY - halfH)) * scale}px`;
    // Small blocks keep room for their name.
    label.style.maxWidth = `${Math.max(block.width * scale, 72)}px`;
    label.style.fontSize = `${Math.max(9, Math.min(13, scale * 1.3))}px`;
  });
  dirty = true;
}

let dirty = true;
function frame(): void {
  if (dirty) {
    renderer.render(scene, camera);
    dirty = false;
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
  const search = searchFor(location.search, name, programs);
  if (search === location.search) return;
  try {
    history.replaceState(null, '', `${location.pathname}${search}${location.hash}`);
  } catch {
    // Some places (a page opened from a file) do not allow it. The link is
    // a convenience; the program runs all the same.
  }
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

// ───────────────────────────── the worker ─────────────────────────────

const worker = new Worker(new URL('./sim.worker.ts', import.meta.url), { type: 'module' });
const send = (msg: ToWorker) => worker.postMessage(msg);

// Real speeds: simulated clock cycles, and frames the program finishes, per
// second of wall-clock time.
const speed = new WindowRate(1000);
const fps = new WindowRate(2000);
let state: FrameState | undefined;

function show(msg: FrameState): void {
  state = msg;
  decodeFramebuffer(msg.framebuffer, screenPixels.data, AMBER);
  screenContext.putImageData(screenPixels, 0, 0);
  if (msg.program !== shownProgram) showProgram(msg.program);
  showConsole(msg);

  if (view) {
    view.wires.set(msg.wires);
    view.texture.needsUpdate = true;
    view.material.uniforms.uLive!.value = msg.engine === 'gate' ? 1 : 0;
    dirty = true;
  }

  const now = performance.now();
  speed.record(now, msg.cycles);
  fps.record(now, msg.frames);

  const s = decodeArch(msg.arch);
  el('engine').textContent = `${msg.engine} engine`;
  el('engine').dataset.engine = msg.engine;
  el('speed').textContent = msg.running && !msg.halted ? formatRate(speed.rate) : msg.halted ? 'halted' : 'paused';
  el('fps').textContent = `${fps.rate.toFixed(1)} frames/s`;
  el('cycles').textContent = `cycle ${msg.cycles.toLocaleString('en-US')}`;
  el('registers').textContent =
    `pc ${hex(s.pc, 4)}  sp ${hex(s.sp, 4)}  ir ${hex(s.ir, 2)}  step ${s.step}  flags ${flagsText(s.flags)}\n` +
    `a ${hex(s.regs[0] ?? 0, 2)}  b ${hex(s.regs[1] ?? 0, 2)}  ` +
    `x ${hex(pairValue(s.regs, 1), 4)}  y ${hex(pairValue(s.regs, 2), 4)}  z ${hex(pairValue(s.regs, 3), 4)}`;
  el('wires-note').hidden = msg.engine === 'gate';
  el('run').textContent = msg.running ? 'Pause' : 'Run';
  el<HTMLButtonElement>('step').disabled = msg.running;
  el('use-gate').setAttribute('aria-pressed', String(msg.engine === 'gate'));
  el('use-fast').setAttribute('aria-pressed', String(msg.engine === 'fast'));
}

worker.onmessage = (e: MessageEvent<FromWorker>) => {
  const msg = e.data;
  if (msg.type === 'ready') {
    buildProgramButtons(msg.programs);
    view = buildGateView(msg);
    fit();
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

const setEngine = (engine: EngineKind) => send({ type: 'engine', engine });
el('use-gate').addEventListener('click', () => setEngine('gate'));
el('use-fast').addEventListener('click', () => setEngine('fast'));
el('engine').addEventListener('click', () => setEngine(state?.engine === 'gate' ? 'fast' : 'gate'));
el('run').addEventListener('click', () => send({ type: 'run', running: !(state?.running ?? true) }));
el('step').addEventListener('click', () => send({ type: 'step', cycles: 1 }));
el('reset').addEventListener('click', () => send({ type: 'reset' }));
el<HTMLSelectElement>('clock').addEventListener('change', (e) => {
  const value = (e.target as HTMLSelectElement).value;
  send({ type: 'clock', hz: value === 'max' ? null : Number(value) });
});

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
  if (!(e.code in KEY_BITS) || e.ctrlKey || e.metaKey || e.altKey) return;
  // A list or a button that has the focus uses some keys itself.
  if (e.target instanceof Element && controlUsesKey(e.target.tagName, e.code)) return;
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

new ResizeObserver(fit).observe(gateCanvas);

// A link can choose the program and the engine: ?program=bounce&engine=gate
const link = new URLSearchParams(location.search);
const wantedProgram = link.get('program');
if (wantedProgram) send({ type: 'program', name: wantedProgram });
const wantedEngine = link.get('engine');
if (wantedEngine === 'gate' || wantedEngine === 'fast') setEngine(wantedEngine);

fit();
frame();
