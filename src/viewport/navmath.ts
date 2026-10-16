// Pure navigation maths for the viewport's camera controller (lane 21, issue #5): orbit
// about a picked pivot that stays pinned on screen, pan that keeps the grabbed point under
// the cursor, and zoom toward the cursor. No three.js, no DOM, so every rule is unit-tested
// (navmath.test.ts). Z up, metres, the kernel's frame (ADR-0001).
//
// A camera is a pose (an eye and an orthonormal screen basis: right, up, and the forward
// direction it looks along) plus a lens (parallel or perspective, field of view or frame
// height, zoom, aspect). Orbiting turns the pose rigidly about the pivot, which is why the
// pivot cannot move on screen: its coordinates in the camera's own frame do not change.
import type { Vec3 } from "./camera";

export interface Pose {
  eye: Vec3;
  /** Screen right, screen up and the view direction: an orthonormal, right-handed basis
   * (right x up = -fwd, as a camera looking down its -Z axis). */
  right: Vec3;
  up: Vec3;
  fwd: Vec3;
}

export interface Lens {
  ortho: boolean;
  /** Perspective: the vertical field of view at zoom 1, degrees. */
  fovDeg: number;
  /** Parallel: half the frame height at zoom 1, metres. */
  halfHeight: number;
  zoom: number;
  /** Width over height. */
  aspect: number;
}

/** The view never tips further than this from the horizon (SketchUp-like: no flip over the pole). */
export const ELEVATION_LIMIT = (89.5 * Math.PI) / 180;

/** World up. */
export const Z: Vec3 = [0, 0, 1];

// ---- vectors ---------------------------------------------------------------------------

