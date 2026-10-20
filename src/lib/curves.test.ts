// Lane 23 (#3): curves in the UI layer: an edge of a curve stands for the whole curve, a
// window takes a curve only whole, a crossing box when it touches it, the Eraser erases it
// whole, Move and Scale carry it as one (a lone point of it is refused), Offset offsets its
// chain, the status line says what it is in the document's units, Explode Curve.
import { describe, expect, it } from "vitest";
import type { CurveView, Entity, MeshData, Picked, V3 } from "../kernel/types";
import { EraseTool } from "../tools/erase";
import { ev, fakeHost } from "../tools/fakeHost";
import { MoveTool } from "../tools/move";
import { OffsetTool } from "../tools/offset";
import { scaleOp } from "../tools/scale";
import { SelectTool } from "../tools/select";
import { CURVE_POINT_REFUSAL, sketchOps, sketchParts, subjectOf } from "../tools/xform";
import { curveOf, curvePhrase, curveSegments, describeCurve, explodeOps, onCurve, segmentPoints } from "./curves";
import { boxSelect, entityKey, eraseOp, eraseOps, objectOf, selectionHighlight } from "./entities";
import { formatLength } from "./units";

const N = 8;
const R = 0.3;
const ring: V3[] = Array.from({ length: N }, (_, k) => [R * Math.cos((2 * Math.PI * k) / N), R * Math.sin((2 * Math.PI * k) / N), 0]);

/** A view mesh with a sketch (id 1) holding an 8-segment circle (curve 1) and a line, and a
 * solid (id 7) with one rim of 4 segments (curve 2). */
function mesh(): MeshData {
  const segs: [V3, V3, number, number][] = [];
  for (let k = 0; k < N; k++) segs.push([ring[k], ring[(k + 1) % N], 1, 1]);
  segs.push([[1, 0, 0], [2, 0, 0], 1, 0]);
  const sq: V3[] = [
    [5, 0, 1],
    [6, 0, 1],
    [6, 1, 1],
    [5, 1, 1],
  ];
  for (let k = 0; k < 4; k++) segs.push([sq[k], sq[(k + 1) % 4], 0, 2]);
  const edges = new Float32Array(segs.flatMap(([a, b]) => [...a, ...b]));
  const curves: CurveView[] = [
    { owner: { kind: "sketch", id: 1 }, curve: 1, kind: "circle", closed: true, segments: N, centre: [0, 0, 0], radius: R, normal: [0, 0, 1] },
    { owner: { kind: "object", id: 7 }, curve: 2, kind: "polygon", closed: true, segments: 4, centre: [5.5, 0.5, 1], radius: Math.SQRT1_2, normal: [0, 0, 1] },
  ];
  return {
    positions: new Float32Array(0),
    indices: new Uint32Array(0),
    faceIds: new Uint32Array(0),
    edges,
    edgeOwner: new Uint32Array(segs.map(([, , o]) => (o === 1 ? 1 : 0))),
    edgeOwners: [
      { kind: "object", id: 7 },
      { kind: "sketch", id: 1 },
    ],
    parts: [],
    edgeCurve: new Uint32Array(segs.map(([, , , c]) => c)),
    curves,
  };
}

const sketch1 = { kind: "sketch" as const, id: 1 };
const curveEdge = (k: number): Entity => ({ kind: "edge", owner: sketch1, a: ring[k], b: ring[(k + 1) % N], drawn: true, curve: 1 });
const lineEdge: Entity = { kind: "edge", owner: sketch1, a: [1, 0, 0], b: [2, 0, 0], drawn: true };
const circle: Entity = { kind: "curve", owner: sketch1, curve: 1 };
const rim: Entity = { kind: "curve", owner: { kind: "object", id: 7 }, curve: 2 };
const pickOf = (e: Entity, point: V3 = [0.5, 0.5, 0]): Picked => ({ entity: e, point, surface: null });

