// The Eraser's modifiers (lane 15): Ctrl softens and smooths, Shift hides, Ctrl+Shift
// unsoftens, exactly as the tool's hint says; plain clicks still erase.
import { describe, expect, it } from "vitest";
import type { DocState, Entity, Picked, V3 } from "../kernel/types";
import { TOOLS } from "../lib/tools";
import { edgeFlagWords } from "../lib/entities";
import { EraseTool, edgeFlagsOp, edgeMode } from "./erase";
import { ev, fakeHost } from "./fakeHost";

const edge = (a: V3, b: V3, flags = 0, owner: "object" | "sketch" = "object"): Entity => ({ kind: "edge", owner: { kind: owner, id: 3 }, a, b, drawn: owner === "sketch", ...(flags ? { flags } : {}) });
const picked = (e: Entity): Picked => ({ entity: e, point: [0, 0, 0], surface: null }) as unknown as Picked;
const E1 = edge([0, 0, 0], [0, 0, 0.0254]);
const E2 = edge([0.05, 0, 0], [0.05, 0, 0.0254], 3);

describe("Eraser modifiers", () => {
  it("the hint says what the modifiers do, and edgeMode does exactly that", () => {
    expect(TOOLS.find((t) => t.id === "erase")?.hint).toBe("Click or drag over edges, guides, dimensions. Ctrl softens and smooths, Shift hides, Ctrl+Shift unsoftens.");
    expect(edgeMode({ ctrl: true, shift: false })).toEqual({ set: 3, clear: 0, verb: "Softened and smoothed" });
    expect(edgeMode({ ctrl: false, shift: true })).toEqual({ set: 4, clear: 0, verb: "Hid" });
    expect(edgeMode({ ctrl: true, shift: true })).toEqual({ set: 0, clear: 7, verb: "Unsoftened" });
    expect(edgeMode({ ctrl: false, shift: false })).toBeNull();
  });

  it("Ctrl+click softens and smooths one edge (one op, nothing erased)", async () => {
    const h = fakeHost({ outcome: { kind: "edges_set", changed: 1 } });
    const t = new EraseTool(h);
    t.onEvent(ev("click", [0, 0, 0], { ctrl: true, picked: picked(E1) }));
    await Promise.resolve();
    expect(h.ops).toEqual([{ op: "set_edge_flags", edges: [{ object: 3, a: E1.kind === "edge" ? E1.a : [0, 0, 0], b: [0, 0, 0.0254] }], set: 3, clear: 0 }]);
    expect(h.statuses.at(-1)?.text).toBe("Softened and smoothed 1 edge.");
  });

  it("Shift+click hides; Ctrl+Shift+click unsoftens; a plain click erases", async () => {
    const h = fakeHost({ outcome: { kind: "edges_set", changed: 1 } });
    const t = new EraseTool(h);
    t.onEvent(ev("click", [0, 0, 0], { shift: true, picked: picked(E1) }));
    t.onEvent(ev("click", [0, 0, 0], { shift: true, ctrl: true, picked: picked(E2) }));
    await Promise.resolve();
    expect(h.ops.map((o) => (o.op === "set_edge_flags" ? [o.set, o.clear] : o.op))).toEqual([
      [4, 0],
      [0, 7],
    ]);
    t.onEvent(ev("click", [0, 0, 0], { picked: picked(E1) }));
    await Promise.resolve();
    expect(h.ops.at(-1)?.op).toBe("erase_edge");
  });

  it("a Ctrl-drag across edges softens them all in one op", async () => {
    const h = fakeHost({ outcome: { kind: "edges_set", changed: 2 } });
    const t = new EraseTool(h);
    t.onEvent(ev("drag", [0, 0, 0], { ctrl: true, picked: picked(E1) }));
    t.onEvent(ev("drag", [0.05, 0, 0], { ctrl: true, picked: picked(E2) }));
    t.onEvent(ev("drag", [0.05, 0, 0], { ctrl: true, picked: picked(E2) }));
    expect(t.busy()).toBe(true);
    t.onEvent(ev("dragend", [0.05, 0, 0], { ctrl: true, picked: picked(E2) }));
    await Promise.resolve();
    expect(h.ops).toHaveLength(1);
    const op = h.ops[0];
    expect(op.op === "set_edge_flags" && op.edges.length).toBe(2);
    expect(h.statuses.at(-1)?.text).toBe("Softened and smoothed 2 edges.");
  });

  it("sketch strokes have no flags: a plain refusal, no op", async () => {
    const h = fakeHost();
    const t = new EraseTool(h);
    t.onEvent(ev("click", [0, 0, 0], { ctrl: true, picked: picked(edge([0, 0, 0], [1, 0, 0], 0, "sketch")) }));
    await Promise.resolve();
    expect(h.ops).toEqual([]);
    expect(h.statuses.at(-1)).toEqual({ text: "Only the edges of solids soften, smooth or hide: click an edge of a solid.", error: true });
    expect(edgeFlagsOp([], { set: 1, clear: 0 })).toBeNull();
  });

  it("hovering an edge says its flags (the edge's tooltip); Ctrl+Shift asks the pick for soft edges", () => {
    const h = fakeHost();
    const t = new EraseTool(h);
    t.onEvent(ev("move", [0, 0, 0], { picked: picked(E2) }));
    expect(h.statuses.at(-1)?.text).toBe("This edge is soft and smooth. Ctrl+Shift+click unsoftens it.");
    t.onEvent(ev("move", [0, 0, 0], { picked: picked(E1) }));
    expect(h.statuses.at(-1)?.text).toBeNull();
    expect(t.inferContext()).toEqual({ pickSoftWithCtrlShift: true, pickDimensions: true });
    expect(edgeFlagWords(7)).toBe("soft, smooth and hidden");
    expect(edgeFlagWords(4)).toBe("hidden");
    expect(edgeFlagWords(0)).toBe("");
  });
});

