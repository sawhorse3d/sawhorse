import { describe, expect, it } from "vitest";
import type { KernelOp, SurfaceHit, V3 } from "../kernel/types";
import { ArcTool } from "./arc";
import { CircleTool } from "./circle";
import {
  alignedFrame,
  arcFromBulge,
  arcThrough,
  bulgeForRadius,
  circleThrough,
  dist,
  planeForSegment,
  planeFromSnap,
  regularPolygon,
  rotatedRectangle,
} from "./draw";
import { ev, fakeHost } from "./fakeHost";
import { LineTool } from "./line";
import { RectTool } from "./rect";
import { finalTarget } from "./target";
import type { Snap } from "./tool";

const IN = 0.0254;
const close = (a: V3, b: V3) => expect(dist(a, b)).toBeLessThan(1e-12);

const topFace: SurfaceHit = {
  at: { kind: "face", object: 1, face: 2 },
  frame: { origin: [0.5, 0.5, 1], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] },
  point: [0.5, 0.5, 1],
  t: 9,
};
const snap = (point: V3, kind: Snap["kind"], surface: SurfaceHit | null = null, entity: Snap["entity"] = null): Snap => ({ point, kind, tip: kind, surface, entity, lines: [] });

describe("drawing planes and frames", () => {
  it("aligns frames with the world", () => {
    expect(alignedFrame([0, 0, 2], [0, 0, 1])).toMatchObject({ u: [1, 0, 0], v: [0, 1, 0] });
    const side = alignedFrame([1, 0, 0], [1, 0, 0]);
    expect(side.u).toEqual([0, 1, 0]);
    expect(side.v[2]).toBeCloseTo(1, 12);
  });

  it("a shape goes on the face under its first click, or the level plane through it", () => {
    const onTop = planeFromSnap(snap([0.2, 0.3, 1], "on-face", topFace));
    expect(onTop.target).toEqual({ on: "face", object: 1, face: 2 });
    expect(onTop.frame.u).toEqual([1, 0, 0]);
    const inAir = planeFromSnap(snap([2, 2, 0.75], "free"));
    expect(inAir.target).toEqual({ on: "plane", origin: [0, 0, 0.75], normal: [0, 0, 1] });
    expect(inAir.where((m) => `${m / IN}`)).toContain("up");
    expect(planeFromSnap(snap([2, 2, 0], "grid")).where((m) => String(m))).toBe("on the ground");
    // Arrow keys before the first click choose an upright plane.
    expect(planeFromSnap(snap([2, 2, 0], "grid"), "x").target).toMatchObject({ on: "plane", normal: [1, 0, 0] });
  });

  it("a free line settles on a level or upright plane", () => {
    expect(planeForSegment([0, 0, 1], [1, 2, 1], [0, 0, -1]).target).toEqual({ on: "plane", origin: [0, 0, 1], normal: [0, 0, 1] });
    const up = planeForSegment([0, 0, 0], [1, 0, 1], [0, 0, -1]);
    expect(up.target.on).toBe("plane");
    expect(Math.abs((up.target as { normal: V3 }).normal[1])).toBeCloseTo(1, 12);
    const plumb = planeForSegment([0, 0, 0], [0, 0, 1], [0.6, 0.8, -1]);
    expect((plumb.target as { normal: V3 }).normal).toEqual([0.6, 0.8, 0]);
  });

  it("a shape that leaves the solid becomes a new sketch on the plane", () => {
    const plane = planeFromSnap(snap([0, 0, 1], "endpoint", topFace, { kind: "vertex", owner: { kind: "object", id: 1 }, at: [0, 0, 1] }));
    const corner2 = snap([3, 1, 1], "endpoint", null, { kind: "vertex", owner: { kind: "object", id: 2 }, at: [3, 1, 1] });
    expect(finalTarget(plane, [snap([0, 0, 1], "endpoint", topFace, { kind: "vertex", owner: { kind: "object", id: 1 }, at: [0, 0, 1] }), corner2]).target).toEqual({
      on: "plane",
      origin: [0, 0, 1],
      normal: [0, 0, 1],
    });
    // Both corners on solid 1 (edges of its top): a cut on the face.
    const e = (p: V3) => snap(p, "on-edge", topFace, { kind: "edge", owner: { kind: "object", id: 1 }, a: [0, 0, 1], b: [1, 0, 1], drawn: false });
    expect(finalTarget(plane, [e([0.2, 0, 1]), e([0.4, 0, 1])]).target).toEqual({ on: "face", object: 1, face: 2 });
    // Started inside the face: stays on the face (the overhang is dropped by the kernel).
    const inside = planeFromSnap(snap([0.5, 0.5, 1], "on-face", topFace));
    expect(finalTarget(inside, [snap([0.5, 0.5, 1], "on-face", topFace), snap([3, 3, 1], "free")]).target.on).toBe("face");
  });
});

