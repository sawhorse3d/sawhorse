import { describe, expect, it } from "vitest";
import type { Entity, KernelOp, Picked, SurfaceHit, V3 } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { FollowTool, orientPath, pathOf } from "./follow";

const tick = () => new Promise((r) => setTimeout(r, 0));
const end: SurfaceHit = { at: { kind: "face", object: 1, face: 9 }, frame: { origin: [0, 0.02, 0.29], normal: [-1, 0, 0], u: [0, 1, 0], v: [0, 0, 1] }, point: [0, 0.02, 0.29], t: 3 };
const onEnd: Picked = { entity: { kind: "face", object: 1, face: 9 }, point: [0, 0.02, 0.29], surface: end };
const topRing: V3[] = [
  [0, 0, 0.3],
  [1, 0, 0.3],
  [1, 0.5, 0.3],
  [0, 0.5, 0.3],
];
const edge = (a: V3, b: V3): Entity => ({ kind: "edge", owner: { kind: "object", id: 1 }, a, b, drawn: false });

describe("Follow Me", () => {
  it("takes its path from edges or from a face's edge", async () => {
    const none = async () => [];
    expect(await pathOf([], none)).toMatch(/Select the path first/);
    const chain = await pathOf([edge([1, 0, 0], [1, 1, 0]), edge([0, 0, 0], [1, 0, 0])], none);
    expect(chain).toMatchObject({ closed: false, what: "2 edges" });
    const face = await pathOf([{ kind: "face", object: 1, face: 2 }], async () => [topRing]);
    expect(face).toMatchObject({ closed: true, points: topRing, what: "the edge of face 2 of a solid" });
    expect(await pathOf([{ kind: "face", object: 1, face: 2 }, edge([0, 0, 0], [1, 0, 0])], none)).toMatch(/select just those/);
  });

  it("starts an open path at the end nearest the profile", () => {
    const pts: V3[] = [[5, 0, 0], [0, 0, 0]];
    expect(orientPath(pts, false, { origin: [0, 0, 0], normal: [1, 0, 0] })).toEqual([[0, 0, 0], [5, 0, 0]]);
    expect(orientPath(pts, true, { origin: [0, 0, 0], normal: [1, 0, 0] })).toBe(pts);
  });

  it("clicking the profile sweeps it along the selected face's edge", async () => {
    const h = fakeHost({ outcome: { kind: "swept", objects: [1], cut: true } });
    h.sel = [{ kind: "face", object: 1, face: 2 }];
    h.outline = async () => [topRing];
    const t = new FollowTool(h);
    t.activate();
    await tick();
    expect(h.statuses.at(-1)?.text).toContain("closed, 4 segments");
    expect(t.view().overlay[0]).toMatchObject({ kind: "loop" });
    t.onEvent(ev("click", [0, 0.02, 0.29], { picked: onEnd }));
    await tick();
    const op = h.ops.at(-1) as unknown as Record<string, unknown>;
    expect(op).toMatchObject({ op: "follow_me", profile: { of: "face", object: 1, face: 9 }, closed: true, path: topRing });
    expect(h.statuses.at(-1)?.text).toContain("cut from");
    expect(h.sel).toEqual([]);
    expect(t.view().overlay).toEqual([]);
  });

  it("refuses a click with no path, or on nothing, typed", async () => {
    const h = fakeHost();
    const t = new FollowTool(h);
    t.activate();
    await tick();
    t.onEvent(ev("click", [0, 0, 0], { picked: onEnd }));
    await tick();
    expect(h.statuses.at(-1)).toMatchObject({ error: true, text: expect.stringContaining("Select the path first") });
    h.sel = [edge([0, 0, 0], [1, 0, 0])];
    t.activate();
    await tick();
    t.onEvent(ev("click", [3, 3, 0]));
    await tick();
    expect(h.statuses.at(-1)).toMatchObject({ error: true, text: expect.stringContaining("Click the profile") });
    expect(h.ops.filter((o: KernelOp) => o.op === "follow_me")).toHaveLength(0);
  });
});
