/**
 * The scene, drawn. Everything that decides what the picture is (where
 * things are, how open each module is, what the words say) lives in the
 * plain modules beside this one; this file turns their arrays into six
 * draw calls and keeps two small textures up to date.
 *
 * **State by texture.** Every gate, wire segment, lid and letter is an
 * instance that knows the index of its wire and of its module. The state of
 * the whole circuit is one texel per wire (level, activity) and one per
 * module (how open, how much of it is seen, how busy). A frame of the
 * simulation or a move of the camera rewrites those few kilobytes and
 * nothing else; no geometry is ever rebuilt.
 *
 * **Relative to the eye.** From the whole computer down to the pins of one
 * gate is a range of scales that 32-bit floats cannot hold around a fixed
 * origin. Every position is therefore stored as a whole number and a small
 * rest (both exact as floats), and so is the eye; the vertex shader
 * subtracts whole from whole and rest from rest. The difference is exact
 * where it matters: near the eye.
 *
 * **Order of drawing.** Floors first, without depth, parents before
 * children. Then gates and wires, opaque. Then the lids, lowest first,
 * blended over what they cover; a lid that is half open lets half of what
 * is inside show through. Last the letters. The camera never goes below the
 * floor, so lower is always behind higher and nothing needs sorting.
 */
import * as THREE from 'three';
import type { NetlistInfo } from '../protocol';
import { GATE_FIELDS } from '../protocol';
import {
  eyeOf,
  focalLength,
  projection,
  rayThrough,
  viewRotation,
  type Camera,
  type Viewport,
} from './camera';
import { ADVANCE, buildLabels, Glyphs, writeValue } from './labels';
import { GATE_TOP, layoutScene, type Hints, type Rect, type SceneLayout } from './layout';
import { createLod, lodRange, updateLod, type Lod } from './lod';
import { pick, type Hit } from './pick';
import { routeWires, type Routing } from './route';
import { buildTree, type ModuleTree } from './tree';

/** Texels per row of the state textures. */
const STATE_WIDTH = 1024;
/** The body of a gate inside its cell. */
const GATE_WIDTH = 0.56;
const GATE_DEPTH = 0.44;
/** Half the width of a wire, in units; never drawn thinner than about a pixel. */
const WIRE_HALF = 0.035;
/** How much of a module's wires is left once the camera is inside one of its parts. */
const WIRE_FADED = '0.12';

/** The glyph sheet: printable ASCII, sixteen to a row. */
const SHEET_COLUMNS = 16;
const SHEET_ROWS = 6;
const CELL_WIDTH = 40;
const CELL_HEIGHT = 64;

/**
 * The scene's space is left-handed (see `camera.ts`), so triangles arrive
 * wound the other way round: the outside of a box is what WebGL calls its
 * back.
 */
const OUTSIDE = THREE.BackSide;

const COMMON = /* glsl */ `
  uniform vec4 uEye;   // x, y as whole parts, then the rests
  uniform float uEyeZ;
  uniform mat3 uView;
  uniform mat4 uProj;
  uniform float uFocal;
  uniform highp sampler2D uModules;
  uniform highp sampler2D uWires;

  // A point given as whole parts and rests, as seen from the eye. Whole
  // minus whole is exact, and so is rest minus rest.
  vec3 fromEye(vec4 at, float z) {
    return vec3((at.xy - uEye.xy) + (at.zw - uEye.zw), z - uEyeZ);
  }
  // r: how open the module is. g: how much of it is seen. b: how busy.
  // a: how far the camera has gone into one of its parts.
  vec4 moduleState(float module) {
    int i = int(module + 0.5);
    return texelFetch(uModules, ivec2(i % ${STATE_WIDTH}, i / ${STATE_WIDTH}), 0);
  }
  // r: the level, 0 or 1/255. g: the activity.
  vec2 wireState(float wire) {
    int i = int(wire + 0.5);
    return texelFetch(uWires, ivec2(i % ${STATE_WIDTH}, i / ${STATE_WIDTH}), 0).rg;
  }
  // Somewhere that is never drawn.
  const vec4 NOWHERE = vec4(2.0, 2.0, 2.0, 1.0);
`;

