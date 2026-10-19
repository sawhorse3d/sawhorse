import { describe, expect, it } from "vitest";
import type { Entity, KernelOp, MeshData, Picked, V3 } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { MoveTool, moveOp } from "./move";
import { setNameSource } from "../lib/names";
import type { DocState } from "../kernel/types";
import { applyX, describeSubject, movedSelection, previewSegments, rotation, scaling, sketchPreviews, sketchSegments, subjectOf, translation, type Subject } from "./xform";

const IN = 0.0254;
const tick = () => new Promise((r) => setTimeout(r, 0));
const face: Entity = { kind: "face", object: 1, face: 2 };
const onBox: Picked = { entity: face, point: [0, 0, 1], surface: null };
const ops = (h: { ops: KernelOp[] }) => h.ops.map((o) => o as unknown as Record<string, unknown>);
const close = (a: V3, b: V3) => a.forEach((x, k) => expect(x).toBeCloseTo(b[k], 12));

/** A unit box (object 1) as a view mesh: its 12 edges. */
function boxMesh(): MeshData {
  const c = (x: number, y: number, z: number): V3 => [x, y, z];
  const corners = [c(0, 0, 0), c(1, 0, 0), c(1, 1, 0), c(0, 1, 0), c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)];
  const pairs = [
    [0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7],
  ];
  const edges = new Float32Array(pairs.flatMap(([a, b]) => [...corners[a], ...corners[b]]));
  return { positions: new Float32Array(), indices: new Uint32Array(), faceIds: new Uint32Array(), edges, edgeOwner: new Uint32Array(pairs.length), edgeOwners: [{ kind: "object", id: 1 }], parts: [] };
}

