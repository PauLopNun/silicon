/**
 * The camera, as plain numbers and pure functions: where the eye is, which
 * way it looks, where a pixel's ray meets the floor, and how to move.
 *
 * Scene space is the plan of `layout.ts` with height added: x to the right,
 * y down the plan (south), z up from the floor. That makes it a left-handed
 * system; nothing here uses a cross product, and the renderer accounts for
 * the mirrored winding where it culls faces.
 *
 * The camera looks at a point of the floor from a distance. It never goes
 * below the floor, so a lid is always seen from above.
 */
import type { Rect } from './layout';

export interface Camera {
  /** The point of the floor in the middle of the view. */
  x: number;
  y: number;
  /** From that point to the eye. */
  distance: number;
  /** Turn about the vertical, in radians; at 0 the top of the view is north. */
  azimuth: number;
  /** Angle of the eye above the floor, in radians; π/2 looks straight down. */
  elevation: number;
}

export type Vec3 = readonly [number, number, number];

/** Vertical field of view, in radians. */
export const FOV = (35 * Math.PI) / 180;
const TAN_HALF_FOV = Math.tan(FOV / 2);
/** The eye stays at least this far above the horizon. */
export const MIN_ELEVATION = (12 * Math.PI) / 180;
export const MAX_ELEVATION = Math.PI / 2;

/** The size of the picture, in CSS pixels. */
export interface Viewport {
  width: number;
  height: number;
}

/** Pixels that one unit covers at a distance of one unit. */
export function focalLength(viewport: Viewport): number {
  return viewport.height / (2 * TAN_HALF_FOV);
}

export function eyeOf(c: Camera): Vec3 {
  const flat = c.distance * Math.cos(c.elevation);
  return [c.x + flat * Math.sin(c.azimuth), c.y + flat * Math.cos(c.azimuth), c.distance * Math.sin(c.elevation)];
}

/** The directions of the picture's right and top, and the direction of view, as unit vectors. */
export function basisOf(c: Camera): { right: Vec3; up: Vec3; forward: Vec3 } {
  const [sa, ca] = [Math.sin(c.azimuth), Math.cos(c.azimuth)];
  const [se, ce] = [Math.sin(c.elevation), Math.cos(c.elevation)];
  return {
    right: [ca, -sa, 0],
    up: [-se * sa, -se * ca, ce],
    forward: [-ce * sa, -ce * ca, -se],
  };
}

/**
 * The rotation from scene space to the camera's (x right, y up, z towards
 * the eye), as the nine numbers of a GLSL `mat3`, column by column.
 */
export function viewRotation(c: Camera): Float32Array {
  const { right, up, forward } = basisOf(c);
  // prettier-ignore
  return Float32Array.of(
    right[0], up[0], -forward[0],
    right[1], up[1], -forward[1],
    right[2], up[2], -forward[2],
  );
}

/** A perspective projection as the sixteen numbers of a GLSL `mat4`, column by column. */
export function projection(aspect: number, near: number, far: number): Float32Array {
  const f = 1 / TAN_HALF_FOV;
  // prettier-ignore
  return Float32Array.of(
    f / aspect, 0, 0, 0,
    0, f, 0, 0,
    0, 0, (far + near) / (near - far), -1,
    0, 0, (2 * far * near) / (near - far), 0,
  );
}

/** Where a point of the scene falls in the picture, in pixels, and how far in front of the eye it is. */
export function project(c: Camera, viewport: Viewport, point: Vec3): { x: number; y: number; depth: number } {
  const eye = eyeOf(c);
  const { right, up, forward } = basisOf(c);
  const rel = [point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]] as const;
  const dot = (v: Vec3) => rel[0] * v[0] + rel[1] * v[1] + rel[2] * v[2];
  const depth = dot(forward);
  const focal = focalLength(viewport);
  return {
    x: viewport.width / 2 + (focal * dot(right)) / depth,
    y: viewport.height / 2 - (focal * dot(up)) / depth,
    depth,
  };
}

