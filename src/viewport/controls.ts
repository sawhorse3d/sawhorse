// The viewport's camera controller (lane 21, issue #5), in place of three.js OrbitControls.
//
//   middle-drag        orbit about the point under the cursor at the press (SketchUp's
//                      pivot): a model point, else the ground under the cursor, else the
//                      look-at point. The pivot stays pinned on screen for the whole drag,
//                      the azimuth turns about world +Z, the elevation about the level right
//                      vector, stopping 0.5 degrees short of straight down or up; a small
//                      marker shows the pivot while the button is held.
//   Shift+middle-drag  pan (Shift may be pressed or let go mid-drag)
//   right-drag         pan: the point under the cursor at the press stays under it
//   wheel              zoom toward the cursor (the point under it stays put)
//   one finger         orbit, as middle-drag
//   two fingers        pinch to zoom about their midpoint, drag to pan
//
// The left button belongs to the active tool. After every move the look-at point
// (`target`, what the standard views, zoom extents, the grid density, the projection
// toggle and the Sheets' "use 3D view" read) is put back on the view direction at the
// depth of the point the gesture holds. The maths is in navmath.ts (unit-tested); this
// file only reads pointers and moves the camera. No pointer capture: the drag listens on
// the document, so it follows the pointer off the canvas, and ends when the button is
// found released (a release outside the window) or the window loses focus.
import * as THREE from "three";
import type { Vec3 } from "./camera";
import { dragAngles, lookTarget, orbit, panOffset, rayAt, wheelZoomFactor, zoomAt, type Lens, type Pose } from "./navmath";

export type NavMode = "orbit" | "pan" | "pinch";

export interface NavHost {
  /** The model point (or the ground point) under canvas CSS pixel (x, y), or null when the
   * cursor ray meets neither. `snap`: a vertex or edge a few pixels off may stand in for it
   * (the orbit pivot, which stays where it is on screen); without it the point lies on the
   * cursor ray (what a pan drags and a zoom closes in on, so nothing jumps). */
  pick(x: number, y: number, snap: boolean): Vec3 | null;
  /** The canvas size in CSS pixels. */
  size(): { w: number; h: number };
  /** A gesture started or ended (the pivot marker shows or goes). */
  navigating(): void;
}

type Camera = THREE.PerspectiveCamera | THREE.OrthographicCamera;

interface Drag {
  mode: NavMode;
  pointerId: number;
  /** MouseEvent.buttons bit of the button holding the drag (0 for touch). */
  mask: number;
  button: number;
  x: number;
  y: number;
  /** Orbit: the pivot. Pan and pinch: the grabbed point. */
  anchor: Vec3;
  /** Pinch: the fingers' spacing at the last move. */
  spacing?: number;
}

const Z = new THREE.Vector3(0, 0, 1);

/** The camera's pose: its eye and its screen basis (three.js cameras look down their -Z). */
export function poseOf(cam: THREE.Camera): Pose {
  cam.updateMatrixWorld();
  const e = cam.matrixWorld.elements;
  const n = (x: number, y: number, z: number, k = 1): Vec3 => {
    const l = Math.hypot(x, y, z) || 1;
    return [(k * x) / l, (k * y) / l, (k * z) / l];
  };
  return { eye: [cam.position.x, cam.position.y, cam.position.z], right: n(e[0], e[1], e[2]), up: n(e[4], e[5], e[6]), fwd: n(e[8], e[9], e[10], -1) };
}

/** The lens of a three.js camera, as navmath reads it. */
export function lensOf(cam: Camera): Lens {
  if (cam instanceof THREE.PerspectiveCamera) return { ortho: false, fovDeg: cam.fov, halfHeight: 0, zoom: cam.zoom, aspect: cam.aspect };
  const h = (cam.top - cam.bottom) / 2;
  return { ortho: true, fovDeg: 0, halfHeight: h, zoom: cam.zoom, aspect: (cam.right - cam.left) / 2 / Math.max(h, 1e-12) };
}

const basis = new THREE.Matrix4();
const vr = new THREE.Vector3();
const vu = new THREE.Vector3();
const vb = new THREE.Vector3();

/** Put a pose on a three.js camera. Its `up` (what lookAt and the drawing exports read) is
 * world +Z whenever the view is unrolled and not looking straight up or down, else the
 * screen's up. */
export function applyPose(cam: THREE.Camera, pose: Pose): void {
  cam.position.set(...pose.eye);
  basis.makeBasis(vr.set(...pose.right), vu.set(...pose.up), vb.set(-pose.fwd[0], -pose.fwd[1], -pose.fwd[2]));
  cam.quaternion.setFromRotationMatrix(basis);
  if (Math.abs(pose.fwd[2]) < 1 - 1e-9 && Math.abs(pose.right[2]) < 1e-9) cam.up.copy(Z);
  else cam.up.set(...pose.up);
  cam.updateMatrixWorld();
}

export class CameraController {
  /** The look-at point: on the view direction, at the depth of what the last gesture held. */
  readonly target = new THREE.Vector3();
  private listeners = new Set<() => void>();
  private drag: Drag | null = null;
  private touches = new Map<number, { x: number; y: number }>();
  private lastPose = "";
  private doc: Document;
  private win: Window | null;

