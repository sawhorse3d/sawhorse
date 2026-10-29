// SketchUp curves (lane 25): openskp 1.4.0 says which edges make up one curve
// (`Edge.curveId`) and, for an arc or a circle, the exact circle behind it
// (`Definition.arcs`: centre, normal, x and y axes, start and end angles). This module turns
// one definition's curves into what the kernel takes (ADR-0013): a circle, an arc or a
// polyline, its points in order, its centre and normal when round, or a reason it stays
// plain edges. Nothing here guesses a curve from the edges' shape: only edges the file
// itself groups into a curve become one, and a record that does not match its edges (centre,
// radius or plane off, points off the circle, the wrong extent) is not trusted.
//
// All lengths here are inches, SketchUp's, in the definition's own frame.
import type { ArcCurve, Definition, Edge } from "openskp";
import type { CurveKind, V3 } from "../kernel/types";

const M_PER_IN = 0.0254;

/** The kernel's curve fit (crates/kernel/src/tolerance.rs: CURVE_FIT = 4 (POINT_MERGE +
 * LENGTH_EPS)), metres: how far a curve's point may sit off its circle. */
export const CURVE_FIT_M = 4 * (1e-5 + 1e-9);

/** One curve's edges in order. A closed chain does not repeat its first vertex. */
export interface Chain {
  verts: number[];
  edges: number[];
  closed: boolean;
}

/**
 * A curve's edges as one chain: connected, no vertex on more than two of them, open with
 * two ends or closed with none. `null` for anything else (a branch, two pieces, an edge from
 * a point to itself, a closed chain of fewer than three edges).
 */
export function chainOf(edges: Pick<Edge, "id" | "v1Id" | "v2Id">[]): Chain | null {
  if (!edges.length) return null;
  const adj = new Map<number, { e: number; o: number }[]>();
  const link = (v: number, e: number, o: number) => {
    const l = adj.get(v);
    if (l) l.push({ e, o });
    else adj.set(v, [{ e, o }]);
  };
  for (const e of edges) {
    if (e.v1Id === e.v2Id) return null;
    link(e.v1Id, e.id, e.v2Id);
    link(e.v2Id, e.id, e.v1Id);
  }
  const ends: number[] = [];
  for (const [v, l] of adj) {
    if (l.length > 2) return null;
    if (l.length === 1) ends.push(v);
  }
  if (ends.length !== 0 && ends.length !== 2) return null;
  const closed = ends.length === 0;
  const start = closed ? edges[0].v1Id : ends[0];
  const verts = [start];
  const order: number[] = [];
  const used = new Set<number>();
  let cur = start;
  for (;;) {
    const next = adj.get(cur)!.find((x) => !used.has(x.e));
    if (!next) break;
    used.add(next.e);
    order.push(next.e);
    cur = next.o;
    if (closed && cur === start) break;
    verts.push(cur);
  }
  if (used.size !== edges.length) return null;
  if (closed && verts.length < 3) return null;
  return { verts, edges: order, closed };
}

/** What a curve with an arc record is, or why it stays plain edges. */
export type ArcFit =
  | { ok: true; kind: "circle" | "arc"; verts: number[]; centre: V3; normal: V3; radius: number }
  /** One curve, but not one the kernel can keep round: an arc scaled unevenly before it was
   * saved (`ellipse`: SketchUp keeps it one curve on an ellipse; Sawhorse has no ellipse),
   * or an arc cut short in SketchUp (`cut`: an end, or a point another edge split a segment
   * at, lies on a chord, inside the circle, as the kernel's own curve rule allows; the
   * kernel's stroke and rim paths read a circle from all of a curve's points). */
  | { ok: true; kind: "polyline"; verts: number[]; ellipse?: true; cut?: true }
  | { ok: false; why: string };

