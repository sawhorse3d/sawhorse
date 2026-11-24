// #130: a rectangle started on a box's corner. The plane follows the cursor until the second
// click; over a face that holds the corner, the rectangle goes on that face, and a guide
// line that leaves the face does not pull the second corner off it. A second corner on a
// line through the first along a side says why nothing is drawn, and typed sizes the
// cursor gave no direction say what to do.
import { describe, expect, it } from "vitest";
import type { Entity, KernelOp, SurfaceHit, V3 } from "../kernel/types";
import { dist } from "./draw";
import { ev, fakeHost, type FakeHost } from "./fakeHost";
import { GUESSED_WAY, RectTool, flatWords } from "./rect";
import type { Snap } from "./tool";

const IN = 0.0254;
const P = (x: number, y: number, z: number): V3 => [x * IN, y * IN, z * IN];
const close = (a: V3, b: V3) => expect(dist(a, b)).toBeLessThan(1e-9);
const tick = () => new Promise((r) => setTimeout(r, 0));

// A 24 x 6 x 3/4 in board, solid 1: its top (face 2) and its front (face 3, facing -green).
const hit = (face: number, origin: V3, normal: V3, u: V3, v: V3, point: V3, object = 1): SurfaceHit => ({ at: { kind: "face", object, face }, frame: { origin, normal, u, v }, point, t: 5 });
const top = (point: V3) => hit(2, P(0, 0, 0.75), [0, 0, 1], [1, 0, 0], [0, 1, 0], point);
const front = (point: V3) => hit(3, P(0, 0, 0), [0, -1, 0], [1, 0, 0], [0, 0, 1], point);
// Another solid's top, 2 in up (it does not hold the board's corner).
const other = (point: V3) => hit(1, P(30, 0, 2), [0, 0, 1], [1, 0, 0], [0, 1, 0], point, 2);

const vertex = (at: V3, id = 1): Entity => ({ kind: "vertex", owner: { kind: "object", id }, at });
const S = (point: V3, kind: Snap["kind"], surface: SurfaceHit | null, extra: Partial<Snap> = {}): Snap => ({ point, kind, tip: kind, surface, entity: null, lines: [], ...extra });
const corner = P(24, 0, 0.75);
const cornerSnap = S(corner, "endpoint", top(corner), { entity: vertex(corner), adjacent: front(corner) });

/** An event with this snap; the eye ray goes through `through` (default: the snap's surface point or the snap). */
function at(type: "move" | "click", snap: Snap | null, through?: V3) {
  const p = through ?? snap?.surface?.point ?? snap?.point ?? [0, 0, 0];
  const e = ev(type, p, { snap });
  return { ...e, ray: { origin: [p[0] + 1, p[1] - 1, p[2] + 1] as V3, dir: [-1 / Math.sqrt(3), 1 / Math.sqrt(3), -1 / Math.sqrt(3)] as V3 } };
}

const lastDraw = (h: FakeHost) => h.ops.filter((o) => o.op === "draw").at(-1) as (KernelOp & { op: "draw"; target: { on: string; object?: number; face?: number; normal?: V3; origin?: V3 }; points: V3[] }) | undefined;
const lastStatus = (h: FakeHost) => h.statuses.at(-1);

async function fromCorner(snap: Snap = cornerSnap) {
  const h = fakeHost({ outcome: { kind: "drawn", sketch: null, regions: [], faces: [2, 7], inside: 2 } });
  const t = new RectTool(h);
  t.onEvent(at("move", snap));
  t.onEvent(at("click", snap));
  await tick();
  return { h, t };
}

/** Move to the snap, then click it; what was drawn. */
async function second(t: RectTool, h: FakeHost, snap: Snap, through?: V3) {
  t.onEvent(at("move", snap, through));
  const view = t.view();
  t.onEvent(at("click", snap, through));
  await tick();
  return { view, op: lastDraw(h), status: lastStatus(h) };
}

