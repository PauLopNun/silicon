/**
 * The scene, drawn. Everything that decides what the picture is (where
 * things are, how open each module is, what the words say) lives in the
 * plain modules beside this one; this file turns their arrays into ten
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
 * gate is a range of scales that 32-bit floats hold poorly around a fixed
 * origin. Every position is therefore stored as a whole number and a small
 * rest (both exact as floats), and so is the eye; the vertex shader
 * subtracts whole from whole and rest from rest. The difference is exact
 * where it matters: near the eye.
 *
 * **Order of drawing.** The floor first, without depth, parents before
 * children. Then the gates, opaque. Then the lids, lowest first, blended
 * over what they cover; a lid that is half open lets half of what is inside
 * show through. Then the lights of the gates under closed lids, the wires,
 * and last the letters. The camera never goes below the floor, so lower is
 * always behind higher and nothing needs sorting.
 *
 * **Light.** What gives off light (a wire that is switching, a pulse on its
 * way, a gate whose turn has come) is drawn brighter than the picture's
 * white, and `bloom.ts` turns that excess into a glow. Every such light is a
 * fact about the simulation: how busy a wire has been, which way its signal
 * goes, when in the order of evaluation a gate changes.
 */
import * as THREE from 'three';
import type { NetlistInfo } from '../protocol';
import { GATE_FIELDS } from '../protocol';
import { Bloom } from './bloom';
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
import { createLod, focusModule, lodRange, updateLod, type Lod } from './lod';
import { pick, type Hit } from './pick';
import { planReplay } from './replay';
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

/**
 * Pulses on the wires. While the program runs, a busy wire carries dashes
 * that move away from the gate driving it: how bright they are is how busy
 * the wire has been; which way they go is the way the signal goes. Their
 * speed is the picture's, not the signal's: this many dashes pass a point
 * each second.
 */
const STREAM_HZ = '1.3';
/**
 * The dashes are drawn against a clock that starts again this often, so
 * that it stays exact as a 32-bit float. A whole number of dashes pass in
 * that time (600 × 1.3 = 780), so nothing jumps when it does.
 */
const CLOCK_WRAP_MS = 600_000;
/**
 * After a single step, what changed is played back in the order of logic
 * depth: a gate changes one gate delay after the deepest gate that feeds
 * it, which is the order the gate engine evaluates them in. This is how
 * long one gate delay is drawn out to, in seconds.
 */
const GATE_DELAY_SECONDS = 0.07;
/** The deepest logic depth the wire texture can hold beside the broken flag. */
const MAX_DEPTH = 127;
/** A wire's dashes are this fraction of the size of the module it belongs to, within limits. */
const DASH_OF_MODULE = 0.1;
const DASH_MIN = 0.9;
const DASH_MAX = 14;
/** How quickly the lights of far-off gates follow their levels: the share of the way covered at each new state. */
const LIGHT_FOLLOWS = 0.35;
/** How much of the glow around bright things is added to the picture. */
const GLOW = 0.6;
/** The floor's grid: a line every this many units, a stronger one every fifth. */
const GRID = 10;
/** The colour of nothing: the floor far away and the sky. `#05070c`. */
const DARK = 'vec3(0.0196, 0.0275, 0.0471)';

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
  uniform float uStream; // 1 while the program runs and its wires are being computed
  uniform float uReplay; // the logic depth the playback of a step has reached, or -1

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

  // What is known of a wire.
  struct WireState {
    float level;  // 0 or 1, now
    float before; // its level before the last step
    float busy;   // how much it has been changing, 0 to 1
    float light;  // its level over the last few states, 0 to 1
    float depth;  // logic depth of the gate driving it
    float broken; // 1 if that gate is broken
  };
  // The texel of a wire: r holds the level in bit 0 and the level before
  // the last step in bit 1; g the activity; b the level over time; a the
  // logic depth of its driver, plus 128 if the driver is broken.
  WireState wireState(float wire) {
    int i = int(wire + 0.5);
    vec4 t = floor(texelFetch(uWires, ivec2(i % ${STATE_WIDTH}, i / ${STATE_WIDTH}), 0) * 255.0 + 0.5);
    WireState w;
    w.level = mod(t.r, 2.0);
    w.before = floor(t.r * 0.5);
    w.busy = t.g / 255.0;
    w.light = t.b / 255.0;
    w.broken = step(127.5, t.a);
    w.depth = t.a - 128.0 * w.broken;
    return w;
  }
  // The level to draw for something that takes the wire's new level \`lag\`
  // gate delays after the turn of the gate driving it: while a step is
  // being played back and that moment has not come, the level before it.
  float levelShown(WireState w, float lag) {
    return uReplay >= 0.0 && uReplay - w.depth < lag ? w.before : w.level;
  }
  // Somewhere that is never drawn.
  const vec4 NOWHERE = vec4(2.0, 2.0, 2.0, 1.0);