/** Distance from `p` to the segment `a`-`b`. */
function toSegment(p: V3, a: V3, b: V3): number {
  const ab = sub(b, a);
  const l2 = dot(ab, ab);
  const t = l2 > 0 ? Math.max(0, Math.min(1, dot(sub(p, a), ab) / l2)) : 0;
  return len(sub(p, [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t]));
}

/**
 * How each point of a chain sits on its curve, by the kernel's rule for a round curve
 * (crates/kernel/src/curve.rs `check_round`): on it (`dev` within `tol`), else on the
 * segment joining its two neighbours (a point another edge split a segment at), else, at an
 * end of an open chain, inside the curve (an end cut on a chord). `null` when a point is none
 * of these. `dev(i)` is how far point i is off the curve (negative: inside).
 */
function sitting(pts: V3[], closed: boolean, dev: (i: number) => number, tol: number): ("on" | "cut")[] | null {
  const n = pts.length;
  const out: ("on" | "cut")[] = [];
  for (let i = 0; i < n; i++) {
    const d = dev(i);
    if (Math.abs(d) <= tol) {
      out.push("on");
      continue;
    }
    const interior = closed || (i > 0 && i + 1 < n);
    if (interior ? toSegment(pts[i], pts[(i + n - 1) % n], pts[(i + 1) % n]) <= tol : d <= tol) {
      out.push("cut");
      continue;
    }
    return null;
  }
  return out;
}

const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const cross = (a: V3, b: V3): V3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const len = (a: V3) => Math.hypot(a[0], a[1], a[2]);
const scale = (a: V3, k: number): V3 => [a[0] * k, a[1] * k, a[2] * k];
const finite = (a: readonly number[]) => a.every(Number.isFinite);
const TAU = 2 * Math.PI;

/**
 * Check an arc record against the chain it names and say what the chain is: a closed chain
 * on a full circle is a circle; an open chain on the record's circle, within its start and
 * end angles, is an arc, oriented from the record's start (counter-clockwise about its
 * normal); an open run of a full circle's edges is an arc too. Every point must lie on the
 * record's circle and in its plane within the kernel's curve fit (and rounding), else the
 * record is not trusted (`ok: false`): the edges come in plain, never as a bent circle.
 */