describe("transform helpers", () => {
  it("affine maps match the kernel's", () => {
    close(applyX(translation([1, 2, 3]), [0, 0, 0]), [1, 2, 3]);
    close(applyX(rotation([1, 0, 0], [0, 0, 1], Math.PI / 2), [2, 0, 0]), [1, 1, 0]);
    close(applyX(scaling([1, 1, 1], [2, 1, -1]), [2, 2, 2]), [3, 2, 0]);
  });

  it("decides what moves: selected solids, part of one solid, or the solid under the cursor", () => {
    expect(subjectOf([], face, new Map())).toEqual({ kind: "objects", objects: [1] });
    expect(subjectOf([{ kind: "object", object: 3 }, { kind: "object", object: 3 }, face], null, new Map())).toEqual({ kind: "objects", objects: [3] });
    const outlines = new Map([["1:2", [[[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]] as V3[]]]]);
    const s = subjectOf([face], null, outlines);
    expect(s).toMatchObject({ kind: "geometry", object: 1 });
    expect((s as { points: V3[] }).points).toHaveLength(4);
    const edge: Entity = { kind: "edge", owner: { kind: "object", id: 1 }, a: [0, 0, 1], b: [1, 0, 1], drawn: false };
    expect(subjectOf([edge, face], null, outlines)).toMatchObject({ kind: "geometry", points: expect.any(Array) });
    expect(((subjectOf([edge, face], null, outlines) as { points: V3[] }).points).length).toBe(4);
    // Lane 22: sketch geometry is a subject of its own (no more "Sketch regions don't move yet").
    expect(subjectOf([{ kind: "region", sketch: 1, region: 1 }], null, new Map())).toMatchObject({ kind: "objects", objects: [], sketches: [{ sketch: 1, regions: [1] }] });
    expect(subjectOf([face, { kind: "face", object: 2, face: 1 }], null, outlines)).toMatch(/one solid at a time/);
    expect(subjectOf([], null, new Map())).toMatch(/Click a solid/);
    const guidePoint: Entity = { kind: "vertex", owner: { kind: "guide", id: 1 }, at: [0, 0, 0] };
    expect(subjectOf([guidePoint], null, new Map())).toMatch(/Guides and dimensions/);
  });

  it("gathers sketch geometry by sketch, with solids and parts of a solid (lane 22)", () => {
    const ring: V3[] = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]];
    const outlines = new Map([["r1:2", [ring]]]);
    const state = { sketches: [{ id: 1, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }, { id: 4, origin: [0, 0, 2], normal: [1, 0, 0], u: [0, 1, 0], v: [0, 0, 1] }] } as unknown as DocState;
    const region: Entity = { kind: "region", sketch: 1, region: 2 };
    const sEdge: Entity = { kind: "edge", owner: { kind: "sketch", id: 4 }, a: [0, 0, 2], b: [0, 1, 2], drawn: true };
    const sPoint: Entity = { kind: "vertex", owner: { kind: "sketch", id: 1 }, at: [1, 0, 0] };
    const s = subjectOf([region, sEdge, sPoint], null, outlines, state) as Subject;
    expect(s).toMatchObject({ kind: "objects", objects: [] });
    expect(s.sketches).toHaveLength(2);
    const [one, four] = s.sketches!;
    expect(one).toMatchObject({ sketch: 1, regions: [2], points: [[1, 0, 0]], normal: [0, 0, 1] });
    expect(one.moving).toHaveLength(4); // the ring's corners (the point is one of them)
    expect(four).toMatchObject({ sketch: 4, edges: [[[0, 0, 2], [0, 1, 2]]], normal: [1, 0, 0], origin: [0, 0, 2] });
    // With a solid: the solid moves and the sketch geometry with it.
    expect(subjectOf([{ kind: "object", object: 3 }, region], null, outlines, state)).toMatchObject({ kind: "objects", objects: [3], sketches: [{ sketch: 1 }] });
    // With part of a solid.
    const faceOutlines = new Map([...outlines, ["1:2", [[[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]] as V3[]]]]);
    expect(subjectOf([face, sEdge], null, faceOutlines, state)).toMatchObject({ kind: "geometry", object: 1, sketches: [{ sketch: 4 }] });
    // Nothing selected: the region, edge or point under the cursor.
    expect(subjectOf([], region, outlines)).toMatchObject({ kind: "objects", objects: [], sketches: [{ sketch: 1, regions: [2] }] });
    expect(subjectOf([], sPoint, outlines)).toMatchObject({ sketches: [{ sketch: 1, points: [[1, 0, 0]] }] });
    expect(describeSubject(s)).toMatch(/a sketch point and a sketch edge|region/);
  });

  it("previews sketch geometry: neighbours follow, part of a shape stays in its plane, a whole shape lifts", () => {
    // A unit square on the ground as sketch 1's edges in the view mesh.
    const sq: V3[] = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]];
    const edges = new Float32Array(sq.flatMap((p, i) => [...p, ...sq[(i + 1) % 4]]));
    const mesh = { positions: new Float32Array(), indices: new Uint32Array(), faceIds: new Uint32Array(), edges, edgeOwner: new Uint32Array(4), edgeOwners: [{ kind: "sketch", id: 1 }], parts: [] } as MeshData;
    const corner = subjectOf([{ kind: "vertex", owner: { kind: "sketch", id: 1 }, at: [1, 1, 0] }], null, new Map()) as Subject;
    const pv = sketchPreviews(corner, mesh);
    expect(pv[0].edges).toHaveLength(2);
    expect(pv[0].liftable).toBe(false);
    // Up and sideways: the corner moves sideways only (projected), its two edges follow.
    const seg = sketchSegments(pv, translation([0.5, 0, 0.75]));
    expect(seg).toHaveLength(4);
    expect(seg.filter((p) => p[0] === 1.5 && p[1] === 1)).toHaveLength(2);
    expect(seg.every((p) => p[2] === 0)).toBe(true);
    // The whole square lifts off the plane as it is.
    const whole = subjectOf([{ kind: "region", sketch: 1, region: 1 }], null, new Map([["r1:1", [sq]]])) as Subject;
    const wpv = sketchPreviews(whole, mesh);
    expect(wpv[0].liftable).toBe(true);
    expect(sketchSegments(wpv, translation([0, 0, 0.5])).every((p) => p[2] === 0.5)).toBe(true);
    // A copy previews the copied edges only.
    expect(sketchSegments(pv, translation([0.5, 0, 0]), true)).toHaveLength(0);
  });

  it("the selection follows moved sketch geometry, onto a new plane too", () => {
    const sq: V3[] = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]];
    const region: Entity = { kind: "region", sketch: 1, region: 1 };
    const edge: Entity = { kind: "edge", owner: { kind: "sketch", id: 1 }, a: [0, 0, 0], b: [1, 0, 0], drawn: true };
    const solid: Entity = { kind: "object", object: 7 };
    const s = subjectOf([region, edge, solid], null, new Map([["r1:1", [sq]]])) as Subject;
    const pv = sketchPreviews(s, null);
    // Within the plane: the region keeps its id, the edge goes where it went.
    const inPlane = movedSelection([region, edge, solid], s, pv, translation([2, 0, 0]), { kind: "batch", outcomes: [{ kind: "transformed", objects: [7] }, { kind: "sketch_moved", sketch: 1, regions: [[1, 1]] }] });
    expect(inPlane).toEqual([region, { ...edge, a: [2, 0, 0], b: [3, 0, 0] }, solid]);
    // Lifted (the kernel says it is in sketch 5 now): the regions it became there.
    const lift = { ...pv[0], liftable: true };
    const lifted = movedSelection([region, edge], s, [lift], translation([0, 0, 1]), { kind: "sketch_moved", sketch: 5, regions: [[5, 9]] });
    expect(lifted).toEqual([{ kind: "region", sketch: 5, region: 9 }, { ...edge, owner: { kind: "sketch", id: 5 }, a: [0, 0, 1], b: [1, 0, 1] }]);
  });

  it("the preview moves whole solids, or only the moving ends of part of one", () => {
    const mesh = boxMesh();
    const whole = previewSegments({ kind: "objects", objects: [1] }, [[[0, 0, 0], [0, 0, 1]]], translation([1, 0, 0]));
    expect(whole).toEqual([[1, 0, 0], [1, 0, 1]]);
    const part = previewSegments({ kind: "geometry", object: 1, points: [[0, 0, 1]], what: "" }, [[[0, 0, 0], [0, 0, 1]]], translation([0, 0, 1]));
    expect(part).toEqual([[0, 0, 0], [0, 0, 2]]);
    expect(mesh.edgeOwner.length).toBe(12);
  });

  it("names what moves without raw ids", () => {
    expect(describeSubject({ kind: "objects", objects: [1, 2] })).toMatch(/and/);
  });

  it("builds move, copy, array and part-move ops", () => {
    const s = { kind: "objects" as const, objects: [1] };
    expect(moveOp(s, [1, 0, 0], false)).toMatchObject({ op: "transform", objects: [1] });
    expect(moveOp(s, [1, 0, 0], true)).toMatchObject({ op: "copy" });
    expect(moveOp(s, [1, 0, 0], true, 3)).toMatchObject({ op: "array", count: 3 });
    expect(moveOp({ kind: "geometry", object: 1, points: [[0, 0, 1]], what: "" }, [0, 0, 1], false, 1, true)).toMatchObject({ op: "transform_geometry", autofold: true });
  });

  it("builds sketch move, copy and array ops, batched with solids (lane 22)", () => {
    const part = { sketch: 1, points: [], edges: [], regions: [2], rings: [], origin: [0, 0, 0] as V3, normal: [0, 0, 1] as V3, moving: [] };
    const sk: Subject = { kind: "objects", objects: [], sketches: [part] };
    expect(moveOp(sk, [1, 0, 0], false)).toEqual({ op: "transform_sketch", sketch: 1, regions: [2], xform: translation([1, 0, 0]) });
    expect(moveOp(sk, [1, 0, 0], true)).toMatchObject({ op: "transform_sketch", copy: true });
    expect(moveOp(sk, [1, 0, 0], true, 3)).toMatchObject({ op: "transform_sketch", copy: true, count: 3 });
    // With a solid: one batch, one undo step.
    expect(moveOp({ ...sk, objects: [4] }, [1, 0, 0], false)).toMatchObject({ op: "batch", ops: [{ op: "transform", objects: [4] }, { op: "transform_sketch", sketch: 1 }] });
    expect(moveOp({ ...sk, objects: [4] }, [1, 0, 0], true, 2)).toMatchObject({ op: "batch", ops: [{ op: "array", count: 2 }, { op: "transform_sketch", copy: true, count: 2 }] });
    // A copy leaves out a sketch with only points (a point copies nothing) next to a solid.
    const pts = { ...part, regions: [], points: [[0, 0, 0] as V3] };
    expect(moveOp({ kind: "objects", objects: [4], sketches: [pts] }, [1, 0, 0], true)).toMatchObject({ op: "copy" });
    // With part of a solid.
    expect(moveOp({ kind: "geometry", object: 1, points: [[0, 0, 1]], what: "", sketches: [part] }, [0, 0, 1], false)).toMatchObject({ op: "batch", ops: [{ op: "transform_geometry" }, { op: "transform_sketch" }] });
  });
});

