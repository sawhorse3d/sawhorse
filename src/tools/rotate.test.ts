import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type { DocState, KernelOp, MeshPart, Picked, SurfaceHit, V3, WasmDoc, Xform } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { NO_TURN, RotateTool, fmtDeg, parseAngle, rotateOp, snapAngle, snapTurn, turnsNothing } from "./rotate";
import { applyX, rotation, turnCosSin } from "./xform";

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
    // Lane 22: sketch geometry turns with a transform_sketch op (copies and arrays too).
    const part = { sketch: 1, points: [], edges: [], regions: [1], rings: [], origin: [0, 0, 0] as V3, normal: [0, 0, 1] as V3, moving: [] };
    const sk = { kind: "objects" as const, objects: [], sketches: [part] };
    const t = rotateOp(sk, [0, 0, 0], [0, 0, 1], Math.PI / 4, false) as { op: string; xform: Xform };
    expect(t.op).toBe("transform_sketch");
    close(applyX(t.xform, [1, 0, 0]), [Math.SQRT1_2, Math.SQRT1_2, 0]);
    expect(rotateOp(sk, [0, 0, 0], [0, 0, 1], 1, true, 3)).toMatchObject({ op: "transform_sketch", copy: true, count: 3 });
  });
});

describe("Rotate tool", () => {
  it("turns sketch geometry on its own plane (lane 22): a typed 45 about an upright sketch's axis", async () => {
    const state = { sketches: [{ id: 2, origin: [0, 0, 0], normal: [1, 0, 0], u: [0, 1, 0], v: [0, 0, 1] }] } as unknown as DocState;
    const h = fakeHost({ outcome: { kind: "sketch_moved", sketch: 2, regions: [[2, 1]] }, _state: state });
    const region = { kind: "region" as const, sketch: 2, region: 1 };
    h.sel = [region];
    const t = new RotateTool(h);
    // The cursor is over level ground, but the protractor lies on the sketch's plane.
    t.onEvent(down("click", 0, 0, 0, { picked: onBox, snapKind: "endpoint" }));
    await tick();
    expect(h.statuses.at(-1)?.text).toMatch(/about the red axis/);
    t.onEvent(down("move", 0, 1, 0));
    t.onEvent(down("click", 0, 1, 0));
    t.onValue("45");
    await tick();
    const op = ops(h).at(-1)!;
    expect(op).toMatchObject({ op: "transform_sketch", sketch: 2, regions: [1] });
    close(applyX(op.xform as Xform, [0, 1, 0]), [0, Math.SQRT1_2, Math.SQRT1_2]);
    expect(h.statuses.at(-1)?.text).toMatch(/^Turned .*45° about the red axis/);
    expect(h.sel).toEqual([region]);
    // An arrow key still chooses another plane (a whole shape may turn off its own).
    const t2 = new RotateTool(h);
    t2.onKey(new KeyboardEvent("keydown", { key: "ArrowUp" }));
    t2.onEvent(down("click", 0, 0, 0, { snapKind: "endpoint" }));
    await tick();
    expect(h.statuses.at(-1)?.text).toMatch(/about the blue axis/);
  });


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

describe("typed angles that turn nothing (lane 27, issue #10)", () => {
  it("0, 360 and -360 mid-gesture say so in words and leave the tool waiting for another angle", async () => {
    expect([0, 360, -360, 720, -0].every(turnsNothing)).toBe(true);
    expect([90, 359.999, 0.001, -45].some(turnsNothing)).toBe(false);
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new RotateTool(h);
    t.onEvent(down("click", 0, 0, 0, { picked: onBox, snapKind: "endpoint" }));
    await tick();
    t.onEvent(down("move", 1, 0));
    t.onEvent(down("click", 1, 0));
    const before = h.ops.length;
    for (const typed of ["0", "360", "-360", "0°"]) {
      t.onValue(typed);
      await tick();
      expect(h.statuses.at(-1)).toEqual({ text: NO_TURN, error: true });
      expect(h.ops.length, typed).toBe(before);
      expect(t.busy(), typed).toBe(true);
    }
    // A real angle afterwards turns as usual.
    t.onValue("90");
    await tick();
    expect(ops(h).at(-1)).toMatchObject({ op: "transform", objects: [1] });
    expect(h.statuses.at(-1)?.text).toMatch(/^Turned .* 90° about the blue axis\.$/);
    expect(t.busy()).toBe(false);
  });

  it("before the centre is clicked too: the gesture is not started by a typed 0", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new RotateTool(h);
    t.onEvent(down("click", 0, 0, 0, { picked: onBox, snapKind: "endpoint" }));
    await tick();
    // Centre clicked, no start yet: a typed 0 keeps the gesture as it was.
    t.onValue("0");
    await tick();
    expect(h.statuses.at(-1)?.text).toBe(NO_TURN);
    expect(h.ops).toHaveLength(0);
    t.onValue("45");
    await tick();
    expect(ops(h).at(-1)).toMatchObject({ op: "transform" });
  });

  it("after a turn: 0 and 360 say so and keep the turn; another angle retypes it", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new RotateTool(h);
    await turn(t, 30);
    const before = h.ops.length;
    for (const typed of ["0", "360", "-360"]) {
      t.onValue(typed);
      await tick();
      expect(h.statuses.at(-1)?.text).toBe(NO_TURN);
      // Nothing undone, nothing applied.
      expect(h.ops.length, typed).toBe(before);
    }
    t.onValue("45");
    await tick();
    expect(ops(h).slice(-2)).toMatchObject([{ op: "undo" }, { op: "transform" }]);
    close(applyX(ops(h).at(-1)!.xform as Xform, [1, 0, 0]), [Math.cos(45 * DEG), Math.sin(45 * DEG), 0]);
  });

  it("the same for sketch geometry", async () => {
    const state = { sketches: [{ id: 2, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }] } as unknown as DocState;
    const h = fakeHost({ outcome: { kind: "sketch_moved", sketch: 2, regions: [[2, 1]] }, _state: state });
    h.sel = [{ kind: "region", sketch: 2, region: 1 }];
    const t = new RotateTool(h);
    t.onEvent(down("click", 0, 0, 0, { picked: onBox, snapKind: "endpoint" }));
    await tick();
    t.onEvent(down("move", 1, 0));
    t.onEvent(down("click", 1, 0));
    for (const typed of ["0", "360", "-360"]) {
      t.onValue(typed);
      await tick();
      expect(h.statuses.at(-1)?.text).toBe(NO_TURN);
    }
    expect(h.ops).toHaveLength(0);
    t.onValue("90");
    await tick();
    expect(ops(h).at(-1)).toMatchObject({ op: "transform_sketch", sketch: 2, regions: [1] });
  });
});

