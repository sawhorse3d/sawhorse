// Shared pieces of the draw tools (Line, Rect, Circle, Polygon, Arc): which plane a stroke
// goes on, a frame aligned with the world on that plane, the kernel op, and the geometry of
// circles, polygons and arcs. Pure functions, unit-tested.
import type { KernelOp, SurfaceHit, V3 } from "../kernel/types";
import type { Frame } from "../lib/edit";
import type { Snap } from "./tool";

/** Where a stroke goes (crates/kernel model::DrawTarget). */
export type DrawTarget = { on: "sketch"; sketch: number } | { on: "plane"; origin: V3; normal: V3 } | { on: "face"; object: number; face: number };

export interface DrawPlane {
  frame: Frame;
  target: DrawTarget;
  /** "on the ground", "on face 3 of solid 1", "on a plane 21 1/2 in up". */
  where: (fmt: (m: number) => string) => string;
}

export const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
export const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
export const mul = (a: V3, s: number): V3 => [a[0] * s, a[1] * s, a[2] * s];
export const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
export const cross = (a: V3, b: V3): V3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
export const len = (a: V3) => Math.hypot(a[0], a[1], a[2]);
export const dist = (a: V3, b: V3) => len(sub(a, b));
export const unit = (a: V3): V3 => {
  const l = len(a);
  return l > 0 ? [a[0] / l, a[1] / l, a[2] / l] : [0, 0, 0];
};

/** A frame on the plane through `origin` with `normal`, aligned with the world: horizontal
 * planes get u = +X; other planes get a horizontal u and v pointing up. */
export function alignedFrame(origin: V3, normal: V3): Frame {
  const n = unit(normal);
  let u: V3;
  if (Math.abs(n[2]) > 0.999999) u = [1, 0, 0];
  else u = unit(cross([0, 0, 1], n));
  const v = cross(n, u);
  return { origin, normal: n, u, v };
}

/** World point to frame coordinates and back. */
export function local(f: Frame, p: V3): [number, number] {
  const d = sub(p, f.origin);
  return [dot(d, f.u), dot(d, f.v)];
}
export function world(f: Frame, x: number, y: number): V3 {
  return add(f.origin, add(mul(f.u, x), mul(f.v, y)));
}

export function onPlane(f: Frame, p: V3, tol = 1e-7): boolean {
  return Math.abs(dot(sub(p, f.origin), f.normal)) <= tol;
}

/** The plane of a surface hit, if the point lies on it. */
function surfacePlane(s: SurfaceHit, p: V3): DrawPlane | null {
  const f = s.frame;
  if (!onPlane(f, p)) return null;
  const frame = alignedFrame(f.origin, f.normal);
  if (s.at.kind === "face") {
    const { object, face } = s.at;
    return { frame, target: { on: "face", object, face }, where: () => `on face ${face} of solid ${object}` };
  }
  const { sketch } = s.at;
  const ground = sketch === 1;
  return { frame, target: { on: "sketch", sketch }, where: (fmt) => (ground ? "on the ground" : `on sketch ${sketch} (${fmt(f.origin[2])} up)`) };
}

/** A horizontal plane through `p` (the ground when p is on it). */
export function horizontalAt(p: V3): DrawPlane {
  const z = Math.abs(p[2]) < 1e-12 ? 0 : p[2];
  return {
    frame: alignedFrame([0, 0, z], [0, 0, 1]),
    target: { on: "plane", origin: [0, 0, z], normal: [0, 0, 1] },
    where: (fmt) => (z === 0 ? "on the ground" : `on a plane ${fmt(z)} up`),
  };
}

/** A plane through `p` with any normal (vertical planes for lines going up). */
export function planeAt(p: V3, normal: V3): DrawPlane {
  const n = unit(normal);
  if (Math.abs(n[2]) > 0.999999) return horizontalAt(p);
  return {
    frame: alignedFrame(p, n),
    target: { on: "plane", origin: p, normal: n },
    where: () => "on a vertical plane",
  };
}

/**
 * The plane a shape started at `snap` goes on: the face or region under the cursor when
 * the point lies on it, otherwise the horizontal plane through the point (the ground for
 * points on the ground). `prefer` (arrow keys before the first click) forces a horizontal
 * plane or a vertical one facing red or green.
 */
export function planeFromSnap(snap: Snap, prefer: "auto" | "z" | "x" | "y" = "auto"): DrawPlane {
  const p = snap.point;
  if (prefer === "x") return planeAt(p, [1, 0, 0]);
  if (prefer === "y") return planeAt(p, [0, 1, 0]);
  if (prefer === "auto") {
    // The surface under the cursor; on a solid's edge or corner the face beside it that
    // faces the eye (the ray may just miss the solid).
    for (const s of [snap.surface, snap.adjacent]) {
      const sp = s ? surfacePlane(s, p) : null;
      if (sp) return sp;
    }
  }
  return horizontalAt(p);
}

/**
 * The plane of a line segment that did not start on a surface: horizontal when level, else
 * the vertical plane containing it (a plumb segment takes the vertical plane facing the
 * camera, `facing` being the view direction).
 */
export function planeForSegment(a: V3, b: V3, facing: V3): DrawPlane {
  const d = sub(b, a);
  if (Math.abs(d[2]) <= 1e-9 * Math.max(1, len(d))) return horizontalAt(a);
  let n = cross(d, [0, 0, 1]);
  if (len(n) < 1e-9 * len(d)) {
    n = [facing[0], facing[1], 0];
    if (len(n) < 1e-9) n = [1, 0, 0];
  }
  return planeAt(a, n);
}

