import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type { Entity, KernelOp, MeshData, OpOutcome, Picked, V3, WasmDoc, Xform } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { MoveTool, moveOp } from "./move";
import { rotateOp } from "./rotate";
import { scaleOp } from "./scale";
import { setNameSource } from "../lib/names";
import { nextTarget } from "../lib/edit";
import type { DocState } from "../kernel/types";
import { applyX, describeSubject, loadOutlines, movedSelection, previewSegments, rotation, scaling, sketchesAfter, sketchPreviews, sketchSegments, subjectOf, targetAfterMove, translation, type Subject } from "./xform";

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

  it("#7: a region moved within its plane that splits is selected as the pieces it became, not its old id", () => {
    // Region 1, a unit square, moved 2 to the right across a rectangle. The kernel says the
    // moved edges now bound regions 4 and 5 (inside the moved square) and 2 (the rectangle's
    // remainder, outside it). Region 1 itself no longer exists.
    const sq: V3[] = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]];
    const region: Entity = { kind: "region", sketch: 1, region: 1 };
    const s = subjectOf([region], null, new Map([["r1:1", [sq]]])) as Subject;
    const pv = sketchPreviews(s, null);
    // One triangle per region, in the plane z = 0: 4 and 5 inside x 2..3, y 0..1; 2 outside.
    const tri = (cx: number, cy: number) => [cx - 0.1, cy - 0.1, 0, cx + 0.1, cy - 0.1, 0, cx, cy + 0.1, 0];
    const part = (index: number) => ({ kind: "region" as const, owner: 1, index, origin: [0, 0, 0] as V3, normal: [0, 0, 1] as V3, u: [1, 0, 0] as V3, v: [0, 1, 0] as V3 });
    const mesh: MeshData = {
      positions: new Float32Array([...tri(2.25, 0.5), ...tri(5, 0.5), ...tri(2.75, 0.5)]),
      indices: new Uint32Array([0, 1, 2, 3, 4, 5, 6, 7, 8]),
      faceIds: new Uint32Array([0, 1, 2]),
      edges: new Float32Array(),
      edgeOwner: new Uint32Array(),
      edgeOwners: [],
      parts: [part(4), part(2), part(5)],
    };
    const moved = movedSelection([region], s, pv, translation([2, 0, 0]), { kind: "sketch_moved", sketch: 1, regions: [[1, 2], [1, 4], [1, 5]] }, mesh);
    expect(moved).toEqual([
      { kind: "region", sketch: 1, region: 4 },
      { kind: "region", sketch: 1, region: 5 },
    ]);
    // A move that splits nothing: the kernel reports the region itself, which stays selected.
    const plainMesh: MeshData = { ...mesh, positions: new Float32Array(tri(2.5, 0.5)), indices: new Uint32Array([0, 1, 2]), faceIds: new Uint32Array([0]), parts: [part(1)] };
    expect(movedSelection([region], s, pv, translation([2, 0, 0]), { kind: "sketch_moved", sketch: 1, regions: [[1, 1]] }, plainMesh)).toEqual([region]);
  });

  it("#7: Push/Pull's next target follows a selected region a move turned into pieces: the one piece, or none", () => {
    const r = (region: number): Entity => ({ kind: "region", sketch: 1, region });
    const t = (region: number) => ({ kind: "region" as const, sketch: 1, region });
    // Split into two pieces (the old id is one of them): no target, so P waits for a click
    // instead of pushing the one piece the old id now names.
    expect(targetAfterMove(t(1), [r(1)], [r(1), r(4)])).toBeNull();
    expect(targetAfterMove(t(1), [r(1)], [r(4), r(5)])).toBeNull();
    // One piece, under a new id: that piece.
    expect(targetAfterMove(t(1), [r(1)], [r(4)])).toEqual(t(4));
    // Nothing split: the target stays.
    expect(targetAfterMove(t(1), [r(1)], [r(1)])).toEqual(t(1));
    // A target that was not among what moved, or not a region, is left alone.
    expect(targetAfterMove(t(2), [r(1)], [r(4), r(5)])).toEqual(t(2));
    expect(targetAfterMove({ kind: "face", object: 3, face: 2 }, [r(1)], [r(4), r(5)])).toEqual({ kind: "face", object: 3, face: 2 });
    expect(targetAfterMove(null, [r(1)], [r(4), r(5)])).toBeNull();
  });

  it("#7 (lane 43): a region lifted to another sketch targets what it became there: the one region, or none", () => {
    const r = (sketch: number, region: number): Entity => ({ kind: "region", sketch, region });
    const t = (sketch: number, region: number) => ({ kind: "region" as const, sketch, region });
    const lifted = new Map([[1, 5]]);
    // Lifted whole onto an empty plane: the region it is there.
    expect(targetAfterMove(t(1, 1), [r(1, 1)], [r(5, 9)], lifted)).toEqual(t(5, 9));
    // Lifted across a shape there: two pieces, so nothing (not the cut shape's remainder).
    expect(targetAfterMove(t(1, 1), [r(1, 1)], [r(5, 9), r(5, 10)], lifted)).toBeNull();
    // A target that did not move stays, lifted or not.
    expect(targetAfterMove(t(5, 2), [r(1, 1)], [r(5, 9)], lifted)).toEqual(t(5, 2));
    // In its own plane the map changes nothing.
    expect(targetAfterMove(t(1, 1), [r(1, 1)], [r(1, 1)], new Map([[1, 1]]))).toEqual(t(1, 1));
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
    // Lane 43 (#7): the host is told what was selected before the step and what after it,
    // so Undo and Redo of it select those again.
    expect(h.remembered).toEqual([{ before: [region], after: [region], target: null }]);
    // Typed again after the move: undo and move again from where it was.
    t.onValue("12");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "transform_sketch", regions: [1] }]);
    close((ops(h).at(-1)!.xform as { translation: V3 }).translation, [12 * IN, 0, 0]);
    expect(h.remembered).toHaveLength(2);
    expect(h.remembered[1].before).toEqual([region]);
  });

  it("#7 (lane 43): a region lifted across a shape on another plane leaves Push/Pull no target, not the cut shape's remainder", async () => {
    // The kernel says the lifted square's edges bound regions 9 and 10 of sketch 5 there: two
    // pieces (the app's nextTarget, run here as the app runs it, names the first).
    const h = fakeHost({ outcome: { kind: "sketch_moved", sketch: 5, regions: [[5, 9], [5, 10]] } });
    const apply = h.apply;
    h.apply = async (op, describe) => {
      const r = await apply(op, describe);
      if (r) h._target = nextTarget(op, r.outcome, h._target);
      return r;
    };
    const region: Entity = { kind: "region", sketch: 1, region: 1 };
    h.sel = [region];
    h._target = { kind: "region", sketch: 1, region: 1 };
    const t = new MoveTool(h);
    t.onEvent(ev("down", [0, 0, 0], { snapKind: "endpoint" }));
    await tick();
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "endpoint" }));
    t.onEvent(ev("move", [0, 0, 0.3], { snapKind: "axis-z" }));
    t.onValue("6");
    await tick();
    expect(ops(h).at(-1)).toMatchObject({ op: "transform_sketch", sketch: 1, regions: [1] });
    expect(h.sel).toEqual([
      { kind: "region", sketch: 5, region: 9 },
      { kind: "region", sketch: 5, region: 10 },
    ]);
    expect(h._target).toBeNull();
    // Lifted whole onto an empty plane (one region there): that region is the target.
    h.outcome = { kind: "sketch_moved", sketch: 6, regions: [[6, 1]] };
    h.sel = [region];
    h._target = { kind: "region", sketch: 1, region: 1 };
    const t2 = new MoveTool(h);
    t2.onEvent(ev("down", [0, 0, 0], { snapKind: "endpoint" }));
    await tick();
    t2.onEvent(ev("click", [0, 0, 0], { snapKind: "endpoint" }));
    t2.onEvent(ev("move", [0, 0, 0.3], { snapKind: "axis-z" }));
    t2.onValue("6");
    await tick();
    expect(h._target).toEqual({ kind: "region", sketch: 6, region: 1 });
  });

  it("names regions of two sketches with their sketches, and of one sketch without (lane 27, #10)", async () => {
    const frame = { origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] };
    const state = { solids: [], sketches: [{ id: 1, ...frame }, { id: 4, ...frame, origin: [0, 0, 1] }] } as unknown as DocState;
    const part = (owner: number, index: number) => ({ kind: "region", owner, index }) as unknown as MeshData["parts"][number];
    const mesh = { ...boxMesh(), edgeOwners: [], edgeOwner: new Uint32Array(), edges: new Float32Array(), parts: [part(1, 1), part(1, 2), part(4, 1)] } as MeshData;
    setNameSource(state, mesh);
    try {
      const h = fakeHost({ outcome: { kind: "batch", outcomes: [{ kind: "sketch_moved", sketch: 1, regions: [[1, 1]] }, { kind: "sketch_moved", sketch: 4, regions: [[4, 1]] }] }, _state: state });
      h.sel = [
        { kind: "region", sketch: 1, region: 1 },
        { kind: "region", sketch: 4, region: 1 },
      ];
      const t = new MoveTool(h);
      t.onEvent(ev("down", [0, 0, 0], { snapKind: "endpoint" }));
      await tick();
      expect(h.statuses.at(-1)?.text).toMatch(/^Moving region 1 of sketch 1 and region 1 of sketch 2:/);
      t.onEvent(ev("click", [0, 0, 0], { snapKind: "endpoint" }));
      t.onEvent(ev("move", [0.3, 0, 0], { snapKind: "axis-x" }));
      t.onValue("36");
      await tick();
      expect(h.statuses.at(-1)?.text).toBe("Moved region 1 of sketch 1 and region 1 of sketch 2 36 in along +red.");
      // Both regions in one sketch: the short form.
      const one = subjectOf([{ kind: "region", sketch: 1, region: 1 }, { kind: "region", sketch: 1, region: 2 }], null, new Map(), state) as Subject;
      expect(describeSubject(one)).toBe("region 1 and region 2");
      // A region of one sketch and an edge of another: only one sketch has regions named.
      const mixed = subjectOf([{ kind: "region", sketch: 1, region: 1 }, { kind: "edge", owner: { kind: "sketch", id: 4 }, a: [0, 0, 1], b: [1, 0, 1], drawn: true }], null, new Map(), state) as Subject;
      expect(describeSubject(mixed)).toBe("region 1 and a sketch edge");
    } finally {
      setNameSource(null, null);
    }
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

const pkg = path.resolve(__dirname, "../wasm/pkg");
const built = existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));
const runKernel = built || process.env.REQUIRE_KERNEL ? describe : describe.skip;

