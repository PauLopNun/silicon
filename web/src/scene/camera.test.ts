import { describe, expect, it } from 'vitest';
import {
  approach,
  basisOf,
  clampCamera,
  eyeOf,
  fit,
  flight,
  focalLength,
  FOV,
  groundUnder,
  MAX_ELEVATION,
  MIN_ELEVATION,
  orbit,
  project,
  projection,
  rayThrough,
  settled,
  viewRotation,
  zoomAbout,
  type Camera,
  type Vec3,
} from './camera';

const viewport = { width: 1200, height: 800 };
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

/** Cameras at all sorts of angles. */
const cameras: Camera[] = [
  { x: 0, y: 0, distance: 100, azimuth: 0, elevation: Math.PI / 2 },
  { x: 40, y: -25, distance: 300, azimuth: 0, elevation: 1 },
  { x: -7, y: 12, distance: 3, azimuth: 2.2, elevation: 0.4 },
  { x: 1e5, y: 2e5, distance: 5, azimuth: -1, elevation: MIN_ELEVATION },
];

describe('the camera', () => {
  it('looks at its point of the floor from above', () => {
    for (const c of cameras) {
      const eye = eyeOf(c);
      expect(eye[2]).toBeGreaterThan(0);
      expect(Math.hypot(eye[0] - c.x, eye[1] - c.y, eye[2])).toBeCloseTo(c.distance, 9);
      const centre = project(c, viewport, [c.x, c.y, 0]);
      expect(centre.x).toBeCloseTo(600, 6);
      expect(centre.y).toBeCloseTo(400, 6);
      expect(centre.depth).toBeCloseTo(c.distance, 9);
    }
  });

  it('has north at the top and east on the right when it is not turned', () => {
    const c = cameras[0]!;
    // y grows southwards: a point with smaller y is further up the picture.
    expect(project(c, viewport, [0, -10, 0]).y).toBeLessThan(400);
    expect(project(c, viewport, [10, 0, 0]).x).toBeGreaterThan(600);
    // Something tall leans away from the middle of the picture: towards the eye.
    const tilted = { ...c, elevation: 1 };
    expect(project(tilted, viewport, [0, 0, 5]).y).toBeLessThan(project(tilted, viewport, [0, 0, 0]).y);
  });

  it('has three directions at right angles, each of length one', () => {
    for (const c of cameras) {
      const { right, up, forward } = basisOf(c);
      for (const v of [right, up, forward]) expect(dot(v, v)).toBeCloseTo(1, 12);
      expect(dot(right, up)).toBeCloseTo(0, 12);
      expect(dot(right, forward)).toBeCloseTo(0, 12);
      expect(dot(up, forward)).toBeCloseTo(0, 12);
      // The matrix for the shaders says the same, column by column.
      const m = viewRotation(c);
      const row = (r: number): Vec3 => [m[r]!, m[r + 3]!, m[r + 6]!];
      for (let k = 0; k < 3; k++) {
        expect(row(0)[k]).toBeCloseTo(right[k]!, 6);
        expect(row(1)[k]).toBeCloseTo(up[k]!, 6);
        expect(row(2)[k]).toBeCloseTo(-forward[k]!, 6);
      }
    }
  });

  it('sees a point where its ray came from', () => {
    for (const c of cameras) {
      for (const [px, py] of [
        [0, 0],
        [600, 400],
        [1100, 700],
        [333, 780],
      ] as const) {
        const ground = groundUnder(c, viewport, px, py);
        if (!ground) continue;
        const back = project(c, viewport, [ground.x, ground.y, 0]);
        expect(back.x).toBeCloseTo(px, 4);
        expect(back.y).toBeCloseTo(py, 4);
        // And the same for a plane above the floor.
        const lid = groundUnder(c, viewport, px, py, 1.5)!;
        expect(project(c, viewport, [lid.x, lid.y, 1.5]).x).toBeCloseTo(px, 4);
      }
    }
  });

  it('sees no floor above the horizon', () => {
    const low = { x: 0, y: 0, distance: 10, azimuth: 0, elevation: MIN_ELEVATION };
    expect(groundUnder(low, viewport, 600, 0)).toBeUndefined();
    expect(groundUnder(low, viewport, 600, 799)).toBeDefined();
    expect(rayThrough(low, viewport, 600, 0).direction[2]).toBeGreaterThan(0);
  });

  it('measures the picture: a unit at a distance of one unit covers the focal length', () => {
    expect(focalLength(viewport)).toBeCloseTo(400 / Math.tan(FOV / 2), 9);
    const c = cameras[0]!;
    const a = project(c, viewport, [0, 0, 0]);
    const b = project(c, viewport, [1, 0, 0]);
    expect((b.x - a.x) * c.distance).toBeCloseTo(focalLength(viewport), 6);
  });
});