describe("shapes", () => {
  const f = alignedFrame([0, 0, 0], [0, 0, 1]);

  it("rotated rectangles stand on their first edge", () => {
    const r = rotatedRectangle(f, [0, 0, 0], [1, 1, 0], Math.SQRT2);
    close(r[2], [0, 2, 0]);
    close(r[3], [-1, 1, 0]);
  });

  it("polygons start toward the cursor", () => {
    const hex = regularPolygon(f, [1, 1, 0], [3, 1, 0], 0.5, 6);
    expect(hex).toHaveLength(6);
    close(hex[0], [1.5, 1, 0]);
    for (const p of hex) expect(dist(p, [1, 1, 0])).toBeCloseTo(0.5, 12);
  });

  it("2-point arcs from a bulge or a radius, 3-point arcs through a point", () => {
    const a = arcFromBulge(f, [0, 0, 0], [2, 0, 0], 1, 12)!; // a half circle of radius 1
    expect(a.radius).toBeCloseTo(1, 12);
    close(a.centre, [1, 0, 0]);
    expect(a.points).toHaveLength(13);
    close(a.points[6], [1, 1, 0]); // apex
    const s = bulgeForRadius(f, [0, 0, 0], [2, 0, 0], 1, [1, -5, 0])!;
    expect(s).toBeCloseTo(-1, 12);
    expect(bulgeForRadius(f, [0, 0, 0], [2, 0, 0], 0.5, [1, 1, 0])).toBeNull();
    const c = circleThrough([0, 0, 0], [1, 1, 0], [2, 0, 0])!;
    close(c, [1, 0, 0]);
    const pts = arcThrough(f, c, [0, 0, 0], [2, 0, 0], [1, 1, 0], 4);
    close(pts[2], [1, 1, 0]);
    const below = arcThrough(f, c, [0, 0, 0], [2, 0, 0], [1, -1, 0], 4);
    close(below[2], [1, -1, 0]);
  });
});

function lastOp(h: { ops: KernelOp[] }): Record<string, unknown> {
  return h.ops[h.ops.length - 1] as unknown as Record<string, unknown>;
}

