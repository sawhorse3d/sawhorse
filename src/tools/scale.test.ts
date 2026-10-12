import { describe, expect, it } from "vitest";
import type { DocState, KernelOp, V3, Xform } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { ScaleTool, factorsFor, fixedPoint, grips, parseScale, scaleOp } from "./scale";
import { applyX } from "./xform";

const IN = 0.0254;
const tick = () => new Promise((r) => setTimeout(r, 0));
const ops = (h: { ops: KernelOp[] }) => h.ops.map((o) => o as unknown as Record<string, unknown>);
const close = (a: V3, b: V3, d = 12) => a.forEach((x, k) => expect(x).toBeCloseTo(b[k], d));
/** A 2 x 1 x 1 m solid (object 1) in the document state. */
const state = { solids: [{ id: 1, number: 1, min: [0, 0, 0], max: [2, 1, 1], volume: 2 }], stats: { objects: 1 } } as unknown as DocState;
/** An oblique projection (so no two grips share a screen point) and events at it. */
const proj = (p: V3): [number, number] => [p[0] * 100 + p[2] * 37, -p[1] * 100 - p[2] * 23];
const at = (type: "move" | "down" | "click" | "drag" | "dragend", p: V3, extra = {}) => {
  const [x, y] = proj(p);
  return { ...ev(type, p, extra), x, y };
};

describe("scale helpers", () => {
  it("26 grips on a box, 8 on a flat face", () => {
    expect(grips([0, 0, 0], [1, 1, 1])).toHaveLength(26);
    const flat = grips([0, 0, 1], [1, 1, 1]);
    expect(flat).toHaveLength(8);
    expect(flat.every((g) => g.code[2] === 0 && g.at[2] === 1)).toBe(true);
  });

  it("scales about the opposite grip or the centre; uniform with Shift", () => {
    const corner = grips([0, 0, 0], [2, 1, 1]).find((g) => g.code.join() === "1,1,1")!;
    expect(fixedPoint(corner, [0, 0, 0], [2, 1, 1], false)).toEqual([0, 0, 0]);
    expect(fixedPoint(corner, [0, 0, 0], [2, 1, 1], true)).toEqual([1, 0.5, 0.5]);
    const face = grips([0, 0, 0], [2, 1, 1]).find((g) => g.code.join() === "1,0,0")!;
    expect(factorsFor(face, 2, false)).toEqual([2, 1, 1]);
    expect(factorsFor(face, 2, true)).toEqual([2, 2, 2]);
  });

  it("reads factors, three factors and absolute sizes", () => {
    expect(parseScale("2", "frac-in")).toEqual({ kind: "factor", t: 2 });
    expect(parseScale("-1", "frac-in")).toEqual({ kind: "factor", t: -1 });
    expect(parseScale("3/4", "frac-in")).toEqual({ kind: "factor", t: 0.75 });
    expect(parseScale("2,1,0.5", "frac-in")).toEqual({ kind: "factors", f: [2, 1, 0.5] });
    const s = parseScale("48in", "frac-in");
    expect(s.kind).toBe("size");
    expect((s as { m: number }).m).toBeCloseTo(48 * IN, 12);
    expect(parseScale("1200mm", "frac-in")).toMatchObject({ kind: "size" });
    expect(parseScale("2,x", "frac-in").kind).toBe("error");
  });

  it("builds whole-solid and part ops", () => {
    const op = scaleOp({ kind: "objects", objects: [1] }, [0, 0, 0], [2, 1, 1]) as { xform: Xform };
    close(applyX(op.xform, [1, 1, 1]), [2, 1, 1]);
    expect(scaleOp({ kind: "geometry", object: 1, points: [[0, 0, 0]], what: "" }, [0, 0, 0], [0.5, 0.5, 1])).toMatchObject({ op: "transform_geometry" });
  });
});