const FRAGMENT_COMMON = /* glsl */ `
  uniform float uLive;
  // Wires that are not being simulated are shown without colour.
  vec3 stale(vec3 colour) {
    float grey = dot(colour, vec3(0.3, 0.5, 0.2));
    return mix(vec3(grey) * 0.6, colour, uLive);
  }
  // 1 on a line about a pixel wide along the edge of a rectangle, given the
  // distance to each pair of edges in units.
  float edgeLine(vec2 fromEdge) {
    vec2 px = fromEdge / max(fwidth(fromEdge), vec2(1e-6));
    return 1.0 - smoothstep(0.7, 1.7, min(px.x, px.y));
  }
`;

const PLATE_VERTEX = /* glsl */ `
  ${COMMON}
  attribute vec4 aAt;
  attribute vec3 aSize; // width, height, title strip
  attribute vec2 aInfo; // module, depth in the tree
  varying vec2 vUv;
  varying vec3 vSize;
  varying float vDepth;
  void main() {
    if (moduleState(aInfo.x).g == 0.0) { gl_Position = NOWHERE; return; }
    vUv = position.xy;
    vSize = aSize;
    vDepth = aInfo.y;
    gl_Position = uProj * vec4(uView * (fromEye(aAt, 0.0) + vec3(position.xy * aSize.xy, 0.0)), 1.0);
  }
`;
const PLATE_FRAGMENT = /* glsl */ `
  ${FRAGMENT_COMMON}
  varying vec2 vUv;
  varying vec3 vSize;
  varying float vDepth;
  void main() {
    // Each level of the tree is a shade lighter than the one it lies on.
    vec3 colour = vec3(0.024, 0.034, 0.058) + vec3(0.007, 0.010, 0.016) * vDepth;
    colour += step(vUv.y * vSize.y, vSize.z) * vec3(0.010, 0.014, 0.024);
    float line = edgeLine(min(vUv, 1.0 - vUv) * vSize.xy);
    gl_FragColor = vec4(mix(colour, vec3(0.105, 0.145, 0.225), line), 1.0);
  }
`;

const GATE_VERTEX = /* glsl */ `
  ${COMMON}
  attribute vec4 aAt;
  attribute vec3 aGate; // kind, output wire, module
  attribute vec2 aIn;   // input wires
  varying vec3 vNormal;
  varying vec2 vTop;
  varying float vKind;
  varying float vLevel;
  varying vec2 vInputs;
  varying float vPixels;
  void main() {
    vec4 state = moduleState(aGate.z);
    if (state.r * state.g == 0.0) { gl_Position = NOWHERE; return; }
    vec3 body = position * vec3(${GATE_WIDTH}, ${GATE_DEPTH}, ${GATE_TOP});
    vec3 seen = uView * (fromEye(aAt, 0.0) + body);
    gl_Position = uProj * vec4(seen, 1.0);
    vNormal = normal;
    vTop = position.xy * vec2(${GATE_WIDTH}, ${GATE_DEPTH});
    vKind = aGate.x;
    vLevel = wireState(aGate.y).r * 255.0;
    vInputs = vec2(wireState(aIn.x).r, wireState(aIn.y).r) * 255.0;
    vPixels = uFocal / max(-seen.z, 1e-6);
  }
`;
const GATE_FRAGMENT = /* glsl */ `
  ${FRAGMENT_COMMON}
  varying vec3 vNormal;
  varying vec2 vTop;
  varying float vKind;
  varying float vLevel;
  varying vec2 vInputs;
  varying float vPixels;
  const vec3 OFF = vec3(0.085, 0.115, 0.175);
  // NAND outputs are amber, flip-flops (the state) cyan.
  vec3 lit(float level) {
    return mix(OFF, mix(vec3(1.0, 0.70, 0.28), vec3(0.35, 0.86, 1.0), vKind), step(0.5, level));
  }
  // 1 inside a disc of radius r around c.
  float disc(vec2 c, float r) {
    float d = length(vTop - c);
    return 1.0 - smoothstep(r - fwidth(d), r, d);
  }
  void main() {
    vec3 colour = lit(vLevel);
    if (vNormal.z > 0.5) {
      // From close enough the pins show on the top: the inputs on the
      // left and the output on the right, each with its own level.
      float detail = smoothstep(18.0, 40.0, vPixels);
      if (detail > 0.0) {
        float split = (1.0 - vKind) * 0.13;
        float inputA = disc(vec2(-0.2, -split), 0.055);
        float inputB = (1.0 - vKind) * disc(vec2(-0.2, split), 0.055);
        float pins = max(inputA, inputB);
        vec3 pinColour = mix(vec3(0.02, 0.03, 0.05), vec3(1.0, 0.86, 0.6), step(0.5, inputA > inputB ? vInputs.x : vInputs.y));
        // A dark bed around the pins keeps them readable on a lit gate.
        float bed = max(disc(vec2(-0.2, -split), 0.085), (1.0 - vKind) * disc(vec2(-0.2, split), 0.085));
        colour = mix(colour, colour * 0.45, bed * detail);
        colour = mix(colour, pinColour, pins * detail);
      }
    } else {
      colour *= 0.5 + 0.2 * abs(vNormal.x);
    }
    gl_FragColor = vec4(stale(colour), 1.0);
  }
`;

