// @vitest-environment node
// Lane 25: SketchUp curves (openskp 1.4.0's Edge.curveId and Definition.arcs) checked and
// turned into what the kernel takes: chains, records against their edges (both windings, an
// open run of a circle, an arc cut on a chord, an ellipse), and the records that are not
// trusted. Hand-built definitions: no SketchUp-authored bytes.
import { describe, expect, it } from "vitest";
import type { ArcCurve } from "openskp";
import { chainOf, curvesOf, faceComponents, fitArc, flat, CURVE_FIT_M, type Chain } from "./skpCurves";
import type { V3 } from "../kernel/types";

const TAU = 2 * Math.PI;

/** n points on a circle about `c` (z = c[2]), from angle a0 through `sweep`; open: n + 1. */
function ring(c: V3, r: number, n: number, a0 = 0, sweep = TAU, closed = true): V3[] {
  const m = closed ? n : n + 1;
  return Array.from({ length: m }, (_, k) => {
    const a = a0 + (sweep * k) / n;
    return [c[0] + r * Math.cos(a), c[1] + r * Math.sin(a), c[2]];
  });
}

/** A definition holding one chain of edges through `pts` with curve id 7 (and the record). */
function defOf(pts: V3[], closed: boolean, arc?: ArcCurve, curveIdOf: (k: number) => number | null = () => 7) {
  const vertices = pts.map((p, i) => ({ id: i + 1, x: p[0], y: p[1], z: p[2] }));
  const m = closed ? pts.length : pts.length - 1;
  const edges = Array.from({ length: m }, (_, k) => ({ id: 100 + k, v1Id: k + 1, v2Id: ((k + 1) % pts.length) + 1, soft: false, smooth: false, hidden: false, curveId: curveIdOf(k) }));
  return { vertices, edges, faces: [], arcs: new Map(arc ? [[7, arc]] : []) };
}

function record(c: V3, r: number, a0: number, a1: number, normal: V3 = [0, 0, 1], full = false): ArcCurve {
  // xAxis at angle a0 = 0 along +x; yAxis = normal x xAxis.
  const x: V3 = [r, 0, 0];
  const y: V3 = [normal[1] * x[2] - normal[2] * x[1], normal[2] * x[0] - normal[0] * x[2], normal[0] * x[1] - normal[1] * x[0]];
  return { center: c, normal, xAxis: x, yAxis: y, startAngle: a0, endAngle: a1, radius: r, isFullCircle: full };
}

describe("skp curves: chains", () => {
  it("orders an open and a closed chain, refuses branches and pieces", () => {
    const e = (id: number, a: number, b: number) => ({ id, v1Id: a, v2Id: b });
    expect(chainOf([e(1, 2, 3), e(2, 1, 2), e(3, 3, 4)])).toEqual({ verts: [1, 2, 3, 4], edges: [2, 1, 3], closed: false });
    expect(chainOf([e(1, 1, 2), e(2, 3, 2), e(3, 3, 1)])).toEqual({ verts: [1, 2, 3], edges: [1, 2, 3], closed: true });
    expect(chainOf([e(1, 1, 2), e(2, 1, 3), e(3, 1, 4)])).toBeNull(); // a branch
    expect(chainOf([e(1, 1, 2), e(2, 3, 4)])).toBeNull(); // two pieces
    expect(chainOf([e(1, 1, 2), e(2, 2, 1)])).toBeNull(); // a closed chain of two edges
    expect(chainOf([e(1, 1, 1)])).toBeNull();
    expect(chainOf([])).toBeNull();
  });

  it("joins faces by their shared edges and says when points are flat", () => {
    const f = (ids: number[]) => ({ loops: [ids.map((edgeId) => ({ edgeId, orientation: 1 }))] });
    expect(faceComponents({ faces: [f([1, 2, 3]), f([3, 4, 5]), f([6, 7, 8])] as never })).toEqual([[0, 1], [2]]);
    expect(flat([ring([0, 0, 0], 1, 8)], 1e-9)).toBe(true);
    const bent = ring([0, 0, 0], 1, 8);
    bent[3] = [bent[3][0], bent[3][1], 1e-6];
    expect(flat([bent], 1e-7)).toBe(false);
    expect(flat([[[0, 0, 0], [1, 0, 0], [2, 0, 0]]], 1e-7)).toBe(false); // no area: no plane
  });
});

