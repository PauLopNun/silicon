/**
 * Glow. The scene is drawn into a picture that can hold light brighter than
 * white; whatever is brighter than white there is light being given off (a
 * wire switching, a pulse on its way), and that excess is blurred into a
 * halo and added back.
 *
 * Nothing is told apart by colour or by what kind of thing it is: only what
 * a shader made brighter than 1 glows. White letters do not; a busy wire
 * does.
 *
 * The blur is a pyramid: the excess is halved in size six times, each time
 * with a small filter, and added back on the way up, so that a wide halo
 * costs about as much as a narrow one (the "dual filtering" of Marius
 * Bjørge, "Bandwidth-Efficient Rendering", SIGGRAPH 2015).
 *
 * Needs pictures of 16-bit floats to draw into. Where the graphics driver
 * has none, `supported` is false and the scene is drawn straight to the
 * screen, without glow, exactly as before.
 */
import * as THREE from 'three';

/** Levels of the pyramid: the widest halo is about 2^LEVELS pixels across. */
const LEVELS = 6;

const VERTEX = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = position.xy * 0.5 + 0.5;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

/** Half the size: keep what is brighter than white, in its own hue. */
const EXCESS = /* glsl */ `
  uniform sampler2D uSource;
  uniform vec2 uTexel; // one texel of the source
  varying vec2 vUv;
  vec3 excess(vec2 uv, out float weight) {
    vec3 c = texture2D(uSource, uv).rgb;
    float brightest = max(c.r, max(c.g, c.b));
    c *= max(brightest - 1.0, 0.0) / max(brightest, 1e-4);
    // One very bright pixel among dark ones (a wire a pixel wide) would
    // flicker as it moves across the four samples: weigh the bright less.
    weight = 1.0 / (1.0 + max(c.r, max(c.g, c.b)));
    return c * weight;
  }
  void main() {
    float w0, w1, w2, w3;
    vec3 sum = excess(vUv + uTexel * vec2(-0.5, -0.5), w0) + excess(vUv + uTexel * vec2(0.5, -0.5), w1)
             + excess(vUv + uTexel * vec2(-0.5, 0.5), w2) + excess(vUv + uTexel * vec2(0.5, 0.5), w3);
    gl_FragColor = vec4(sum / (w0 + w1 + w2 + w3), 1.0);
  }
`;

/** Half the size again: the middle and the four corners. */
const DOWN = /* glsl */ `
  uniform sampler2D uSource;
  uniform vec2 uTexel;
  varying vec2 vUv;
  void main() {
    vec3 sum = texture2D(uSource, vUv).rgb * 4.0;
    sum += texture2D(uSource, vUv - uTexel).rgb;
    sum += texture2D(uSource, vUv + uTexel).rgb;
    sum += texture2D(uSource, vUv + vec2(uTexel.x, -uTexel.y)).rgb;
    sum += texture2D(uSource, vUv - vec2(uTexel.x, -uTexel.y)).rgb;
    gl_FragColor = vec4(sum / 8.0, 1.0);
  }
`;

/** Twice the size: eight samples around, added to what is already there. */
const UP = /* glsl */ `
  uniform sampler2D uSource;
  uniform vec2 uTexel;
  uniform float uGain;
  varying vec2 vUv;
  void main() {
    vec3 sum = texture2D(uSource, vUv + vec2(-uTexel.x * 2.0, 0.0)).rgb;
    sum += texture2D(uSource, vUv + vec2(-uTexel.x, uTexel.y)).rgb * 2.0;
    sum += texture2D(uSource, vUv + vec2(0.0, uTexel.y * 2.0)).rgb;
    sum += texture2D(uSource, vUv + vec2(uTexel.x, uTexel.y)).rgb * 2.0;
    sum += texture2D(uSource, vUv + vec2(uTexel.x * 2.0, 0.0)).rgb;
    sum += texture2D(uSource, vUv + vec2(uTexel.x, -uTexel.y)).rgb * 2.0;
    sum += texture2D(uSource, vUv + vec2(0.0, -uTexel.y * 2.0)).rgb;
    sum += texture2D(uSource, vUv + vec2(-uTexel.x, -uTexel.y)).rgb * 2.0;
    gl_FragColor = vec4(sum / 12.0 * uGain, 1.0);
  }
`;

/** The picture and its halo, on the screen. */
const COMPOSE = /* glsl */ `
  uniform sampler2D uScene;
  uniform sampler2D uHalo;
  uniform float uStrength;
  varying vec2 vUv;
  void main() {
    // What the picture can show as it is, and the light it cannot as a halo.
    // Something brighter than white is brought back to what can be shown
    // mostly by dimming it, which keeps its hue (amber stays amber), and a
    // little by clipping it, which whitens its core as heat does.
    vec3 scene = texture2D(uScene, vUv).rgb;
    vec3 dimmed = scene / max(1.0, max(scene.r, max(scene.g, scene.b)));
    vec3 colour = mix(dimmed, min(scene, 1.0), 0.3) + texture2D(uHalo, vUv).rgb * uStrength;
    // The corners a little darker, which draws the eye to the middle.
    vec2 fromMiddle = vUv - 0.5;
    colour *= 1.0 - 0.38 * smoothstep(0.25, 0.85, dot(fromMiddle, fromMiddle) * 2.0);
    // Dark, smooth gradients show steps in eight bits: a grain of noise
    // smaller than one step hides them.
    float grain = fract(sin(dot(gl_FragCoord.xy, vec2(12.9898, 78.233))) * 43758.5453);
    gl_FragColor = vec4(colour + (grain - 0.5) / 255.0, 1.0);
  }
`;

