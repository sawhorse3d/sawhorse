import { describe, expect, it } from "vitest";
import type { MeshPart, V3 } from "../kernel/types";
import {
  GROUND,
  frameOfPart,
  nextTarget,
  partIndex,
  pushOp,
  rayPlane,
  rectCorners,
  rectangleOp,
  sameTarget,
  targetOfPart,
  toLocal,
  toWorld,
  type Frame,
} from "./edit";

const top: MeshPart = { kind: "face", owner: 1, index: 1, origin: [0.3, 0.2, 0.3], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] };
const side: MeshPart = { kind: "face", owner: 1, index: 2, origin: [0.3, 0, 0.15], normal: [0, -1, 0], u: [1, 0, 0], v: [0, 0, 1] };

describe("tool ops", () => {
  it("frames round-trip points", () => {
    const f: Frame = frameOfPart(side);
    const p = toWorld(f, 0.25, -0.1);
    expect(toLocal(f, p)).toEqual([0.25, -0.1].map((x) => expect.closeTo(x, 12)));
  });

  it("rectangles on the ground are sketch ops, any corner order", () => {
    expect(rectangleOp({ frame: GROUND, point: [1, 2, 0], face: null }, [0.5, 1])).toEqual({
      op: "add_rectangle",
      sketch: 1,
      x: 1,
      y: 2,
      w: -0.5,
      h: -1,
    });
    expect(rectangleOp({ frame: GROUND, point: [0, 0, 0], face: null }, null, [0.6096, 0.4064])).toMatchObject({ w: 0.6096, h: 0.4064 });
  });

  it("rectangles on an object face are imprints in the face's frame", () => {
    const op = rectangleOp({ frame: frameOfPart(top), point: [0.1, 0.1, 0.3], face: { object: 1, face: 1 } }, null, [0.2, 0.1]);
    expect(op.op).toBe("imprint");
    if (op.op !== "imprint") return;
    expect(op.object).toBe(1);
    expect(op.points).toHaveLength(4);
    for (const p of op.points) expect(p[2]).toBeCloseTo(0.3, 12);
    expect(op.points[2][0]).toBeCloseTo(0.3, 12);
    expect(op.points[2][1]).toBeCloseTo(0.2, 12);
  });

  it("corners are counter-clockwise whatever the drag direction", () => {
    expect(rectCorners([1, 1], [0, 0])).toEqual([
      [0, 0],
      [1, 0],
      [1, 1],
      [0, 1],
    ]);
  });

  it("push targets: regions extrude, faces push/pull", () => {
    expect(pushOp({ kind: "region", sketch: 1, region: 3 }, 0.3)).toEqual({ op: "extrude", sketch: 1, region: 3, distance: 0.3 });
    expect(pushOp({ kind: "face", object: 2, face: 6 }, -0.1)).toEqual({ op: "push_pull", object: 2, face: 6, distance: -0.1 });
  });

  it("the next push target follows what was just made", () => {
    const rect = rectangleOp({ frame: GROUND, point: [0, 0, 0], face: null }, [1, 1]);
    expect(nextTarget(rect, { kind: "sketch_edited", regions: [4] }, null)).toEqual({ kind: "region", sketch: 1, region: 4 });
    expect(nextTarget({ op: "extrude", sketch: 1, region: 4, distance: 1 }, { kind: "object_created", object: 7 }, null)).toEqual({
      kind: "face",
      object: 7,
      face: 2, // the cap: persistent id 2
    });
    const imp = rectangleOp({ frame: frameOfPart(top), point: [0.1, 0.1, 0.3], face: { object: 1, face: 1 } }, [0.2, 0.2]);
    expect(nextTarget(imp, { kind: "imprinted", face: 6 }, null)).toEqual({ kind: "face", object: 1, face: 6 });
    expect(nextTarget({ op: "push_pull", object: 1, face: 6, distance: 1 }, { kind: "face_moved" }, null)).toEqual({ kind: "face", object: 1, face: 6 });
    // Lane 22: a region moved in its plane stays the target; lifted, its new self is.
    const was = { kind: "region" as const, sketch: 1, region: 4 };
    const id = { linear: [[1, 0, 0], [0, 1, 0], [0, 0, 1]] as [V3, V3, V3], translation: [0, 0, 1] as V3 };
    const lift = { op: "transform_sketch" as const, sketch: 1, regions: [4], xform: id };
    expect(nextTarget(lift, { kind: "sketch_moved", sketch: 1, regions: [[1, 4]] }, was)).toEqual(was);
    expect(nextTarget(lift, { kind: "sketch_moved", sketch: 3, regions: [[3, 1]] }, was)).toEqual({ kind: "region", sketch: 3, region: 1 });
    expect(nextTarget({ ...lift, regions: [5] }, { kind: "sketch_moved", sketch: 3, regions: [[3, 1]] }, was)).toEqual(was);
  });

  it("parts and targets match", () => {
    expect(sameTarget(targetOfPart(top), { kind: "face", object: 1, face: 1 })).toBe(true);
    expect(partIndex([side, top], { kind: "face", object: 1, face: 1 })).toBe(1);
    expect(partIndex([side, top], null)).toBe(-1);
  });

  it("rays meet planes only in front", () => {
    expect(rayPlane([0, 0, 5], [0, 0, -1], GROUND)).toEqual([0, 0, 0]);
    expect(rayPlane([0, 0, 5], [0, 0, 1], GROUND)).toBeNull();
    expect(rayPlane([0, 0, 5], [1, 0, 0], GROUND)).toBeNull();
  });
});