describe("skp curves: records against their edges", () => {
  const pos = (pts: V3[]) => (v: number) => pts[v - 1];
  const chain = (n: number, closed: boolean): Chain => ({ verts: Array.from({ length: n }, (_, i) => i + 1), edges: [], closed });

  it("a full circle's closed chain is a circle about the record's centre", () => {
    const pts = ring([1, 2, 3], 6, 24);
    const fit = fitArc(record([1, 2, 3], 6, 0, TAU, [0, 0, 1], true), chain(24, true), pos(pts));
    expect(fit).toMatchObject({ ok: true, kind: "circle", centre: [1, 2, 3], radius: 6 });
  });

  it("an arc comes in oriented from the record's start, whichever way its edges run and whichever way its normal points", () => {
    // A quarter arc from +x to +y, counter-clockwise about +z.
    const pts = ring([0, 0, 0], 6, 6, 0, Math.PI / 2, false);
    const up = fitArc(record([0, 0, 0], 6, 0, Math.PI / 2), chain(7, false), pos(pts));
    expect(up).toMatchObject({ ok: true, kind: "arc", verts: [1, 2, 3, 4, 5, 6, 7] });
    // The same edges listed from the other end: still from the record's start.
    const rev = [...pts].reverse();
    expect(fitArc(record([0, 0, 0], 6, 0, Math.PI / 2), chain(7, false), pos(rev))).toMatchObject({ ok: true, kind: "arc", verts: [7, 6, 5, 4, 3, 2, 1] });
    // The same physical arc recorded about -z: counter-clockwise about -z is from +x to -y...
    // so a record about -z from 0 to pi/2 is the quarter from +x down to -y.
    const down = ring([0, 0, 0], 6, 6, 0, -Math.PI / 2, false);
    const fitDown = fitArc(record([0, 0, 0], 6, 0, Math.PI / 2, [0, 0, -1]), chain(7, false), pos(down));
    expect(fitDown).toMatchObject({ ok: true, kind: "arc", verts: [1, 2, 3, 4, 5, 6, 7], normal: [0, 0, -1] });
    // ...and the +x-to-+y quarter is outside that record's angles: not trusted.
    expect(fitArc(record([0, 0, 0], 6, 0, Math.PI / 2, [0, 0, -1]), chain(7, false), pos(pts))).toMatchObject({ ok: false });
  });

  it("an open run of a full circle's edges is an arc, counter-clockwise about the normal", () => {
    const pts = ring([0, 0, 0], 6, 12, 0, Math.PI, false).reverse();
    expect(fitArc(record([0, 0, 0], 6, 0, TAU, [0, 0, 1], true), chain(13, false), pos(pts))).toMatchObject({ ok: true, kind: "arc", verts: Array.from({ length: 13 }, (_, i) => 13 - i) });
  });

  it("a record whose centre, radius or plane is off, or whose extent is wrong, is not trusted", () => {
    const pts = ring([0, 0, 0], 6, 24);
    const off = CURVE_FIT_M / 0.0254;
    expect(fitArc(record([0.1, 0, 0], 6, 0, TAU, [0, 0, 1], true), chain(24, true), pos(pts))).toMatchObject({ ok: false, why: "a point is off the record's circle" });
    expect(fitArc(record([0, 0, 0], 6 + 3 * off, 0, TAU, [0, 0, 1], true), chain(24, true), pos(pts))).toMatchObject({ ok: false });
    expect(fitArc(record([0, 0, 0.1], 6, 0, TAU, [0, 0, 1], true), chain(24, true), pos(pts))).toMatchObject({ ok: false, why: "a point is off the record's plane" });
    // A closed chain with a partial record.
    expect(fitArc(record([0, 0, 0], 6, 0, Math.PI), chain(24, true), pos(pts))).toMatchObject({ ok: false });
    // Within the kernel's fit it is trusted.
    expect(fitArc(record([0, 0, 0], 6 + off / 2, 0, TAU, [0, 0, 1], true), chain(24, true), pos(pts))).toMatchObject({ ok: true, kind: "circle" });
  });

  it("an arc cut short on a chord is one curve, a polyline (the kernel reads a circle from every point)", () => {
    const pts = ring([0, 0, 0], 6, 6, 0, Math.PI / 2, false);
    // The last point moved back along its segment, half way: on the chord, inside the circle.
    pts[6] = [(pts[5][0] + pts[6][0]) / 2, (pts[5][1] + pts[6][1]) / 2, 0];
    expect(fitArc(record([0, 0, 0], 6, 0, Math.PI / 2), chain(7, false), pos(pts))).toMatchObject({ ok: true, kind: "polyline", cut: true });
    // An end outside the circle is not a chord: not trusted.
    pts[6] = [7, 7, 0];
    expect(fitArc(record([0, 0, 0], 6, 0, Math.PI / 2), chain(7, false), pos(pts))).toMatchObject({ ok: false });
  });

  it("an arc scaled unevenly (an ellipse) is one polyline", () => {
    const pts = ring([0, 0, 0], 1, 24).map(([x, y, z]): V3 => [3 * x, 2 * y, z]);
    const ellipse: ArcCurve = { center: [0, 0, 0], normal: [0, 0, 1], xAxis: [3, 0, 0], yAxis: [0, 2, 0], startAngle: 0, endAngle: TAU, radius: 3, isFullCircle: true };
    expect(fitArc(ellipse, chain(24, true), pos(pts))).toMatchObject({ ok: true, kind: "polyline", ellipse: true });
    // Points on a circle against an ellipse record: not trusted.
    expect(fitArc(ellipse, chain(24, true), pos(ring([0, 0, 0], 3, 24)))).toMatchObject({ ok: false });
  });
});

