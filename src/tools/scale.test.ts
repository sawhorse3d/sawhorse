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
  it("scales sketch geometry with a transform_sketch op (lane 22)", () => {
    const part = { sketch: 1, points: [], edges: [], regions: [1], rings: [], origin: [0, 0, 0] as V3, normal: [0, 0, 1] as V3, moving: [] };
    const op = scaleOp({ kind: "objects", objects: [], sketches: [part] }, [0, 0, 0], [2, 2, 1]) as { op: string; xform: Xform };
    expect(op.op).toBe("transform_sketch");
    close(applyX(op.xform, [1, 1, 0]), [2, 2, 0]);
  });

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

  // #76: Undo and Redo, and another model in this one's place, are not the tool's own steps;
  // the app tells it with reset() then modelChanged(), the host already answering for the
  // model as it is now.
  /** The solid after the kernel stretched it to `x` m along red. */
  const stretched = (x: number) => ({ ...state, solids: [{ ...state.solids[0], max: [x, 1, 1] }] }) as unknown as DocState;
  const gripsAt = (t: ScaleTool) => t.view().labels.filter((l) => l.className?.startsWith("grip")).map((l) => l.at);
  /** A solid selected and stretched to 3 m by its +red face grip. */
  async function scaled() {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] }, _state: state, toScreen: proj });
    h.sel = [{ kind: "object", object: 1 }];
    const t = new ScaleTool(h);
    t.activate();
    await tick();
    t.onEvent(at("down", [2, 0.5, 0.5]));
    t.onEvent(at("click", [2, 0.5, 0.5]));
    h._state = stretched(3);
    t.onValue("1.5");
    await tick();
    expect(Math.max(...gripsAt(t).map((p) => p[0]))).toBeCloseTo(3, 12);
    return { h, t };
  }

  it("after Undo the box and its grips are round the selection as it is now (#76)", async () => {
    const { h, t } = await scaled();
    h._state = state;
    t.reset();
    t.modelChanged();
    await tick();
    expect(gripsAt(t)).toHaveLength(26);
    expect(Math.max(...gripsAt(t).map((p) => p[0]))).toBeCloseTo(2, 12);
    const box = t.view().overlay.find((o) => o.kind === "segments" && o.dashed)!;
    expect(Math.max(...(box as { points: V3[] }).points.map((p) => p[0]))).toBeCloseTo(2, 12);
  });

  it("a value typed after Undo scales the selection and undoes nothing more (#76)", async () => {
    const { h, t } = await scaled();
    h._state = state;
    t.reset();
    t.modelChanged();
    await tick();
    const n = h.ops.length;
    t.onValue("2");
    await tick();
    // Not "undo, then the last grip again": that scale is no longer the last step.
    expect(ops(h).slice(n)).toMatchObject([{ op: "transform", objects: [1] }]);
    // Uniform about the centre (1, 0.5, 0.5), as when no grip has been picked.
    close(applyX(ops(h).at(-1)!.xform as Xform, [2, 1, 1]), [3, 1.5, 1.5]);
  });

  it("another model in this one's place: no box is left over it (#76)", async () => {
    const { h, t } = await scaled();
    h.sel = [];
    h._state = { ...state, solids: [], stats: { objects: 0 } } as unknown as DocState;
    t.reset();
    t.modelChanged();
    await tick();
    expect(gripsAt(t)).toHaveLength(0);
    expect(t.view().overlay).toHaveLength(0);
  });

  it("the status says which key is held and which is tapped (#81)", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] }, _state: state, toScreen: proj });
    h.sel = [{ kind: "object", object: 1 }];
    const t = new ScaleTool(h);
    t.activate();
    await tick();
    // Shift applies while it is down (onShift); Ctrl switches on a tap (onModifier).
    expect(h.statuses.at(-1)?.text).toContain("Hold Shift: uniform. Tap Ctrl: switch between the opposite grip and the centre.");
    // Over a grip, the line that stays up while the pointer is on it.
    t.onEvent(at("move", [2, 0.5, 0.5]));
    expect(h.statuses.at(-1)?.text).toMatch(/^A face grip.*Hold Shift: uniform\. Tap Ctrl: switch between the opposite grip and the centre\.$/);
  });
});