describe('projection', () => {
  it('maps the near plane to -1 and the far plane to 1', () => {
    const m = projection(1.5, 2, 50);
    const depth = (z: number) => (m[10]! * z + m[14]!) / (m[11]! * z);
    expect(depth(-2)).toBeCloseTo(-1, 6);
    expect(depth(-50)).toBeCloseTo(1, 5);
    // A point at the edge of the field of view lands on the edge of the picture.
    const y = Math.tan(FOV / 2) * 10;
    expect((m[5]! * y) / 10).toBeCloseTo(1, 6);
    expect((m[0]! * y * 1.5) / 10).toBeCloseTo(1, 6);
  });
});

describe('moving', () => {
  it('zooms about a point of the floor, which stays where it is in the picture', () => {
    for (const c of cameras) {
      const point = groundUnder(c, viewport, 900, 650);
      if (!point) continue;
      for (const factor of [0.5, 0.9, 1.3, 4]) {
        const zoomed = zoomAbout(c, point.x, point.y, factor);
        expect(zoomed.distance).toBeCloseTo(c.distance * factor, 9);
        const after = project(zoomed, viewport, [point.x, point.y, 0]);
        expect(after.x).toBeCloseTo(900, 4);
        expect(after.y).toBeCloseTo(650, 4);
      }
    }
  });

  it('keeps that point still all the way while it eases into a zoom', () => {
    const start = cameras[1]!;
    const point = groundUnder(start, viewport, 200, 150)!;
    const goal = zoomAbout(start, point.x, point.y, 0.2);
    let c = start;
    for (let i = 0; i < 200 && !settled(c, goal); i++) {
      c = approach(c, goal, 16);
      const p = project(c, viewport, [point.x, point.y, 0]);
      expect(p.x).toBeCloseTo(200, 3);
      expect(p.y).toBeCloseTo(150, 3);
    }
    expect(settled(c, goal)).toBe(true);
    expect(c.distance).toBeCloseTo(goal.distance, 1);
  });

  it('eases the short way round when it turns', () => {
    const from = { ...cameras[0]!, azimuth: 3 };
    const to = { ...cameras[0]!, azimuth: -3 };
    // From 3 to -3 radians is a quarter of a radian onwards, not six back.
    expect(approach(from, to, 16).azimuth).toBeGreaterThan(3);
    expect(settled(from, { ...from, azimuth: 3 + 2 * Math.PI })).toBe(true);
  });

  it('turns about its point without going under the floor', () => {
    const c = cameras[1]!;
    const turned = orbit(c, 0.5, -5);
    expect([turned.x, turned.y, turned.distance]).toEqual([c.x, c.y, c.distance]);
    expect(turned.azimuth).toBeCloseTo(0.5, 12);
    expect(turned.elevation).toBe(MIN_ELEVATION);
    expect(orbit(c, 0, 5).elevation).toBe(MAX_ELEVATION);
  });

  it('stays within its limits', () => {
    const limits = { bounds: { x: 0, y: 0, w: 100, h: 50 }, minDistance: 2, maxDistance: 500 };
    expect(clampCamera({ x: -5, y: 70, distance: 1, azimuth: 1, elevation: 0 }, limits)).toEqual({
      x: 0,
      y: 50,
      distance: 2,
      azimuth: 1,
      elevation: MIN_ELEVATION,
    });
    const inside = { x: 10, y: 10, distance: 20, azimuth: 0, elevation: 1 };
    expect(clampCamera(inside, limits)).toEqual(inside);
    expect(clampCamera({ ...inside, distance: 1e9 }, limits).distance).toBe(500);
  });
});