describe("skp curves: a definition's curves", () => {
  it("a circle, an arc, a welded polyline, a disagreeing record, a broken chain and a one-edge curve", () => {
    expect(curvesOf(defOf(ring([0, 0, 0], 6, 24), true, record([0, 0, 0], 6, 0, TAU, [0, 0, 1], true)))).toMatchObject([{ ok: true, kind: "circle", closed: true, centre: [0, 0, 0] }]);
    expect(curvesOf(defOf(ring([0, 0, 0], 6, 12, 0, Math.PI / 2, false), false, record([0, 0, 0], 6, 0, Math.PI / 2)))).toMatchObject([{ ok: true, kind: "arc", closed: false }]);
    expect(curvesOf(defOf(ring([0, 0, 0], 6, 6), true))).toMatchObject([{ ok: true, kind: "polyline", closed: true }]);
    expect(curvesOf(defOf(ring([0, 0, 0], 6, 6, 0, Math.PI, false), false))).toMatchObject([{ ok: true, kind: "polyline", closed: false }]);
    expect(curvesOf(defOf(ring([0, 0, 0], 6, 24), true, record([0.5, 0, 0], 6, 0, TAU, [0, 0, 1], true)))).toMatchObject([{ ok: false, why: "record" }]);
    expect(curvesOf(defOf([[0, 0, 0], [1, 0, 0]], false))).toMatchObject([{ ok: false, why: "short" }]);
    // Every other edge of a circle in the curve: pieces, not one chain.
    expect(curvesOf(defOf(ring([0, 0, 0], 6, 24), true, undefined, (k) => (k % 2 ? 7 : null)))).toMatchObject([{ ok: false, why: "chain" }]);
    // Edges with no curve id: no curves at all.
    expect(curvesOf(defOf(ring([0, 0, 0], 6, 24), true, undefined, () => null))).toEqual([]);
  });

  it("a chain where only some of a circle's edges carry its id is an arc on that circle", () => {
    const def = defOf(ring([0, 0, 0], 6, 24), true, record([0, 0, 0], 6, 0, TAU, [0, 0, 1], true), (k) => (k < 12 ? 7 : null));
    const [c] = curvesOf(def);
    expect(c).toMatchObject({ ok: true, kind: "arc", closed: false, centre: [0, 0, 0] });
    if (!c.ok) throw new Error();
    expect(c.verts).toHaveLength(13);
  });
});
