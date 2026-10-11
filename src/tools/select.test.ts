import { describe, expect, it } from "vitest";
import type { Entity, Picked } from "../kernel/types";
import { EraseTool } from "./erase";
import { ev, fakeHost } from "./fakeHost";
import { SelectTool } from "./select";

const face: Entity = { kind: "face", object: 1, face: 2 };
const other: Entity = { kind: "face", object: 1, face: 3 };
const edge: Entity = { kind: "edge", owner: { kind: "object", id: 1 }, a: [0, 0, 1], b: [1, 0, 1], drawn: true };
const pickOf = (e: Entity): Picked => ({ entity: e, point: [0.5, 0.5, 1], surface: null });

describe("Select tool", () => {
  it("hovers, clicks, adds with Shift, toggles with Ctrl, clears on empty click", () => {
    const h = fakeHost();
    const t = new SelectTool(h);
    t.onEvent(ev("move", [0.5, 0.5, 1], { picked: pickOf(face) }));
    expect(h.hovered).toEqual(face);
    t.onEvent(ev("click", [0.5, 0.5, 1], { picked: pickOf(face) }));
    expect(h.sel).toEqual([face]);
    t.onEvent(ev("click", [0.5, 0.5, 1], { picked: pickOf(other), shift: true }));
    expect(h.sel).toEqual([face, other]);
    t.onEvent(ev("click", [0.5, 0.5, 1], { picked: pickOf(face), ctrl: true }));
    expect(h.sel).toEqual([other]);
    t.onEvent(ev("click", [0.5, 0.5, 1], { picked: pickOf(edge), ctrl: true }));
    expect(h.sel).toEqual([other, edge]);
    t.onEvent(ev("click", [5, 5, 0], { picked: null }));
    expect(h.sel).toEqual([]);
  });

  it("double-click selects the whole solid", () => {
    const h = fakeHost({ _state: { groups: [] } as never });
    const t = new SelectTool(h);
    t.onEvent(ev("dblclick", [0.5, 0.5, 1], { picked: pickOf(face) }));
    expect(h.sel).toEqual([{ kind: "object", object: 1 }]);
  });

  it("double-click on a grouped solid enters the group (lane-5 hook)", () => {
    let entered: number | null = null;
    const h = fakeHost({ _state: { groups: [{ id: 3, name: "Carcass", objects: [1] }] } as never, enterGroup: (g) => (entered = g) });
    new SelectTool(h).onEvent(ev("dblclick", [0.5, 0.5, 1], { picked: pickOf(face) }));
    expect(entered).toBe(3);
  });

  it("drags a window left to right and a crossing box right to left", () => {
    const h = fakeHost();
    const t = new SelectTool(h);
    t.onEvent(ev("drag", [1, 0, 0], { x: 100, y: 50, start: [10, 10] }));
    expect(t.view().box).toMatchObject({ x0: 10, x1: 100, crossing: false });
    expect(t.busy()).toBe(true);
    t.onEvent(ev("drag", [1, 0, 0], { x: 0, y: 50, start: [10, 10] }));
    expect(t.view().box?.crossing).toBe(true);
    expect(t.cancel()).toBe(true);
    expect(t.view().box).toBeNull();
  });
});

describe("Erase tool", () => {
  it("erases what is clicked through the kernel, and reports refusals", async () => {
    const h = fakeHost();
    const t = new EraseTool(h);
    t.onEvent(ev("click", [0.5, 0, 1], { picked: pickOf(edge) }));
    await Promise.resolve();
    expect(h.ops).toEqual([{ op: "erase_edge", object: 1, a: [0, 0, 1], b: [1, 0, 1] }]);
    t.onEvent(ev("click", [0, 0, 1], { picked: pickOf({ kind: "vertex", owner: { kind: "object", id: 1 }, at: [0, 0, 1] }) }));
    expect(h.statuses.at(-1)?.error).toBe(true);
    expect(h.ops).toHaveLength(1);
  });

  it("a drag across edges erases them all as one batch", async () => {
    const h = fakeHost();
    const t = new EraseTool(h);
    const e2: Entity = { ...edge, a: [1, 0, 1], b: [1, 1, 1] } as Entity;
    t.onEvent(ev("drag", [0.5, 0, 1], { picked: pickOf(edge), start: [0, 0] }));
    t.onEvent(ev("drag", [1, 0.5, 1], { picked: pickOf(e2), start: [0, 0] }));
    t.onEvent(ev("drag", [1, 0.6, 1], { picked: pickOf(e2), start: [0, 0] }));
    expect(t.view().overlay[0].points).toHaveLength(4);
    t.onEvent(ev("dragend", [1, 1, 1], { start: [0, 0] }));
    await Promise.resolve();
    expect(h.ops).toHaveLength(1);
    expect(h.ops[0]).toMatchObject({ op: "batch" });
    expect((h.ops[0] as { ops: unknown[] }).ops).toHaveLength(2);
  });
});