export function drawOp(target: DrawTarget, points: V3[], closed: boolean): KernelOp {
  return { op: "draw", target, points, closed } as KernelOp;
}

// ---- shapes ------------------------------------------------------------------------------

/** Rectangle corners from a to b (frame-local), counter-clockwise in the frame. */
export function rectangle(f: Frame, a: [number, number], b: [number, number]): V3[] {
  return [
    world(f, a[0], a[1]),
    world(f, b[0], a[1]),
    world(f, b[0], b[1]),
    world(f, a[0], b[1]),
  ];
}

/** A rotated rectangle: base edge p0 -> p1, width `w` toward the side of the frame's
 * normal x (p1 - p0) (negative goes the other way). */
export function rotatedRectangle(f: Frame, p0: V3, p1: V3, w: number): V3[] {
  const e = unit(sub(p1, p0));
  const side = unit(cross(f.normal, e));
  return [p0, p1, add(p1, mul(side, w)), add(p0, mul(side, w))];
}

/** Signed width of a rotated rectangle whose third point is `p` (distance from the base line). */
export function rotatedWidth(f: Frame, p0: V3, p1: V3, p: V3): number {
  const e = unit(sub(p1, p0));
  const side = unit(cross(f.normal, e));
  return dot(sub(p, p0), side);
}

/** A regular polygon (or a circle as one) of `n` sides around `centre` with radius `r`,
 * its first vertex toward `toward` (any point off the centre on the plane). */
export function regularPolygon(f: Frame, centre: V3, toward: V3, r: number, n: number): V3[] {
  let d = sub(toward, centre);
  d = sub(d, mul(f.normal, dot(d, f.normal)));
  const u = len(d) > 1e-12 ? unit(d) : f.u;
  const v = cross(f.normal, u);
  const out: V3[] = [];
  for (let k = 0; k < n; k++) {
    const a = (2 * Math.PI * k) / n;
    out.push(add(centre, add(mul(u, r * Math.cos(a)), mul(v, r * Math.sin(a)))));
  }
  return out;
}

export interface ArcGeom {
  centre: V3;
  radius: number;
  points: V3[];
}

/** A 2-point arc from p0 to p1 bulging by `bulge` (sagitta; sign = side, positive toward
 * normal x (p1 - p0)), as `segments` straight segments. A zero bulge is the chord. */
export function arcFromBulge(f: Frame, p0: V3, p1: V3, bulge: number, segments: number): ArcGeom | null {
  const c = dist(p0, p1);
  if (c < 1e-12 || Math.abs(bulge) < 1e-12) return null;
  const s = Math.abs(bulge);
  const r = (c * c) / (8 * s) + s / 2;
  const mid = mul(add(p0, p1), 0.5);
  const e = unit(sub(p1, p0));
  const side = mul(unit(cross(f.normal, e)), Math.sign(bulge));
  const centre = add(mid, mul(side, s - r)); // the apex is mid + side * s
  return { centre, radius: r, points: arcThrough(f, centre, p0, p1, add(mid, mul(side, s)), segments) };
}

/** Bulge (signed sagitta) for an arc of radius `r` over chord p0 -> p1 on the side of `toward`
 * (the minor arc), or null when the chord is longer than the diameter. */
export function bulgeForRadius(f: Frame, p0: V3, p1: V3, r: number, toward: V3): number | null {
  const c = dist(p0, p1);
  if (r < c / 2 - 1e-12) return null;
  const s = r - Math.sqrt(Math.max(0, r * r - (c * c) / 4));
  const e = unit(sub(p1, p0));
  const side = unit(cross(f.normal, e));
  return dot(sub(toward, mul(add(p0, p1), 0.5)), side) >= 0 ? s : -s;
}

/** Signed bulge from a cursor point (its distance from the chord, toward normal x chord). */
export function bulgeFromPoint(f: Frame, p0: V3, p1: V3, p: V3): number {
  const e = unit(sub(p1, p0));
  const side = unit(cross(f.normal, e));
  return dot(sub(p, mul(add(p0, p1), 0.5)), side);
}

/** Centre of the circle through three points on the frame's plane, or null when collinear. */
export function circleThrough(a: V3, b: V3, c: V3): V3 | null {
  const ab = sub(b, a);
  const ac = sub(c, a);
  const n = cross(ab, ac);
  const d = 2 * dot(n, n);
  if (d <= 1e-30) return null;
  return add(a, mul(add(mul(cross(n, ab), dot(ac, ac)), mul(cross(ac, n), dot(ab, ab))), 1 / d));
}

/** The arc from p0 to p1 around `centre` passing through (the side of) `via`, as points. */
export function arcThrough(f: Frame, centre: V3, p0: V3, p1: V3, via: V3, segments: number): V3[] {
  const r = dist(centre, p0);
  const u = unit(sub(p0, centre));
  const v = cross(f.normal, u);
  const angle = (p: V3) => {
    const d = sub(p, centre);
    let a = Math.atan2(dot(d, v), dot(d, u));
    if (a < 0) a += 2 * Math.PI;
    return a;
  };
  const a1 = angle(p1);
  const av = angle(via);
  // Counter-clockwise from p0 reaches via before p1, or the arc runs clockwise.
  const sweep = av <= a1 ? a1 : a1 - 2 * Math.PI;
  const n = Math.max(1, segments);
  const out: V3[] = [p0];
  for (let k = 1; k < n; k++) {
    const a = (sweep * k) / n;
    out.push(add(centre, add(mul(u, r * Math.cos(a)), mul(v, r * Math.sin(a)))));
  }
  out.push(p1);
  return out;
}