  constructor(
    public object: Camera,
    private dom: HTMLElement,
    private host: NavHost,
  ) {
    this.doc = dom.ownerDocument;
    this.win = this.doc.defaultView;
    // Touch gestures belong to the controller, not to the page's scrolling.
    dom.style.touchAction = "none";
    dom.addEventListener("pointerdown", this.onDown);
    dom.addEventListener("wheel", this.onWheel, { passive: false });
  }

  /** Listen for camera moves (orbit, pan, zoom, and `update`). */
  onChange(cb: () => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  /** What the controller is doing, and the pivot while orbiting (the marker). */
  state(): { mode: NavMode | null; pivot: Vec3 | null } {
    const d = this.drag;
    return { mode: d?.mode ?? null, pivot: d?.mode === "orbit" ? d.anchor : null };
  }

  /** Aim the camera at `target` (after the engine moved the camera or the target), telling
   * listeners when that moved it. */
  update(): boolean {
    this.object.lookAt(this.target);
    this.object.updateMatrixWorld();
    return this.changed();
  }

  dispose(): void {
    this.endDrag(false);
    this.dom.removeEventListener("pointerdown", this.onDown);
    this.dom.removeEventListener("wheel", this.onWheel);
    this.listeners.clear();
  }

  // ---- internals ---------------------------------------------------------------------------

  /** Tell listeners if the camera moved since they last heard. */
  private changed(): boolean {
    const c = this.object;
    const key = `${c.position.toArray().join()}|${c.quaternion.toArray().join()}|${c.zoom}|${this.target.toArray().join()}`;
    if (key === this.lastPose) return false;
    this.lastPose = key;
    for (const cb of this.listeners) cb();
    return true;
  }

  private local(e: { clientX: number; clientY: number }): [number, number] {
    const r = this.dom.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  private ndc(x: number, y: number): [number, number] {
    const { w, h } = this.host.size();
    return [(x / Math.max(w, 1)) * 2 - 1, 1 - (y / Math.max(h, 1)) * 2];
  }

  /** Depth of the look-at point in front of the eye (the fallback for a miss). */
  private targetDepth(pose: Pose): number {
    const t = this.target;
    return (t.x - pose.eye[0]) * pose.fwd[0] + (t.y - pose.eye[1]) * pose.fwd[1] + (t.z - pose.eye[2]) * pose.fwd[2];
  }

  /** The point a pan or zoom holds at pixel (x, y): what is under it, else the point on
   * its ray at the look-at point's depth. */
  private grab(x: number, y: number): Vec3 {
    const hit = this.host.pick(x, y, false);
    if (hit) return hit;
    const pose = poseOf(this.object);
    const [nx, ny] = this.ndc(x, y);
    const ray = rayAt(pose, lensOf(this.object), nx, ny);
    // Along the ray (it starts on the eye's plane in both projections) to the plane of the
    // look-at point, square to the view.
    const d = Math.max(this.targetDepth(pose), 1e-6);
    const t = d / Math.max(ray.dir[0] * pose.fwd[0] + ray.dir[1] * pose.fwd[1] + ray.dir[2] * pose.fwd[2], 1e-9);
    return [ray.origin[0] + ray.dir[0] * t, ray.origin[1] + ray.dir[1] * t, ray.origin[2] + ray.dir[2] * t];
  }

  /** The pivot an orbit turns about at pixel (x, y): what is under it, else the look-at point. */
  private pivotAt(x: number, y: number): Vec3 {
    return this.host.pick(x, y, true) ?? [this.target.x, this.target.y, this.target.z];
  }

  private start(mode: NavMode, e: PointerEvent, x: number, y: number, mask: number): void {
    const anchor = mode === "orbit" ? this.pivotAt(x, y) : this.grab(x, y);
    const fresh = !this.drag;
    this.drag = { mode, pointerId: e.pointerId, mask, button: e.button, x, y, anchor };
    if (fresh) {
      this.doc.addEventListener("pointermove", this.onMove);
      this.doc.addEventListener("pointerup", this.onUp);
      this.doc.addEventListener("pointercancel", this.onUp);
      this.win?.addEventListener("blur", this.onBlur);
    }
    this.host.navigating();
  }

  private endDrag(notify = true): void {
    if (!this.drag && !this.touches.size) return;
    this.drag = null;
    this.touches.clear();
    this.doc.removeEventListener("pointermove", this.onMove);
    this.doc.removeEventListener("pointerup", this.onUp);
    this.doc.removeEventListener("pointercancel", this.onUp);
    this.win?.removeEventListener("blur", this.onBlur);
    if (notify) this.host.navigating();
  }

  /** Move the camera to `pose` (and the zoom), the look-at point to `anchor`'s depth. */
  private moveTo(pose: Pose, anchor: Vec3, zoom?: number): void {
    const cam = this.object;
    const fallback = Math.max(this.targetDepth(poseOf(cam)), 1e-3);
    applyPose(cam, pose);
    if (zoom !== undefined && zoom !== cam.zoom) {
      cam.zoom = zoom;
      cam.updateProjectionMatrix();
    }
    this.target.set(...lookTarget(pose, anchor, fallback));
    this.changed();
  }

  private orbitBy(dx: number, dy: number, pivot: Vec3): void {
    const a = dragAngles(dx, dy, this.host.size().h);
    const r = orbit(poseOf(this.object), pivot, a.azimuth, a.elevation);
    this.moveTo(r.pose, pivot);
  }

  /** Slide the view so `anchor` is under pixel (x, y). */
  private panTo(x: number, y: number, anchor: Vec3): void {
    const pose = poseOf(this.object);
    const [nx, ny] = this.ndc(x, y);
    const off = panOffset(pose.fwd, anchor, rayAt(pose, lensOf(this.object), nx, ny));
    this.moveTo({ ...pose, eye: [pose.eye[0] + off[0], pose.eye[1] + off[1], pose.eye[2] + off[2]] }, anchor);
  }

  /** Zoom by `factor` about pixel (x, y), where `anchor` is (on its ray). */
  private zoomBy(x: number, y: number, factor: number, anchor: Vec3): void {
    if (!(factor > 0) || factor === 1) return;
    const pose = poseOf(this.object);
    const [nx, ny] = this.ndc(x, y);
    const z = zoomAt(pose, lensOf(this.object), nx, ny, factor, anchor);
    this.moveTo(z.pose, anchor, z.zoom);
  }

  // ---- events ------------------------------------------------------------------------------

  private onDown = (e: PointerEvent): void => {
    const [x, y] = this.local(e);
    if (e.pointerType === "touch") {
      this.touches.set(e.pointerId, { x, y });
      this.retouch(e);
      return;
    }
    if (this.drag) return;
    if (e.button === 1) {
      e.preventDefault();
      this.start(e.shiftKey ? "pan" : "orbit", e, x, y, 4);
    } else if (e.button === 2) {
      this.start("pan", e, x, y, 2);
    }
  };

  /** Touch: one finger orbits, two pan and pinch; a finger more or less starts afresh. */
  private retouch(e: PointerEvent): void {
    const pts = [...this.touches.values()];
    if (pts.length === 1) {
      this.start("orbit", e, pts[0].x, pts[0].y, 0);
    } else if (pts.length >= 2) {
      const [a, b] = pts;
      const mx = (a.x + b.x) / 2;
      const my = (a.y + b.y) / 2;
      this.start("pinch", e, mx, my, 0);
      if (this.drag) this.drag.spacing = Math.hypot(a.x - b.x, a.y - b.y);
    }
  }

  private onMove = (e: PointerEvent): void => {
    const d = this.drag;
    if (!d) return;
    const [x, y] = this.local(e);
    if (e.pointerType === "touch") {
      if (!this.touches.has(e.pointerId)) return;
      this.touches.set(e.pointerId, { x, y });
      const pts = [...this.touches.values()];
      if (d.mode === "orbit" && pts.length === 1) {
        this.orbitBy(x - d.x, y - d.y, d.anchor);
        d.x = x;
        d.y = y;
      } else if (d.mode === "pinch" && pts.length >= 2) {
        const [a, b] = pts;
        const mx = (a.x + b.x) / 2;
        const my = (a.y + b.y) / 2;
        const spacing = Math.hypot(a.x - b.x, a.y - b.y);
        this.panTo(mx, my, d.anchor);
        if (d.spacing && spacing > 0) this.zoomBy(mx, my, spacing / d.spacing, d.anchor);
        d.spacing = spacing;
        d.x = mx;
        d.y = my;
      }
      return;
    }
    if (e.pointerId !== d.pointerId) return;
    // The button was let go where we could not hear it (outside the window).
    if (e.pointerType === "mouse" && (e.buttons & d.mask) === 0) {
      this.endDrag();
      return;
    }
    // Shift pressed or let go during a middle drag switches between orbit and pan.
    if (d.button === 1) {
      const want: NavMode = e.shiftKey ? "pan" : "orbit";
      if (want !== d.mode) {
        this.start(want, e, x, y, d.mask);
        return;
      }
    }
    if (d.mode === "orbit") this.orbitBy(x - d.x, y - d.y, d.anchor);
    else this.panTo(x, y, d.anchor);
    d.x = x;
    d.y = y;
  };

  private onUp = (e: PointerEvent): void => {
    if (e.pointerType === "touch") {
      if (!this.touches.delete(e.pointerId)) return;
      if (this.touches.size) {
        this.retouch(e);
      } else {
        this.endDrag();
      }
      return;
    }
    if (this.drag && e.pointerId === this.drag.pointerId && e.button === this.drag.button) this.endDrag();
  };

  private onBlur = (): void => this.endDrag();

  private onWheel = (e: WheelEvent): void => {
    e.preventDefault();
    const f = wheelZoomFactor(e.deltaY, e.deltaMode, e.ctrlKey);
    if (f === 1) return;
    const [x, y] = this.local(e);
    this.zoomBy(x, y, f, this.grab(x, y));
  };
}