describe("Move tool", () => {
  it("click, move, click moves the solid under the cursor by the inferred distance", async () => {
    setNameSource({ solids: [{ id: 1, number: 1, min: [0, 0, 0], max: [1, 1, 1], volume: 1 }] } as unknown as DocState, null);
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] }, _mesh: boxMesh() });
    const t = new MoveTool(h);
    t.onEvent(ev("down", [0, 0, 1], { picked: onBox, snapKind: "endpoint" }));
    await tick();
    t.onEvent(ev("click", [0, 0, 1], { picked: onBox, snapKind: "endpoint" }));
    expect(t.busy()).toBe(true);
    t.onEvent(ev("move", [36 * IN, 0, 1], { snapKind: "axis-x" }));
    expect(t.view().overlay.some((o) => o.points.length === 24)).toBe(true); // the box's 12 edges, moved
    t.onEvent(ev("click", [36 * IN, 0, 1], { snapKind: "axis-x" }));
    await tick();
    const op = ops(h).at(-1)!;
    expect(op).toMatchObject({ op: "transform", objects: [1] });
    close((op.xform as { translation: V3 }).translation, [36 * IN, 0, 0]);
    expect(h.statuses.at(-1)?.text).toContain("Moved solid 1");
    setNameSource(null, null);
    expect(h.statuses.at(-1)?.text).toContain("36 in along +red");
  });

  it("drag and release; a typed distance mid-gesture goes the way the cursor went", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new MoveTool(h);
    t.onEvent(ev("down", [0, 0, 0], { picked: onBox }));
    await tick();
    t.onEvent({ ...ev("drag", [0, -0.3, 0], { snapKind: "axis-y" }), start: [0, 0] });
    t.onValue("12");
    await tick();
    const op = ops(h).at(-1)!;
    close((op.xform as { translation: V3 }).translation, [0, -12 * IN, 0]);
    expect(t.busy()).toBe(false);
  });

  it("Ctrl copies; x3 then replaces the copy with an array; /4 divides it", async () => {
    const h = fakeHost({ outcome: { kind: "copied", objects: [2] } });
    const t = new MoveTool(h);
    t.onEvent(ev("down", [0, 0, 0], { picked: onBox }));
    await tick();
    expect(h.statuses.at(-1)?.text).toMatch(/^Moving /);
    t.onModifier("ctrl");
    // The status line says it is copying now, not moving.
    expect(h.statuses.at(-1)?.text).toMatch(/^Copying .*the original stays/);
    t.onEvent(ev("click", [0, 0, 0], { picked: onBox }));
    t.onEvent(ev("move", [1, 0, 0], { snapKind: "axis-x" }));
    t.onEvent(ev("click", [1, 0, 0], { snapKind: "axis-x" }));
    await tick();
    expect(ops(h).at(-1)).toMatchObject({ op: "copy", objects: [1] });
    expect(h.statuses.at(-1)?.text).toContain("Type x3");
    t.onValue("x3");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "array", objects: [1], count: 3 }]);
    close((ops(h).at(-1)!.xform as { translation: V3 }).translation, [1, 0, 0]);
    t.onValue("/4");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "array", count: 4 }]);
    // The divide spans the whole array (3 m): four steps of 0.75 m.
    close((ops(h).at(-1)!.xform as { translation: V3 }).translation, [0.75, 0, 0]);
  });

  it("Ctrl tapped before the click: the gesture starts as a copy and says so", async () => {
    const h = fakeHost({ outcome: { kind: "copied", objects: [2] } });
    const t = new MoveTool(h);
    t.onModifier("ctrl");
    expect(h.statuses.at(-1)?.text).toMatch(/^Copying:/);
    t.onEvent(ev("down", [0, 0, 0], { picked: onBox }));
    await tick();
    expect(h.statuses.at(-1)?.text).toMatch(/^Copying .*Tap Ctrl to move it instead/);
    t.onModifier("ctrl");
    expect(h.statuses.at(-1)?.text).toMatch(/^Moving /);
  });

  it("a distance typed after the move changes it; arrays need a copy", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new MoveTool(h);
    t.onEvent(ev("down", [0, 0, 0], { picked: onBox }));
    await tick();
    t.onEvent(ev("click", [0, 0, 0], { picked: onBox }));
    t.onEvent(ev("move", [0, 0, 0.5], { snapKind: "axis-z" }));
    t.onEvent(ev("click", [0, 0, 0.5], { snapKind: "axis-z" }));
    await tick();
    t.onValue("6");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "transform" }]);
    close((ops(h).at(-1)!.xform as { translation: V3 }).translation, [0, 0, 6 * IN]);
    t.onValue("x3");
    expect(h.statuses.at(-1)).toMatchObject({ error: true, text: expect.stringContaining("Arrays follow a copy") });
  });

  it("moves part of a solid; Alt turns autofold on", async () => {
    const h = fakeHost({ outcome: { kind: "geometry_moved", faces: [2, 3] } });
    h.sel = [face];
    const t = new MoveTool(h);
    t.onModifier("alt");
    t.onEvent(ev("down", [0.5, 0.5, 1], { picked: onBox }));
    await tick();
    t.onEvent(ev("click", [0.5, 0.5, 1], { picked: onBox }));
    t.onEvent(ev("move", [0.5, 0.5, 1.25], { snapKind: "axis-z" }));
    t.onEvent(ev("click", [0.5, 0.5, 1.25], { snapKind: "axis-z" }));
    await tick();
    const op = ops(h).at(-1)!;
    expect(op).toMatchObject({ op: "transform_geometry", object: 1, autofold: true });
    expect((op.points as V3[]).length).toBe(4);
    expect(h.statuses.at(-1)?.text).toContain("Moved face 2");
  });

  it("moves a selected sketch region by a typed distance; the selection stays on it (lane 22)", async () => {
    const h = fakeHost({ outcome: { kind: "sketch_moved", sketch: 1, regions: [[1, 1]] } });
    const region: Entity = { kind: "region", sketch: 1, region: 1 };
    h.sel = [region];
    const t = new MoveTool(h);
    t.onEvent(ev("down", [0, 0, 0], { snapKind: "endpoint" }));
    await tick();
    expect(t.busy()).toBe(true);
    expect(h.statuses.at(-1)?.text).toMatch(/^Moving .*region|^Moving a region/);
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "endpoint" }));
    t.onEvent(ev("move", [0.3, 0, 0], { snapKind: "axis-x" }));
    // The preview has no solid edges and shows nothing of a sketch the mesh does not hold.
    t.onValue("36");
    await tick();
    const op = ops(h).at(-1)!;
    expect(op).toMatchObject({ op: "transform_sketch", sketch: 1, regions: [1] });
    close((op.xform as { translation: V3 }).translation, [36 * IN, 0, 0]);
    expect(h.statuses.at(-1)?.text).toMatch(/^Moved .* 36 in along \+red/);
    expect(h.sel).toEqual([region]);
    // Typed again after the move: undo and move again from where it was.
    t.onValue("12");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "transform_sketch", regions: [1] }]);
    close((ops(h).at(-1)!.xform as { translation: V3 }).translation, [12 * IN, 0, 0]);
  });

  it("picks up the sketch shape under the cursor when nothing is selected", async () => {
    const h = fakeHost({ outcome: { kind: "sketch_moved", sketch: 1, regions: [[1, 3]] } });
    const t = new MoveTool(h);
    const under: Picked = { entity: { kind: "region", sketch: 1, region: 3 }, point: [0.5, 0.5, 0], surface: null };
    t.onEvent(ev("move", [0.5, 0.5, 0], { picked: under }));
    expect(h.hovered).toEqual({ kind: "region", sketch: 1, region: 3 });
    t.onEvent(ev("down", [0.5, 0.5, 0], { picked: under }));
    await tick();
    t.onEvent(ev("click", [0.5, 0.5, 0], { picked: under }));
    t.onEvent(ev("move", [0.5, 1.5, 0], { snapKind: "axis-y" }));
    t.onEvent(ev("click", [0.5, 1.5, 0], { snapKind: "axis-y" }));
    await tick();
    expect(ops(h).at(-1)).toMatchObject({ op: "transform_sketch", sketch: 1, regions: [3] });
    close((ops(h).at(-1)!.xform as { translation: V3 }).translation, [0, 1, 0]);
  });

  it("Ctrl copies sketch geometry; x3 makes an array of it", async () => {
    const h = fakeHost({ outcome: { kind: "sketch_copied", regions: [[1, 2]] } });
    h.sel = [{ kind: "region", sketch: 1, region: 1 }];
    const t = new MoveTool(h);
    t.onModifier("ctrl");
    t.onEvent(ev("down", [0, 0, 0]));
    await tick();
    t.onEvent(ev("click", [0, 0, 0]));
    t.onEvent(ev("move", [1, 0, 0], { snapKind: "axis-x" }));
    t.onEvent(ev("click", [1, 0, 0], { snapKind: "axis-x" }));
    await tick();
    expect(ops(h).at(-1)).toMatchObject({ op: "transform_sketch", copy: true });
    expect(h.statuses.at(-1)?.text).toMatch(/^Copied .*Type x3/);
    t.onValue("x3");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "transform_sketch", copy: true, count: 3 }]);
    expect(h.statuses.at(-1)?.text).toMatch(/^Array: 3 copies/);
  });

  it("a moved sketch point stays selected where it went", async () => {
    const h = fakeHost({ outcome: { kind: "sketch_moved", sketch: 1, regions: [[1, 1]] } });
    const corner: Entity = { kind: "vertex", owner: { kind: "sketch", id: 1 }, at: [1, 1, 0] };
    h.sel = [corner];
    const t = new MoveTool(h);
    t.onEvent(ev("down", [1, 1, 0], { snapKind: "endpoint" }));
    await tick();
    t.onEvent(ev("click", [1, 1, 0], { snapKind: "endpoint" }));
    t.onEvent(ev("move", [1.5, 1.25, 0]));
    t.onEvent(ev("click", [1.5, 1.25, 0]));
    await tick();
    expect(ops(h).at(-1)).toMatchObject({ op: "transform_sketch", points: [[1, 1, 0]] });
    expect(h.sel).toEqual([{ ...corner, at: [1.5, 1.25, 0] }]);
  });
});