`;

const FRAGMENT_COMMON = /* glsl */ `
  uniform float uLive;
  uniform float uTime;   // seconds, for what moves by itself
  uniform float uStream;
  uniform float uReplay;
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

/** Behind everything: the colour of nothing, over the whole picture. */
const BACKDROP_VERTEX = /* glsl */ `
  void main() {
    gl_Position = vec4(position.xy, 1.0, 1.0);
  }
`;
const BACKDROP_FRAGMENT = /* glsl */ `
  void main() {
    gl_FragColor = vec4(${DARK}, 1.0);
  }
`;

/** The floor beyond the circuit: a faint grid that fades into the dark. */
const GROUND_VERTEX = /* glsl */ `
  ${COMMON}
  uniform vec3 uGround; // the middle of the scene, and how far the floor reaches from it
  varying vec2 vGrid;
  varying vec2 vFromMiddle;
  void main() {
    vFromMiddle = (position.xy - 0.5) * 2.0;
    vec2 world = uGround.xy + vFromMiddle * uGround.z;
    vGrid = world;
    gl_Position = uProj * vec4(uView * vec3((world - uEye.xy) - uEye.zw, -uEyeZ), 1.0);
  }
`;
const GROUND_FRAGMENT = /* glsl */ `
  varying vec2 vGrid;
  varying vec2 vFromMiddle;
  // 1 on the lines of a grid of the given spacing, about a pixel wide; the
  // lines fade away where they would be closer together than a few pixels.
  float lines(float spacing) {
    vec2 toLine = abs(fract(vGrid / spacing - 0.5) - 0.5) * spacing;
    vec2 pixel = max(fwidth(vGrid), vec2(1e-6));
    vec2 px = toLine / pixel;
    float line = 1.0 - smoothstep(0.6, 1.6, min(px.x, px.y));
    return line * smoothstep(6.0, 24.0, spacing / max(pixel.x, pixel.y));
  }
  void main() {
    float grid = 0.45 * lines(${GRID}.0) + 0.55 * lines(${GRID * 5}.0);
    // Gone well before the floor's edge, so that the edge is never seen.
    float fade = 1.0 - smoothstep(0.2, 0.9, length(vFromMiddle));
    gl_FragColor = vec4(${DARK} + vec3(0.034, 0.047, 0.076) * grid * fade, 1.0);
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
  varying float vBroken;
  varying float vBusy;
  varying float vSince;
  void main() {
    vec4 state = moduleState(aGate.z);
    if (state.r * state.g == 0.0) { gl_Position = NOWHERE; return; }
    vec3 body = position * vec3(${GATE_WIDTH}, ${GATE_DEPTH}, ${GATE_TOP});
    vec3 seen = uView * (fromEye(aAt, 0.0) + body);
    gl_Position = uProj * vec4(seen, 1.0);
    vNormal = normal;
    vTop = position.xy * vec2(${GATE_WIDTH}, ${GATE_DEPTH});
    vKind = aGate.x;
    WireState output_ = wireState(aGate.y);
    vLevel = levelShown(output_, 0.0);
    vBroken = output_.broken;
    vBusy = output_.busy;
    // Gate delays since this gate's turn came in the step being played
    // back; negative before its turn, and when nothing is being played.
    vSince = uReplay < 0.0 ? -1.0 : uReplay - output_.depth;
    // An input pin takes its wire's new level when the pulse on the wire
    // arrives: one gate delay after the wire's driver changed.
    vInputs = vec2(levelShown(wireState(aIn.x), 1.0), levelShown(wireState(aIn.y), 1.0));
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
  varying float vBroken;
  varying float vBusy;
  varying float vSince;
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
    float top = step(0.5, vNormal.z);
    if (top > 0.5) {
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
    // Light given off, brighter than the picture's white so that it glows:
    // a gate that has been switching while the program runs, and one whose
    // output has just changed in the step being played back.
    vec3 hot = mix(vec3(1.0, 0.80, 0.45), vec3(0.55, 0.92, 1.0), vKind);
    float flash = vSince >= 0.0 && vBusy > 0.0 ? exp(-vSince * 0.45) : 0.0;
    colour += hot * (0.12 * vBusy * uStream + 0.55 * flash) * mix(0.6, 1.0, top);
    // A broken gate: red, striped across the top, whatever its level.
    if (vBroken > 0.5) {
      float stripe = step(0.5, fract((vTop.x + vTop.y) * 9.0));
      vec3 red = mix(vec3(0.55, 0.05, 0.06), vec3(1.0, 0.18, 0.16), stripe);
      colour = top > 0.5 ? mix(colour, red, 0.8) : red * 0.7;
    }
    gl_FragColor = vec4(stale(colour), 1.0);
  }
`;

