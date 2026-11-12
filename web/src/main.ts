/**
 * M0 walking skeleton: a four-NAND XOR simulated in Rust/WASM inside a worker,
 * drawn with Three.js on the main thread. Every colour on screen is a wire
 * value read from the simulation. The real scene replaces this in M4.
 */
import * as THREE from 'three';
import { breadcrumb, parentPath, wireLevel, type FromWorker, type GateInfo, type ToWorker } from './protocol';

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const canvas = el<HTMLCanvasElement>('scene');
const status = el('status');

const ON = new THREE.Color(0xffb347); // logic 1 glows
const OFF = new THREE.Color(0x1b2536); // logic 0 is dim

const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
const scene = new THREE.Scene();
scene.background = new THREE.Color(0x05070c);
const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 100);
camera.position.set(0, 2.2, 9);
camera.lookAt(0, 0, 0);

// Where each gate of the XOR sits: n0 feeds n1 and n2, which feed n3.
const GATE_POS: readonly (readonly [number, number])[] = [[-1.5, 0], [0.5, 1.1], [0.5, -1.1], [2.5, 0]];
const INPUT_POS = { a: [-3.5, 1.1], b: [-3.5, -1.1] } as const;

interface Lit { wire: number; material: THREE.MeshBasicMaterial | THREE.LineBasicMaterial }
const lit: Lit[] = [];

function addNode(x: number, y: number, wire: number, size: number): void {
  const material = new THREE.MeshBasicMaterial({ color: OFF });
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(size, size, size * 0.4), material);
  mesh.position.set(x, y, 0);
  scene.add(mesh);
  lit.push({ wire, material });
}

function addWire(from: readonly [number, number], to: readonly [number, number], wire: number): void {
  const material = new THREE.LineBasicMaterial({ color: OFF });
  const mid = (from[0] + to[0]) / 2;
  const points = [[from[0], from[1]], [mid, from[1]], [mid, to[1]], [to[0], to[1]]].map(
    ([x, y]) => new THREE.Vector3(x, y, 0),
  );
  scene.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(points), material));
  lit.push({ wire, material });
}

function buildScene(gates: GateInfo[]): void {
  const posOfWire = new Map<number, readonly [number, number]>();
  gates.forEach((g, i) => posOfWire.set(g.out, GATE_POS[i] ?? [i * 2, 0]));
  // The two wires not driven by any gate are the primary inputs a and b.
  const first = gates[0];
  if (first) {
    posOfWire.set(first.a, INPUT_POS.a);
    posOfWire.set(first.b, INPUT_POS.b);
    addNode(...INPUT_POS.a, first.a, 0.45);
    addNode(...INPUT_POS.b, first.b, 0.45);
  }
  gates.forEach((g, i) => {
    const pos = GATE_POS[i] ?? [i * 2, 0];
    addNode(pos[0], pos[1], g.out, 0.9);
    for (const input of [g.a, g.b]) {
      const from = posOfWire.get(input);
      if (from) addWire(from, pos, input);
    }
  });
  el('crumbs').textContent = breadcrumb(parentPath(first?.path ?? ''));
}

function resize(): void {
  const { clientWidth: w, clientHeight: h } = canvas;
  renderer.setSize(w, h, false);
  camera.aspect = w / h;
  // Keep the whole circuit in view on narrow (portrait) screens.
  camera.position.z = Math.max(9, 9 / camera.aspect);
  camera.updateProjectionMatrix();
}

let dirty = true;
function frame(): void {
  if (dirty) {
    renderer.render(scene, camera);
    dirty = false;
  }
  requestAnimationFrame(frame);
}

const worker = new Worker(new URL('./sim.worker.ts', import.meta.url), { type: 'module' });
const send = (msg: ToWorker) => worker.postMessage(msg);

worker.onmessage = (e: MessageEvent<FromWorker>) => {
  const msg = e.data;
  if (msg.type === 'ready') {
    buildScene(msg.gates);
    el('engine').textContent = `${msg.engine} engine`;
    status.textContent = `core v${msg.version} · ${msg.gates.length} NAND gates · press A / B`;
    document.body.dataset.ready = 'true';
  } else if (msg.type === 'state') {
    for (const { wire, material } of lit) material.color.copy(wireLevel(msg.wires, wire) ? ON : OFF);
    dirty = true;
  } else {
    status.textContent = `simulation failed: ${msg.message}`;
  }
};
worker.onerror = (e) => { status.textContent = `simulation failed: ${e.message}`; };

const inputs = { a: false, b: false };
function toggle(name: 'a' | 'b'): void {
  inputs[name] = !inputs[name];
  const button = el(`in-${name}`);
  button.textContent = `${name.toUpperCase()} = ${inputs[name] ? 1 : 0}`;
  button.setAttribute('aria-pressed', String(inputs[name]));
  send({ type: 'setInputs', ...inputs });
}
el('in-a').addEventListener('click', () => toggle('a'));
el('in-b').addEventListener('click', () => toggle('b'));
window.addEventListener('keydown', (e) => {
  if (e.repeat || e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key === 'a' || e.key === 'b') toggle(e.key);
});
window.addEventListener('resize', () => { resize(); dirty = true; });

resize();
frame();