export function fitArc(arc: ArcCurve, chain: Chain, pos: (v: number) => V3 | undefined): ArcFit {
  const pts: V3[] = [];
  for (const v of chain.verts) {
    const p = pos(v);
    if (!p || !finite(p)) return { ok: false, why: "a point is missing" };
    pts.push(p);
  }
  const c = arc.center;
  const X = arc.xAxis;
  const Y = arc.yAxis;
  if (!finite(c) || !finite(X) || !finite(Y) || !finite(arc.normal)) return { ok: false, why: "the record is not finite" };
  const rx = len(X);
  const ry = len(Y);
  const nl = len(arc.normal);
  const tol = CURVE_FIT_M / M_PER_IN + 1e-9 * (len(c) + Math.max(rx, ry));
  if (!(rx > tol && ry > tol && nl > 0)) return { ok: false, why: "the record has no size" };
  const n = scale(arc.normal, 1 / nl);
  // The axes lie in the plane the normal says.
  if (Math.abs(dot(X, n)) > 1e-6 * rx || Math.abs(dot(Y, n)) > 1e-6 * ry) return { ok: false, why: "the record's axes leave its plane" };
  for (const p of pts) if (Math.abs(dot(sub(p, c), n)) > tol) return { ok: false, why: "a point is off the record's plane" };
  const round = Math.abs(rx - ry) <= 1e-6 * Math.max(rx, ry) && Math.abs(dot(X, Y)) <= 1e-6 * rx * ry;
  if (!round) {
    // p = c + a X + b Y with a^2 + b^2 = 1 (an affine image of a circle).
    const xx = dot(X, X);
    const xy = dot(X, Y);
    const yy = dot(Y, Y);
    const det = xx * yy - xy * xy;
    if (!(Math.abs(det) > 1e-12 * xx * yy)) return { ok: false, why: "the record's axes are parallel" };
    const ab = pts.map((p) => {
      const d = sub(p, c);
      const dx = dot(d, X);
      const dy = dot(d, Y);
      const a = (dx * yy - dy * xy) / det;
      const b = (dy * xx - dx * xy) / det;
      return { off: len(sub(d, [a * X[0] + b * Y[0], a * X[1] + b * Y[1], a * X[2] + b * Y[2]])), rho: Math.hypot(a, b) };
    });
    if (ab.some((x) => x.off > tol)) return { ok: false, why: "a point is off the record's ellipse" };
    if (!sitting(pts, chain.closed, (i) => (ab[i].rho - 1) * Math.min(rx, ry), tol)) return { ok: false, why: "a point is off the record's ellipse" };
    return { ok: true, kind: "polyline", verts: chain.verts, ellipse: true };
  }
  const r = rx;
  const how = sitting(pts, chain.closed, (i) => len(sub(pts[i], c)) - r, tol);
  if (!how) return { ok: false, why: "a point is off the record's circle" };
  const cut = how.includes("cut");
  const ux = scale(X, 1 / rx);
  const uy = cross(n, ux);
  const angle = (p: V3) => {
    const d = sub(p, c);
    return Math.atan2(dot(d, uy), dot(d, ux));
  };
  const out = (kind: "circle" | "arc", verts: number[]): ArcFit => (cut ? { ok: true, kind: "polyline", verts, cut: true } : { ok: true, kind, verts, centre: [c[0], c[1], c[2]], normal: n, radius: r });
  if (chain.closed) {
    if (!arc.isFullCircle) return { ok: false, why: "a closed chain with a partial arc record" };
    return out("circle", chain.verts);
  }
  const t = pts.map(angle);
  if (arc.isFullCircle) {
    // An open run of a circle's edges (the rest not in this curve): an arc, counter-clockwise.
    let turn = 0;
    for (let i = 0; i + 1 < t.length; i++) {
      let s = t[i + 1] - t[i];
      if (s > Math.PI) s -= TAU;
      if (s < -Math.PI) s += TAU;
      turn += s;
    }
    return out("arc", turn < 0 ? [...chain.verts].reverse() : chain.verts);
  }
  let sweep = arc.endAngle - arc.startAngle;
  if (!Number.isFinite(sweep)) return { ok: false, why: "the record's angles are not finite" };
  sweep = ((sweep % TAU) + TAU) % TAU;
  if (sweep === 0) sweep = TAU;
  const slack = tol / r;
  const u = t.map((a) => {
    let x = (((a - arc.startAngle) % TAU) + TAU) % TAU;
    if (x > TAU - slack) x -= TAU;
    return x;
  });
  if (u.some((x) => x < -slack || x > sweep + slack)) return { ok: false, why: "a point is outside the record's angles" };
  for (let i = 0; i + 1 < u.length; i++) if (Math.sign(u[i + 1] - u[i]) !== Math.sign(u[u.length - 1] - u[0])) return { ok: false, why: "the chain turns back" };
  return out("arc", u[0] > u[u.length - 1] ? [...chain.verts].reverse() : chain.verts);
}

/** One SketchUp curve as it comes in, or the reason it stays plain edges. */
export type SkpCurve =
  | { ok: true; id: number; kind: CurveKind; verts: number[]; edges: number[]; closed: boolean; centre?: V3; normal?: V3; ellipse?: boolean; cut?: boolean }
  | { ok: false; id: number; edges: number[]; why: "record" | "chain" | "short" };

/**
 * Every curve of a definition (edges sharing a `curveId`), in curve-id order: a circle or an
 * arc where an arc record matches its edges, a polyline where there is no record (a welded
 * curve, open or closed), the record is an ellipse or the arc was cut short on a chord, or a
 * reason it stays plain edges: the record disagrees with the edges, the edges do not make
 * one chain, or the curve is one open edge without a circle to keep (a polyline needs two).
 */
