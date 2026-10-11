// Pure helpers that turn tool gestures into kernel ops (no DOM, no three.js), so the
// tool logic is unit-tested. Lengths are metres; frames are right-handed (u x v = normal).
import type { KernelOp, MeshPart, OpOutcome } from "../kernel/types";

export type Vec3 = [number, number, number];

/** A plane with an in-plane frame: where a rectangle is drawn. */
export interface Frame {
  origin: Vec3;
  normal: Vec3;
  u: Vec3;
  v: Vec3;
}

/** The ground sketch (public id 1): origin 0, +Z normal, u = +X, v = +Y. */
export const GROUND_SKETCH = 1;
export const GROUND: Frame = { origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] };

/**
 * Persistent face id of a new solid's cap (the face opposite the sketch). The kernel numbers
 * an extrusion's faces base 1, cap 2, walls 3.. and never reuses an id, so the UI holds on to
 * face ids across edits; it never addresses a face by its position in a list.
 */
export const CAP_FACE_ID = 2;

/** What a Push/Pull acts on: a sketch region (extrude) or an object face (by persistent id). */
export type Target = { kind: "region"; sketch: number; region: number } | { kind: "face"; object: number; face: number };

/** Where a rectangle starts: on the ground sketch, or on an object face (imprint). */
export interface RectAnchor {
  frame: Frame;
  point: Vec3;
  /** The face it was started on; null = the ground sketch. */
  face: { object: number; face: number } | null;
}

const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const add = (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const scale = (a: Vec3, s: number): Vec3 => [a[0] * s, a[1] * s, a[2] * s];

export function toLocal(f: Frame, p: Vec3): [number, number] {
  const d = sub(p, f.origin);
  return [dot(d, f.u), dot(d, f.v)];
}

export function toWorld(f: Frame, x: number, y: number): Vec3 {
  return add(f.origin, add(scale(f.u, x), scale(f.v, y)));
}

export function frameOfPart(p: MeshPart): Frame {
  return { origin: p.origin, normal: p.normal, u: p.u, v: p.v };
}

export function targetOfPart(p: MeshPart): Target {
  return p.kind === "face" ? { kind: "face", object: p.owner, face: p.index } : { kind: "region", sketch: p.owner, region: p.index };
}

export function sameTarget(a: Target | null, b: Target | null): boolean {
  if (!a || !b) return a === b;
  if (a.kind === "face" && b.kind === "face") return a.object === b.object && a.face === b.face;
  if (a.kind === "region" && b.kind === "region") return a.sketch === b.sketch && a.region === b.region;
  return false;
}

/** Index of the part a target names in the current mesh, or -1. */
export function partIndex(parts: MeshPart[], t: Target | null): number {
  if (!t) return -1;
  return parts.findIndex((p) => sameTarget(targetOfPart(p), t));
}

/** Ray (origin, direction) meets the frame's plane, in front of the eye. */
export function rayPlane(origin: Vec3, dir: Vec3, f: Frame): Vec3 | null {
  const denom = dot(dir, f.normal);
  if (Math.abs(denom) < 1e-12) return null;
  const t = dot(sub(f.origin, origin), f.normal) / denom;
  if (!(t > 0)) return null;
  return add(origin, scale(dir, t));
}

/** Corners (frame-local) of the rectangle from `a` to `b`, counter-clockwise in the frame. */
export function rectCorners(a: [number, number], b: [number, number]): [number, number][] {
  const [x0, x1] = [Math.min(a[0], b[0]), Math.max(a[0], b[0])];
  const [y0, y1] = [Math.min(a[1], b[1]), Math.max(a[1], b[1])];
  return [
    [x0, y0],
    [x1, y0],
    [x1, y1],
    [x0, y1],
  ];
}

/**
 * The op for a rectangle from `anchor` to the frame-local corner `end` (or, with `size`,
 * width along u and depth along v from the anchor). Ground: add_rectangle on the ground
 * sketch. Object face: imprint (lane 3 adds rectangles that cross a face's edges).
 */
export function rectangleOp(anchor: RectAnchor, end: [number, number] | null, size?: [number, number]): KernelOp {
  const a = toLocal(anchor.frame, anchor.point);
  const b: [number, number] = size ? [a[0] + size[0], a[1] + size[1]] : (end ?? a);
  if (!anchor.face) {
    return { op: "add_rectangle", sketch: GROUND_SKETCH, x: a[0], y: a[1], w: b[0] - a[0], h: b[1] - a[1] };
  }
  const points = rectCorners(a, b).map(([x, y]) => toWorld(anchor.frame, x, y));
  return { op: "imprint", object: anchor.face.object, face: anchor.face.face, points };
}

/** The op for pushing/pulling a target by `distance` (a region extrudes into a solid). */
export function pushOp(t: Target, distance: number): KernelOp {
  return t.kind === "region"
    ? { op: "extrude", sketch: t.sketch, region: t.region, distance }
    : { op: "push_pull", object: t.object, face: t.face, distance };
}

/**
 * What the Push tool should act on next after an op: the region a rectangle drew, the
 * sub-face an imprint made, the cap of a new solid (face id 2), the same face after a move.
 */
export function nextTarget(op: KernelOp, outcome: OpOutcome, previous: Target | null): Target | null {
  switch (outcome.kind) {
    case "sketch_edited":
      return outcome.regions.length && "sketch" in op ? { kind: "region", sketch: op.sketch, region: outcome.regions[0] } : previous;
    case "imprinted":
      return op.op === "imprint" ? { kind: "face", object: op.object, face: outcome.face } : previous;
    case "object_created":
      return { kind: "face", object: outcome.object, face: CAP_FACE_ID };
    case "face_moved":
      return op.op === "push_pull" ? { kind: "face", object: op.object, face: op.face } : previous;
    case "object_collapsed":
      return null;
    case "drawn": {
      // A closed stroke: what it encloses. An open one that formed a region: that region.
      if (op.op !== "draw") return previous;
      if (outcome.sketch !== null) {
        const region = outcome.inside ?? outcome.regions[0];
        return region !== undefined ? { kind: "region", sketch: outcome.sketch, region } : previous;
      }
      if (op.target.on === "face" && outcome.inside !== null) return { kind: "face", object: op.target.object, face: outcome.inside };
      return previous;
    }
    default:
      return previous;
  }
}

export function describeTarget(t: Target | null): string {
  if (!t) return "nothing selected";
  return t.kind === "region" ? `region ${t.region}` : `face ${t.face} of solid ${t.object}`;
}