/**
 * The gates' lights. A closed block lets the light of the gates inside it
 * through, as points: seen from afar the CPU is a city at night, each light
 * a gate whose output is at 1, brighter the busier it is. As a block opens
 * its points give way to the gates themselves.
 *
 * A gate that changes level many times between two pictures would only
 * flicker, so a light follows its gate's level over the last few states
 * the simulation sent, as a lamp's glow outlasts the current through it.
 */
const LIGHT_VERTEX = /* glsl */ `
  ${COMMON}
  attribute vec4 aAt;
  attribute vec3 aGate; // kind, output wire, module
  varying vec2 vUv;
  varying vec3 vLight;
  void main() {
    vec4 state = moduleState(aGate.z);
    // As much of a point as there is not yet of the gate itself.
    float hidden = 1.0 - state.r * state.g;
    WireState output_ = wireState(aGate.y);
    float light = output_.light * 0.26 + output_.busy * uStream * 0.9;
    // In a step played back, the light of a gate that changed flares when
    // its turn comes: a closed block shows the change passing through it.
    float since = uReplay - output_.depth;
    if (uReplay >= 0.0 && since >= 0.0 && output_.busy > 0.0) light += 1.5 * exp(-since * 0.45);
    if (hidden < 0.02 || light < 0.004) { gl_Position = NOWHERE; return; }
    vec3 middle = fromEye(aAt, ${GATE_TOP});
    // Never smaller than a pixel or so across, or it would shimmer.
    float least = 0.8 * -(uView * middle).z / uFocal;
    float reach = max(0.32, least);
    vUv = position.xy * 2.0 - 1.0;
    gl_Position = uProj * vec4(uView * (middle + vec3(vUv * reach, 0.0)), 1.0);
    // A point drawn larger than its gate is dimmer by as much: far away the
    // lights add up to what the gates give off, not to more.
    float spread = (0.32 * 0.32) / (reach * reach);
    vec3 colour = mix(vec3(1.0, 0.70, 0.28), vec3(0.35, 0.86, 1.0), aGate.x);
    vLight = colour * light * hidden * spread;
  }
`;
const LIGHT_FRAGMENT = /* glsl */ `
  ${FRAGMENT_COMMON}
  varying vec2 vUv;
  varying vec3 vLight;
  void main() {
    float d = dot(vUv, vUv);
    gl_FragColor = vec4(stale(vLight) * (1.0 - smoothstep(0.1, 1.0, d)) * uLive, 1.0);
  }
`;