describe("curves in the view", () => {
  it("an edge of a curve stands for the curve; others stay edges", () => {
    const m = mesh();
    expect(curveOf(curveEdge(3), m)).toEqual(circle);
    expect(curveOf(lineEdge, m)).toEqual(lineEdge);
    expect(curveOf({ ...lineEdge, curve: 99 } as Entity, m)).toEqual({ ...lineEdge, curve: 99 });
    expect(curveOf(null, m)).toBeNull();
    expect(curveSegments(m, sketch1, 1)).toHaveLength(N);
    expect(segmentPoints(curveSegments(m, sketch1, 1))).toHaveLength(N);
    expect(curveSegments(m, { kind: "object", id: 7 }, 2)).toHaveLength(4);
    expect(onCurve(m, sketch1, ring[2])).toBe(true);
    expect(onCurve(m, sketch1, [1, 0, 0])).toBe(false);
  });

  it("names, keys and owners", () => {
    expect(entityKey(circle)).toBe("c:sketch1:1");
    expect(entityKey(rim)).toBe("c:object7:2");
    expect(objectOf(circle)).toBeNull();
    expect(objectOf(rim)).toBe(7);
  });

  it("says what a curve is, in the document's units", () => {
    const m = mesh();
    const mm = (x: number) => formatLength(x, "mm");
    expect(describeCurve(circle as Extract<Entity, { kind: "curve" }>, m, mm, "mm")).toBe("Selected a circle (8 segments, radius 300 mm). Delete erases it; Edit > Explode Curve makes its edges plain.");
    expect(describeCurve(rim as Extract<Entity, { kind: "curve" }>, m, mm, "mm")).toMatch(/^Selected a polygon \(4 sides, radius ~ 707\.1 mm\), a rim of a solid\./);
    const inch = (x: number) => formatLength(x, "frac-in");
    expect(curvePhrase({ ...m.curves![0], kind: "arc", closed: false, sweep: Math.PI / 2, segments: 12, radius: 0.0254 * 1.5 }, inch, "in")).toBe("an arc (12 segments, radius 1 1/2 in, 90 degrees)");
    expect(curvePhrase({ owner: sketch1, curve: 3, kind: "polyline", closed: false, segments: 23, normal: [0, 0, 1] }, inch, "in")).toBe("a curve (23 segments)");
    expect(curvePhrase({ ...m.curves![0], radius: 0.3048 }, (x) => formatLength(x, "ft-in"), "ft-in")).toBe("a circle (8 segments, radius 1' 0\")");
  });

  it("highlights every segment of a selected curve", () => {
    const h = selectionHighlight([circle], mesh());
    expect(h.selectedEdges).toHaveLength(N);
  });

  it("a window takes a curve only whole; a crossing box when it touches any segment", () => {
    const m = mesh();
    const toScreen = (p: V3): [number, number] => [p[0] * 100, -p[1] * 100];
    const curvesIn = (es: Entity[]) => es.filter((e) => e.kind === "curve");
    // Half the circle inside a window: not taken, and no lone segment either.
    const half = boxSelect(m, toScreen, { x0: -40, y0: -40, x1: 0, y1: 40 }, false);
    expect(curvesIn(half)).toEqual([]);
    expect(half.filter((e) => e.kind === "edge")).toEqual([]);
    // All of it: taken once.
    expect(curvesIn(boxSelect(m, toScreen, { x0: -40, y0: -40, x1: 40, y1: 40 }, false))).toEqual([circle]);
    // A crossing box over one segment: taken.
    expect(curvesIn(boxSelect(m, toScreen, { x0: 25, y0: -5, x1: 35, y1: 5 }, true))).toEqual([circle]);
    // The line is still its own edge.
    expect(boxSelect(m, toScreen, { x0: 90, y0: -5, x1: 210, y1: 5 }, false)).toEqual([{ kind: "edge", owner: sketch1, a: [1, 0, 0], b: [2, 0, 0], drawn: true }]);
  });

  it("erases a sketch's curve whole; a solid's rim is refused", () => {
    expect(eraseOp(circle, null)).toEqual({ op: "erase_curve", sketch: 1, curve: 1 });
    expect(eraseOp(rim, null)).toMatch(/rim of a solid/);
    expect(eraseOps([lineEdge, circle], null).ops[0]).toEqual({ op: "erase_curve", sketch: 1, curve: 1 });
    expect(explodeOps([circle, rim, lineEdge])).toEqual([
      { op: "explode_curve", sketch: 1, curve: 1 },
      { op: "explode_curve", object: 7, curve: 2 },
    ]);
  });
});