export class Bloom {
  /** Can the graphics driver draw into pictures of floats at all? */
  readonly supported: boolean;
  /** Draw the scene into this. */
  readonly scene: THREE.WebGLRenderTarget;

  private readonly pyramid: THREE.WebGLRenderTarget[] = [];
  private readonly stage = new THREE.Scene();
  private readonly eye = new THREE.Camera();
  private readonly quad: THREE.Mesh;
  private readonly excess: THREE.ShaderMaterial;
  private readonly down: THREE.ShaderMaterial;
  private readonly up: THREE.ShaderMaterial;
  private readonly compose: THREE.ShaderMaterial;

  constructor(private readonly renderer: THREE.WebGLRenderer) {
    this.supported =
      renderer.extensions.has('EXT_color_buffer_float') || renderer.extensions.has('EXT_color_buffer_half_float');
    const floats = { type: THREE.HalfFloatType, format: THREE.RGBAFormat, minFilter: THREE.LinearFilter, magFilter: THREE.LinearFilter };
    // Four samples a pixel, as the screen itself would have.
    this.scene = new THREE.WebGLRenderTarget(1, 1, { ...floats, samples: 4, depthBuffer: true });
    for (let i = 0; i < LEVELS; i++) this.pyramid.push(new THREE.WebGLRenderTarget(1, 1, { ...floats, depthBuffer: false }));

    const pass = (fragmentShader: string, uniforms: Record<string, THREE.IUniform>, adding = false) =>
      new THREE.ShaderMaterial({
        vertexShader: VERTEX,
        fragmentShader,
        uniforms,
        depthTest: false,
        depthWrite: false,
        blending: adding ? THREE.CustomBlending : THREE.NoBlending,
        blendEquation: THREE.AddEquation,
        blendSrc: THREE.OneFactor,
        blendDst: THREE.OneFactor,
      });
    const source = () => ({ uSource: { value: null }, uTexel: { value: new THREE.Vector2() } });
    this.excess = pass(EXCESS, source());
    this.down = pass(DOWN, source());
    this.up = pass(UP, { ...source(), uGain: { value: 1 } }, true);
    this.compose = pass(COMPOSE, { uScene: { value: null }, uHalo: { value: null }, uStrength: { value: 1 } });

    // One triangle that covers the picture.
    const triangle = new THREE.BufferGeometry();
    triangle.setAttribute('position', new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
    this.quad = new THREE.Mesh(triangle, this.excess);
    this.quad.frustumCulled = false;
    this.stage.add(this.quad);
  }

  /** Follow the size of the picture, in device pixels. */
  setSize(width: number, height: number): void {
    this.scene.setSize(width, height);
    let [w, h] = [width, height];
    for (const level of this.pyramid) {
      w = Math.max(1, Math.round(w / 2));
      h = Math.max(1, Math.round(h / 2));
      level.setSize(w, h);
    }
  }

  private draw(material: THREE.ShaderMaterial, source: THREE.WebGLRenderTarget, target: THREE.WebGLRenderTarget | null): void {
    material.uniforms.uSource!.value = source.texture;
    (material.uniforms.uTexel!.value as THREE.Vector2).set(1 / source.width, 1 / source.height);
    this.quad.material = material;
    this.renderer.setRenderTarget(target);
    this.renderer.render(this.stage, this.eye);
  }

  /**
   * The scene has been drawn into `scene`: put it on the screen with its
   * halo. `strength` is how much of the halo is added.
   */
  render(strength: number): void {
    const { renderer, pyramid } = this;
    const clearing = renderer.autoClear;
    // Down: the excess of the scene, then each level half the one before.
    this.draw(this.excess, this.scene, pyramid[0]!);
    for (let i = 1; i < LEVELS; i++) this.draw(this.down, pyramid[i - 1]!, pyramid[i]!);
    // Up: each level added onto the one twice its size, which keeps both the
    // tight glow of the fine levels and the wide one of the coarse.
    renderer.autoClear = false;
    for (let i = LEVELS - 1; i > 0; i--) this.draw(this.up, pyramid[i]!, pyramid[i - 1]!);
    renderer.autoClear = clearing;

    this.compose.uniforms.uScene!.value = this.scene.texture;
    this.compose.uniforms.uHalo!.value = pyramid[0]!.texture;
    this.compose.uniforms.uStrength!.value = strength;
    this.quad.material = this.compose;
    renderer.setRenderTarget(null);
    renderer.render(this.stage, this.eye);
  }
}