// #129: the Eraser takes dimensions and guides: its hint says so, a click takes one, and a
// drag across one takes it with the rest of the drag, in the drag's one op.
describe("Eraser, guides and dimensions", () => {
  const DIM: Entity = { kind: "dimension", dimension: 5 };
  const GUIDE: Entity = { kind: "guide", guide: 2 };
  const state = { dimensions: [{ id: 5, a: [0, 0, 0], b: [0.3, 0, 0], offset: [0, -0.1, 0], length: 0.3 }], guides: [{ id: 2, point: [0, 0.2, 0], dir: [1, 0, 0] }], sketches: [] } as unknown as DocState;

  it("the hint names edges, guides and dimensions, and the tool asks for a dimension's whole hit area", () => {
    const hint = TOOLS.find((t) => t.id === "erase")!.hint;
    for (const w of ["edges", "guides", "dimensions"]) expect(hint).toContain(w);
    expect(new EraseTool(fakeHost()).inferContext().pickDimensions).toBe(true);
  });

  it("a click on a dimension erases it", async () => {
    const h = fakeHost({ _state: state, outcome: { kind: "erased" } as never });
    new EraseTool(h).onEvent(ev("click", [0, 0, 0], { picked: picked(DIM) }));
    await Promise.resolve();
    expect(h.ops).toEqual([{ op: "erase_dimension", dimension: 5 }]);
  });

  it("a drag across an edge, a dimension and a guide erases all three in one op, drawn as it sweeps", async () => {
    const h = fakeHost({ _state: state, outcome: { kind: "batch", outcomes: [] } as never });
    const t = new EraseTool(h);
    t.onEvent(ev("drag", [0, 0, 0], { picked: picked(E1) }));
    t.onEvent(ev("drag", [0.1, -0.1, 0], { picked: picked(DIM) }));
    t.onEvent(ev("drag", [0.1, -0.1, 0], { picked: picked(DIM) }));
    t.onEvent(ev("drag", [0.1, 0.2, 0], { picked: picked(GUIDE) }));
    const drawn = t.view().overlay[0] as { points: V3[] };
    // The edge (2 points), the dimension's three lines (6) and the guide (2).
    expect(drawn.points).toHaveLength(10);
    t.onEvent(ev("dragend", [0.1, 0.2, 0], { picked: picked(GUIDE) }));
    await Promise.resolve();
    expect(h.ops).toHaveLength(1);
    const op = h.ops[0] as unknown as { op: string; ops: { op: string }[] };
    expect(op.op).toBe("batch");
    expect(op.ops.map((o) => o.op)).toEqual(["erase_edge", "erase_dimension", "erase_guide"]);
    expect(h.statuses.at(-1)?.text).toBe("Erased 3 items.");
  });

  it("with Ctrl held a drag softens the edges it crossed and leaves dimensions and guides alone", async () => {
    const h = fakeHost({ _state: state, outcome: { kind: "edges_set", changed: 1 } });
    const t = new EraseTool(h);
    t.onEvent(ev("drag", [0, 0, 0], { ctrl: true, picked: picked(E1) }));
    t.onEvent(ev("drag", [0.1, -0.1, 0], { ctrl: true, picked: picked(DIM) }));
    t.onEvent(ev("drag", [0.1, 0.2, 0], { ctrl: true, picked: picked(GUIDE) }));
    t.onEvent(ev("dragend", [0.1, 0.2, 0], { ctrl: true, picked: picked(GUIDE) }));
    await Promise.resolve();
    expect(h.ops).toHaveLength(1);
    expect(h.ops[0].op).toBe("set_edge_flags");
  });
});

// A sweep whose release never reached the view (the button let go outside it) must not be
// erased by a later drag: a new press starts a new sweep.
describe("Eraser, a sweep let go outside the view", () => {
  const DIM: Entity = { kind: "dimension", dimension: 5 };
  const state = { dimensions: [{ id: 5, a: [0, 0, 0], b: [0.3, 0, 0], offset: [0, -0.1, 0], length: 0.3 }], guides: [], sketches: [] } as unknown as DocState;

  it("the next press drops it: a later small drag over nothing erases nothing", async () => {
    const h = fakeHost({ _state: state, outcome: { kind: "erased" } as never });
    const t = new EraseTool(h);
    t.onEvent(ev("down", [0, -0.2, 0]));
    t.onEvent(ev("drag", [0.1, -0.1, 0], { picked: picked(DIM) }));
    // No dragend: the button came up outside the view. Back in, a new press and a small drag.
    t.onEvent(ev("down", [0.5, 0.5, 0]));
    t.onEvent(ev("drag", [0.52, 0.5, 0]));
    t.onEvent(ev("dragend", [0.52, 0.5, 0]));
    await Promise.resolve();
    expect(h.ops).toEqual([]);
    expect(t.view().overlay).toEqual([]);
  });

  it("a drag that goes on without a new press still erases what it crossed", async () => {
    const h = fakeHost({ _state: state, outcome: { kind: "erased" } as never });
    const t = new EraseTool(h);
    t.onEvent(ev("down", [0, -0.2, 0]));
    t.onEvent(ev("drag", [0.1, -0.1, 0], { picked: picked(DIM) }));
    t.onEvent(ev("drag", [0.1, 0.1, 0]));
    t.onEvent(ev("dragend", [0.1, 0.1, 0]));
    await Promise.resolve();
    expect(h.ops).toEqual([{ op: "erase_dimension", dimension: 5 }]);
  });
});