const WIRE_VERTEX = /* glsl */ `
  ${COMMON}
  attribute vec4 aA;
  attribute vec4 aB;
  attribute vec4 aWire; // height of each end, wire, module
  varying float vLevel;
  varying float vBusy;
  varying float vCover;
  varying float vAlpha;
  void main() {
    vec4 state = moduleState(aWire.w);
    // A module's wires appear as its lid fades, and fall back to a faint
    // trace once the camera goes into one of its parts (alpha channel):
    // there the wires inside that part are what matters, and the ones
    // passing over it would hide it.
    float alpha = state.r * state.g * mix(1.0, ${WIRE_FADED}, state.a);
    if (alpha < 0.01) { gl_Position = NOWHERE; return; }
    vAlpha = alpha;
    vec3 a = fromEye(aA, aWire.x);
    vec3 b = fromEye(aB, aWire.y);
    vec3 along = normalize(b - a);
    vec3 here = mix(a, b, position.x);
    // A ribbon facing the eye: across both the wire and the line of sight.
    vec3 across = cross(along, here);
    float size = length(across);
    across = size > 1e-6 ? across / size : vec3(0.0);
    // Never thinner than about a pixel, or it would break up into dots.
    float least = 0.6 * -(uView * here).z / uFocal;
    float halfWidth = max(${WIRE_HALF}, least);
    here += across * halfWidth * position.y + along * halfWidth * (position.x * 2.0 - 1.0);
    gl_Position = uProj * vec4(uView * here, 1.0);
    vec2 wire = wireState(aWire.z);
    vLevel = wire.r * 255.0;
    vBusy = wire.g;
    vCover = clamp(${WIRE_HALF} / least, 0.0, 1.0);
  }
`;
const WIRE_FRAGMENT = /* glsl */ `
  ${FRAGMENT_COMMON}
  uniform float uGlow;
  varying float vLevel;
  varying float vBusy;
  varying float vCover;
  varying float vAlpha;
  void main() {
    vec3 colour = mix(vec3(0.11, 0.155, 0.24), vec3(1.0, 0.70, 0.28), step(0.5, vLevel));
    // A wire that has been switching glows, whatever its level this instant.
    colour += vec3(0.95, 0.50, 0.12) * vBusy * uGlow * 0.55;
    // One too thin to be seen properly sinks towards the floor instead of sparkling.
    colour = mix(vec3(0.05, 0.07, 0.11), colour, mix(0.45, 1.0, vCover));
    gl_FragColor = vec4(stale(colour), vAlpha);
  }
`;