describe("Rectangle from a corner: the face under the cursor (#130)", () => {
  it("an on-face point draws on the face (as before)", async () => {
    const { h, t } = await fromCorner();
    const q = P(14.88, 2.22, 0.75);
    const r = await second(t, h, S(q, "on-face", top(q), { entity: { kind: "face", object: 1, face: 2 } }));
    expect(r.op?.target).toEqual({ on: "face", object: 1, face: 2 });
    close(r.op!.points[2], q);
    expect(r.status?.text).toMatch(/on face 2 of a solid/);
  });

  it("a remembered point's From Point line off the face gives way to the point on the face under the cursor", async () => {
    const { h, t } = await fromCorner();
    const q = P(14.88, 2.22, 0.75);
    // The line of the bottom edge's midpoint (0, 3, 0), under the board.
    const guide = S(P(15.645, 3, 0), "from-point", top(q), { lines: [{ from: P(0, 3, 0), to: P(15.645, 3, 0), kind: "from-point" }] });
    const r = await second(t, h, guide);
    expect(r.op?.target).toEqual({ on: "face", object: 1, face: 2 });
    close(r.op!.points[0], corner);
    close(r.op!.points[2], q);
    expect(r.status?.text).toMatch(/Rectangle .* on face 2 of a solid/);
    // The preview showed what the click did: the face's marker, no dotted line.
    expect(r.view.marker?.kind).toBe("on-face");
    close(r.view.marker!.at, q);
    expect(r.view.overlay.some((o) => o.kind === "segments" && o.dashed)).toBe(false);
  });

  it("the start's own axis line off a side face gives way to the point on that face", async () => {
    const { h, t } = await fromCorner();
    const q = P(20, 0, 0.5);
    const guide = S(P(24, 1, 0.75), "axis-y", front(q), { lines: [{ from: corner, to: P(24, 1, 0.75), kind: "axis-y" }] });
    const r = await second(t, h, guide);
    expect(r.op?.target).toEqual({ on: "face", object: 1, face: 3 });
    close(r.op!.points[2], q);
    expect(r.status?.text).toMatch(/on face 3 of a solid/);
    expect(r.view.marker?.kind).toBe("on-face");
    expect(r.view.overlay.some((o) => o.kind === "segments" && o.points.length === 2 && o.color === "--axis-y")).toBe(false);
  });

  it("a guide line that lies on the face's plane is kept", async () => {
    const { h, t } = await fromCorner();
    // A From Point line along the top's plane (from a remembered top corner (0, 6, 3/4)).
    const p = P(10, 6, 0.75);
    const r = await second(t, h, S(p, "from-point", top(P(10, 5.9, 0.75)), { lines: [{ from: P(0, 6, 0.75), to: p, kind: "from-point" }] }));
    expect(r.op?.target).toEqual({ on: "face", object: 1, face: 2 });
    close(r.op!.points[2], p);
    expect(r.view.marker?.kind).toBe("from-point");
  });

  it("a real corner or midpoint of the face is kept (as before)", async () => {
    for (const [p, kind] of [
      [P(0, 6, 0.75), "endpoint"],
      [P(12, 6, 0.75), "midpoint"],
    ] as [V3, Snap["kind"]][]) {
      const { h, t } = await fromCorner();
      const r = await second(t, h, S(p, kind, top(p), { entity: vertex(p) }));
      expect(r.op?.target).toEqual({ on: "face", object: 1, face: 2 });
      close(r.op!.points[2], p);
      expect(r.view.marker?.kind).toBe(kind);
    }
  });

  it("a real point of another solid off the face's plane is kept: it goes where it went before", async () => {
    const { h, t } = await fromCorner();
    // An endpoint of another solid, seen past the board's top; the eye ray meets the
    // corner's level at `ray`.
    const p = P(30, 3, 2);
    const ray = P(20, 2, 0.75);
    const r = await second(t, h, S(p, "endpoint", top(ray), { entity: vertex(p, 2) }), ray);
    expect(r.view.marker?.kind).toBe("endpoint");
    // No level or upright plane through the corner holds it: level, where the eye ray meets it.
    expect(r.op?.target).toMatchObject({ on: "plane", normal: [0, 0, 1] });
    expect(r.op!.points[2][2]).toBeCloseTo(0.75 * IN, 12);
    expect(r.status?.text).toMatch(/on a plane 3\/4 up/);
  });

  it("over a face that does not hold the corner, a guide line goes where it went before", async () => {
    const { h, t } = await fromCorner();
    const p = P(32, 1, 3);
    const r = await second(t, h, S(p, "from-point", other(P(33, 2, 2)), { lines: [{ from: P(40, 1, 3), to: p, kind: "from-point" }] }));
    expect(r.view.marker?.kind).toBe("from-point");
    expect(r.op?.target).toMatchObject({ on: "plane", normal: [0, 0, 1] });
    close(r.op!.points[0], corner);
    expect(r.status?.text).toMatch(/on a plane 3\/4 up/);
  });

  it("over nothing: up into the sky on an upright plane, and level, as before", async () => {
    {
      const { h, t } = await fromCorner();
      const p = P(30, 0, 5);
      const r = await second(t, h, S(p, "from-point", null, { lines: [{ from: P(30, 0, 0), to: p, kind: "from-point" }] }));
      // The plane of the front face, as a new sketch: the rectangle leaves the board.
      expect(r.op?.target).toMatchObject({ on: "plane", normal: [0, -1, 0] });
      close(r.op!.points[2], p);
      expect(r.status?.text).toMatch(/on a vertical plane/);
    }
    {
      // Off the board's own planes: the upright plane through the corner facing red.
      const { h, t } = await fromCorner();
      const p = P(24, -5, 5);
      const r = await second(t, h, S(p, "from-point", null, { lines: [{ from: P(24, -5, 0), to: p, kind: "from-point" }] }));
      expect(r.op?.target).toMatchObject({ on: "plane", normal: [1, 0, 0] });
      close(r.op!.points[2], p);
      expect(r.status?.text).toMatch(/on a vertical plane/);
    }
    {
      const { h, t } = await fromCorner();
      const p = P(30, 10, 0);
      const r = await second(t, h, S(p, "free", null), P(30, 10, 0.75));
      expect(r.op?.target).toMatchObject({ on: "plane", normal: [0, 0, 1] });
      expect(r.status?.text).toMatch(/on a plane 3\/4 up/);
    }
  });

  it("on the ground from a bottom corner, as before", async () => {
    const c = P(0, 0, 0);
    const { h, t } = await fromCorner(S(c, "endpoint", null, { entity: vertex(c), adjacent: front(c) }));
    const p = P(-5, -5, 0);
    const r = await second(t, h, S(p, "free", null));
    expect(r.op?.target).toMatchObject({ on: "plane", normal: [0, 0, 1], origin: [0, 0, 0] });
    expect(r.status?.text).toMatch(/on the ground/);
  });

  it("an axis lock already set is kept", async () => {
    const { h, t } = await fromCorner();
    expect(t.onKey({ key: "ArrowLeft" } as KeyboardEvent)).toBe(true);
    const p = P(24, 1, 0.75);
    const r = await second(t, h, S(p, "axis-y", front(P(20, 0, 0.5)), { lines: [{ from: corner, to: p, kind: "axis-y" }] }));
    expect(r.view.marker?.kind).toBe("axis-y");
    // Main's way: both corners on the green line, so nothing is drawn and the words say why.
    expect(r.op).toBeUndefined();
  });

  it("over the face, the Down arrow and Shift still lock to the guide line the face rule hides", async () => {
    for (const how of ["down", "shift"] as const) {
      const { h, t } = await fromCorner();
      const p = P(24, 1, 0.75);
      const guide = S(p, "axis-y", front(P(20, 0, 0.5)), { lines: [{ from: corner, to: p, kind: "axis-y" }] });
      t.onEvent(at("move", guide));
      expect(t.view().marker?.kind).toBe("on-face");
      if (how === "down") {
        expect(t.onKey({ key: "ArrowDown" } as KeyboardEvent)).toBe(true);
        expect(lastStatus(h)?.text).toBe("Locked to the green axis. Press the same arrow to unlock.");
      } else t.onShift(true);
      expect(t.inferContext().lock).toMatchObject({ kind: "axis-y" });
      // Locked, the engine answers with the line, and the marker and the click follow it.
      t.onEvent(at("move", guide));
      expect(t.view().marker?.kind).toBe("axis-y");
    }
  });

  it("the Rotated Rectangle's second click keeps to the face the same way", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: null, regions: [], faces: [2, 7], inside: 2 } });
    const t = new RectTool(h);
    t.cycleVariant();
    t.onEvent(at("click", cornerSnap));
    await tick();
    const q = P(14.88, 0, 0.75);
    t.onEvent(at("move", S(P(15, 3, 0), "from-point", top(q))));
    expect(t.view().marker?.kind).toBe("on-face");
    t.onEvent(at("click", S(P(15, 3, 0), "from-point", top(q))));
    await tick();
    t.onEvent(at("click", S(P(14.88, 2, 0.75), "on-face", top(P(14.88, 2, 0.75)))));
    await tick();
    expect(lastDraw(h)?.target).toEqual({ on: "face", object: 1, face: 2 });
  });
});

