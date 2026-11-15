/**
 * Moving about the scene: the pointer, the wheel, fingers and a few keys
 * turned into movements of the camera.
 *
 * - Drag moves the floor with the pointer; the point that was grabbed stays
 *   under it.
 * - The wheel (or a pinch) moves towards or away from the point under it.
 * - Dragging with the right button, or with Shift held, turns and tilts.
 * - A click on a closed block goes into it.
 *
 * The camera has a goal and eases towards it, so that the wheel's steps
 * become one smooth movement and levels of detail fade instead of jumping.
 * A journey to somewhere else (into a block, back out) is a flight.
 */
import {
  approach,
  clampCamera,
  fit,
  fitBeside,
  flight,
  groundUnder,
  orbit,
  settled,
  zoomAbout,
  type Camera,
  type Flight,
  type Limits,
} from './camera';
import { gateRect, rectOf, type Rect } from './layout';
import type { Hit } from './pick';
import type { SceneView } from './view';

/** How the scene is looked at to begin with: a little from the south, not straight down. */
const HOME_ELEVATION = (62 * Math.PI) / 180;
/** A pointer that moved less than this many pixels between press and release clicked. */
const CLICK_SLOP = 5;

export interface NavigatorEvents {
  /** Something changed that needs a new picture. */
  invalidate(): void;
  /** The pointer is on something else than before, or on nothing. */
  hover?(hit: Hit | undefined): void;
  /** Something was clicked that the camera does not go into: a gate, or the floor of an open module. */
  select?(hit: Hit): void;
}

export class Navigator {
  /** Where the camera is heading. */
  goal: Camera;
  private flying: { flight: Flight; start: number } | undefined;
  private last = 0;
  private readonly pointers = new Map<number, { x: number; y: number }>();
  private press: { x: number; y: number; moved: number; turning: boolean } | undefined;
  private grabbed: { x: number; y: number } | undefined;