const LID_VERTEX = /* glsl */ `
  ${COMMON}
  attribute vec4 aAt;
  attribute vec3 aSize; // width, height, height above the floor
  attribute vec2 aInfo; // module, depth in the tree
  varying vec3 vNormal;
  varying vec2 vUv;
  varying vec2 vSize;
  varying float vAlpha;
  varying float vDepth;
  varying float vBusy;
  void main() {
    vec4 state = moduleState(aInfo.x);
    float alpha = (1.0 - state.r) * step(0.002, state.g);
    if (alpha < 0.004) { gl_Position = NOWHERE; return; }
    gl_Position = uProj * vec4(uView * (fromEye(aAt, 0.0) + position * aSize), 1.0);
    vNormal = normal;
    vUv = position.xy;
    vSize = aSize.xy;
    vAlpha = alpha;
    vDepth = aInfo.y;
    vBusy = state.b;
  }
`;
const LID_FRAGMENT = /* glsl */ `
  ${FRAGMENT_COMMON}
  varying vec3 vNormal;
  varying vec2 vUv;
  varying vec2 vSize;
  varying float vAlpha;
  varying float vDepth;
  varying float vBusy;
  void main() {
    vec3 colour = vec3(0.050, 0.070, 0.112) + vec3(0.009, 0.012, 0.019) * vDepth;
    // A block with a lot going on inside is warm.
    colour += vec3(0.50, 0.26, 0.05) * vBusy * uLive;
    if (vNormal.z > 0.5) {
      colour = mix(colour, vec3(0.20, 0.27, 0.40), edgeLine(min(vUv, 1.0 - vUv) * vSize));
    } else {
      colour *= 0.55 + 0.2 * abs(vNormal.x);
    }
    gl_FragColor = vec4(colour, vAlpha);
  }
`;

const GLYPH_VERTEX = /* glsl */ `
  ${COMMON}
  attribute vec4 aAt;
  attribute vec4 aGlyph; // height above the floor, size, character, module
  attribute vec2 aStyle; // when it shows, ink
  varying vec2 vUv;
  varying float vAlpha;
  varying float vInk;
  void main() {
    vec4 state = moduleState(aGlyph.w);
    float alpha = (aStyle.x < 0.5 ? 1.0 - state.r : state.r) * state.g;
    vec3 seen = uView * (fromEye(aAt, aGlyph.x) + vec3(position.x * ${ADVANCE}, position.y, 0.0) * aGlyph.y);
    // Letters too small to read are not drawn: they would only be noise.
    alpha *= smoothstep(4.0, 7.0, aGlyph.y * uFocal / max(-seen.z, 1e-6));
    if (alpha < 0.01) { gl_Position = NOWHERE; return; }
    gl_Position = uProj * vec4(seen, 1.0);
    float code = aGlyph.z - 32.0;
    vec2 cell = vec2(mod(code, ${SHEET_COLUMNS}.0), floor(code / ${SHEET_COLUMNS}.0));
    vUv = (cell + position.xy) / vec2(${SHEET_COLUMNS}.0, ${SHEET_ROWS}.0);
    vAlpha = alpha;
    vInk = aStyle.y;
  }
`;
const GLYPH_FRAGMENT = /* glsl */ `
  uniform sampler2D uGlyphs;
  varying vec2 vUv;
  varying float vAlpha;
  varying float vInk;
  void main() {
    float cover = texture2D(uGlyphs, vUv).r;
    // Enlarged, the sheet's soft edges are drawn sharp again around the
    // half-covered line; reduced, the sheet's own smaller copies are right.
    float texels = fwidth(vUv.x) * ${SHEET_COLUMNS * CELL_WIDTH}.0;
    float sharp = clamp((cover - 0.5) / max(fwidth(cover), 1e-4) + 0.5, 0.0, 1.0);
    cover = mix(cover, sharp, clamp(1.0 / max(texels, 1e-4) - 0.75, 0.0, 1.0));
    vec3 ink = vInk < 0.5 ? vec3(0.79, 0.84, 0.91) : vInk < 1.5 ? vec3(0.42, 0.48, 0.57) : vec3(1.0, 0.70, 0.28);
    gl_FragColor = vec4(ink, cover * vAlpha);
  }
`;

const MARK_VERTEX = /* glsl */ `
  ${COMMON}
  uniform vec4 uMarkAt;
  uniform vec3 uMarkSize; // width, height, height above the floor
  varying vec2 vUv;
  varying vec2 vSize;
  void main() {
    vUv = position.xy;
    vSize = uMarkSize.xy;
    gl_Position = uProj * vec4(uView * (fromEye(uMarkAt, uMarkSize.z) + vec3(position.xy * uMarkSize.xy, 0.0)), 1.0);
  }
`;
const MARK_FRAGMENT = /* glsl */ `
  varying vec2 vUv;
  varying vec2 vSize;
  void main() {
    vec2 fromEdge = min(vUv, 1.0 - vUv) * vSize;
    vec2 px = fromEdge / max(fwidth(fromEdge), vec2(1e-6));
    float line = 1.0 - smoothstep(1.2, 2.2, min(px.x, px.y));
    gl_FragColor = vec4(1.0, 0.70, 0.28, line * 0.9 + 0.05);
  }
`;