describe("tools with curves", () => {
  it("Select: a segment selects and hovers the whole curve; a point stays a point", () => {
    const h = fakeHost({ _mesh: mesh(), _units: "mm" });
    const t = new SelectTool(h);
    t.onEvent(ev("move", [0.3, 0, 0], { picked: pickOf(curveEdge(0)) }));
    expect(h.hovered).toEqual(circle);
    t.onEvent(ev("click", [0.3, 0, 0], { picked: pickOf(curveEdge(0)) }));
    expect(h.sel).toEqual([circle]);
    expect(h.statuses.at(-1)?.text).toBe("Selected a circle (8 segments, radius 300 mm). Delete erases it; Edit > Explode Curve makes its edges plain.");
    // Shift adds the line, Ctrl on another segment toggles the curve off.
    t.onEvent(ev("click", [1.5, 0, 0], { picked: pickOf(lineEdge), shift: true }));
    expect(h.sel).toEqual([circle, lineEdge]);
    t.onEvent(ev("click", [0, 0.3, 0], { picked: pickOf(curveEdge(2)), ctrl: true }));
    expect(h.sel).toEqual([lineEdge]);
    const point: Entity = { kind: "vertex", owner: sketch1, at: ring[1] };
    t.onEvent(ev("click", ring[1], { picked: pickOf(point, ring[1]) }));
    expect(h.sel).toEqual([point]);
  });

  it("Eraser: a click or a drag over segments erases the curve once", async () => {
    const state = { sketches: [{ id: 1, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] } as never;
    const h = fakeHost({ _mesh: mesh(), _state: state, outcome: { kind: "sketch_edited", regions: [] } });
    const t = new EraseTool(h);
    t.onEvent(ev("click", [0.3, 0, 0], { picked: pickOf(curveEdge(0)) }));
    await Promise.resolve();
    expect(h.ops).toEqual([{ op: "erase_curve", sketch: 1, curve: 1 }]);
    h.ops = [];
    t.onEvent(ev("drag", [0.3, 0, 0], { picked: pickOf(curveEdge(0)) }));
    t.onEvent(ev("drag", [0, 0.3, 0], { picked: pickOf(curveEdge(2)) }));
    t.onEvent(ev("drag", [1.5, 0, 0], { picked: pickOf(lineEdge) }));
    t.onEvent(ev("dragend", [1.5, 0, 0], { picked: null }));
    await Promise.resolve();
    expect(h.ops).toHaveLength(1);
    const op = h.ops[0];
    expect(op.op).toBe("batch");
    if (op.op === "batch") {
      expect(op.ops.filter((o) => o.op === "erase_curve")).toHaveLength(1);
      expect(op.ops.filter((o) => o.op === "erase_segment")).toHaveLength(1);
    }
  });

  it("Eraser with Ctrl softens every segment of a solid's rim", async () => {
    const h = fakeHost({ _mesh: mesh(), outcome: { kind: "edges_set", changed: 4 } });
    const t = new EraseTool(h);
    const rimEdge: Entity = { kind: "edge", owner: { kind: "object", id: 7 }, a: [5, 0, 1], b: [6, 0, 1], drawn: false, curve: 2 };
    t.onEvent(ev("click", [5.5, 0, 1], { picked: pickOf(rimEdge), ctrl: true }));
    await Promise.resolve();
    const op = h.ops[0];
    expect(op.op).toBe("set_edge_flags");
    if (op.op === "set_edge_flags") expect(op.edges).toHaveLength(4);
  });

  it("Move carries a selected curve as one op; a lone point of it is refused", async () => {
    const outlines = new Map<string, V3[][]>([["c1:1", [ring]]]);
    const state = { sketches: [{ id: 1, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] } as never;
    const parts = sketchParts([circle], outlines, state);
    expect(parts[0].curves).toEqual([1]);
    expect(parts[0].moving).toHaveLength(N);
    const I = { linear: [[1, 0, 0], [0, 1, 0], [0, 0, 1]] as [V3, V3, V3], translation: [1, 0, 0] as V3 };
    expect(sketchOps(parts, I, false)).toEqual([{ op: "transform_sketch", sketch: 1, curves: [1], xform: I }]);
    expect(sketchOps(parts, I, true)).toEqual([{ op: "transform_sketch", sketch: 1, curves: [1], xform: I, copy: true }]);
    const s = subjectOf([circle], null, outlines, state);
    expect(typeof s).not.toBe("string");
    // The Move tool on a lone point of the circle: refused before anything moves.
    const h = fakeHost({ _mesh: mesh(), _state: state, sel: [{ kind: "vertex", owner: sketch1, at: ring[1] }] });
    const t = new MoveTool(h);
    t.onEvent(ev("down", ring[1], { snapKind: "endpoint" }));
    await new Promise((r) => setTimeout(r, 0));
    expect(h.statuses.at(-1)).toEqual({ text: CURVE_POINT_REFUSAL, error: true });
    expect(h.ops).toEqual([]);
    // A solid's rim moves its points (part of the solid).
    const g = subjectOf([rim], null, new Map([["k7:2", [[[5, 0, 1], [6, 0, 1], [6, 1, 1], [5, 1, 1]] as V3[]]]]), state);
    expect(typeof g === "object" && g.kind === "geometry" ? g.points.length : 0).toBe(4);
  });

  it("Scale scales a selected curve as one transform_sketch op (the kernel keeps it a circle)", () => {
    const outlines = new Map<string, V3[][]>([["c1:1", [ring]]]);
    const state = { sketches: [{ id: 1, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] } as never;
    const s = subjectOf([circle], null, outlines, state);
    if (typeof s === "string") throw new Error(s);
    const op = scaleOp(s, [0, 0, 0], [2, 2, 1]) as { op: string; curves?: number[]; xform: { linear: number[][] } };
    expect(op.op).toBe("transform_sketch");
    expect(op.curves).toEqual([1]);
    expect(op.xform.linear[0][0]).toBe(2);
  });

  it("Offset offsets a selected curve's chain (the kernel makes it a curve of its kind)", async () => {
    const tick = () => new Promise((r) => setTimeout(r, 0));
    const h = fakeHost({ _mesh: mesh(), outcome: { kind: "drawn", sketch: 1, regions: [], faces: [], inside: null, curve: 2 } });
    h._state = { sketches: [{ id: 1, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] } as never;
    h.sel = [circle];
    const t = new OffsetTool(h);
    const at = (type: "down" | "click" | "move", p: V3) => ev(type, p, { ray: { origin: [p[0], p[1], 10], dir: [0, 0, -1] } });
    t.onEvent(at("down", [0.3, 0, 0]));
    await tick();
    t.onEvent(at("click", [0.3, 0, 0]));
    t.onEvent(at("move", [0.35, 0, 0]));
    t.onEvent(at("click", [0.35, 0, 0]));
    await tick();
    const op = h.ops.at(-1) as unknown as { op: string; source: { of: string; closed: boolean; points: V3[] } };
    expect(op.op).toBe("offset");
    expect(op.source.of).toBe("edges");
    expect(op.source.closed).toBe(true);
    expect(op.source.points).toHaveLength(N);
  });
});