/** The ray from the eye through a pixel. The direction is not normalised: its part along the view is 1. */
export function rayThrough(c: Camera, viewport: Viewport, px: number, py: number): { origin: Vec3; direction: Vec3 } {
  const { right, up, forward } = basisOf(c);
  const focal = focalLength(viewport);
  const a = (px - viewport.width / 2) / focal;
  const b = (viewport.height / 2 - py) / focal;
  return {
    origin: eyeOf(c),
    direction: [
      forward[0] + a * right[0] + b * up[0],
      forward[1] + a * right[1] + b * up[1],
      forward[2] + a * right[2] + b * up[2],
    ],
  };
}

/** Where a pixel's ray meets the plane at height `z`, or `undefined` if it looks above the horizon. */
export function groundUnder(
  c: Camera,
  viewport: Viewport,
  px: number,
  py: number,
  z = 0,
): { x: number; y: number } | undefined {
  const { origin, direction } = rayThrough(c, viewport, px, py);
  if (direction[2] >= -1e-9) return undefined;
  const t = (z - origin[2]) / direction[2];
  return { x: origin[0] + t * direction[0], y: origin[1] + t * direction[1] };
}

/** How far the camera may go. */
export interface Limits {
  /** The floor point stays inside this rectangle. */
  bounds: Rect;
  minDistance: number;
  maxDistance: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export function clampCamera(c: Camera, limits: Limits): Camera {
  const { bounds } = limits;
  return {
    x: clamp(c.x, bounds.x, bounds.x + bounds.w),
    y: clamp(c.y, bounds.y, bounds.y + bounds.h),
    distance: clamp(c.distance, limits.minDistance, limits.maxDistance),
    azimuth: c.azimuth,
    elevation: clamp(c.elevation, MIN_ELEVATION, MAX_ELEVATION),
  };
}

/**
 * Move towards (`factor` < 1) or away from a point of the floor. The point
 * stays where it is in the picture: everything scales about it.
 */
export function zoomAbout(c: Camera, x: number, y: number, factor: number): Camera {
  return {
    ...c,
    x: x + (c.x - x) * factor,
    y: y + (c.y - y) * factor,
    distance: c.distance * factor,
  };
}

/** Turn about the point being looked at. */
export function orbit(c: Camera, azimuth: number, elevation: number): Camera {
  return {
    ...c,
    azimuth: c.azimuth + azimuth,
    elevation: clamp(c.elevation + elevation, MIN_ELEVATION, MAX_ELEVATION),
  };
}

/**
 * The camera that shows the whole of `rect`, keeping the angles of `c`:
 * the closest one that has the rectangle's four corners inside the picture
 * with `margin` (a fraction of the picture) to spare on every side.
 */
export function fit(rect: Rect, viewport: Viewport, c: Pick<Camera, 'azimuth' | 'elevation'>, margin = 0.08): Camera {
  const centred = { ...c, x: rect.x + rect.w / 2, y: rect.y + rect.h / 2 };
  const corners: Vec3[] = [
    [rect.x, rect.y, 0],
    [rect.x + rect.w, rect.y, 0],
    [rect.x, rect.y + rect.h, 0],
    [rect.x + rect.w, rect.y + rect.h, 0],
  ];
  const inside = (distance: number) =>
    corners.every((corner) => {
      const p = project({ ...centred, distance }, viewport, corner);
      return (
        p.depth > 0 &&
        p.x >= margin * viewport.width &&
        p.x <= (1 - margin) * viewport.width &&
        p.y >= margin * viewport.height &&
        p.y <= (1 - margin) * viewport.height
      );
    });
  // Farther always shows more, so the boundary can be bisected.
  const size = Math.max(rect.w, rect.h, 1e-6);
  let near = size * 1e-3;
  let far = size * 1e3;
  for (let i = 0; i < 60; i++) {
    const mid = Math.sqrt(near * far);
    if (inside(mid)) far = mid;
    else near = mid;
  }
  return { ...centred, distance: far };
}

/** The difference `to - from` between two angles, the short way round. */
function turn(from: number, to: number): number {
  const d = (to - from) % (2 * Math.PI);
  return d > Math.PI ? d - 2 * Math.PI : d < -Math.PI ? d + 2 * Math.PI : d;
}

/**
 * One step of easing `current` towards `goal`: after `dt` milliseconds the
 * camera has covered `1 - e^(-dt/tau)` of what was left. Position and
 * distance move in proportion, so a zoom about a point keeps that point
 * still all the way.
 */
export function approach(current: Camera, goal: Camera, dt: number, tau = 70): Camera {
  const k = 1 - Math.exp(-dt / tau);
  const mix = (a: number, b: number) => a + (b - a) * k;
  return {
    x: mix(current.x, goal.x),
    y: mix(current.y, goal.y),
    distance: mix(current.distance, goal.distance),
    azimuth: current.azimuth + turn(current.azimuth, goal.azimuth) * k,
    elevation: mix(current.elevation, goal.elevation),
  };
}

/** Are two cameras close enough that easing between them can stop? */
export function settled(a: Camera, b: Camera): boolean {
  const scale = Math.min(a.distance, b.distance);
  return (
    Math.abs(a.x - b.x) < scale * 1e-4 &&
    Math.abs(a.y - b.y) < scale * 1e-4 &&
    Math.abs(a.distance - b.distance) < scale * 1e-4 &&
    Math.abs(turn(a.azimuth, b.azimuth)) < 1e-4 &&
    Math.abs(a.elevation - b.elevation) < 1e-4
  );
}

/** A journey between two cameras. */
export interface Flight {
  /** How long it should take, in milliseconds. */
  duration: number;
  /** The camera at `t` from 0 (the start) to 1 (the end). */
  at(t: number): Camera;
}

/**
 * The journey from `a` to `b` that looks smooth: it pulls back while it
 * covers ground and closes in at the end, so that what passes through the
 * picture moves at a steady speed. This is the path of van Wijk and Nuij,
 * "Smooth and efficient zooming and panning" (2003), with their ρ = √2.
 */
export function flight(a: Camera, b: Camera): Flight {
  const RHO = Math.SQRT2;
  // The paper measures the view by its width on the floor.
  const [w0, w1] = [a.distance * 2 * TAN_HALF_FOV, b.distance * 2 * TAN_HALF_FOV];
  const [dx, dy] = [b.x - a.x, b.y - a.y];
  const d = Math.hypot(dx, dy);

  let length: number;
  let path: (s: number) => { along: number; width: number };
  if (d < 1e-9 * Math.max(w0, w1)) {
    // Nowhere to go: only the distance changes.
    length = Math.abs(Math.log(w1 / w0)) / RHO;
    const sign = w1 >= w0 ? 1 : -1;
    path = (s) => ({ along: 0, width: w0 * Math.exp(sign * RHO * s) });
  } else {
    const b0 = (w1 * w1 - w0 * w0 + RHO ** 4 * d * d) / (2 * w0 * RHO * RHO * d);
    const b1 = (w1 * w1 - w0 * w0 - RHO ** 4 * d * d) / (2 * w1 * RHO * RHO * d);
    const r0 = Math.log(Math.sqrt(b0 * b0 + 1) - b0);
    const r1 = Math.log(Math.sqrt(b1 * b1 + 1) - b1);
    length = (r1 - r0) / RHO;
    path = (s) => ({
      along: ((w0 / (RHO * RHO)) * (Math.cosh(r0) * Math.tanh(RHO * s + r0) - Math.sinh(r0))) / d,
      width: (w0 * Math.cosh(r0)) / Math.cosh(RHO * s + r0),
    });
  }

  const ease = (t: number) => t * t * (3 - 2 * t);
  return {
    duration: clamp(length * 420, 250, 1400),
    at(t) {
      if (t <= 0) return a;
      if (t >= 1) return b;
      const e = ease(t);
      const { along, width } = path(e * length);
      return {
        x: a.x + dx * along,
        y: a.y + dy * along,
        distance: width / (2 * TAN_HALF_FOV),
        azimuth: a.azimuth + turn(a.azimuth, b.azimuth) * e,
        elevation: a.elevation + (b.elevation - a.elevation) * e,
      };
    },
  };
}