describe("Rectangle with no width (#130)", () => {
  it("flatWords: no width, no depth, both, or a rectangle", () => {
    const f = { origin: [0, 0, 0] as V3, normal: [0, 0, 1] as V3, u: [1, 0, 0] as V3, v: [0, 1, 0] as V3 };
    expect(flatWords(f, [0, 0, 0], [1, 0, 0])).toMatch(/^On the drawing plane both corners are on one line/);
    expect(flatWords(f, [0, 0, 0], [0, 1, 5])).toMatch(/^On the drawing plane both corners are on one line/);
    expect(flatWords(f, [0, 0, 0], [0, 0, 1])).toMatch(/^On the drawing plane the second corner is on the first/);
    expect(flatWords(f, [0, 0, 0], [1e-6, 1e-6, 0])).toBeNull();
  });

  it("a click on the edge through the first corner draws nothing, says why and keeps the corner and the open plane", async () => {
    const { h, t } = await fromCorner();
    const n = h.statuses.length;
    const e = P(12, 0, 0.75);
    const onEdge = S(e, "on-edge", top(e), { entity: { kind: "edge", owner: { kind: "object", id: 1 }, a: P(0, 0, 0.75), b: corner, drawn: false } as Entity });
    t.onEvent(at("move", onEdge));
    // Hovering alone says nothing.
    expect(h.statuses.length).toBe(n);
    t.onEvent(at("click", onEdge));
    await tick();
    expect(h.ops).toHaveLength(0);
    expect(lastStatus(h)).toEqual({ text: "On the drawing plane both corners are on one line, so the rectangle has no width. Click off the line, or type width, depth.", error: true });
    expect(t.busy()).toBe(true);
    expect(t.start()).toEqual(corner);
    expect(t.inferContext().plane).toBeNull();
    // The next click off the edge draws, on the face under it.
    const q = P(12, 3, 0.75);
    const r = await second(t, h, S(q, "on-face", top(q)));
    expect(r.op?.target).toEqual({ on: "face", object: 1, face: 2 });
    close(r.op!.points[0], corner);
  });

  it("on a set plane too, and a click on the first corner itself", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: null, regions: [], faces: [2, 7], inside: 2 } });
    const t = new RectTool(h);
    const a = P(2, 2, 0.75);
    t.onEvent(at("click", S(a, "on-face", top(a))));
    await tick();
    t.onEvent(at("click", S(P(10, 2, 0.75), "on-face", top(P(10, 2, 0.75)))));
    await tick();
    expect(lastStatus(h)?.text).toMatch(/^On the drawing plane both corners are on one line/);
    t.onEvent(at("click", S(a, "on-face", top(a))));
    await tick();
    expect(lastStatus(h)).toEqual({ text: "On the drawing plane the second corner is on the first. Click the opposite corner, or type width, depth.", error: true });
    expect(h.ops).toHaveLength(0);
    expect(t.start()).toEqual(a);
  });
});

