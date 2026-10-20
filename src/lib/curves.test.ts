// Lane 23 (#3): curves in the UI layer: an edge of a curve stands for the whole curve, a
// window takes a curve only whole, a crossing box when it touches it, the Eraser erases it
// whole, Move and Scale carry it as one (a lone point of it is refused), Offset offsets its
// chain, the status line says what it is in the document's units, Explode Curve.
import { describe, expect, it } from "vitest";
import type { EditResult } from "../kernel/client";
import type { CurveView, Entity, MeshData, Picked, V3 } from "../kernel/types";
import { EraseTool } from "../tools/erase";
import { ev, fakeHost } from "../tools/fakeHost";
import { MoveTool } from "../tools/move";
import { OffsetTool } from "../tools/offset";
import { RotateTool } from "../tools/rotate";
import { ScaleTool, scaleOp, scaledWords } from "../tools/scale";
import { SelectTool } from "../tools/select";
import { CURVE_POINT_REFUSAL, describeSubject, sketchOps, sketchParts, subjectOf } from "../tools/xform";
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
    // Lane 23b (verifier F1): a drag across the circle sweeps its face too. The face's
    // erase takes the circle (its ring), so the circle is not listed again; a region the
    // circle is only part of the boundary of keeps both ops (the kernel's batch tolerates
    // the curve already gone).
    const m = mesh();
    const face = (ringPts: V3[], region: number): MeshData => {
      const tris = ringPts.slice(1, -1).map((_, k) => [0, k + 1, k + 2]).flat();
      return { ...m, positions: new Float32Array(ringPts.flat()), indices: new Uint32Array(tris), faceIds: new Uint32Array(tris.length / 3), parts: [{ kind: "region", owner: 1, index: region, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] };
    };
    const disc: Entity = { kind: "region", sketch: 1, region: 4 };
    expect(eraseOps([circle, disc], null, face(ring, 4)).ops).toEqual([{ op: "erase_region", sketch: 1, region: 4 }]);
    expect(eraseOps([disc, circle], null, face(ring, 4)).ops).toEqual([{ op: "erase_region", sketch: 1, region: 4 }]);
    // Without the mesh, or when the face is not the circle's own (a half disc), both go.
    expect(eraseOps([circle, disc], null).ops).toHaveLength(2);
    const half: V3[] = [...ring.slice(0, N / 2 + 1)];
    expect(eraseOps([circle, disc], null, face(half, 4)).ops.map((o) => o.op)).toEqual(["erase_region", "erase_curve"]);
    // Another sketch's region of the same shape is not this circle's face.
    expect(eraseOps([circle, { kind: "region", sketch: 2, region: 4 }], null, { ...face(ring, 4), parts: [{ ...face(ring, 4).parts[0], owner: 2 }] }).ops).toHaveLength(2);
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

  it("Rotate and Scale refuse a lone point of a curve before the gesture starts (lane 23b)", async () => {
    const tick = () => new Promise((r) => setTimeout(r, 0));
    const state = { sketches: [{ id: 1, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] } as never;
    const point: Entity = { kind: "vertex", owner: sketch1, at: ring[1] };
    // Rotate: the first click (the protractor's centre) is refused; no gesture, no op.
    const h = fakeHost({ _mesh: mesh(), _state: state, sel: [point] });
    const r = new RotateTool(h);
    r.onEvent(ev("click", [1, 1, 0]));
    await tick();
    expect(h.statuses.at(-1)).toEqual({ text: CURVE_POINT_REFUSAL, error: true });
    expect(r.busy()).toBe(false);
    r.onEvent(ev("click", [2, 1, 0]));
    r.onEvent(ev("click", [2, 2, 0]));
    await tick();
    expect(h.ops).toEqual([]);
    // Scale: no grips, and the status says why when the tool starts and on a press.
    const k = fakeHost({ _mesh: mesh(), _state: state, sel: [point] });
    const s = new ScaleTool(k);
    s.activate();
    await tick();
    expect(k.statuses.at(-1)).toEqual({ text: CURVE_POINT_REFUSAL, error: true });
    expect(s.view().labels.filter((l) => l.className?.startsWith("grip"))).toEqual([]);
    s.onEvent(ev("down", ring[1]));
    expect(k.statuses.at(-1)).toEqual({ text: CURVE_POINT_REFUSAL, error: true });
    s.onValue("2");
    await tick();
    expect(k.ops).toEqual([]);
    // The whole circle selected: grips, no refusal.
    const w = fakeHost({ _mesh: mesh(), _state: state, sel: [circle], outline: async () => [ring] });
    const s2 = new ScaleTool(w);
    s2.activate();
    await tick();
    expect(s2.view().labels.filter((l) => l.className === "grip").length).toBeGreaterThan(0);
    expect(w.statuses.some((x) => x.error)).toBe(false);
  });

  it("Scale says what the curve is now: its new radius, or out of round (lane 23b)", () => {
    const outlines = new Map<string, V3[][]>([["c1:1", [ring]]]);
    const state = { sketches: [{ id: 1, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] } as never;
    const s = subjectOf([circle], null, outlines, state);
    if (typeof s === "string") throw new Error(s);
    const before = mesh();
    const mm = (x: number) => formatLength(x, "mm");
    const after = (v: Partial<CurveView>): EditResult => ({ outcome: { kind: "sketch_moved", sketch: 1, regions: [], curves: [[1, 1]] }, scene: { mesh: { ...before, curves: [{ ...before.curves![0], ...v }, before.curves![1]] }, state } }) as unknown as EditResult;
    expect(scaledWords(s, "a circle", [2, 2, 1], false, before, after({ radius: 0.6 }), mm, "mm")).toBe("Scaled a circle ×2: radius 600 mm.");
    expect(scaledWords(s, "a circle", [0.5, 0.5, 1], true, before, after({ radius: 0.15 }), mm, "mm")).toBe("Scaled a circle ×0.5: radius 150 mm.");
    expect(scaledWords(s, "a circle", [2, 1, 1], false, before, after({ kind: "polyline", radius: undefined, segments: 8 }), mm, "mm")).toBe("Scaled a circle ×2 along red: out of round, it is a curve of 8 segments now.");
    // Anything else says the factors, as before.
    expect(scaledWords({ kind: "objects", objects: [7] }, "solid 7", [2, 2, 2], true, before, null, mm, "mm")).toBe("Scaled solid 7 ×2 uniform about its centre.");
  });

  it("a curve and its face are one shape in the status line: 1, 2 and 200 circles (lane 23b)", () => {
    // n circles in sketch 1, each with its disc: curves 1..n, regions 101..100+n.
    const many = (n: number) => {
      const disc = (k: number): V3[] => ring.map(([x, y, z]) => [x + 2 * k, y, z] as V3);
      const curves: CurveView[] = Array.from({ length: n }, (_, k) => ({ owner: sketch1, curve: k + 1, kind: "circle" as const, closed: true, segments: N, centre: [2 * k, 0, 0] as V3, radius: R, normal: [0, 0, 1] as V3 }));
      const m = { ...mesh(), edges: new Float32Array(0), edgeOwner: new Uint32Array(0), edgeCurve: new Uint32Array(0), curves } as MeshData;
      const outlines = new Map<string, V3[][]>();
      const sel: Entity[] = [];
      for (let k = 0; k < n; k++) {
        outlines.set(`c1:${k + 1}`, [disc(k)]);
        // The region's ring starts elsewhere round the circle and runs the other way.
        outlines.set(`r1:${101 + k}`, [[...disc(k)].reverse()]);
        sel.push({ kind: "curve", owner: sketch1, curve: k + 1 }, { kind: "region", sketch: 1, region: 101 + k });
      }
      return { m, outlines, sel };
    };
    const state = { sketches: [{ id: 1, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] } as never;
    for (const [n, said] of [
      [1, "a circle"],
      [2, "2 circles"],
      [200, "200 circles"],
    ] as const) {
      const { m, outlines, sel } = many(n);
      const s = subjectOf(sel, null, outlines, state);
      if (typeof s === "string") throw new Error(s);
      expect(s.sketches![0].faced).toHaveLength(n);
      expect(describeSubject(s, m)).toBe(said);
    }
    // 200 closed polylines with their faces: "200 curves", not "201 items".
    const { m, outlines, sel } = many(200);
    const poly = { ...m, curves: m.curves!.map((c) => ({ ...c, kind: "polyline" as const, radius: undefined })) };
    const s = subjectOf(sel, null, outlines, state);
    if (typeof s === "string") throw new Error(s);
    expect(describeSubject(s, poly)).toBe("200 curves");
    // A face that is not a selected curve's keeps its own name; more than three names count
    // every shape: 2 circles, a rectangle's face, 3 lines, 1 point and a solid are 8 items.
    const two = many(2);
    const rect: V3[] = [[10, 0, 0], [11, 0, 0], [11, 1, 0], [10, 1, 0]];
    two.outlines.set("r1:7", [rect]);
    const lines: Entity[] = [0, 1, 2].map((k) => ({ kind: "edge", owner: sketch1, a: [20 + k, 0, 0], b: [20 + k, 1, 0], drawn: true }));
    const s2 = subjectOf([...two.sel, { kind: "region", sketch: 1, region: 7 }], null, two.outlines, state);
    if (typeof s2 === "string") throw new Error(s2);
    expect(describeSubject(s2, two.m)).toBe("a region and 2 circles");
    const s3 = subjectOf([...two.sel, { kind: "region", sketch: 1, region: 7 }, ...lines, { kind: "vertex", owner: sketch1, at: [30, 0, 0] }, { kind: "object", object: 7 }], null, two.outlines, state);
    if (typeof s3 === "string") throw new Error(s3);
    expect(describeSubject(s3, two.m)).toBe("8 items");
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