export const add = (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
export const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
export const scale = (a: Vec3, k: number): Vec3 => [a[0] * k, a[1] * k, a[2] * k];
export const dot = (a: Vec3, b: Vec3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
export const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
export const length = (a: Vec3): number => Math.hypot(a[0], a[1], a[2]);
export function normalize(a: Vec3): Vec3 {
  const l = length(a);
  return l > 0 ? scale(a, 1 / l) : [0, 0, 0];
}
/** a + b * k */
export const addScaled = (a: Vec3, b: Vec3, k: number): Vec3 => [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k];

/** Rotate `v` about the unit `axis` by `angle` radians (right-hand rule; Rodrigues). */
export function rotate(v: Vec3, axis: Vec3, angle: number): Vec3 {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  const k = dot(axis, v) * (1 - c);
  const x = cross(axis, v);
  return [v[0] * c + x[0] * s + axis[0] * k, v[1] * c + x[1] * s + axis[1] * k, v[2] * c + x[2] * s + axis[2] * k];
}

// ---- projection ------------------------------------------------------------------------

/** Tangent of half the vertical field of view, with the zoom applied. */
const halfTan = (lens: Lens) => Math.tan((lens.fovDeg * Math.PI) / 360) / lens.zoom;

/** Half the visible height of a parallel lens, with the zoom applied. */
const halfFrame = (lens: Lens) => lens.halfHeight / lens.zoom;

/** Normalised device coordinates (-1..1, y up) of world point `p`. */
export function projectNdc(pose: Pose, lens: Lens, p: Vec3): [number, number] {
  const d = sub(p, pose.eye);
  const x = dot(d, pose.right);
  const y = dot(d, pose.up);
  if (lens.ortho) {
    const h = halfFrame(lens);
    return [x / (h * lens.aspect), y / h];
  }
  const depth = dot(d, pose.fwd);
  const t = halfTan(lens);
  return [x / (depth * t * lens.aspect), y / (depth * t)];
}

/** The eye ray through NDC (x, y): its origin (on the eye's plane for a parallel lens) and unit direction. */
export function rayAt(pose: Pose, lens: Lens, x: number, y: number): { origin: Vec3; dir: Vec3 } {
  if (lens.ortho) {
    const h = halfFrame(lens);
    const origin = addScaled(addScaled(pose.eye, pose.right, x * h * lens.aspect), pose.up, y * h);
    return { origin, dir: pose.fwd };
  }
  const t = halfTan(lens);
  const dir = normalize(addScaled(addScaled(pose.fwd, pose.right, x * t * lens.aspect), pose.up, y * t));
  return { origin: pose.eye, dir };
}

/** Distance of `p` in front of the eye, along the view direction. */
export const depthOf = (pose: Pose, p: Vec3): number => dot(sub(p, pose.eye), pose.fwd);

// ---- orbit -----------------------------------------------------------------------------

/** How far the view looks down from the horizon (radians; negative looking up). */
export function elevation(fwd: Vec3): number {
  return Math.asin(Math.max(-1, Math.min(1, -fwd[2])));
}

/**
 * The elevation step that is actually taken: the view stops `limit` short of straight down
 * or straight up instead of flipping over the pole. A view already beyond the limit (the
 * Top view looks exactly down) may come back towards the horizon but goes no further out.
 */
export function elevationStep(current: number, step: number, limit = ELEVATION_LIMIT): number {
  const lo = Math.min(-limit, current);
  const hi = Math.max(limit, current);
  return Math.max(lo, Math.min(hi, current + step)) - current;
}

/** Orbit angles for a drag of (dx, dy) CSS pixels on a viewport `height` pixels tall: a
 * drag the height of the view turns a full circle (the speed three.js OrbitControls used).
 * Dragging right turns the model to the right (the eye goes left round the pivot);
 * dragging down tips its top towards you (the eye rises). */
export function dragAngles(dx: number, dy: number, height: number): { azimuth: number; elevation: number } {
  const k = (2 * Math.PI) / Math.max(height, 1);
  return { azimuth: -dx * k, elevation: dy * k };
}

/** The horizontal axis an elevation turns about: the camera's right vector laid level
 * (they are the same for a camera that is not rolled); the right vector itself when the
 * view looks straight up or down. */
export function elevationAxis(pose: Pose): Vec3 {
  const h = cross(pose.fwd, Z);
  if (length(h) < 1e-9) return pose.right;
  const n = normalize(h);
  return dot(n, pose.right) < 0 ? scale(n, -1) : n;
}

/**
 * Orbit `pose` about `pivot`: first `azimuth` radians about world +Z (positive turns the
 * eye counter-clockwise seen from above), then `elevation` radians about the level right
 * vector (positive tips the view further down), clamped by `elevationStep`. The turn is
 * rigid, so the pivot keeps its place on screen exactly (up to rounding), in both
 * projections, and an unrolled camera stays unrolled (its up vector stays in the vertical
 * plane through the view direction, never below the horizon).
 */
export function orbit(pose: Pose, pivot: Vec3, azimuth: number, dElevation: number, limit = ELEVATION_LIMIT): { pose: Pose; azimuth: number; elevation: number } {
  // About +Z through the pivot.
  let rel = rotate(sub(pose.eye, pivot), Z, azimuth);
  let right = rotate(pose.right, Z, azimuth);
  let up = rotate(pose.up, Z, azimuth);
  let fwd = rotate(pose.fwd, Z, azimuth);
  const turned: Pose = { eye: add(pivot, rel), right, up, fwd };
  const step = elevationStep(elevation(fwd), dElevation, limit);
  if (step !== 0) {
    // Tipping the view down turns the forward vector about the right vector by -step.
    const axis = elevationAxis(turned);
    rel = rotate(rel, axis, -step);
    right = rotate(right, axis, -step);
    up = rotate(up, axis, -step);
    fwd = rotate(fwd, axis, -step);
  }
  return { pose: { eye: add(pivot, rel), right, up, fwd }, azimuth, elevation: step };
}

/** The look-at point after a move: along the view direction at the pivot's depth (a small
 * positive depth when the pivot is level with or behind the eye). */
export function lookTarget(pose: Pose, pivot: Vec3, fallbackDepth: number): Vec3 {
  const d = depthOf(pose, pivot);
  return addScaled(pose.eye, pose.fwd, d > 1e-9 ? d : Math.max(fallbackDepth, 1e-6));
}

// ---- pan -------------------------------------------------------------------------------

/**
 * How far to move the eye (and the look-at point) so that world point `anchor` lies on the
 * cursor ray (origin, dir): the ray is met in the plane through the anchor square to the
 * view, so the move stays in that plane and the anchor's depth (its size on screen in
 * perspective) does not change.
 */
export function panOffset(fwd: Vec3, anchor: Vec3, ray: { origin: Vec3; dir: Vec3 }): Vec3 {
  const denom = dot(ray.dir, fwd);
  if (Math.abs(denom) < 1e-12) return [0, 0, 0];
  const t = dot(sub(anchor, ray.origin), fwd) / denom;
  return sub(anchor, addScaled(ray.origin, ray.dir, t));
}

// ---- zoom ------------------------------------------------------------------------------

/**
 * The factor a wheel event zooms by (above 1 zooms in): 5% per 100 pixels of wheel, lines
 * counted as 16 pixels and pages as 100 lines, a pinch on a trackpad (the browser sends it
 * with Ctrl) ten times as fast. The same steps three.js OrbitControls took.
 */
export function wheelZoomFactor(deltaY: number, deltaMode = 0, ctrl = false): number {
  let d = deltaY * (deltaMode === 1 ? 16 : deltaMode === 2 ? 100 : 1);
  if (ctrl) d *= 10;
  if (d === 0) return 1;
  const s = Math.pow(0.95, Math.abs(d) * 0.01);
  return d < 0 ? 1 / s : s;
}

/**
 * Zoom by `factor` toward the cursor at NDC (x, y). Perspective: the eye moves along the
 * cursor ray toward `anchor` (a point on that ray: what is under the cursor), keeping
 * `1 / factor` of its distance, so the anchor stays under the cursor. Parallel: the zoom
 * scales and the eye slides in its own plane so the point under the cursor stays put.
 */
export function zoomAt(pose: Pose, lens: Lens, x: number, y: number, factor: number, anchor: Vec3): { pose: Pose; zoom: number } {
  if (lens.ortho) {
    const zoom = lens.zoom * factor;
    const k = 1 - 1 / factor; // (1/z - 1/z') * z
    const h = halfFrame(lens);
    const eye = addScaled(addScaled(pose.eye, pose.right, x * h * lens.aspect * k), pose.up, y * h * k);
    return { pose: { ...pose, eye }, zoom };
  }
  const eye = addScaled(anchor, sub(pose.eye, anchor), 1 / factor);
  return { pose: { ...pose, eye }, zoom: lens.zoom };
}