describe("draw tools", () => {
  it("Line: segments go in one by one; the chain stops when it closes", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: 1, regions: [], faces: [], inside: null } });
    const t = new LineTool(h);
    const click = async (p: V3) => {
      t.onEvent(ev("click", p, { snapKind: "grid" }));
      await new Promise((r) => setTimeout(r, 0));
    };
    await click([0, 0, 0]);
    await click([1, 0, 0]);
    expect(lastOp(h)).toMatchObject({ op: "draw", target: { on: "plane", normal: [0, 0, 1] }, points: [[0, 0, 0], [1, 0, 0]], closed: false });
    await click([1, 1, 0]);
    await click([0, 0, 0]); // back to the start: the chain ends
    expect(h.ops).toHaveLength(3);
    expect(t.busy()).toBe(false);
  });

  it("Line: typed length along the cursor direction", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: 1, regions: [], faces: [], inside: null } });
    const t = new LineTool(h);
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "grid" }));
    await new Promise((r) => setTimeout(r, 0));
    t.onEvent(ev("move", [0, 0.3, 0], { snapKind: "axis-y" }));
    t.onValue("18");
    await new Promise((r) => setTimeout(r, 0));
    const op = lastOp(h) as { points: V3[] };
    close(op.points[1], [0, 18 * IN, 0]);
  });

  it("Line on a face: a dangling stroke stays a preview until it reaches an edge", async () => {
    const h = fakeHost({ outcome: null });
    h.lastError = () => "a line on a face must start and end on its edges, or close into a loop";
    const t = new LineTool(h);
    const edge = { kind: "edge" as const, owner: { kind: "object" as const, id: 1 }, a: [0, 0, 1] as V3, b: [1, 0, 1] as V3, drawn: false };
    t.onEvent(ev("click", [0.5, 0, 1], { snap: snap([0.5, 0, 1], "on-edge", topFace, edge) }));
    await new Promise((r) => setTimeout(r, 0));
    t.onEvent(ev("click", [0.5, 0.5, 1], { snap: snap([0.5, 0.5, 1], "on-face", topFace) }));
    await new Promise((r) => setTimeout(r, 0));
    expect(lastOp(h)).toMatchObject({ op: "draw", target: { on: "face", object: 1, face: 2 } });
    expect(t.busy()).toBe(true);
    expect(h.statuses.at(-1)?.text).toMatch(/carry on to an edge/);
    // Now it reaches the far edge: the whole stroke goes in as one cut.
    h.outcome = { kind: "drawn", sketch: null, regions: [], faces: [2, 7], inside: null };
    t.onEvent(ev("click", [0.5, 1, 1], { snap: snap([0.5, 1, 1], "on-edge", topFace, { ...edge, a: [0, 1, 1], b: [1, 1, 1] }) }));
    await new Promise((r) => setTimeout(r, 0));
    expect((lastOp(h) as { points: V3[] }).points).toHaveLength(3);
    expect(t.busy()).toBe(false);
  });

  it("Rect: two clicks, or a click and typed sizes toward the cursor; R again rotates", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: 1, regions: [3], faces: [], inside: null } });
    const t = new RectTool(h);
    t.onEvent(ev("click", [36 * IN, 0, 0], { snapKind: "grid" }));
    await new Promise((r) => setTimeout(r, 0));
    t.onEvent(ev("move", [36 * IN, 0, 0], { snapKind: "grid" }));
    t.onValue("-3/4, 11 1/4");
    await new Promise((r) => setTimeout(r, 0));
    const op = lastOp(h) as { points: V3[]; target: unknown; closed: boolean };
    expect(op.closed).toBe(true);
    close(op.points[0], [36 * IN, 0, 0]);
    close(op.points[2], [35.25 * IN, 11.25 * IN, 0]);
    expect(t.cycleVariant()).toMatch(/Rotated Rectangle/);
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "grid" }));
    await new Promise((r) => setTimeout(r, 0));
    t.onEvent(ev("click", [1, 1, 0], { snapKind: "grid" }));
    await new Promise((r) => setTimeout(r, 0));
    t.onEvent(ev("move", [0, 2, 0], { snapKind: "free" }));
    t.onValue(String(Math.SQRT2 / IN));
    await new Promise((r) => setTimeout(r, 0));
    const rot = lastOp(h) as { points: V3[] };
    close(rot.points[2], [0, 2, 0]);
  });

  it("Circle and Polygon: centre, radius, typed segments and sides", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: 1, regions: [3], faces: [], inside: null } });
    const t = new CircleTool(h);
    t.onValue("48s");
    t.onEvent(ev("click", [1, 1, 0], { snapKind: "grid" }));
    await new Promise((r) => setTimeout(r, 0));
    t.onEvent(ev("move", [2, 1, 0], { snapKind: "free" }));
    t.onValue("3");
    await new Promise((r) => setTimeout(r, 0));
    const c = lastOp(h) as { points: V3[] };
    expect(c.points).toHaveLength(48);
    expect(dist(c.points[0], [1, 1, 0])).toBeCloseTo(3 * IN, 12);
    t.cycleVariant();
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "grid" }));
    await new Promise((r) => setTimeout(r, 0));
    t.onEvent(ev("click", [0, 1, 0], { snapKind: "grid" }));
    await new Promise((r) => setTimeout(r, 0));
    expect((lastOp(h) as { points: V3[] }).points).toHaveLength(6);
  });

  it("Arc: ends, bulge or radius; open strokes", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: 1, regions: [], faces: [], inside: null } });
    const t = new ArcTool(h);
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "grid" }));
    await new Promise((r) => setTimeout(r, 0));
    t.onEvent(ev("click", [2, 0, 0], { snapKind: "grid" }));
    await new Promise((r) => setTimeout(r, 0));
    t.onEvent(ev("move", [1, 0.5, 0], { snapKind: "free" }));
    t.onValue(`${1 / IN}r`);
    await new Promise((r) => setTimeout(r, 0));
    const a = lastOp(h) as { points: V3[]; closed: boolean };
    expect(a.closed).toBe(false);
    expect(a.points).toHaveLength(13);
    close(a.points[6], [1, 1, 0]);
  });
});

describe("rectangle direction and extents (lane-3 verifier)", () => {
  it("the preview labels each side with its length and direction", () => {
    const h = fakeHost();
    const t = new RectTool(h);
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "grid" }));
    t.onEvent(ev("move", [-0.5, 0.25, 0], { snapKind: "free" }));
    const texts = t.view().labels.map((l) => l.text);
    expect(texts.some((x) => x.endsWith("−red"))).toBe(true);
    expect(texts.some((x) => x.endsWith("+green"))).toBe(true);
    // Two short ticks at the first corner show the directions.
    expect(t.view().overlay.some((o) => o.kind === "segments" && o.points.length === 4)).toBe(true);
  });

  it("the commit message states the extents, directions and the first corner", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: 1, regions: [1], faces: [], inside: 1 } });
    const t = new RectTool(h);
    t.onValue("24, 16");
    await Promise.resolve();
    const msg = h.statuses.map((s) => s.text).join(" | ");
    expect(msg).toContain("Rectangle 24 × 16 in on the ground");
    expect(msg).toContain("24 in along +red and 16 in along +green from (0, 0, 0)");
  });
});
