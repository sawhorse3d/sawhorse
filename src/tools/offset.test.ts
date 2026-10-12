import { describe, expect, it } from "vitest";
import type { Entity, KernelOp, Picked, SurfaceHit, V3 } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { OffsetTool, chainNormal, chainOf, chainSide, faceSide } from "./offset";

const IN = 0.0254;
const tick = () => new Promise((r) => setTimeout(r, 0));
const ops = (h: { ops: KernelOp[] }) => h.ops.map((o) => o as unknown as Record<string, unknown>);
const top: SurfaceHit = { at: { kind: "face", object: 1, face: 2 }, frame: { origin: [0.5, 0.5, 1], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }, point: [0.5, 0.5, 1], t: 9 };
const onTop = (p: V3): Picked => ({ entity: { kind: "face", object: 1, face: 2 }, point: p, surface: { ...top, point: p } });
/** An event looking straight down at (x, y) on the top (z = 1). */
const at = (type: "move" | "down" | "click", x: number, y: number) => ev(type, [x, y, 1], { picked: onTop([x, y, 1]), ray: { origin: [x, y, 10], dir: [0, 0, -1] } });
const unitSquare: V3[] = [
  [0, 0, 1],
  [1, 0, 1],
  [1, 1, 1],
  [0, 1, 1],
];

describe("offset helpers", () => {
  it("joins selected edges into one chain, open or closed", () => {
    const open = chainOf([
      [[1, 0, 0], [1, 1, 0]],
      [[0, 0, 0], [1, 0, 0]],
    ]);
    expect(open).toMatchObject({ closed: false });
    expect((open as { points: V3[] }).points).toHaveLength(3);
    const ring = chainOf(unitSquare.map((p, k) => [p, unitSquare[(k + 1) % 4]] as [V3, V3]));
    expect(ring).toMatchObject({ closed: true });
    expect((ring as { points: V3[] }).points).toHaveLength(4);
    expect(
      chainOf([
        [[0, 0, 0], [1, 0, 0]],
        [[0, 0, 0], [0, 1, 0]],
        [[0, 0, 0], [0, 0, 1]],
      ]),
    ).toMatch(/branch/);
    expect(chainOf([[[0, 0, 0], [1, 0, 0]], [[5, 5, 0], [6, 5, 0]]])).toMatch(/not one chain/);
    expect(chainNormal(unitSquare)).toEqual([0, 0, 1]);
    expect(chainNormal([[0, 0, 0], [1, 0, 0]])).toEqual([0, 0, 0]);
  });

  it("measures signed distances: inside a face is negative, left of a chain positive", () => {
    expect(faceSide([0.5, 0.5, 1], [unitSquare], [0, 0, 1])).toBeCloseTo(-0.5, 12);
    expect(faceSide([1.25, 0.5, 1], [unitSquare], [0, 0, 1])).toBeCloseTo(0.25, 12);
    expect(chainSide([0.5, 0.2, 0], [[0, 0, 0], [1, 0, 0]], [0, 0, 1])).toBeCloseTo(0.2, 12);
    expect(chainSide([0.5, -0.2, 0], [[0, 0, 0], [1, 0, 0]], [0, 0, 1])).toBeCloseTo(-0.2, 12);
  });
});

describe("Offset tool", () => {
  it("press on the edge of a face, move inward, click: the kernel's preview, then the op", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: null, regions: [], faces: [2, 7], inside: 7 } });
    h.outline = async () => [unitSquare];
    const t = new OffsetTool(h);
    t.onEvent(at("down", 0.5, 0));
    await tick();
    t.onEvent(at("click", 0.5, 0));
    expect(t.busy()).toBe(true);
    t.onEvent(at("move", 0.5, 0.25));
    await tick();
    await tick();
    const loop = t.view().overlay.find((o) => o.kind === "loop");
    expect(loop?.points[0]).toEqual([0.25, 0.25, 1]);
    t.onEvent(at("click", 0.5, 0.25));
    await tick();
    const op = ops(h).at(-1)!;
    expect(op).toMatchObject({ op: "offset", source: { of: "face", object: 1, face: 2 } });
    expect(op.distance as number).toBeCloseTo(-0.25, 12);
    expect(h.statuses.at(-1)?.text).toContain("inward, on the solid");
  });

  it("typed mid-gesture goes the way the cursor went; typed after changes it; too far shows", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: 3, regions: [1], faces: [], inside: 1 } });
    h.outline = async () => [unitSquare];
    const t = new OffsetTool(h);
    t.onEvent(at("down", 1, 0.5));
    await tick();
    t.onEvent(at("click", 1, 0.5));
    t.onEvent(at("move", 1.3, 0.5));
    t.onValue("1");
    await tick();
    expect(ops(h).at(-1)!.distance as number).toBeCloseTo(IN, 12);
    expect(h.statuses.at(-1)?.text).toContain("outward, into a sketch on its plane");
    t.onValue("2");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "offset" }]);
    expect(ops(h).at(-1)!.distance as number).toBeCloseTo(2 * IN, 12);
    // Too far inward: the preview says so before any click.
    t.onEvent(at("down", 0.5, 0));
    await tick();
    t.onEvent(at("click", 0.5, 0));
    t.onEvent(at("move", 0.5, 0.49));
    await tick();
    await tick();
    expect(t.view().labels.some((l) => l.text.includes("too far"))).toBe(true);
  });

  it("offsets a selected chain of edges to the side of the cursor", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: null, regions: [], faces: [2, 8], inside: null } });
    const edge: Entity = { kind: "edge", owner: { kind: "object", id: 1 }, a: [0, 0, 1], b: [1, 0, 1], drawn: false };
    h.sel = [edge];
    h._mesh = { parts: [{ kind: "face", owner: 1, index: 2, origin: [0.5, 0.5, 1], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] } as never;
    const t = new OffsetTool(h);
    t.onEvent(at("down", 0.5, 0));
    await tick();
    t.onEvent(at("click", 0.5, 0));
    t.onEvent(at("move", 0.5, 0.1));
    t.onEvent(at("click", 0.5, 0.1));
    await tick();
    const op = ops(h).at(-1)!;
    expect(op).toMatchObject({ op: "offset", source: { of: "edges", closed: false, object: 1, normal: [0, 0, 1] } });
    expect(op.distance as number).toBeCloseTo(0.1, 12);
    expect(h.statuses.at(-1)?.text).toContain("to the left");
  });

  it("refuses a click on nothing and branching edges, typed", async () => {
    const h = fakeHost();
    const t = new OffsetTool(h);
    t.onEvent(ev("down", [5, 5, 0]));
    await tick();
    expect(h.statuses.at(-1)).toMatchObject({ error: true });
    h.sel = [
      { kind: "edge", owner: { kind: "object", id: 1 }, a: [0, 0, 0], b: [1, 0, 0], drawn: false },
      { kind: "edge", owner: { kind: "object", id: 2 }, a: [0, 0, 0], b: [0, 1, 0], drawn: false },
    ];
    t.onEvent(ev("down", [0, 0, 0]));
    await tick();
    expect(h.statuses.at(-1)?.text).toContain("one solid or one sketch");
  });
});