describe("Scale tool", () => {
  it("drag a face grip: stretches one way about the opposite face", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] }, _state: state, toScreen: proj });
    h.sel = [{ kind: "object", object: 1 }];
    const t = new ScaleTool(h);
    t.activate();
    await tick();
    // The grip at the middle of the +red face is at (2, 0.5, 0.5).
    t.onEvent(at("move", [2, 0.5, 0.5]));
    expect(t.view().labels.filter((l) => l.className === "grip hot")).toHaveLength(1);
    t.onEvent(at("down", [2, 0.5, 0.5]));
    t.onEvent({ ...at("drag", [3, 0.5, 0.5]), start: [200, -50] });
    expect(t.view().labels.some((l) => l.text === "×1.5 along red")).toBe(true);
    t.onEvent({ ...at("dragend", [3, 0.5, 0.5]), start: [200, -50] });
    await tick();
    const op = ops(h).at(-1)!;
    expect(op).toMatchObject({ op: "transform", objects: [1] });
    close(applyX(op.xform as Xform, [2, 1, 1]), [3, 1, 1]);
    close(applyX(op.xform as Xform, [0, 0, 0]), [0, 0, 0]);
    expect(h.statuses.at(-1)?.text).toContain("×1.5 along red about the opposite grip");
  });

  it("a corner grip with a typed factor; Ctrl about the centre; Shift uniform", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] }, _state: state, toScreen: proj });
    h.sel = [{ kind: "object", object: 1 }];
    const t = new ScaleTool(h);
    t.activate();
    await tick();
    t.onModifier("ctrl");
    t.onEvent(at("down", [2, 1, 1]));
    t.onEvent(at("click", [2, 1, 1]));
    t.onValue("0.5");
    await tick();
    const op = ops(h).at(-1)!;
    // About the centre (1, 0.5, 0.5): the far corner comes in halfway.
    close(applyX(op.xform as Xform, [2, 1, 1]), [1.5, 0.75, 0.75]);
    expect(h.statuses.at(-1)?.text).toContain("×0.5 uniform about its centre");
    // A face grip with Shift held: all three ways.
    t.onModifier("ctrl");
    t.onShift(true);
    t.onEvent(at("down", [1, 0.5, 1]));
    t.onEvent(at("click", [1, 0.5, 1]));
    t.onValue("2");
    await tick();
    expect(h.statuses.at(-1)?.text).toContain("×2 uniform");
  });

  it("an absolute size along the grip; typed again after the scale changes it", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] }, _state: state, toScreen: proj });
    h.sel = [{ kind: "object", object: 1 }];
    const t = new ScaleTool(h);
    t.activate();
    await tick();
    t.onEvent(at("down", [2, 0.5, 0.5]));
    t.onEvent(at("click", [2, 0.5, 0.5]));
    t.onValue("1m");
    await tick();
    close(applyX(ops(h).at(-1)!.xform as Xform, [2, 1, 1]), [1, 1, 1]);
    t.onValue("3");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "transform" }]);
    close(applyX(ops(h).at(-1)!.xform as Xform, [2, 1, 1]), [6, 1, 1]);
  });

  it("a zero factor is refused before the kernel; a press away from the grips says why", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] }, _state: state, toScreen: proj });
    h.sel = [{ kind: "object", object: 1 }];
    const t = new ScaleTool(h);
    t.activate();
    await tick();
    t.onEvent(at("down", [5, 5, 5]));
    expect(h.statuses.at(-1)).toMatchObject({ error: true, text: expect.stringContaining("Press on a grip") });
    t.onEvent(at("down", [2, 0.5, 0.5]));
    t.onEvent(at("click", [2, 0.5, 0.5]));
    const n = h.ops.length;
    t.onValue("0");
    await tick();
    expect(h.ops.length).toBe(n);
    expect(h.statuses.at(-1)).toMatchObject({ error: true, text: expect.stringContaining("flatten") });
  });
});
