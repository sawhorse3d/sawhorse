import { describe, expect, it } from "vitest";
import type { KernelOp, Picked, SurfaceHit, V3, Xform } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { RotateTool, fmtDeg, parseAngle, rotateOp, snapAngle } from "./rotate";
import { applyX } from "./xform";

const DEG = Math.PI / 180;
const tick = () => new Promise((r) => setTimeout(r, 0));
const ops = (h: { ops: KernelOp[] }) => h.ops.map((o) => o as unknown as Record<string, unknown>);
const close = (a: V3, b: V3, d = 12) => a.forEach((x, k) => expect(x).toBeCloseTo(b[k], d));
const top: SurfaceHit = { at: { kind: "face", object: 1, face: 2 }, frame: { origin: [0, 0, 1], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }, point: [0, 0, 1], t: 5 };
const onBox: Picked = { entity: { kind: "face", object: 1, face: 2 }, point: [0, 0, 1], surface: top };
/** A pointer event straight down onto the plane z = h at (x, y). */
const down = (type: "move" | "click", x: number, y: number, z = 0, extra = {}) => ev(type, [x, y, z], { ray: { origin: [x, y, 10], dir: [0, 0, -1] }, ...extra });

/** Click centre at the origin, start along +x, end at `deg` on the level plane. */
async function turn(t: RotateTool, deg: number) {
  t.onEvent(down("click", 0, 0, 0, { picked: onBox, snapKind: "endpoint" }));
  await tick();
  t.onEvent(down("move", 1, 0));
  t.onEvent(down("click", 1, 0));
  const a = deg * DEG;
  t.onEvent(down("move", Math.cos(a), Math.sin(a)));
  t.onEvent(down("click", Math.cos(a), Math.sin(a)));
  await tick();
}

describe("rotate helpers", () => {
  it("snaps cursor angles to the protractor's ticks, parses typed angles", () => {
    expect(snapAngle(88.5 * DEG) / DEG).toBeCloseTo(90, 12);
    expect(snapAngle(41 * DEG) / DEG).toBeCloseTo(41, 12);
    expect(snapAngle(-46 * DEG) / DEG).toBeCloseTo(-45, 12);
    expect(parseAngle("90")).toBe(90);
    expect(parseAngle("-22.5°")).toBe(-22.5);
    expect(parseAngle("30deg")).toBe(30);
    expect(parseAngle("x3")).toBeNull();
    expect(parseAngle("3/4")).toBeNull();
    expect(fmtDeg(-Math.PI / 8)).toBe("-22.5°");
  });

  it("builds turn, copy, array and part-turn ops", () => {
    const s = { kind: "objects" as const, objects: [1] };
    const op = rotateOp(s, [1, 0, 0], [0, 0, 1], Math.PI / 2, false) as { xform: Xform };
    close(applyX(op.xform, [2, 0, 0]), [1, 1, 0]);
    expect(rotateOp(s, [0, 0, 0], [0, 0, 1], 1, true, 3)).toMatchObject({ op: "array", count: 3 });
    expect(rotateOp({ kind: "geometry", object: 1, points: [[0, 0, 1]], what: "" }, [0, 0, 0], [0, 0, 1], 1, false)).toMatchObject({ op: "transform_geometry" });
  });
});

describe("Rotate tool", () => {
  it("centre, start, end: turns the solid by the snapped angle about the face's axis", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new RotateTool(h);
    await turn(t, 88);
    const op = ops(h).at(-1)!;
    expect(op).toMatchObject({ op: "transform", objects: [1] });
    close(applyX(op.xform as Xform, [1, 0, 0]), [0, 1, 0]);
    expect(h.statuses.at(-1)?.text).toContain("90° about the blue axis");
    expect(t.busy()).toBe(false);
  });

  it("an arrow key locks the protractor upright; a typed angle is exact and keeps the side", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new RotateTool(h);
    t.onKey(new KeyboardEvent("keydown", { key: "ArrowRight" }));
    expect(h.statuses.at(-1)?.text).toContain("facing red");
    t.onEvent(ev("click", [0, 0, 0], { picked: onBox, snapKind: "endpoint", ray: { origin: [10, 0, 0], dir: [-1, 0, 0] } }));
    await tick();
    t.onEvent(ev("move", [0, 1, 0], { ray: { origin: [10, 1, 0], dir: [-1, 0, 0] } }));
    t.onEvent(ev("click", [0, 1, 0], { ray: { origin: [10, 1, 0], dir: [-1, 0, 0] } }));
    // Cursor swings the negative way (toward -z from +y about +x).
    t.onEvent(ev("move", [0, 0.5, -0.5], { ray: { origin: [10, 0.5, -0.5], dir: [-1, 0, 0] } }));
    t.onValue("30");
    await tick();
    const op = ops(h).at(-1)!;
    close(applyX(op.xform as Xform, [0, 1, 0]), [0, Math.cos(30 * DEG), -Math.sin(30 * DEG)]);
    // Typed after the turn: undo and turn again by the new angle.
    t.onValue("45");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "transform" }]);
    close(applyX(ops(h).at(-1)!.xform as Xform, [0, 1, 0]), [0, Math.cos(45 * DEG), -Math.sin(45 * DEG)]);
  });

  it("Ctrl copies; x3 arrays by the angle; /4 divides it", async () => {
    const h = fakeHost({ outcome: { kind: "copied", objects: [2] } });
    const t = new RotateTool(h);
    t.onModifier("ctrl");
    await turn(t, 90);
    expect(ops(h).at(-1)).toMatchObject({ op: "copy", objects: [1] });
    t.onValue("x3");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "array", count: 3 }]);
    close(applyX(ops(h).at(-1)!.xform as Xform, [1, 0, 0]), [0, 1, 0]);
    t.onValue("/6");
    await tick();
    // The whole array spanned 270 degrees: six steps of 45.
    expect(ops(h).at(-1)).toMatchObject({ op: "array", count: 6 });
    close(applyX(ops(h).at(-1)!.xform as Xform, [1, 0, 0]), [Math.SQRT1_2, Math.SQRT1_2, 0]);
  });

  it("refuses arrays without a copy and non-angles", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new RotateTool(h);
    await turn(t, 45);
    t.onValue("x3");
    expect(h.statuses.at(-1)).toMatchObject({ error: true, text: expect.stringContaining("Arrays follow a copy") });
    t.onValue("3/4");
    expect(h.statuses.at(-1)).toMatchObject({ error: true });
  });

  it("shows the protractor while hovering and the turned edges while turning", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new RotateTool(h);
    t.onEvent(down("move", 0, 0, 1, { picked: onBox, snap: { point: [0, 0, 1], kind: "on-face", tip: "", surface: top, entity: null, lines: [] } }));
    const ring = t.view().overlay.find((o) => o.kind === "loop");
    expect(ring?.points.length).toBe(48);
    ring!.points.forEach((p) => expect(p[2]).toBeCloseTo(1, 12));
  });
});