describe("Rectangle: typed sizes after a corner click (#130)", () => {
  it("with the cursor still on the corner and the kernel refusing: the corner stays, the kernel's words and a sentence after them", async () => {
    const { h, t } = await fromCorner();
    t.onEvent(at("move", cornerSnap));
    h.outcome = null;
    t.onValue("24, 3/8");
    await tick();
    expect(h.ops).toHaveLength(1);
    expect(lastStatus(h)).toEqual({ text: `refused. ${GUESSED_WAY}`, error: true });
    expect(t.busy()).toBe(true);
    expect(t.start()).toEqual(corner);
    expect(t.inferContext().plane).toBeNull();
    // Then onto the face: the sizes go toward the cursor, on the face.
    h.outcome = { kind: "drawn", sketch: null, regions: [], faces: [2, 7], inside: null };
    const q = P(12, 3, 0.75);
    t.onEvent(at("move", S(q, "on-face", top(q))));
    t.onValue("24, 3/8");
    await tick();
    const op = lastDraw(h)!;
    expect(op.target).toEqual({ on: "face", object: 1, face: 2 });
    close(op.points[2], P(0, 0.375, 0.75));
    expect(t.busy()).toBe(false);
  });

  it("with the cursor on the corner and the kernel drawing them: drawn as before", async () => {
    const { h, t } = await fromCorner();
    t.onEvent(at("move", cornerSnap));
    t.onValue("2, 1");
    await tick();
    const op = lastDraw(h)!;
    close(op.points[2], P(26, 1, 0.75));
    expect(t.busy()).toBe(false);
  });

  it("with a direction from the cursor and the kernel refusing: its words, the corner gone, as before", async () => {
    const { h, t } = await fromCorner();
    const q = P(12, 3, 0.75);
    t.onEvent(at("move", S(q, "on-face", top(q))));
    h.outcome = null;
    t.onValue("48, 3/8");
    await tick();
    expect(lastStatus(h)).toEqual({ text: "refused", error: true });
    expect(t.busy()).toBe(false);
  });
});

