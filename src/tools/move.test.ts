import { describe, expect, it } from "vitest";
import type { Entity, KernelOp, MeshData, Picked, V3 } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { MoveTool, moveOp } from "./move";
import { setNameSource } from "../lib/names";
import type { DocState } from "../kernel/types";
import { applyX, describeSubject, previewSegments, rotation, scaling, subjectOf, translation } from "./xform";

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
    expect(subjectOf([{ kind: "region", sketch: 1, region: 1 }], null, new Map())).toMatch(/Sketch regions/);
    expect(subjectOf([face, { kind: "face", object: 2, face: 1 }], null, outlines)).toMatch(/one solid at a time/);
    expect(subjectOf([], null, new Map())).toMatch(/Click a solid/);
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

  it("refuses to pick up a sketch region, typed", async () => {
    const h = fakeHost();
    h.sel = [{ kind: "region", sketch: 1, region: 1 }];
    const t = new MoveTool(h);
    t.onEvent(ev("down", [0, 0, 0]));
    await tick();
    expect(t.busy()).toBe(false);
    expect(h.statuses.at(-1)).toMatchObject({ error: true });
  });
});