/** Every component exactly 0 or plus or minus 1, and equal to `want` (=== treats -0 as 0). */
function exactly(got: number[], want: number[]) {
  expect(got.length).toBe(want.length);
  got.forEach((x, k) => expect(x === want[k], `${got} is not exactly ${want}`).toBe(true));
}

describe("quarter turns are exact (lane 27, issue #10)", () => {
  it("cos and sin of a whole number of quarter turns are exactly 0 and plus or minus 1, from the degrees", () => {
    exactly(turnCosSin(Math.PI / 2, 90), [0, 1]);
    exactly(turnCosSin(-Math.PI / 2, -90), [0, -1]);
    exactly(turnCosSin(Math.PI, 180), [-1, 0]);
    exactly(turnCosSin((3 * Math.PI) / 2, 270), [0, -1]);
    exactly(turnCosSin(-Math.PI, -180), [-1, 0]);
    exactly(turnCosSin(5 * Math.PI, 900), [-1, 0]);
    // Without the degrees, or off a quarter turn, the radians as before.
    expect(turnCosSin(Math.PI / 2)[0]).toBe(Math.cos(Math.PI / 2));
    expect(turnCosSin(Math.PI / 2, null)[0]).not.toBe(0);
    expect(turnCosSin(45 * DEG, 45)).toEqual([Math.cos(45 * DEG), Math.sin(45 * DEG)]);
    // The protractor's snap reports the tick it snapped to; a free angle has none.
    expect(snapTurn(88.5 * DEG)).toEqual({ angle: 90 * DEG, deg: 90 });
    expect(snapTurn(-89 * DEG).deg).toBe(-90);
    expect(snapTurn(41 * DEG).deg).toBeNull();
  });

  it("a ground normal turned Right+90, -90, 180 and 270 is axis-aligned to the bit", () => {
    const up: V3 = [0, 0, 1];
    const turned = (deg: number) => {
      const x = rotation([0, 0, 0], [1, 0, 0], deg * DEG, deg);
      return x.linear.map((row) => row[0] * up[0] + row[1] * up[1] + row[2] * up[2]);
    };
    exactly(turned(90), [0, -1, 0]);
    exactly(turned(-90), [0, 1, 0]);
    exactly(turned(180), [0, 0, -1]);
    exactly(turned(270), [0, 1, 0]);
    // What issue #10 saw without the degrees: 6e-17 off.
    const loose = rotation([0, 0, 0], [1, 0, 0], 90 * DEG).linear[2][2];
    expect(loose).not.toBe(0);
    expect(Math.abs(loose)).toBeLessThan(1e-16);
  });

  it("typed and snapped quarter turns reach the op exactly; a free drag does not change", async () => {
    const h = fakeHost({ outcome: { kind: "transformed", objects: [1] } });
    const t = new RotateTool(h);
    t.onKey(new KeyboardEvent("keydown", { key: "ArrowRight" }));
    t.onEvent(ev("click", [0, 0, 0], { picked: onBox, snapKind: "endpoint", ray: { origin: [10, 0, 0], dir: [-1, 0, 0] } }));
    await tick();
    t.onEvent(ev("move", [0, 1, 0], { ray: { origin: [10, 1, 0], dir: [-1, 0, 0] } }));
    t.onEvent(ev("click", [0, 1, 0], { ray: { origin: [10, 1, 0], dir: [-1, 0, 0] } }));
    t.onEvent(ev("move", [0, 0.5, 0.5], { ray: { origin: [10, 0.5, 0.5], dir: [-1, 0, 0] } }));
    t.onValue("90");
    await tick();
    const typed = ops(h).at(-1)!.xform as Xform;
    exactly(typed.linear.flat(), [1, 0, 0, 0, 0, -1, 0, 1, 0]);
    // Snapped by the protractor (88 degrees is within the snap of the 90 tick), level.
    const level = new RotateTool(h);
    await turn(level, 88);
    const snapped = ops(h).at(-1)!.xform as Xform;
    exactly(snapped.linear.flat(), [0, -1, 0, 1, 0, 0, 0, 0, 1]);
    // A drag that lands on a free angle is the radians as before.
    await turn(level, 41);
    const free = ops(h).at(-1)!.xform as Xform;
    const a = Math.atan2(free.linear[1][0], free.linear[0][0]);
    expect(a / DEG).toBeCloseTo(41, 9);
    expect(free).toEqual(rotation([0, 0, 0], [0, 0, 1], a));
    // A copy turned 180 and divided in two: 90 a step, exactly.
    const c = fakeHost({ outcome: { kind: "copied", objects: [2] } });
    const ct = new RotateTool(c);
    ct.onModifier("ctrl");
    await turn(ct, 180);
    ct.onValue("/2");
    await tick();
    exactly((ops(c).at(-1)!.xform as Xform).linear.flat(), [0, -1, 0, 1, 0, 0, 0, 0, 1]);
  });
});