// #7 through the real kernel (needs `npm run wasm`): what is selected after a split is the
// moved shape's pieces, read back by area, for a turn, a mirror and a region with a hole.
runKernel("#7 through the kernel: a split shape is selected as its pieces", () => {
  async function doc(): Promise<WasmDoc> {
    const mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as { initSync(o: { module: Buffer }): void; document_new(): WasmDoc };
    mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
    return mod.document_new();
  }
  function meshOf(d: WasmDoc): MeshData {
    const m = d.get_scene_mesh();
    const mesh = { positions: m.positions(), indices: m.indices(), faceIds: m.face_ids(), edges: m.edges(), edgeOwner: m.edge_owner(), edgeOwners: JSON.parse(m.edge_owners_json()), parts: JSON.parse(m.parts_json()) };
    m.free();
    return mesh;
  }
  const apply = (d: WasmDoc, op: unknown): OpOutcome => {
    const out = JSON.parse(d.apply_op(JSON.stringify(op)));
    expect(out.error, JSON.stringify(out)).toBeUndefined();
    return out;
  };
  const draw = (d: WasmDoc, pts: [number, number][]) => apply(d, { op: "draw", target: { on: "plane", origin: [0, 0, 0], normal: [0, 0, 1] }, points: pts.map(([x, y]) => [x, y, 0]), closed: true });
  const rect = (d: WasmDoc, x: number, y: number, w: number, h: number) => draw(d, [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]);
  /** Each region's area in the view mesh, by id. */
  function areas(mesh: MeshData): Map<number, number> {
    const out = new Map<number, number>();
    for (let t = 0; t < mesh.faceIds.length; t++) {
      const part = mesh.parts[mesh.faceIds[t]];
      if (part.kind !== "region") continue;
      const [a, b, c] = [0, 1, 2].map((k) => Array.from(mesh.positions.subarray(mesh.indices[t * 3 + k] * 3, mesh.indices[t * 3 + k] * 3 + 3)));
      const area = Math.abs((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2;
      out.set(part.index, (out.get(part.index) ?? 0) + area);
    }
    return out;
  }
  const regionOfArea = (d: WasmDoc, area: number): Entity => {
    const hit = [...areas(meshOf(d))].find(([, a]) => Math.abs(a - area) < 1e-6);
    expect(hit, `a region of area ${area}`).toBeDefined();
    return { kind: "region", sketch: 1, region: hit![0] };
  };
  /** Run the gesture the tools run: outlines, subject, previews, the op, then movedSelection. */
  async function moved(d: WasmDoc, sel: Entity[], x: Xform, op: (s: Subject) => KernelOp) {
    const outlines = await loadOutlines(async (t) => JSON.parse(d.outline(t.kind, (t as { sketch: number }).sketch, (t as { region: number }).region)), sel);
    const s = subjectOf(sel, null, outlines, JSON.parse(d.state_json()) as DocState) as Subject;
    const pv = sketchPreviews(s, meshOf(d));
    const outcome = apply(d, op(s));
    const mesh = meshOf(d);
    const next = movedSelection(sel, s, pv, x, outcome, mesh);
    const all = areas(mesh);
    const ids = next.map((e) => (e.kind === "region" ? e.region : -1));
    // Every selected id is a live region, each once.
    expect(ids.every((r) => all.has(r))).toBe(true);
    expect(new Set(ids).size).toBe(ids.length);
    return { ids, area: ids.reduce((a, r) => a + all.get(r)!, 0), all };
  }

  it("Rotate: a bar turned 180 degrees across a square's edge is its two halves; the square's remainder is not selected", async () => {
    const d = await doc();
    rect(d, 0, 0, 2, 2);
    rect(d, 4, 0.5, 2, 1);
    const bar = regionOfArea(d, 2);
    const x = rotation([3.5, 1, 0], [0, 0, 1], Math.PI, 180);
    const r = await moved(d, [bar], x, (s) => rotateOp(s, [3.5, 1, 0], [0, 0, 1], Math.PI, false, 1, 180));
    expect(r.ids).toHaveLength(2);
    expect(r.area).toBeCloseTo(2, 5);
    // The square's remainder (3 of its 4) stays unselected.
    const rest = [...r.all].find(([, a]) => Math.abs(a - 3) < 1e-6)!;
    expect(r.ids).not.toContain(rest[0]);
    d.free();
  });

  it("Scale: a triangle mirrored across a square's edge is both its pieces", async () => {
    const d = await doc();
    rect(d, 0, 0, 2, 2);
    draw(d, [[2.5, 0.5], [4, 0.5], [2.5, 1.5]]);
    const tri = regionOfArea(d, 0.75);
    const r = await moved(d, [tri], scaling([2.6, 0, 0], [-1, 1, 1]), (s) => scaleOp(s, [2.6, 0, 0], [-1, 1, 1]));
    expect(r.ids).toHaveLength(2);
    expect(r.area).toBeCloseTo(0.75, 5);
    d.free();
  });

  it("Move: a region with a hole moved across a square is its pieces; its island and the square's remainder are not selected", async () => {
    const d = await doc();
    rect(d, 0, 0, 4, 4);
    rect(d, 10, 0, 6, 6);
    rect(d, 12, 2, 2, 1);
    // The 6 x 6 square less its 2 x 1 island: 34. Moved so it overlaps the 4 x 4 square.
    const ring = regionOfArea(d, 34);
    const r = await moved(d, [ring], translation([-7, -1, 0]), (s) => moveOp(s, [-7, -1, 0], false));
    expect(r.ids).toHaveLength(2);
    expect(r.area).toBeCloseTo(34, 5);
    // Its pieces are 4 (over the square) and 30. Not selected: the island (2, moved along
    // with its hole) and the square's remainder (12).
    expect(r.ids.map((id) => Math.round(r.all.get(id)!)).sort((a, b) => a - b)).toEqual([4, 30]);
    expect([...r.all.values()].map(Math.round).sort((a, b) => a - b)).toEqual([2, 4, 12, 30]);
    d.free();
  });
});

// Lane 43 (#7): the cases lane 41 left, through the real kernel. Each reads the selection the
// tools' own steps make and checks every entity in it against the new view mesh: a region or
// a curve the mesh does not draw names nothing.
runKernel("#7 through the kernel: a rim that splits, a merge and a lift", () => {
  async function doc(): Promise<WasmDoc> {
    const mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as { initSync(o: { module: Buffer }): void; document_new(): WasmDoc };
    mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
    return mod.document_new();
  }
  function meshOf(d: WasmDoc): MeshData {
    const m = d.get_scene_mesh();
    const mesh: MeshData = {
      positions: m.positions(),
      indices: m.indices(),
      faceIds: m.face_ids(),
      edges: m.edges(),
      edgeOwner: m.edge_owner(),
      edgeOwners: JSON.parse(m.edge_owners_json()),
      parts: JSON.parse(m.parts_json()),
      edgeCurve: m.edge_curves(),
      curves: JSON.parse(m.curves_json()),
    };
    m.free();
    return mesh;
  }
  const apply = (d: WasmDoc, op: unknown): OpOutcome => {
    const out = JSON.parse(d.apply_op(JSON.stringify(op)));
    expect(out.error, JSON.stringify(out)).toBeUndefined();
    return out;
  };
  const plane = (z: number) => ({ on: "plane", origin: [0, 0, z], normal: [0, 0, 1] });
  /** A closed stroke at height `z`; the region it made (the last the kernel names). */
  const draw = (d: WasmDoc, pts: [number, number][], z = 0): Entity => {
    const out = apply(d, { op: "draw", target: plane(z), points: pts.map(([x, y]) => [x, y, z]), closed: true }) as Extract<OpOutcome, { kind: "drawn" }>;
    expect(out.sketch).not.toBeNull();
    return { kind: "region", sketch: out.sketch!, region: out.regions.at(-1)! };
  };
  const rect = (d: WasmDoc, x: number, y: number, w: number, h: number, z = 0) => draw(d, [[x, y], [x + w, y], [x + w, y + h], [x, y + h]], z);
  /** A circle of 24 segments on the ground, as the Circle tool draws it: one curve. */
  const circle = (d: WasmDoc, c: [number, number], r: number): Entity => {
    const pts = Array.from({ length: 24 }, (_, k) => [c[0] + r * Math.cos((k * Math.PI) / 12), c[1] + r * Math.sin((k * Math.PI) / 12), 0]);
    const out = apply(d, { op: "draw", target: plane(0), points: pts, closed: true, curve: { kind: "circle", centre: [c[0], c[1], 0] } }) as Extract<OpOutcome, { kind: "drawn" }>;
    expect(out.curve).toBeDefined();
    return { kind: "curve", owner: { kind: "sketch", id: out.sketch! }, curve: out.curve! };
  };
  /** Each region's area in the view mesh, keyed `sketch:region`. */
  function areas(mesh: MeshData): Map<string, number> {
    const out = new Map<string, number>();
    for (let t = 0; t < mesh.faceIds.length; t++) {
      const part = mesh.parts[mesh.faceIds[t]];
      if (part.kind !== "region") continue;
      const [a, b, c] = [0, 1, 2].map((k) => Array.from(mesh.positions.subarray(mesh.indices[t * 3 + k] * 3, mesh.indices[t * 3 + k] * 3 + 3)));
      const area = Math.abs((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2;
      const key = `${part.owner}:${part.index}`;
      out.set(key, (out.get(key) ?? 0) + area);
    }
    return out;
  }
  const key = (e: Entity) => (e.kind === "region" ? `${e.sketch}:${e.region}` : e.kind === "curve" ? `c${e.owner.id}:${e.curve}` : JSON.stringify(e));
  /** Run the gesture the tools run (outlines, subject, previews, the op, movedSelection) and
   * check that everything it selects is drawn in the new mesh, each once. */
  async function moved(d: WasmDoc, sel: Entity[], x: Xform, op: (s: Subject) => KernelOp) {
    const outlines = await loadOutlines(async (t) => {
      const [owner, index] = t.kind === "region" ? [t.sketch, t.region] : t.kind === "curve" ? [t.sketch, t.curve] : [0, 0];
      return JSON.parse(d.outline(t.kind, owner, index));
    }, sel);
    const s = subjectOf(sel, null, outlines, JSON.parse(d.state_json()) as DocState) as Subject;
    const pv = sketchPreviews(s, meshOf(d));
    const kop = op(s);
    const outcome = apply(d, kop);
    const mesh = meshOf(d);
    const next = movedSelection(sel, s, pv, x, outcome, mesh);
    const all = areas(mesh);
    const curves = new Set((mesh.curves ?? []).filter((c) => c.owner.kind === "sketch").map((c) => `c${c.owner.id}:${c.curve}`));
    const keys = next.map(key);
    for (const k of keys) expect(all.has(k) || curves.has(k), `${k} is drawn`).toBe(true);
    expect(new Set(keys).size).toBe(keys.length);
    return { next, keys, op: kop, outcome, mesh, all, curves, subject: s, area: keys.reduce((a, k) => a + (all.get(k) ?? 0), 0) };
  }

  it("a circle's rim selected on its own and moved across a rectangle's edge is every arc it became", async () => {
    const d = await doc();
    rect(d, 0, 0, 4, 2);
    const rim = circle(d, [7, 1], 0.5);
    // Its centre lands on the rectangle's right edge: the rim crosses it twice.
    const r = await moved(d, [rim], translation([-3, 0, 0]), (s) => moveOp(s, [-3, 0, 0], false));
    // The rim is two arcs now, and the selection is both of them.
    expect([...r.curves]).toHaveLength(2);
    expect(r.keys.sort()).toEqual([...r.curves].sort());
    expect(r.next.every((e) => e.kind === "curve")).toBe(true);
    d.free();
  });

  it("a circle's rim moved exactly onto an identical circle is never selected as an id that names nothing", async () => {
    // The older of two curves on the same edges keeps them (ADR-0013): a newer rim moved onto
    // an older circle goes, and nothing is selected for it; an older rim moved onto a newer
    // circle keeps its id and stays selected.
    const d = await doc();
    circle(d, [1, 1], 0.5);
    const newer = circle(d, [4, 1], 0.5);
    const a = await moved(d, [newer], translation([-3, 0, 0]), (s) => moveOp(s, [-3, 0, 0], false));
    expect([...a.curves]).toEqual(["c1:1"]);
    expect(a.keys).toEqual([]);
    d.free();
    const e = await doc();
    const older = circle(e, [4, 1], 0.5);
    circle(e, [1, 1], 0.5);
    const b = await moved(e, [older], translation([-3, 0, 0]), (s) => moveOp(s, [-3, 0, 0], false));
    expect([...b.curves]).toEqual([key(older)]);
    expect(b.keys).toEqual([key(older)]);
    e.free();
  });

  it("a circle's rim lifted onto another plane across a rectangle's edge there is every arc it became there", async () => {
    const d = await doc();
    rect(d, 0, 0, 4, 2, 1);
    const rim = circle(d, [7, 1], 0.5);
    const r = await moved(d, [rim], translation([-3, 0, 1]), (s) => moveOp(s, [-3, 0, 1], false));
    const up = (r.outcome as Extract<OpOutcome, { kind: "sketch_moved" }>).sketch;
    expect([...r.curves].every((k) => k.startsWith(`c${up}:`))).toBe(true);
    expect([...r.curves]).toHaveLength(2);
    expect(r.keys.sort()).toEqual([...r.curves].sort());
    d.free();
  });

  it("a shape moved exactly onto an identical one is selected as the region there, never an id that names nothing", async () => {
    const d = await doc();
    const stays = rect(d, 0, 0, 1, 1);
    const goes = rect(d, 3, 0, 1, 1);
    const r = await moved(d, [goes], translation([-3, 0, 0]), (s) => moveOp(s, [-3, 0, 0], false));
    // The two merge: one region of area 1 is left, and it is what is selected.
    expect([...r.all.values()]).toHaveLength(1);
    expect(r.keys).toEqual([...r.all.keys()]);
    expect(r.area).toBeCloseTo(1, 6);
    // The moved square's own id is gone (the region that stays keeps its id).
    expect(r.all.has(key(goes))).toBe(false);
    expect(r.all.has(key(stays))).toBe(true);
    d.free();
  });

  it("a shape lifted onto another sketch's plane where it cuts a shape is selected as its own pieces only", async () => {
    const d = await doc();
    // A 2 x 2 square up at z = 1, and a 2 x 1 bar on the ground beside it.
    const up = rect(d, 0, 0, 2, 2, 1);
    const bar = rect(d, 3, 0.5, 2, 1);
    expect(bar.kind === "region" && up.kind === "region" && bar.sketch !== up.sketch).toBe(true);
    // Lifted 1 up and 2 to the left: half of it lands over the square's right edge.
    const r = await moved(d, [bar], translation([-2, 0, 1]), (s) => moveOp(s, [-2, 0, 1], false));
    const upSketch = (up as Extract<Entity, { kind: "region" }>).sketch;
    expect(r.next.every((e) => e.kind === "region" && e.sketch === upSketch)).toBe(true);
    // Its two halves (1 each), not the square's remainder (3).
    expect(r.keys.map((k) => Math.round(r.all.get(k)!))).toEqual([1, 1]);
    expect(r.area).toBeCloseTo(2, 6);
    // Push/Pull's target (the bar, before the lift) follows: two pieces, so none. The target
    // the app gave a lifted region (nextTarget: the first region its edges bound there) is
    // the square's remainder here, and the tools used to keep it.
    const b = bar as Extract<Entity, { kind: "region" }>;
    const was = { kind: "region" as const, sketch: b.sketch, region: b.region };
    const given = nextTarget(r.op, r.outcome, was);
    expect(given?.kind === "region" && Math.round(r.all.get(`${given.sketch}:${given.region}`)!)).toBe(3);
    expect(targetAfterMove(was, [bar], r.next, sketchesAfter(r.subject, r.outcome))).toBeNull();
    d.free();
  });
});