const WIRE_VERTEX = /* glsl */ `
  ${COMMON}
  attribute vec4 aA;
  attribute vec4 aB;
  attribute vec4 aWire; // height of each end, wire, module
  // How far each end is from the gate that drives the wire: in dashes, and
  // as a share of the way to the farthest gate it feeds. Negative for a
  // wire that no gate here drives.
  attribute vec4 aFlow;
  varying float vNow;
  varying float vBefore;
  varying float vBusy;
  varying float vDepth;
  varying float vCover;
  varying float vAlpha;
  varying vec2 vAlong;
  varying float vPhase;
  varying float vWide;
  void main() {
    vec4 state = moduleState(aWire.w);
    if (state.r * state.g < 0.01) { gl_Position = NOWHERE; return; }
    vec3 a = fromEye(aA, aWire.x);
    vec3 b = fromEye(aB, aWire.y);
    vec3 along = normalize(b - a);
    vec3 here = mix(a, b, position.x);
    // A ribbon facing the eye: across both the wire and the line of sight.
    vec3 across = cross(along, here);
    float size = length(across);
    across = size > 1e-6 ? across / size : vec3(0.0);
    // Never thinner than about a pixel, or it would break up into dots.
    float least = 0.6 * max(-(uView * here).z, 1e-6) / uFocal;
    // Half the wire's true width, in pixels.
    vWide = ${WIRE_HALF} * 0.6 / least;
    // A module's wires appear as its lid fades, and fall back to a faint
    // trace once the camera goes into one of its parts (alpha channel):
    // there the wires inside that part are what matters, and the ones
    // passing over it would hide it. The nearest of them, which pass
    // between the eye and that part and would be wide bars across the
    // picture, go altogether.
    float trace = ${WIRE_FADED} * (1.0 - smoothstep(1.2, 4.5, vWide));
    float alpha = state.r * state.g * mix(1.0, trace, state.a);
    if (alpha < 0.004) { gl_Position = NOWHERE; return; }
    vAlpha = alpha;
    float halfWidth = max(${WIRE_HALF}, least);
    here += across * halfWidth * position.y + along * halfWidth * (position.x * 2.0 - 1.0);
    gl_Position = uProj * vec4(uView * here, 1.0);
    WireState wire = wireState(aWire.z);
    vNow = wire.level;
    vBefore = wire.before;
    vBusy = wire.busy;
    vDepth = wire.depth;
    vCover = clamp(${WIRE_HALF} / least, 0.0, 1.0);
    vAlong = mix(aFlow.xz, aFlow.yw, position.x);
    // So that the dashes of different wires do not march in step.
    vPhase = fract(aWire.z * 0.6180339887);
  }
`;
const WIRE_FRAGMENT = /* glsl */ `
  ${FRAGMENT_COMMON}
  varying float vNow;
  varying float vBefore;
  varying float vBusy;
  varying float vDepth;
  varying float vCover;
  varying float vAlpha;
  varying vec2 vAlong;
  varying float vPhase;
  varying float vWide;
  const vec3 HOT = vec3(1.0, 0.80, 0.46);
  void main() {
    bool driven = vAlong.x >= 0.0;
    // While a step is played back: gate delays since the turn of the gate
    // driving this wire, and how many of them the change needs to get here.
    bool replay = uReplay >= 0.0;
    float since = uReplay - vDepth;
    float lag = driven ? vAlong.y : 0.0;
    bool reached = !replay || since >= lag;

    vec3 colour = mix(vec3(0.11, 0.155, 0.24), vec3(1.0, 0.70, 0.28), reached ? vNow : vBefore);
    // A wire that has been switching is a little brighter all along,
    // whatever its level this instant.
    if (reached) colour += vec3(0.95, 0.50, 0.12) * vBusy * 0.3;
    // One too thin to be seen properly sinks towards the floor instead of sparkling.
    colour = mix(vec3(0.05, 0.07, 0.11), colour, mix(0.45, 1.0, vCover));

    // Light given off, brighter than the picture's white so that it glows.
    float light = 0.0;
    if (driven) {
      // Running: dashes moving away from the driving gate, a bright head
      // with a tail behind it, as bright as the wire has been busy.
      float along = fract(vAlong.x - uTime * ${STREAM_HZ} + vPhase);
      float dash = along * along * along * (1.0 - smoothstep(0.86, 1.0, along));
      light += 1.5 * dash * vBusy * uStream;
      // A step played back: one pulse runs from the driving gate to the
      // farthest gate it feeds during the gate delay after the driver
      // changed, and what it has passed fades over the next two.
      if (replay && since >= 0.0 && vBusy > 0.0) {
        float behind = since - vAlong.y;
        light += 2.0 * step(0.0, behind) * exp(-behind * 1.1) * (1.0 - smoothstep(2.5, 6.0, since));
      }
    } else if (replay && since >= 0.0 && vBusy > 0.0) {
      // Driven from outside: there is no way along it to follow. It lights
      // up whole when its turn comes.
      light += 1.6 * exp(-since * 0.7);
    }
    // A wire seen from close covers many pixels, and so would its light: it
    // is given off per length of wire, not per pixel of picture.
    colour += HOT * light * mix(0.5, 1.0, vCover) * mix(1.0, 0.24, smoothstep(1.5, 9.0, vWide));
    // A wire thinner than the pixel-wide ribbon drawn for it covers a part of it only.
    gl_FragColor = vec4(stale(colour), vAlpha * mix(0.55, 1.0, vCover));
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
    vec3 colour = vec3(0.046, 0.064, 0.104) + vec3(0.008, 0.011, 0.018) * vDepth;
    // A block with a lot going on inside is warm.
    colour += vec3(0.30, 0.15, 0.03) * vBusy * uLive;
    if (vNormal.z > 0.5) {
      // Lighter towards the far edge, as a lid seen at a slant is.
      colour *= 1.0 + 0.22 * (0.5 - vUv.y);
      colour = mix(colour, vec3(0.22, 0.30, 0.44), edgeLine(min(vUv, 1.0 - vUv) * vSize));
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
    // The name across the whole scene would lie over everything in it while
    // its lid is nearly gone: it goes sooner than the lid does.
    if (aStyle.x < 0.5 && aGlyph.w < 0.5) alpha *= alpha * alpha;
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
    // A shadow around each letter, from a smaller and so softer copy of the
    // sheet, keeps a name readable where bright wires pass under it.
    float shadow = smoothstep(0.02, 0.45, texture2D(uGlyphs, vUv, 2.6).r) * 0.8;
    float alpha = max(cover, shadow);
    gl_FragColor = vec4(ink * (cover / max(alpha, 1e-4)), alpha * vAlpha);
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

/** A texture of `count` texels of four bytes, laid out in rows of `STATE_WIDTH`. */
function stateTexture(count: number): { data: Uint8Array; texture: THREE.DataTexture } {
  const rows = Math.max(1, Math.ceil(count / STATE_WIDTH));
  const data = new Uint8Array(STATE_WIDTH * rows * 4);
  const texture = new THREE.DataTexture(data, STATE_WIDTH, rows, THREE.RGBAFormat, THREE.UnsignedByteType);
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

/** How the wires given to {@link SceneView.setWires} came about. */
export interface WiresHow {
  /** They are being computed by the simulation; if not, they are drawn without colour. */
  live: boolean;
  /** The program is running: busy wires carry moving dashes. */
  streaming: boolean;
  /** The indices of the broken gates. */
  broken?: Iterable<number>;
  /**
   * They are the result of a single step from the state shown before: what
   * changed is played back in the order of logic depth.
   */
  step?: boolean;
}

/** Times the graphics card took over a picture, where it can be asked. */
interface GpuTimer {
  ext: { TIME_ELAPSED_EXT: number; GPU_DISJOINT_EXT: number };
  pending: WebGLQuery[];
  times: number[];
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
  /** Per wire: its level over the last few states, 0 to 1; see `LIGHT_FOLLOWS`. */
  private readonly light: Float32Array;
  private readonly values = new Map<number, string>();
  private readonly valueGlyphs: Glyphs;
  private readonly valueGeometry: THREE.InstancedBufferGeometry;
  private valuesChanged = false;
  private readonly diagonal: number;
  private readonly mark: THREE.Mesh;
  private readonly bloom: Bloom | undefined;
  private readonly timer: GpuTimer | undefined;
  /** The program is running and its wires are being computed: dashes move. */
  private streaming = false;
  /** When the step being played back began (as `performance.now()`), or -1. */
  private replayStart = -1;
  /** The logic depth at which the playback begins and the one at which it is over; see `replay.ts`. */
  private replayFrom = 0;
  private replayUntil = 0;
  /** Per gate: its logic depth, as the wire texture holds it. */
  private readonly depthOfGate: Uint8Array;

  /**
   * @param glow draw what gives off light with a glow around it, where the
   *   graphics driver can; without it the scene is drawn as plainly as before
   */
  constructor(
    readonly canvas: HTMLCanvasElement,
    net: NetlistInfo,
    hints: Hints = {},
    glow = true,
  ) {
    this.tree = buildTree(net);
    this.layout = layoutScene(this.tree, hints);
    this.routing = routeWires(net, this.tree, this.layout);
    this.lod = createLod(this.tree);
    this.diagonal = Math.hypot(this.layout.width, this.layout.height);
    const { tree, layout, routing } = this;

    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    if (glow) {
      const bloom = new Bloom(this.renderer);
      if (bloom.supported) this.bloom = bloom;
    }
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    const ext = gl.getExtension('EXT_disjoint_timer_query_webgl2') as GpuTimer['ext'] | null;
    if (ext) this.timer = { ext, pending: [], times: [] };

    this.modules = stateTexture(tree.count);
    this.wires = stateTexture(net.wireCount);
    this.busy = new Uint8Array(tree.count);
    this.light = new Float32Array(net.wireCount);
    this.outputOfGate = new Uint32Array(tree.gateCount);
    // The logic depth of the gate driving each wire never changes: it is
    // written once. What nothing here drives is at depth 0.
    this.depthOfGate = new Uint8Array(tree.gateCount);
    for (let g = 0; g < tree.gateCount; g++) {
      const depth = Math.min(MAX_DEPTH, net.levels[g]!);
      this.wires.data[net.gates[g * GATE_FIELDS + 3]! * 4 + 3] = depth;
      this.depthOfGate[g] = depth;
    }
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
      uTime: { value: 0 },
      uStream: { value: 0 },
      uReplay: { value: -1 },
      // The floor reaches well past the circuit on every side.
      uGround: { value: new THREE.Vector3(layout.width / 2, layout.height / 2, 1.3 * this.diagonal) },
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
    const flat: THREE.ShaderMaterialParameters = { depthTest: false, depthWrite: false, side: THREE.DoubleSide };

    // ── nothing, and the floor beyond the circuit ──
    {
      // One triangle that covers the picture.
      const everything = new THREE.BufferGeometry();
      everything.setAttribute('position', new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
      add(everything, material(BACKDROP_VERTEX, BACKDROP_FRAGMENT, flat), -2);
      add(unitSquare(), material(GROUND_VERTEX, GROUND_FRAGMENT, flat), -1);
    }

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
      add(geometry, material(PLATE_VERTEX, PLATE_FRAGMENT, flat), 0);
    }

    // ── gates: a small block each, and a light each for when it is under a lid ──
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
      // The lights are drawn after the lids and add to them: they shine
      // through. A light is on a lid's far side from the eye by a few units
      // at most, so not testing depth puts it a pixel out at the very worst.
      const lights = instanced(unitSquare(), tree.gateCount, { aAt: [at, 4], aGate: [gate, 3] });
      add(lights, material(LIGHT_VERTEX, LIGHT_FRAGMENT, { ...flat, transparent: true, blending: THREE.AdditiveBlending }), 3.2);
    }

    // ── wires: a ribbon per segment ──
    {
      const a = new Float32Array(routing.count * 4);
      const b = new Float32Array(routing.count * 4);
      const wire = new Float32Array(routing.count * 4);
      const flow = new Float32Array(routing.count * 4);
      for (let s = 0; s < routing.count; s++) {
        const e = routing.ends.subarray(s * 6, s * 6 + 6);
        splitPoint(a, s * 4, e[0]!, e[1]!);
        splitPoint(b, s * 4, e[3]!, e[4]!);
        const module = routing.module[s]!;
        wire.set([e[2]!, e[5]!, routing.wire[s]!, module], s * 4);
        const reach = routing.reach[s]!;
        if (reach <= 0) {
          flow.set([-1, -1, -1, -1], s * 4);
        } else {
          // Dashes of a size that suits the module the wire belongs to:
          // long between the blocks of the CPU, short between two gates.
          const size = Math.sqrt(layout.rects[module * 4 + 2]! * layout.rects[module * 4 + 3]!);
          const dash = Math.min(DASH_MAX, Math.max(DASH_MIN, DASH_OF_MODULE * size));
          const [s0, s1] = [routing.along[s * 2]!, routing.along[s * 2 + 1]!];
          flow.set([s0 / dash, s1 / dash, s0 / reach, s1 / reach], s * 4);
        }
      }
      // x runs along the wire from 0 to 1, y across it from -1 to 1.
      const ribbon = new THREE.BufferGeometry();
      ribbon.setAttribute('position', new THREE.Float32BufferAttribute([0, -1, 0, 1, -1, 0, 0, 1, 0, 1, 1, 0], 3));
      ribbon.setIndex([0, 1, 2, 2, 1, 3]);
      const geometry = instanced(ribbon, routing.count, { aA: [a, 4], aB: [b, 4], aWire: [wire, 4], aFlow: [flow, 4] });
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
    this.mark = add(unitSquare(), material(MARK_VERTEX, MARK_FRAGMENT, { ...flat, transparent: true }), 6);
    this.mark.visible = false;
  }

  /** Is what gives off light drawn with a glow around it? */
  get glows(): boolean {
    return this.bloom !== undefined;
  }

  /** Is something in the picture moving by itself, so that it must be drawn again? */
  get animating(): boolean {
    return this.streaming || this.replayStart >= 0;
  }

  /** Is a step being played back? */
  get replaying(): boolean {
    return this.replayStart >= 0;
  }

  /**
   * What the graphics card took over a picture, in milliseconds: the
   * median of the last sixty it could be asked about. `undefined` where the
   * browser does not let it be asked, or before the first answer.
   */
  get gpuMs(): number | undefined {
    const times = this.timer?.times;
    if (!times || times.length === 0) return undefined;
    return [...times].sort((a, b) => a - b)[Math.floor(times.length / 2)];
  }

  /** Follow the size of the canvas on the page. Returns whether there is anything to draw on. */
  resize(): boolean {
    const { clientWidth: width, clientHeight: height } = this.canvas;
    if (width === 0 || height === 0) return false;
    if (width !== this.viewport.width || height !== this.viewport.height) {
      this.viewport = { width, height };
      this.renderer.setSize(width, height, false);
      const pixels = this.renderer.getDrawingBufferSize(new THREE.Vector2());
      this.bloom?.setSize(pixels.x, pixels.y);
    }
    return true;
  }

  /**
   * The wires as the simulation last saw them: a level and an activity for
   * each, and how they came about.
   *
   * A state in which nothing happened and nothing changed leaves the wires
   * as they are drawn: what the last thing that happened did stays on view
   * (the wires a step changed, the step being played back) until something
   * else happens.
   */
  setWires(levels: Uint8Array, activity: Uint8Array, how: WiresHow): void {
    const { data } = this.wires;
    const count = levels.length;
    this.uniforms.uLive!.value = how.live ? 1 : 0;
    this.streaming = how.streaming && how.live;
    this.uniforms.uStream!.value = this.streaming ? 1 : 0;

    // The broken gates' output wires are marked.
    for (let w = 0; w < count; w++) data[w * 4 + 3]! &= MAX_DEPTH;
    for (const gate of how.broken ?? []) data[this.outputOfGate[gate]! * 4 + 3]! |= 128;

    // The lights of far-off gates follow their levels a part of the way.
    if (how.live) {
      const { light } = this;
      for (let w = 0; w < count; w++) {
        light[w]! += (levels[w]! - light[w]!) * LIGHT_FOLLOWS;
        data[w * 4 + 2] = Math.round(light[w]! * 255);
      }
    }
    this.wires.texture.needsUpdate = true;

    let quiet = true;
    for (let w = 0; w < count && quiet; w++) quiet = activity[w] === 0 && (data[w * 4]! & 1) === levels[w];
    if (quiet) return;

    // A step is played back from the levels there were before it.
    for (let w = 0; w < count; w++) {
      const before = how.step ? data[w * 4]! & 1 : levels[w]!;
      data[w * 4] = levels[w]! | (before << 1);
      data[w * 4 + 1] = activity[w]!;
    }
    this.replayStart = how.step ? performance.now() : -1;
    if (how.step) {
      // From where the camera is: see `replay.ts`.
      const focus = focusModule(this.tree, this.layout, this.lod, this.camera.x, this.camera.y);
      const plan = planReplay(this.tree, this.depthOfGate, this.outputOfGate, activity, focus);
      this.replayFrom = plan.from;
      this.replayUntil = plan.until;
    }

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

  /**
   * Work out the levels of detail for the camera as it is, and draw.
   *
   * @param now the time of this picture, as `performance.now()` gives it
   */
  render(now: number = performance.now()): void {
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
    // looked at, to beyond the far edge of the floor.
    const near = camera.distance * 0.05;
    const far = camera.distance + 2.4 * this.diagonal;
    (u.uProj!.value as THREE.Matrix4).fromArray(projection(viewport.width / viewport.height, near, far));
    u.uFocal!.value = focal;

    u.uTime!.value = (now % CLOCK_WRAP_MS) / 1000;
    if (this.replayStart >= 0) {
      // (A picture's time can be a moment before the step arrived.)
      const delays = this.replayFrom + Math.max(0, now - this.replayStart) / 1000 / GATE_DELAY_SECONDS;
      if (delays > this.replayUntil) this.replayStart = -1;
      u.uReplay!.value = this.replayStart >= 0 ? delays : -1;
    } else {
      u.uReplay!.value = -1;
    }

    const timed = this.beginTiming();
    if (this.bloom) {
      this.renderer.setRenderTarget(this.bloom.scene);
      this.renderer.render(this.scene, this.eye);
      this.bloom.render(GLOW);
    } else {
      this.renderer.render(this.scene, this.eye);
    }
    if (timed) this.endTiming();
  }

  /** Start asking the graphics card how long this picture takes it, if it can be asked now. */
  private beginTiming(): boolean {
    const { timer } = this;
    if (!timer) return false;
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    // Answers come a few pictures later.
    while (timer.pending.length > 0) {
      const query = timer.pending[0]!;
      if (!gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE)) break;
      // An answer is void if the card was interrupted while it measured.
      if (!gl.getParameter(timer.ext.GPU_DISJOINT_EXT)) {
        timer.times.push((gl.getQueryParameter(query, gl.QUERY_RESULT) as number) / 1e6);
        if (timer.times.length > 60) timer.times.shift();
      }
      gl.deleteQuery(query);
      timer.pending.shift();
    }
    if (timer.pending.length >= 4) return false;
    const query = gl.createQuery();
    if (!query) return false;
    gl.beginQuery(timer.ext.TIME_ELAPSED_EXT, query);
    timer.pending.push(query);
    return true;
  }

  private endTiming(): void {
    const gl = this.renderer.getContext() as WebGL2RenderingContext;
    gl.endQuery(this.timer!.ext.TIME_ELAPSED_EXT);
  }
}