/** Write a point as whole parts and rests: x, y → [round x, round y, x - round x, y - round y]. */
function splitPoint(target: Float32Array, at: number, x: number, y: number): void {
  target[at] = Math.round(x);
  target[at + 1] = Math.round(y);
  target[at + 2] = x - Math.round(x);
  target[at + 3] = y - Math.round(y);
}

/** A unit square from (0, 0) to (1, 1). */
function unitSquare(): THREE.BufferGeometry {
  const square = new THREE.BufferGeometry();
  square.setAttribute('position', new THREE.Float32BufferAttribute([0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0], 3));
  square.setIndex([0, 1, 2, 2, 1, 3]);
  return square;
}

/** `base` drawn once for each of `count` instances, each with the given attributes. */
function instanced(
  base: THREE.BufferGeometry,
  count: number,
  attributes: Record<string, [Float32Array, number]>,
): THREE.InstancedBufferGeometry {
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.index = base.index;
  for (const name of ['position', 'normal']) {
    if (base.hasAttribute(name)) geometry.setAttribute(name, base.getAttribute(name));
  }
  for (const [name, [array, size]] of Object.entries(attributes)) {
    geometry.setAttribute(name, new THREE.InstancedBufferAttribute(array, size));
  }
  geometry.instanceCount = count;
  return geometry;
}

/** A texture of `count` texels, `channels` bytes each, laid out in rows of `STATE_WIDTH`. */
function stateTexture(count: number, channels: 2 | 4): { data: Uint8Array; texture: THREE.DataTexture } {
  const rows = Math.max(1, Math.ceil(count / STATE_WIDTH));
  const data = new Uint8Array(STATE_WIDTH * rows * channels);
  const texture = new THREE.DataTexture(
    data,
    STATE_WIDTH,
    rows,
    channels === 2 ? THREE.RGFormat : THREE.RGBAFormat,
    THREE.UnsignedByteType,
  );
  texture.needsUpdate = true;
  return { data, texture };
}

/** Printable ASCII drawn white on black, one character per cell. */
function glyphSheet(): THREE.CanvasTexture {
  const canvas = document.createElement('canvas');
  canvas.width = SHEET_COLUMNS * CELL_WIDTH;
  canvas.height = SHEET_ROWS * CELL_HEIGHT;
  const context = canvas.getContext('2d')!;
  context.fillStyle = '#000';
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.fillStyle = '#fff';
  context.font = '600 50px ui-monospace, "SF Mono", Menlo, Consolas, "DejaVu Sans Mono", monospace';
  context.textAlign = 'center';
  context.textBaseline = 'alphabetic';
  for (let code = 33; code < 127; code++) {
    const cell = code - 32;
    const x = (cell % SHEET_COLUMNS) * CELL_WIDTH + CELL_WIDTH / 2;
    const y = Math.floor(cell / SHEET_COLUMNS) * CELL_HEIGHT + 47;
    context.fillText(String.fromCharCode(code), x, y);
  }
  const texture = new THREE.CanvasTexture(canvas);
  // Row 0 of the canvas is row 0 of the texture: v grows downwards, like y on the plan.
  texture.flipY = false;
  texture.generateMipmaps = true;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.magFilter = THREE.LinearFilter;
  return texture;
}

export class SceneView {
  readonly tree: ModuleTree;
  readonly layout: SceneLayout;
  readonly routing: Routing;
  readonly lod: Lod;
  /** Size of the picture in CSS pixels. */
  viewport: Viewport = { width: 1, height: 1 };
  camera: Camera = { x: 0, y: 0, distance: 1, azimuth: 0, elevation: Math.PI / 2 };

  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly eye = new THREE.Camera();
  private readonly uniforms: Record<string, THREE.IUniform>;
  private readonly modules: { data: Uint8Array; texture: THREE.DataTexture };
  private readonly wires: { data: Uint8Array; texture: THREE.DataTexture };
  /** The wire each gate drives, for adding up how busy each module is. */
  private readonly outputOfGate: Uint32Array;
  /** Per module: how busy the wires driven inside it have been, 0 to 255. */
  private readonly busy: Uint8Array;
  private readonly values = new Map<number, string>();
  private readonly valueGlyphs: Glyphs;
  private readonly valueGeometry: THREE.InstancedBufferGeometry;
  private valuesChanged = false;
  private readonly diagonal: number;