describe("Rectangle: refusals that are true, and corners that stay gone (#130 fix pass)", () => {
  it("typed sizes the kernel calls too small: its words alone, the corner kept", async () => {
    for (const typed of ["24, 0", "0, 0", "0.001mm, 5"]) {
      const { h, t } = await fromCorner();
      t.onEvent(at("move", cornerSnap));
      h.outcome = null;
      const n = h.statuses.length;
      t.onValue(typed);
      await tick();
      expect(h.statuses.slice(n)).toEqual([{ text: "refused", error: true }]);
      expect(t.start()).toEqual(corner);
    }
  });

  it("typed sizes with no cursor at all are a guess too", async () => {
    const h = fakeHost({ outcome: null });
    const t = new RectTool(h);
    t.onEvent(at("click", cornerSnap));
    await tick();
    t.onValue("24, 3/8");
    await tick();
    expect(lastStatus(h)).toEqual({ text: `refused. ${GUESSED_WAY}`, error: true });
    expect(t.start()).toEqual(corner);
  });

  it("the first corner's snaps come back with it: the next plane is chosen as for a fresh corner", async () => {
    const { h, t } = await fromCorner();
    t.onEvent(at("move", cornerSnap));
    h.outcome = null;
    t.onValue("24, 3/8");
    await tick();
    h.outcome = { kind: "drawn", sketch: null, regions: [], faces: [2, 7], inside: null };
    // Past the board's end at its top's height, over nothing: the start's own face holds it.
    t.onEvent(at("move", S(P(30, 3, 0.75), "free", null)));
    t.onValue("2, 1");
    await tick();
    expect(lastDraw(h)?.target).toEqual({ on: "face", object: 1, face: 2 });
  });

  /** A host whose next apply waits until `release` is called, then refuses. */
  function slowRefusal(h: FakeHost) {
    let release = () => {};
    const apply = h.apply;
    h.apply = (op, describe) =>
      new Promise((resolve) => {
        release = () => {
          h.outcome = null;
          void apply(op, describe).then(resolve);
        };
      });
    return () => release();
  }

  it("Esc while typed sizes are on their way: the refusal does not bring the corner back", async () => {
    const { h, t } = await fromCorner();
    t.onEvent(at("move", cornerSnap));
    const release = slowRefusal(h);
    t.onValue("24, 3/8");
    await tick();
    expect(t.cancel()).toBe(true);
    release();
    await tick();
    await tick();
    expect(t.busy()).toBe(false);
    expect(t.view().overlay.some((o) => o.kind === "loop")).toBe(false);
    expect(h.statuses.at(-1)).toEqual({ text: "refused", error: true });
  });

  it("a tool switch while typed sizes are on their way: no corner on return, no words from the Rectangle", async () => {
    const { h, t } = await fromCorner();
    t.onEvent(at("move", cornerSnap));
    const release = slowRefusal(h);
    t.onValue("24, 3/8");
    await tick();
    t.deactivate();
    release();
    await tick();
    await tick();
    expect(t.busy()).toBe(false);
    expect(h.statuses.at(-1)).toEqual({ text: "refused", error: true });
  });

  it("the face rule waits for an open plane: on a plane set by a click inside the face, a guide line is projected as before", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: null, regions: [], faces: [2, 7], inside: 2 } });
    const t = new RectTool(h);
    const a = P(2, 1, 0.75);
    t.onEvent(at("click", S(a, "on-face", top(a))));
    await tick();
    const r = await second(t, h, S(P(10, 4, 0), "from-point", top(P(12, 3, 0.75)), { lines: [{ from: P(0, 4, 0), to: P(10, 4, 0), kind: "from-point" }] }));
    close(r.op!.points[2], P(10, 4, 0.75));
  });

  it("the replaced snap names the face, not the guide's own entity", async () => {
    const { h, t } = await fromCorner();
    const q = P(14, 2, 0.75);
    // A parallel guide from an edge of another solid (2), off the top's plane.
    const edge2 = { kind: "edge", owner: { kind: "object", id: 2 }, a: P(30, 0, 0), b: P(30, 9, 0), drawn: false } as Entity;
    const r = await second(t, h, S(P(14, 4, 0), "parallel", top(q), { entity: edge2, lines: [{ from: corner, to: P(14, 4, 0), kind: "parallel" }] }));
    expect(r.op?.target).toEqual({ on: "face", object: 1, face: 2 });
  });

  it("the Rotated Rectangle's width click, after a typed length: no face rule, the width from the cursor's snap", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", sketch: null, regions: [], faces: [2, 7], inside: 2 } });
    const t = new RectTool(h);
    t.cycleVariant();
    t.onEvent(at("click", cornerSnap));
    await tick();
    t.onEvent(at("move", S(P(20, 0, 0.75), "on-edge", top(P(20, 0, 0.75)))));
    t.onValue("4");
    await tick();
    const g = S(P(20, 4, 0), "from-point", top(P(20, 2, 0.75)), { lines: [{ from: P(0, 4, 0), to: P(20, 4, 0), kind: "from-point" }] });
    t.onEvent(at("move", g));
    t.onEvent(at("click", g));
    await tick();
    const pts = lastDraw(h)!.points;
    expect(dist(pts[1], pts[2]) / IN).toBeCloseTo(4, 9);
  });

  it("a Rotated Rectangle with no width gets the kernel's words, not the Rectangle's", async () => {
    const h = fakeHost({ outcome: null });
    const t = new RectTool(h);
    t.cycleVariant();
    t.onEvent(at("click", cornerSnap));
    await tick();
    const e = P(12, 0, 0.75);
    t.onEvent(at("click", S(e, "on-edge", top(e))));
    await tick();
    const f = P(6, 0, 0.75);
    t.onEvent(at("click", S(f, "on-edge", top(f))));
    await tick();
    expect(h.ops).toHaveLength(1);
    expect(lastStatus(h)).toEqual({ text: "refused", error: true });
  });

  it("on a set plane, a click straight above the first corner is on it, seen on the drawing plane", async () => {
    const h = fakeHost();
    const t = new RectTool(h);
    const a = P(2, 2, 0.75);
    t.onEvent(at("click", S(a, "on-face", top(a))));
    await tick();
    t.onEvent(at("click", S(P(2, 2, 5), "free", null)));
    await tick();
    expect(lastStatus(h)?.text).toMatch(/^On the drawing plane the second corner is on the first/);
    expect(h.ops).toHaveLength(0);
  });
});