  constructor(
    private readonly view: SceneView,
    private readonly events: NavigatorEvents,
  ) {
    this.goal = view.camera;
    const { canvas } = view;
    canvas.addEventListener('wheel', (e) => this.wheel(e), { passive: false });
    canvas.addEventListener('pointerdown', (e) => this.down(e));
    canvas.addEventListener('pointermove', (e) => this.move(e));
    for (const type of ['pointerup', 'pointercancel'] as const) canvas.addEventListener(type, (e) => this.up(e));
    canvas.addEventListener('pointerleave', () => this.events.hover?.(undefined));
    canvas.addEventListener('dblclick', (e) => this.doubleClick(e));
    // The right button turns the scene; it does not ask for a menu.
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());
  }

  /** How far the camera may go: over the scene, from a gate's width to several times the whole. */
  limits(): Limits {
    const whole = rectOf(this.view.layout, 0);
    return {
      bounds: whole,
      minDistance: 1.5,
      maxDistance: fit(whole, this.view.viewport, { azimuth: 0, elevation: HOME_ELEVATION }).distance * 8,
    };
  }

  /** The camera that shows a rectangle of the plan, at the angles of the present goal. */
  framing(rect: Rect, margin = 0.1): Camera {
    return clampCamera(fitBeside(rect, this.view.viewport, this.goal, this.inset, margin), this.limits());
  }

  /**
   * Pixels of the picture covered by something over it (a panel), on the
   * left and at the top. Framing keeps what it frames out from under them.
   */
  inset = { left: 0, top: 0 };

  /** The camera that shows everything. */
  home(): Camera {
    const whole = rectOf(this.view.layout, 0);
    return fit(whole, this.view.viewport, { azimuth: 0, elevation: HOME_ELEVATION }, 0.06);
  }

  /** Be somewhere at once. */
  jumpTo(camera: Camera): void {
    this.flying = undefined;
    this.goal = camera;
    this.view.camera = camera;
    this.events.invalidate();
  }

  /** Travel somewhere. */
  flyTo(camera: Camera): void {
    this.flying = { flight: flight(this.view.camera, camera), start: performance.now() };
    this.goal = camera;
    this.events.invalidate();
  }

  /** Travel to where a module fills the picture, which opens it. */
  flyToModule(module: number): void {
    this.flyTo(this.framing(rectOf(this.view.layout, module)));
  }

  /** Travel to a gate: close enough to see it and its neighbours with their pins. */
  flyToGate(gate: number): void {
    const cell = gateRect(this.view.layout, gate);
    this.flyTo(this.framing({ x: cell.x - 2.5, y: cell.y - 1.5, w: 6, h: 4 }));
  }

  /**
   * Move the camera on by one frame. Returns whether it moved, which is
   * whether a new picture is needed.
   */
  tick(now: number): boolean {
    const dt = Math.min(64, now - this.last);
    this.last = now;
    if (this.flying) {
      const t = (now - this.flying.start) / this.flying.flight.duration;
      this.view.camera = this.flying.flight.at(t);
      if (t >= 1) this.flying = undefined;
      return true;
    }
    if (this.view.camera === this.goal) return false;
    this.view.camera = settled(this.view.camera, this.goal) ? this.goal : approach(this.view.camera, this.goal, dt);
    return true;
  }

  /** Is the camera on its way somewhere? */
  get moving(): boolean {
    return this.flying !== undefined || this.view.camera !== this.goal;
  }

  /** A key that moves the camera. Returns whether it was one. */
  key(code: string): boolean {
    switch (code) {
      case 'Equal':
      case 'NumpadAdd':
        this.zoomAt(this.view.viewport.width / 2, this.view.viewport.height / 2, 0.6);
        return true;
      case 'Minus':
      case 'NumpadSubtract':
        this.zoomAt(this.view.viewport.width / 2, this.view.viewport.height / 2, 1 / 0.6);
        return true;
      case 'Digit0':
      case 'Home':
        this.flyTo(this.home());
        return true;
      default:
        return false;
    }
  }

  /** Whatever the hand does next starts from where the camera is now. */
  private take(): void {
    if (this.flying) {
      this.flying = undefined;
      this.goal = this.view.camera;
    }
  }

  private set(goal: Camera, atOnce = false): void {
    this.goal = clampCamera(goal, this.limits());
    if (atOnce) this.view.camera = this.goal;
    this.events.invalidate();
  }

  /** Zoom about the point of the floor under a pixel, by a factor of the distance. */
  private zoomAt(px: number, py: number, factor: number, atOnce = false): void {
    this.take();
    const { minDistance, maxDistance } = this.limits();
    // At a limit the camera stops; it must not slide sideways instead.
    const allowed = Math.min(maxDistance, Math.max(minDistance, this.goal.distance * factor)) / this.goal.distance;
    const point = groundUnder(this.goal, this.view.viewport, px, py) ?? this.goal;
    this.set(zoomAbout(this.goal, point.x, point.y, allowed), atOnce);
  }

  private place(e: MouseEvent): { x: number; y: number } {
    const box = this.view.canvas.getBoundingClientRect();
    return { x: e.clientX - box.left, y: e.clientY - box.top };
  }

  private wheel(e: WheelEvent): void {
    e.preventDefault();
    const { x, y } = this.place(e);
    // Lines and pages (a mouse wheel in some browsers) as pixels.
    const delta = e.deltaY * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? this.view.viewport.height : 1);
    // A pinch on a trackpad arrives as a wheel with Ctrl held, in small steps.
    const speed = e.ctrlKey ? 0.012 : 0.0016;
    this.zoomAt(x, y, Math.exp(Math.max(-300, Math.min(300, delta)) * speed));
  }

  private down(e: PointerEvent): void {
    const at = this.place(e);
    this.view.canvas.setPointerCapture(e.pointerId);
    this.pointers.set(e.pointerId, at);
    this.take();
    if (this.pointers.size === 1) {
      const turning = e.button === 2 || e.shiftKey;
      this.press = { ...at, moved: 0, turning };
      this.grabbed = turning ? undefined : groundUnder(this.goal, this.view.viewport, at.x, at.y);
    } else {
      // A second finger: this is a pinch, not a click.
      this.press = undefined;
    }
  }

  private move(e: PointerEvent): void {
    const at = this.place(e);
    const before = this.pointers.get(e.pointerId);
    if (!before) {
      // Nothing is held: the pointer is only passing over.
      if (e.pointerType !== 'touch') this.events.hover?.(this.view.pickAt(at.x, at.y));
      return;
    }

    if (this.pointers.size === 2) {
      // Two fingers: what was under each stays under it, as far as moving,
      // turning and zooming can make it.
      const [first, second] = [...this.pointers.entries()];
      const other = (first![0] === e.pointerId ? second! : first!)[1];
      const centreBefore = { x: (before.x + other.x) / 2, y: (before.y + other.y) / 2 };
      const centreNow = { x: (at.x + other.x) / 2, y: (at.y + other.y) / 2 };
      const spanBefore = Math.hypot(before.x - other.x, before.y - other.y);
      const spanNow = Math.hypot(at.x - other.x, at.y - other.y);
      const turnBy =
        Math.atan2(at.y - other.y, at.x - other.x) - Math.atan2(before.y - other.y, before.x - other.x);
      this.pointers.set(e.pointerId, at);
      const held = groundUnder(this.goal, this.view.viewport, centreBefore.x, centreBefore.y);
      if (spanBefore > 0 && spanNow > 0) this.zoomAt(centreBefore.x, centreBefore.y, spanBefore / spanNow, true);
      this.set(orbit(this.goal, Math.atan2(Math.sin(turnBy), Math.cos(turnBy)), 0), true);
      const now = groundUnder(this.goal, this.view.viewport, centreNow.x, centreNow.y);
      if (held && now) this.set({ ...this.goal, x: this.goal.x + held.x - now.x, y: this.goal.y + held.y - now.y }, true);
      return;
    }

    this.pointers.set(e.pointerId, at);
    if (!this.press) return;
    this.press.moved += Math.hypot(at.x - before.x, at.y - before.y);
    if (this.press.turning) {
      this.set(orbit(this.goal, -(at.x - before.x) * 0.006, (at.y - before.y) * 0.006), true);
    } else if (this.grabbed) {
      const now = groundUnder(this.goal, this.view.viewport, at.x, at.y);
      if (now) {
        this.set({ ...this.goal, x: this.goal.x + this.grabbed.x - now.x, y: this.goal.y + this.grabbed.y - now.y }, true);
      }
    }
  }

  private up(e: PointerEvent): void {
    const held = this.pointers.delete(e.pointerId);
    const press = this.press;
    if (this.pointers.size === 0) {
      this.press = undefined;
      this.grabbed = undefined;
    } else if (this.pointers.size === 1) {
      // One finger of two was lifted: the other goes on dragging from where it is.
      const [rest] = this.pointers.values();
      this.press = { ...rest!, moved: CLICK_SLOP, turning: false };
      this.grabbed = groundUnder(this.goal, this.view.viewport, rest!.x, rest!.y);
    }
    if (!held || !press || e.type === 'pointercancel' || press.turning || press.moved >= CLICK_SLOP) return;

    const at = this.place(e);
    const hit = this.view.pickAt(at.x, at.y);
    if (!hit) return;
    // A closed block is entered; anything else is only pointed at.
    if (hit.gate < 0 && this.view.lod.open[hit.module]! < 0.5) this.flyToModule(hit.module);
    else this.events.select?.(hit);
  }

  private doubleClick(e: MouseEvent): void {
    const at = this.place(e);
    const hit = this.view.pickAt(at.x, at.y);
    if (hit && hit.gate >= 0) this.flyToGate(hit.gate);
  }
}