  constructor(
    readonly canvas: HTMLCanvasElement,
    net: NetlistInfo,
    hints: Hints = {},
  ) {
    this.tree = buildTree(net);
    this.layout = layoutScene(this.tree, hints);
    this.routing = routeWires(net, this.tree, this.layout);
    this.lod = createLod(this.tree);
    this.diagonal = Math.hypot(this.layout.width, this.layout.height);
    const { tree, layout, routing } = this;

    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    this.scene.background = new THREE.Color(0x05070c);

    this.modules = stateTexture(tree.count, 4);
    this.wires = stateTexture(net.wireCount, 2);
    this.busy = new Uint8Array(tree.count);
    this.outputOfGate = new Uint32Array(tree.gateCount);
    const sheet = glyphSheet();
    sheet.anisotropy = Math.min(8, this.renderer.capabilities.getMaxAnisotropy());

    this.uniforms = {
      uEye: { value: new THREE.Vector4() },
      uEyeZ: { value: 1 },
      uView: { value: new THREE.Matrix3() },
      uProj: { value: new THREE.Matrix4() },
      uFocal: { value: 1 },
      uModules: { value: this.modules.texture },
      uWires: { value: this.wires.texture },
      uGlyphs: { value: sheet },
      uLive: { value: 0 },
      uGlow: { value: 1 },
      uMarkAt: { value: new THREE.Vector4() },
      uMarkSize: { value: new THREE.Vector3() },
    };
    const material = (vertexShader: string, fragmentShader: string, options: THREE.ShaderMaterialParameters = {}) =>
      new THREE.ShaderMaterial({ uniforms: this.uniforms, vertexShader, fragmentShader, ...options });
    const add = (geometry: THREE.BufferGeometry, shader: THREE.ShaderMaterial, order: number) => {
      const mesh = new THREE.Mesh(geometry, shader);
      // Instances are placed by the shaders; nothing here knows their bounds.
      mesh.frustumCulled = false;
      mesh.renderOrder = order;
      this.scene.add(mesh);
      return mesh;
    };

    // ── floors: one rectangle per module, parents before children ──
    {
      const at = new Float32Array(tree.count * 4);
      const size = new Float32Array(tree.count * 3);
      const info = new Float32Array(tree.count * 2);
      for (let m = 0; m < tree.count; m++) {
        splitPoint(at, m * 4, layout.rects[m * 4]!, layout.rects[m * 4 + 1]!);
        size.set([layout.rects[m * 4 + 2]!, layout.rects[m * 4 + 3]!, layout.headers[m]!], m * 3);
        info.set([m, tree.depth[m]!], m * 2);
      }
      const geometry = instanced(unitSquare(), tree.count, { aAt: [at, 4], aSize: [size, 3], aInfo: [info, 2] });
      add(geometry, material(PLATE_VERTEX, PLATE_FRAGMENT, { depthTest: false, depthWrite: false, side: THREE.DoubleSide }), 0);
    }

    // ── gates: a small block each ──
    {
      const at = new Float32Array(tree.gateCount * 4);
      const gate = new Float32Array(tree.gateCount * 3);
      const inputs = new Float32Array(tree.gateCount * 2);
      for (let g = 0; g < tree.gateCount; g++) {
        const [kind, a, b, out, module] = net.gates.subarray(g * GATE_FIELDS, (g + 1) * GATE_FIELDS);
        splitPoint(at, g * 4, layout.gates[g * 2]!, layout.gates[g * 2 + 1]!);
        gate.set([kind!, out!, module!], g * 3);
        inputs.set([a!, b!], g * 2);
        this.outputOfGate[g] = out!;
      }
      // Centred on the cell, standing on the floor.
      const block = new THREE.BoxGeometry(1, 1, 1).translate(0, 0, 0.5);
      const geometry = instanced(block, tree.gateCount, { aAt: [at, 4], aGate: [gate, 3], aIn: [inputs, 2] });
      add(geometry, material(GATE_VERTEX, GATE_FRAGMENT, { side: OUTSIDE }), 1);
    }

    // ── wires: a ribbon per segment ──
    {
      const a = new Float32Array(routing.count * 4);
      const b = new Float32Array(routing.count * 4);
      const wire = new Float32Array(routing.count * 4);
      for (let s = 0; s < routing.count; s++) {
        const e = routing.ends.subarray(s * 6, s * 6 + 6);
        splitPoint(a, s * 4, e[0]!, e[1]!);
        splitPoint(b, s * 4, e[3]!, e[4]!);
        wire.set([e[2]!, e[5]!, routing.wire[s]!, routing.module[s]!], s * 4);
      }
      // x runs along the wire from 0 to 1, y across it from -1 to 1.
      const ribbon = new THREE.BufferGeometry();
      ribbon.setAttribute('position', new THREE.Float32BufferAttribute([0, -1, 0, 1, -1, 0, 0, 1, 0, 1, 1, 0], 3));
      ribbon.setIndex([0, 1, 2, 2, 1, 3]);
      const geometry = instanced(ribbon, routing.count, { aA: [a, 4], aB: [b, 4], aWire: [wire, 4] });
      // After the lids and without depth: a module's wires run above every
      // lid that can still be seen inside it, so drawing them later is
      // drawing them in front. The letters come after them, and so stay
      // readable where a wire crosses a name.
      add(
        geometry,
        material(WIRE_VERTEX, WIRE_FRAGMENT, { side: THREE.DoubleSide, transparent: true, depthWrite: false }),
        3.5,
      );
    }

    // ── lids: a box per module, lowest first ──
    {
      const order = Array.from({ length: tree.count }, (_, m) => m).sort(
        (p, q) => layout.lidZ[p]! - layout.lidZ[q]! || p - q,
      );
      const at = new Float32Array(tree.count * 4);
      const size = new Float32Array(tree.count * 3);
      const info = new Float32Array(tree.count * 2);
      order.forEach((m, i) => {
        splitPoint(at, i * 4, layout.rects[m * 4]!, layout.rects[m * 4 + 1]!);
        size.set([layout.rects[m * 4 + 2]!, layout.rects[m * 4 + 3]!, layout.lidZ[m]!], i * 3);
        info.set([m, tree.depth[m]!], i * 2);
      });
      const box = new THREE.BoxGeometry(1, 1, 1).translate(0.5, 0.5, 0.5);
      const geometry = instanced(box, tree.count, { aAt: [at, 4], aSize: [size, 3], aInfo: [info, 2] });
      add(geometry, material(LID_VERTEX, LID_FRAGMENT, { side: OUTSIDE, transparent: true, depthWrite: false }), 3);
    }

    // ── letters ──
    const glyphMaterial = material(GLYPH_VERTEX, GLYPH_FRAGMENT, {
      side: THREE.DoubleSide,
      transparent: true,
      depthWrite: false,
    });
    const glyphGeometry = (glyphs: Glyphs) =>
      instanced(unitSquare(), glyphs.count, { aAt: [glyphs.at, 4], aGlyph: [glyphs.glyph, 4], aStyle: [glyphs.style, 2] });
    add(glyphGeometry(buildLabels(tree, layout)), glyphMaterial, 4);
    // Values change with the simulation; room for two lines of 16 on every module.
    this.valueGlyphs = new Glyphs(tree.count * 32);
    this.valueGeometry = glyphGeometry(this.valueGlyphs);
    for (const name of ['aAt', 'aGlyph', 'aStyle']) {
      (this.valueGeometry.getAttribute(name) as THREE.InstancedBufferAttribute).setUsage(THREE.DynamicDrawUsage);
    }
    add(this.valueGeometry, glyphMaterial, 5);

    // ── the mark around what the pointer is on ──
    this.mark = add(
      unitSquare(),
      material(MARK_VERTEX, MARK_FRAGMENT, { side: THREE.DoubleSide, transparent: true, depthTest: false, depthWrite: false }),
      6,
    );
    this.mark.visible = false;
  }