describe("Rectangle: a new start while typed sizes are on their way (#130 fix pass)", () => {
  function held(h: FakeHost) {
    let release = () => {};
    const apply = h.apply;
    h.apply = (op, describe) =>
      new Promise((resolve) => {
        release = () => {
          h.outcome = null;
          void apply(op, describe).then(resolve);
        };
      });
    return () => release();
  }

  it("a new first corner clicked before the refusal comes back is kept, and no sentence is added", async () => {
    const { h, t } = await fromCorner();
    t.onEvent(at("move", cornerSnap));
    const release = held(h);
    t.onValue("24, 3/8");
    await tick();
    const b = P(2, 2, 0.75);
    t.onEvent(at("click", S(b, "on-face", top(b))));
    await tick();
    release();
    await tick();
    await tick();
    expect(t.start()).toEqual(b);
    expect(t.inferContext().plane).not.toBeNull();
    expect(h.statuses.some((s) => s.text?.includes(GUESSED_WAY))).toBe(false);
  });

  it("sizes typed again before the refusal comes back: the first refusal adds nothing", async () => {
    const { h, t } = await fromCorner();
    t.onEvent(at("move", cornerSnap));
    const release = held(h);
    t.onValue("24, 3/8");
    await tick();
    const first = release;
    h.outcome = { kind: "drawn", sketch: 1, regions: [3], faces: [], inside: null };
    h.apply = fakeHost({ outcome: { kind: "drawn", sketch: 1, regions: [3], faces: [], inside: null } }).apply;
    t.onValue("2, 1");
    await tick();
    first();
    await tick();
    await tick();
    expect(t.busy()).toBe(false);
    expect(h.statuses.some((s) => s.text?.includes(GUESSED_WAY))).toBe(false);
  });
});

