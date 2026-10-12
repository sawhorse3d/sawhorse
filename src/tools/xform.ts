// Shared pieces of the transform tools (Move, Rotate, Scale): affine maps in the kernel's
// plain-data form, what a gesture acts on (whole solids, or part of one), and the edges a
// preview draws. Pure: no DOM, no three.js, unit-tested in move.test.ts and friends.
import type { Entity, MeshData, V3, Xform } from "../kernel/types";
import { objectEdges, objectOf } from "../lib/entities";
import { faceOf, solidName } from "../lib/names";
import { add, cross, dot, len, mul, sub } from "./draw";

export const IDENTITY: Xform = {
  linear: [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
  ],
  translation: [0, 0, 0],
};

export function translation(d: V3): Xform {
  return { ...IDENTITY, translation: [d[0], d[1], d[2]] };
}

/** Turn `angle` radians about the line through `c` along unit `axis` (right-handed). */
export function rotation(c: V3, axis: V3, angle: number): Xform {
  const n = len(axis);
  const [x, y, z] = n > 0 ? mul(axis, 1 / n) : [0, 0, 1];
  const s = Math.sin(angle);
  const k = Math.cos(angle);
  const t = 1 - k;
  const linear: [V3, V3, V3] = [
    [t * x * x + k, t * x * y - s * z, t * x * z + s * y],
    [t * x * y + s * z, t * y * y + k, t * y * z - s * x],
    [t * x * z - s * y, t * y * z + s * x, t * z * z + k],
  ];
  // p -> R (p - c) + c
  const rc = mulVec(linear, c);
  return { linear, translation: [c[0] - rc[0], c[1] - rc[1], c[2] - rc[2]] };
}

/** Scale by `f` along the world axes about `c` (negative factors mirror). */
export function scaling(c: V3, f: V3): Xform {
  return {
    linear: [
      [f[0], 0, 0],
      [0, f[1], 0],
      [0, 0, f[2]],
    ],
    translation: [c[0] * (1 - f[0]), c[1] * (1 - f[1]), c[2] * (1 - f[2])],
  };
}

export function mulVec(m: [V3, V3, V3], v: V3): V3 {
  return [dot(m[0], v), dot(m[1], v), dot(m[2], v)];
}

export function applyX(x: Xform, p: V3): V3 {
  return add(mulVec(x.linear, p), x.translation);
}

/** What a transform gesture acts on. */
export type Subject =
  | { kind: "objects"; objects: number[] }
  /** Part of one solid: its points (a face's corners, an edge's ends, a vertex). */
  | { kind: "geometry"; object: number; points: V3[]; what: string };

/** "solid 3", "solids 2 and 5", "face 6 of solid 3". */
export function describeSubject(s: Subject): string {
  if (s.kind === "geometry") return s.what;
  const names = s.objects.map(solidName);
  if (names.length === 1) return names[0];
  if (names.length <= 3) return `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
  return `${names.length} solids`;
}

/**
 * What to transform: the selected solids; or the selected faces, edges and points of ONE
 * solid (part of it); or, with nothing selected, the solid under the cursor. Face corners
 * come from `outlines` (face key -> world rings). A string says why nothing can move.
 */
export function subjectOf(selection: Entity[], picked: Entity | null, outlines: Map<string, V3[][]>): Subject | string {
  if (selection.length) {
    const objects = [...new Set(selection.filter((e) => e.kind === "object").map((e) => (e as { object: number }).object))];
    if (objects.length) return { kind: "objects", objects };
    if (selection.some((e) => e.kind === "region")) return "Sketch regions don't move yet: push/pull them into a solid first.";
    if (selection.some((e) => e.kind === "guide" || e.kind === "dimension")) return "Guides and dimensions don't move: erase and place them again.";
    const owners = [...new Set(selection.map(objectOf))];
    if (owners.length !== 1 || owners[0] === null) {
      if (owners.every((o) => o !== null)) return "Move parts of one solid at a time, or double-click to select whole solids.";
      return "Sketch edges don't move yet.";
    }
    const object = owners[0];
    const points: V3[] = [];
    const push = (p: V3) => {
      if (!points.some((q) => Math.hypot(q[0] - p[0], q[1] - p[1], q[2] - p[2]) <= 1e-9)) points.push(p);
    };
    let faces = 0;
    let what = "";
    for (const e of selection) {
      if (e.kind === "face") {
        faces++;
        what = faceOf(e.object, e.face);
        for (const ring of outlines.get(`${e.object}:${e.face}`) ?? []) ring.forEach(push);
      } else if (e.kind === "edge") {
        push(e.a);
        push(e.b);
        what ||= `an edge of ${solidName(object)}`;
      } else if (e.kind === "vertex") {
        push(e.at);
        what ||= `a point of ${solidName(object)}`;
      }
    }
    if (!points.length) return "Nothing to move there.";
    if (selection.length > 1) what = faces > 1 ? `${faces} faces of ${solidName(object)}` : `${selection.length} parts of ${solidName(object)}`;
    return { kind: "geometry", object, points, what };
  }
  const o = picked ? objectOf(picked) : null;
  if (o === null) return "Click a solid, or select what to move first.";
  return { kind: "objects", objects: [o] };
}

/** The faces whose outlines a geometry subject needs (to collect their corners). */
export function facesToOutline(selection: Entity[]): { object: number; face: number }[] {
  return selection.filter((e): e is Extract<Entity, { kind: "face" }> => e.kind === "face").map((e) => ({ object: e.object, face: e.face }));
}

/** Edges a preview shows for the subject, before the transform. */
export function subjectEdges(s: Subject, mesh: MeshData | null): [V3, V3][] {
  if (s.kind === "objects") return s.objects.flatMap((o) => objectEdges(o, mesh));
  // Part of a solid: every edge of that solid touching a moving point (both ends known).
  const moving = (p: V3) => s.points.some((q) => Math.hypot(q[0] - p[0], q[1] - p[1], q[2] - p[2]) <= 1e-7);
  return objectEdges(s.object, mesh).filter(([a, b]) => moving(a) || moving(b));
}

/** The preview segments: each edge with its moving ends mapped (whole solids: both ends). */
export function previewSegments(s: Subject, edges: [V3, V3][], x: Xform): V3[] {
  const moving = (p: V3) => s.kind === "objects" || s.points.some((q) => Math.hypot(q[0] - p[0], q[1] - p[1], q[2] - p[2]) <= 1e-7);
  const out: V3[] = [];
  for (const [a, b] of edges) {
    out.push(moving(a) ? applyX(x, a) : a, moving(b) ? applyX(x, b) : b);
  }
  return out;
}

/** World bounds (min, max) of points. */
export function boundsOf(points: V3[]): [V3, V3] | null {
  if (!points.length) return null;
  const lo: V3 = [Infinity, Infinity, Infinity];
  const hi: V3 = [-Infinity, -Infinity, -Infinity];
  for (const p of points) {
    for (let k = 0; k < 3; k++) {
      lo[k] = Math.min(lo[k], p[k]);
      hi[k] = Math.max(hi[k], p[k]);
    }
  }
  return [lo, hi];
}

/** Signed angle (radians) from `a` to `b` about `n` (all in the plane across `n`). */
export function signedAngle(a: V3, b: V3, n: V3): number {
  return Math.atan2(dot(cross(a, b), n), dot(a, b));
}

/** `p` projected onto the plane through `c` across unit `n`. */
export function projectToPlane(p: V3, c: V3, n: V3): V3 {
  return sub(p, mul(n, dot(sub(p, c), n)));
}