  private readonly mark: THREE.Mesh;

  /** Follow the size of the canvas on the page. Returns whether there is anything to draw on. */
  resize(): boolean {
    const { clientWidth: width, clientHeight: height } = this.canvas;
    if (width === 0 || height === 0) return false;
    if (width !== this.viewport.width || height !== this.viewport.height) {
      this.viewport = { width, height };
      this.renderer.setSize(width, height, false);
    }
    return true;
  }

  /**
   * The wires as the simulation last saw them: a level and an activity for
   * each. `live` says whether they are being simulated now; if not, they
   * are drawn without colour.
   */
  setWires(levels: Uint8Array, activity: Uint8Array, live: boolean): void {
    const { data } = this.wires;
    for (let w = 0; w < levels.length; w++) {
      data[w * 2] = levels[w]!;
      data[w * 2 + 1] = activity[w]!;
    }
    this.wires.texture.needsUpdate = true;
    this.uniforms.uLive!.value = live ? 1 : 0;

    // How busy each module is: the mean activity of the wires driven in it
    // and in everything below it.
    const { tree } = this;
    const sum = new Float64Array(tree.count);
    for (let g = 0; g < tree.gateCount; g++) sum[tree.moduleOfGate[g]!]! += activity[this.outputOfGate[g]!]!;
    for (let m = tree.count - 1; m >= 0; m--) {
      const total = tree.totalGates[m]!;
      this.busy[m] = total === 0 ? 0 : Math.min(255, Math.round(sum[m]! / total));
      const parent = tree.parents[m]!;
      if (parent >= 0) sum[parent]! += sum[m]!;
    }
  }