describe("Rectangle: the hidden line and the added sentence, only when they apply (#130 fix pass)", () => {
  const guideAt = (p: V3) => S(p, "axis-y", front(P(20, 0, 0.5)), { lines: [{ from: corner, to: p, kind: "axis-y" }] });

  it("off the face again, the Down arrow and Shift lock only to what the marker shows there", async () => {
    for (const how of ["down", "shift"] as const) {
      const { t } = await fromCorner();
      t.onEvent(at("move", guideAt(P(24, 1, 0.75))));
      // Then onto a plain point of the face: nothing hidden there.
      const q = P(12, 3, 0.75);
      t.onEvent(at("move", S(q, "on-face", top(q))));
      if (how === "down") t.onKey({ key: "ArrowDown" } as KeyboardEvent);
      else t.onShift(true);
      expect(t.inferContext().lock ?? null).toBeNull();
    }
  });

  it("after Esc and a new first corner, the Down arrow and Shift do not lock to a line from before", async () => {
    for (const how of ["down", "shift"] as const) {
      const { t } = await fromCorner();
      t.onEvent(at("move", guideAt(P(24, 1, 0.75))));
      expect(t.cancel()).toBe(true);
      t.onEvent(at("click", cornerSnap));
      await tick();
      if (how === "down") t.onKey({ key: "ArrowDown" } as KeyboardEvent);
      else t.onShift(true);
      expect(t.inferContext().lock ?? null).toBeNull();
    }
  });

  it("a refusal with no reason from the kernel: no sentence from the Rectangle, the corner kept", async () => {
    const { h, t } = await fromCorner();
    t.onEvent(at("move", cornerSnap));
    h.outcome = null;
    h.lastError = () => null;
    const n = h.statuses.length;
    t.onValue("24, 3/8");
    await tick();
    expect(h.statuses.slice(n)).toEqual([{ text: "refused", error: true }]);
    expect(t.start()).toEqual(corner);
  });
});
