// Curves (lane 23, ADR-0013): a circle, an arc or a polygon drawn by its tool is one curve,
// as in SketchUp. The kernel keeps them (crates/kernel/src/curve.rs) and the view mesh names
// each edge segment's curve (`edgeCurve`, an index into `curves`). This module answers the
// questions the tools ask: which curve an edge belongs to, its segments and points, and how
// to say it in the status line. Pure (no DOM, no three.js), unit-tested in curves.test.ts.
import type { CurveView, Entity, KernelOp, MeshData, Owner, V3 } from "../kernel/types";
import { meshIndex } from "./meshIndex";
import { solidName } from "./names";

/** The key a curve is found by in the mesh index. */
export function curveKey(owner: Owner | CurveView["owner"], curve: number): string {
  return `${owner.kind}${owner.id}:${curve}`;
}

/** The curve's view in the mesh, and its index there (null when it is not in view). */
export function curveView(mesh: MeshData | null, owner: Owner | CurveView["owner"], curve: number): { view: CurveView; index: number } | null {
  if (!mesh?.curves?.length) return null;
  const k = meshIndex(mesh).curveAt.get(curveKey(owner, curve));
  return k === undefined ? null : { view: mesh.curves[k], index: k };
}

/** The curve's edge segments in the view, in mesh order. */
export function curveSegments(mesh: MeshData | null, owner: Owner | CurveView["owner"], curve: number): [V3, V3][] {
  const c = curveView(mesh, owner, curve);
  if (!c || !mesh) return [];
  const ix = meshIndex(mesh);
  const e = mesh.edges;
  const out: [V3, V3][] = [];
  for (let j = ix.curveStart[c.index]; j < ix.curveStart[c.index + 1]; j++) {
    const i = ix.curveEdges[j];
    out.push([
      [e[6 * i], e[6 * i + 1], e[6 * i + 2]],
      [e[6 * i + 3], e[6 * i + 4], e[6 * i + 5]],
    ]);
  }
  return out;
}

const near = (p: V3, q: V3) => Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]) <= 2e-6 + 4e-7 * Math.max(Math.abs(q[0]), Math.abs(q[1]), Math.abs(q[2]));

/** The points of a set of segments, each once (the view mesh is single precision, so ends
 * match within a few micrometres). */
export function segmentPoints(segs: [V3, V3][]): V3[] {
  const out: V3[] = [];
  for (const [a, b] of segs) {
    if (!out.some((q) => near(q, a))) out.push(a);
    if (!out.some((q) => near(q, b))) out.push(b);
  }
  return out;
}

/** The whole curve an edge is a segment of (a sketch's curve, or a solid's rim), or the
 * entity itself when it is not an edge of a curve in view. */
export function curveOf(e: Entity | null, mesh: MeshData | null): Entity | null {
  if (!e || e.kind !== "edge" || e.curve === undefined || e.owner.kind === "guide") return e;
  return curveView(mesh, e.owner, e.curve) ? { kind: "curve", owner: e.owner, curve: e.curve } : e;
}

/** A point is a point of a curve in view (of its owner): the vertex ends one of the curve's
 * segments. */
export function onCurve(mesh: MeshData | null, owner: Owner, at: V3): boolean {
  if (!mesh?.edgeCurve || !mesh.curves?.length) return false;
  const ix = meshIndex(mesh);
  const k = mesh.edgeOwners.findIndex((o) => o && o.kind === owner.kind && o.id === owner.id);
  if (k < 0 || k + 1 >= ix.ownerStart.length) return false;
  const e = mesh.edges;
  for (let j = ix.ownerStart[k]; j < ix.ownerStart[k + 1]; j++) {
    const i = ix.ownerEdges[j];
    if (!mesh.edgeCurve[i]) continue;
    const a: V3 = [e[6 * i], e[6 * i + 1], e[6 * i + 2]];
    const b: V3 = [e[6 * i + 3], e[6 * i + 4], e[6 * i + 5]];
    if (near(a, at) || near(b, at)) return true;
  }
  return false;
}

/** A length with its unit ("300 mm", "1 1/2 in", "1' 0\"": ft-in carries its own marks). */
export function lengthWithUnit(m: number, fmt: (m: number) => string, unit: string): string {
  return unit === "ft-in" ? fmt(m) : `${fmt(m)} ${unit}`;
}

/** "a circle (24 segments, radius 300 mm)", "an arc (12 segments, radius 1 in, 90 degrees)",
 * "a polygon (6 sides, radius 2 in)", "a curve (23 segments)". */
export function curvePhrase(v: CurveView, fmt: (m: number) => string, unit: string): string {
  const r = v.radius !== undefined ? `, radius ${lengthWithUnit(v.radius, fmt, unit)}` : "";
  const n = v.segments;
  const segs = `${n} segment${n === 1 ? "" : "s"}`;
  switch (v.kind) {
    case "circle":
      return `a circle (${segs}${r})`;
    case "polygon":
      return `a polygon (${n} side${n === 1 ? "" : "s"}${r})`;
    case "arc": {
      const deg = v.sweep !== undefined ? `, ${Math.round(Math.abs((v.sweep * 180) / Math.PI) * 100) / 100} degrees` : "";
      return `an arc (${segs}${r}${deg})`;
    }
    default:
      return `a curve (${segs}${v.closed ? ", closed" : ""})`;
  }
}

/** What the status line says of a selected curve. */
export function describeCurve(e: Extract<Entity, { kind: "curve" }>, mesh: MeshData | null, fmt: (m: number) => string, unit: string): string {
  const c = curveView(mesh, e.owner, e.curve);
  if (!c) return "Selected a curve.";
  const what = curvePhrase(c.view, fmt, unit);
  if (e.owner.kind === "object") return `Selected ${what}, a rim of ${solidName(e.owner.id)}. Edit > Explode Curve makes its edges plain.`;
  return `Selected ${what}. Delete erases it; Edit > Explode Curve makes its edges plain.`;
}

/** The ops that explode the curves of a selection (one undo step as a batch). */
export function explodeOps(sel: Entity[]): KernelOp[] {
  const out: KernelOp[] = [];
  for (const e of sel) {
    if (e.kind !== "curve") continue;
    if (e.owner.kind === "object") out.push({ op: "explode_curve", object: e.owner.id, curve: e.curve });
    else if (e.owner.kind === "sketch") out.push({ op: "explode_curve", sketch: e.owner.id, curve: e.curve });
  }
  return out;
}