export function curvesOf(def: Pick<Definition, "edges" | "vertices"> & { arcs?: Map<number, ArcCurve> }): SkpCurve[] {
  const byCurve = new Map<number, Edge[]>();
  for (const e of def.edges) {
    const id = e.curveId;
    if (id === null || id === undefined) continue;
    const l = byCurve.get(id);
    if (l) l.push(e);
    else byCurve.set(id, [e]);
  }
  if (!byCurve.size) return [];
  const verts = new Map<number, V3>(def.vertices.map((v) => [v.id, [v.x, v.y, v.z]]));
  const out: SkpCurve[] = [];
  for (const [id, es] of [...byCurve].sort((a, b) => a[0] - b[0])) {
    const edges = es.map((e) => e.id);
    const chain = chainOf(es);
    if (!chain) {
      out.push({ ok: false, id, edges, why: "chain" });
      continue;
    }
    const arc = def.arcs?.get(id);
    if (arc) {
      const fit = fitArc(arc, chain, (v) => verts.get(v));
      if (!fit.ok) out.push({ ok: false, id, edges, why: "record" });
      else if (fit.kind === "polyline") {
        if (!chain.closed && chain.edges.length < 2) out.push({ ok: false, id, edges, why: "short" });
        else out.push({ ok: true, id, kind: "polyline", verts: fit.verts, edges: chain.edges, closed: chain.closed, ...(fit.ellipse ? { ellipse: true } : {}), ...(fit.cut ? { cut: true } : {}) });
      } else out.push({ ok: true, id, kind: fit.kind, verts: fit.verts, edges: chain.edges, closed: chain.closed, centre: fit.centre, normal: fit.normal });
      continue;
    }
    if (!chain.closed && chain.edges.length < 2) {
      out.push({ ok: false, id, edges, why: "short" });
      continue;
    }
    out.push({ ok: true, id, kind: "polyline", verts: chain.verts, edges: chain.edges, closed: chain.closed });
  }
  return out;
}

/** Faces of a definition joined by shared edges, as lists of face indices. */
export function faceComponents(def: Pick<Definition, "faces">): number[][] {
  const parent = def.faces.map((_, i) => i);
  const find = (i: number): number => {
    while (parent[i] !== i) {
      parent[i] = parent[parent[i]];
      i = parent[i];
    }
    return i;
  };
  const byEdge = new Map<number, number>();
  def.faces.forEach((f, i) => {
    for (const loop of f.loops)
      for (const ce of loop) {
        const j = byEdge.get(ce.edgeId);
        if (j === undefined) byEdge.set(ce.edgeId, i);
        else parent[find(i)] = find(j);
      }
  });
  const groups = new Map<number, number[]>();
  def.faces.forEach((_, i) => {
    const r = find(i);
    const l = groups.get(r);
    if (l) l.push(i);
    else groups.set(r, [i]);
  });
  return [...groups.values()];
}

/** The Newell normal of a ring (not normalised). */
export function newellOf(pts: V3[]): V3 {
  const n: V3 = [0, 0, 0];
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % pts.length];
    n[0] += (a[1] - b[1]) * (a[2] + b[2]);
    n[1] += (a[2] - b[2]) * (a[0] + b[0]);
    n[2] += (a[0] - b[0]) * (a[1] + b[1]);
  }
  return n;
}

/**
 * Whether points lie on one plane within `tol` (the plane of the largest of `rings`' Newell
 * normals, through that ring's first point). False when no ring has an area.
 */
export function flat(rings: V3[][], tol: number): boolean {
  let best: { n: V3; at: V3; size: number } | null = null;
  for (const r of rings) {
    if (r.length < 3) continue;
    const n = newellOf(r);
    const size = len(n);
    if (size > 0 && (!best || size > best.size)) best = { n: scale(n, 1 / size), at: r[0], size };
  }
  if (!best) return false;
  for (const r of rings) for (const p of r) if (Math.abs(dot(sub(p, best.at), best.n)) > tol) return false;
  return true;
}