describe('fit', () => {
  const rect = { x: 100, y: 40, w: 60, h: 20 };
  const corners: Vec3[] = [
    [100, 40, 0],
    [160, 40, 0],
    [100, 60, 0],
    [160, 60, 0],
  ];

  it('shows the whole rectangle, as large as the margin allows', () => {
    for (const angles of [
      { azimuth: 0, elevation: Math.PI / 2 },
      { azimuth: 0, elevation: 1 },
      { azimuth: 0.7, elevation: 0.5 },
      { azimuth: 3, elevation: MIN_ELEVATION },
    ]) {
      for (const view of [viewport, { width: 390, height: 700 }]) {
        const c = fit(rect, view, angles, 0.1);
        expect([c.x, c.y]).toEqual([130, 50]);
        const points = corners.map((p) => project(c, view, p));
        const reach = Math.max(
          ...points.map((p) => Math.abs(p.x - view.width / 2) / (view.width / 2)),
          ...points.map((p) => Math.abs(p.y - view.height / 2) / (view.height / 2)),
        );
        // The corner that reaches furthest is right at the margin.
        expect(reach).toBeLessThanOrEqual(0.8 + 1e-6);
        expect(reach).toBeGreaterThan(0.8 - 1e-3);
      }
    }
  });
});

describe('flight', () => {
  const a: Camera = { x: 0, y: 0, distance: 4, azimuth: 0, elevation: 1 };
  const b: Camera = { x: 5000, y: -3000, distance: 6, azimuth: 1, elevation: 0.6 };

  it('starts at one camera and ends at the other', () => {
    const f = flight(a, b);
    expect(f.at(0)).toEqual(a);
    expect(f.at(1)).toEqual(b);
    const nearStart = f.at(1e-6);
    const nearEnd = f.at(1 - 1e-6);
    expect(Math.hypot(nearStart.x - a.x, nearStart.y - a.y)).toBeLessThan(1e-3);
    expect(nearStart.distance).toBeCloseTo(a.distance, 3);
    expect(Math.hypot(nearEnd.x - b.x, nearEnd.y - b.y)).toBeLessThan(1e-2);
    expect(nearEnd.distance).toBeCloseTo(b.distance, 3);
  });

  it('pulls back to cross a long way, and takes a moment over it', () => {
    const f = flight(a, b);
    const middle = f.at(0.5);
    expect(middle.distance).toBeGreaterThan(500);
    expect(f.duration).toBeGreaterThan(600);
    expect(f.duration).toBeLessThanOrEqual(1400);
    // A hop next door is quick and stays low.
    const hop = flight(a, { ...a, x: 1 });
    expect(hop.duration).toBeLessThan(600);
    expect(hop.at(0.5).distance).toBeLessThan(8);
  });

  it('never jumps: what is in view moves a little at each step', () => {
    const f = flight(a, b);
    let previous = f.at(0);
    for (let i = 1; i <= 400; i++) {
      const c = f.at(i / 400);
      // Measured against the size of the view at that moment.
      const moved = Math.hypot(c.x - previous.x, c.y - previous.y) / Math.max(c.distance, previous.distance);
      const zoomed = Math.abs(Math.log(c.distance / previous.distance));
      expect(moved).toBeLessThan(0.2);
      expect(zoomed).toBeLessThan(0.2);
      previous = c;
    }
  });

  it('only changes the distance when there is nowhere to go', () => {
    const f = flight(a, { ...a, distance: 400 });
    const middle = f.at(0.5);
    expect([middle.x, middle.y]).toEqual([0, 0]);
    expect(middle.distance).toBeCloseTo(40, 6);
    expect(flight(a, a).at(0.5).distance).toBeCloseTo(4, 9);
  });
});
