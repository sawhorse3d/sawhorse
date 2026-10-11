import { describe, expect, it } from "vitest";
import type { KernelOp, Picked, SurfaceHit } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { PushTool } from "./push";

const IN = 0.0254;
const top: SurfaceHit = {
  at: { kind: "face", object: 1, face: 2 },
  frame: { origin: [0.5, 0.5, 1], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] },
  point: [0.5, 0.5, 1],
  t: 9,
};
const onTop: Picked = { entity: { kind: "face", object: 1, face: 2 }, point: [0.5, 0.5, 1], surface: top };
const at = (z: number, extra = {}) => ev("move", [0.5, 0.5, z], { picked: onTop, snapKind: "axis-z", ...extra });
const tick = () => new Promise((r) => setTimeout(r, 0));
const last = (h: { ops: KernelOp[] }) => h.ops[h.ops.length - 1] as unknown as Record<string, unknown>;

describe("Push/Pull feel", () => {
  it("press, drag and let go: the face follows the cursor and commits on release", async () => {
    const h = fakeHost();
    const t = new PushTool(h);
    t.onEvent(ev("down", [0.5, 0.5, 1], { picked: onTop }));
    await tick();
    expect(t.busy()).toBe(true);
    t.onEvent({ ...at(1.25), type: "drag", start: [0, 0] });
    expect(h.values.at(-1)).toMatch(/9/); // the live distance (about 9 27/32 in)
    expect(t.view().overlay.length).toBeGreaterThan(0); // the preview (outline loaded)
    t.onEvent({ ...at(1.25), type: "dragend", start: [0, 0] });
    await tick();
    expect(last(h)).toMatchObject({ op: "push_pull", object: 1, face: 2 });
    expect(last(h).distance as number).toBeCloseTo(0.25, 12);
    expect(t.busy()).toBe(false);
  });

  it("click, move, click; a typed distance mid-gesture goes the way the cursor went", async () => {
    const h = fakeHost();
    const t = new PushTool(h);
    t.onEvent(ev("down", [0.5, 0.5, 1], { picked: onTop }));
    t.onEvent(ev("click", [0.5, 0.5, 1], { picked: onTop }));
    expect(t.busy()).toBe(true);
    t.onEvent(at(0.9)); // moved inward
    t.onValue("3/4");
    await tick();
    expect(last(h).distance as number).toBeCloseTo(-0.75 * IN, 12);
    // Click, move, click.
    t.onEvent(ev("down", [0.5, 0.5, 1], { picked: onTop }));
    t.onEvent(ev("click", [0.5, 0.5, 1], { picked: onTop }));
    t.onEvent(at(1.5));
    t.onEvent(ev("down", [0.5, 0.5, 1.5], { picked: onTop }));
    t.onEvent(ev("click", [0.5, 0.5, 1.5], { picked: onTop, snapKind: "axis-z" }));
    await tick();
    expect(last(h).distance as number).toBeCloseTo(0.5, 12);
  });

  it("double-click repeats the last distance; Ctrl pulls out a new face", async () => {
    const h = fakeHost();
    const t = new PushTool(h);
    t.onEvent(ev("down", [0.5, 0.5, 1], { picked: onTop }));
    t.onEvent({ ...at(1.1), type: "drag", start: [0, 0] });
    t.onEvent({ ...at(1.1), type: "dragend", start: [0, 0], ctrl: true });
    await tick();
    expect(last(h)).toMatchObject({ op: "push_pull_new_face" });
    // A double-click elsewhere: down, click, down, click, dblclick.
    const other: Picked = { ...onTop, entity: { kind: "face", object: 1, face: 5 }, surface: { ...top, at: { kind: "face", object: 1, face: 5 } } };
    const n = h.ops.length;
    for (const type of ["down", "click", "down", "click", "dblclick"] as const) t.onEvent(ev(type, [0.5, 0.5, 1], { picked: other }));
    await tick();
    expect(h.ops.length).toBe(n + 1);
    expect(last(h)).toMatchObject({ op: "push_pull", face: 5 });
    expect(last(h).distance as number).toBeCloseTo(0.1, 12);
  });

  it("with no gesture a typed distance applies to the last thing drawn", async () => {
    const h = fakeHost({ _target: { kind: "region", sketch: 1, region: 4 } });
    const t = new PushTool(h);
    t.onValue("12");
    await tick();
    expect(last(h)).toMatchObject({ op: "extrude", sketch: 1, region: 4 });
    t.onValue("x3");
    expect(h.statuses.at(-1)?.text).toMatch(/lane 4/);
  });
});
