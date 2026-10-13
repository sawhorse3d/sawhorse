// The Eraser's modifiers (lane 15): Ctrl softens and smooths, Shift hides, Ctrl+Shift
// unsoftens, exactly as the tool's hint says; plain clicks still erase.
import { describe, expect, it } from "vitest";
import type { Entity, Picked, V3 } from "../kernel/types";
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
    expect(TOOLS.find((t) => t.id === "erase")?.hint).toBe("Click or drag over edges to erase. Ctrl softens and smooths, Shift hides, Ctrl+Shift unsoftens.");
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
    expect(t.inferContext()).toEqual({ pickSoftWithCtrlShift: true });
    expect(edgeFlagWords(7)).toBe("soft, smooth and hidden");
    expect(edgeFlagWords(4)).toBe("hidden");
    expect(edgeFlagWords(0)).toBe("");
  });
});