  /** Show `text` on a module, as the value it holds; an empty text removes it. */
  setValue(module: number, text: string): void {
    if ((this.values.get(module) ?? '') === text) return;
    if (text === '') this.values.delete(module);
    else this.values.set(module, text);
    this.valuesChanged = true;
  }

  /** Put a mark around a rectangle of the plan at a height, or take it away. */
  setMark(rect: Rect | undefined, z = 0): void {
    this.mark.visible = rect !== undefined;
    if (!rect) return;
    const at = new Float32Array(4);
    splitPoint(at, 0, rect.x, rect.y);
    (this.uniforms.uMarkAt!.value as THREE.Vector4).fromArray(at);
    (this.uniforms.uMarkSize!.value as THREE.Vector3).set(rect.w, rect.h, z);
  }

  /** What is under a point of the picture, given in CSS pixels. */
  pickAt(px: number, py: number): Hit | undefined {
    const { origin, direction } = rayThrough(this.camera, this.viewport, px, py);
    return pick(this.tree, this.layout, this.lod, origin, direction);
  }

  /** Work out the levels of detail for the camera as it is, and draw. */
  render(): void {
    const { camera, viewport, tree, layout, lod } = this;
    const eye = eyeOf(camera);
    const focal = focalLength(viewport);
    updateLod(tree, layout, eye, focal, lod, lodRange(viewport));
    const state = this.modules.data;
    for (let m = 0; m < tree.count; m++) {
      state[m * 4] = Math.round(lod.open[m]! * 255);
      // Rounded up: something seen at all is never taken for hidden.
      state[m * 4 + 1] = Math.ceil(lod.seen[m]! * 255);
      state[m * 4 + 2] = this.busy[m]!;
      state[m * 4 + 3] = Math.round(lod.entered[m]! * 255);
    }
    this.modules.texture.needsUpdate = true;

    if (this.valuesChanged) {
      this.valuesChanged = false;
      this.valueGlyphs.clear();
      for (const [module, text] of this.values) writeValue(this.valueGlyphs, tree, layout, module, text);
      this.valueGeometry.instanceCount = this.valueGlyphs.count;
      for (const name of ['aAt', 'aGlyph', 'aStyle']) this.valueGeometry.getAttribute(name).needsUpdate = true;
    }

    const u = this.uniforms;
    (u.uEye!.value as THREE.Vector4).set(
      Math.round(eye[0]),
      Math.round(eye[1]),
      eye[0] - Math.round(eye[0]),
      eye[1] - Math.round(eye[1]),
    );
    u.uEyeZ!.value = eye[2];
    (u.uView!.value as THREE.Matrix3).fromArray(viewRotation(camera));
    // Near and far follow the camera, so that depth is as fine as it can
    // be at every scale: from a twentieth of the distance to the point
    // looked at, to beyond the far corner of the scene.
    const near = camera.distance * 0.05;
    const far = camera.distance + 1.5 * this.diagonal;
    (u.uProj!.value as THREE.Matrix4).fromArray(projection(viewport.width / viewport.height, near, far));
    u.uFocal!.value = focal;
    this.renderer.render(this.scene, this.eye);
  }
}