const pkg = path.resolve(__dirname, "../wasm/pkg");
const built = existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));
const runKernel = built || process.env.REQUIRE_KERNEL ? describe : describe.skip;

runKernel("quarter turns through the kernel (lane 27, issue #10)", () => {
  async function doc(): Promise<WasmDoc> {
    const mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as { initSync(o: { module: Buffer }): void; document_new(): WasmDoc };
    mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
    return mod.document_new();
  }
  const parts = (d: WasmDoc): MeshPart[] => {
    const m = d.get_scene_mesh();
    const out = JSON.parse(m.parts_json()) as MeshPart[];
    m.free();
    return out;
  };
  const apply = (d: WasmDoc, op: unknown) => {
    const out = JSON.parse(d.apply_op(JSON.stringify(op)));
    expect(out.error, JSON.stringify(out)).toBeUndefined();
    return out;
  };
  const square = { op: "draw", target: { on: "plane", origin: [0, 0, 0], normal: [0, 0, 1] }, points: [[0, 0, 0], [0.5, 0, 0], [0.5, 0.5, 0], [0, 0.5, 0]], closed: true };
  const ground = (d: WasmDoc) => parts(d).find((p) => p.kind === "region")!;

  for (const [deg, normal] of [
    [90, [0, -1, 0]],
    [-90, [0, 1, 0]],
    [180, [0, 0, -1]],
    [270, [0, 1, 0]],
  ] as [number, V3][]) {
    it(`a ground sketch stood up Right+${deg}: its normal is exactly (${normal}) up to which way it faces`, async () => {
      const d = await doc();
      apply(d, square);
      const r = ground(d);
      const sk = { kind: "objects" as const, objects: [], sketches: [{ sketch: r.owner, points: [], edges: [], regions: [r.index], rings: [], origin: [0, 0, 0] as V3, normal: [0, 0, 1] as V3, moving: [] }] };
      apply(d, rotateOp(sk, [0, 0, 0], [1, 0, 0], deg * DEG, false, 1, deg));
      const region = parts(d).find((p) => p.kind === "region")!;
      // A sketch faces either way along its plane's normal; the plane itself is exact.
      const n = region.normal;
      const flip = n.some((x, k) => x !== 0 && x === -normal[k]);
      exactly(flip ? n.map((x) => -x) : n, normal);
      const st = JSON.parse(d.state_json()) as DocState;
      const sketch = st.sketches.find((s) => s.id === region.owner)!;
      exactly(sketch.normal.map(Math.abs), normal.map(Math.abs));
      d.free();
    });

    it(`a solid turned Right+${deg}: every face normal is exactly a world axis`, async () => {
      const d = await doc();
      apply(d, square);
      const r = ground(d);
      const made = apply(d, { op: "extrude", sketch: r.owner, region: r.index, distance: 0.25 });
      const object = made.object ?? made.objects?.[0];
      apply(d, rotateOp({ kind: "objects", objects: [object] }, [0, 0, 0], [1, 0, 0], deg * DEG, false, 1, deg));
      const faces = parts(d).filter((p) => p.kind === "face");
      expect(faces).toHaveLength(6);
      for (const f of faces) {
        expect(f.normal.every((x) => x === 0 || x === 1 || x === -1), `${f.normal}`).toBe(true);
      }
      // The top (it faced +z) now faces the way the ground normal went.
      expect(faces.some((f) => f.normal.every((x, k) => x === normal[k]))).toBe(true);
      d.free();
    });
  }
});
