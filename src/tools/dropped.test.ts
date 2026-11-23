// #130: a click with no point under it and a drag (press, move, release) draw nothing, and
// the status line says why, in every tool that takes points by clicking. Halfway through a
// gesture the words end with the next step, and the gesture is kept. A hover says nothing.
import { describe, expect, it } from "vitest";
import type { V3 } from "../kernel/types";
import type { Target } from "../lib/edit";
import { ArcTool } from "./arc";
import { CircleTool } from "./circle";
import { dragWords, noPointWords } from "./dropped";
import { ev, fakeHost, type FakeHost } from "./fakeHost";
import { FollowTool } from "./follow";
import { LineTool } from "./line";
import { MoveTool } from "./move";
import { PaintTool, type PaintContext } from "./paint";
import { PushTool } from "./push";
import { RectTool } from "./rect";
import { RotateTool } from "./rotate";
import { SectionTool } from "./section";
import { DimensionTool, TapeTool } from "./tape";
import type { Tool, ToolEvent } from "./tool";

const tick = () => new Promise((r) => setTimeout(r, 0));
/** An event with nothing under the cursor: no snap, no pick. */
const nothing = (type: ToolEvent["type"]): ToolEvent => ({ ...ev(type, [0, 0, 0]), snap: null, picked: null, ray: { origin: [0, -10, 0.5], dir: [0, 1, 0] } });
const NO_POINT_DRAW = "Nothing to draw on there: click the ground or a face, or use Top or Iso.";
const DRAG_DRAW = "Click, move, then click again. A drag does not draw.";
const last = (h: FakeHost) => h.statuses.at(-1);

describe("the words", () => {
  it("before a gesture they say how to find a point; halfway they end with the next step", () => {
    expect(noPointWords("draw on", null)).toBe(NO_POINT_DRAW);
    expect(noPointWords("draw on", "Click the opposite corner, or type width, depth.")).toBe("Nothing to click on there. Click the opposite corner, or type width, depth.");
    expect(dragWords("draw", "Click, move, then click again.", null)).toBe(DRAG_DRAW);
    expect(dragWords("draw", "Click, move, then click again.", "Click the radius or type it.")).toBe("A drag does not draw. Click the radius or type it.");
  });
});

/** Each draw tool, the words its first-click refusals use, and its hint once one point is down. */
const STROKES: { name: string; make: (h: FakeHost) => Tool; noPoint: string; drag: string; step: string }[] = [
  { name: "Line", make: (h) => new LineTool(h), noPoint: NO_POINT_DRAW, drag: DRAG_DRAW, step: "Click the next point or type a length." },
  { name: "Rectangle", make: (h) => new RectTool(h), noPoint: NO_POINT_DRAW, drag: DRAG_DRAW, step: "Click the opposite corner, or type width, depth." },
  {
    name: "Rotated Rectangle",
    make: (h) => {
      const t = new RectTool(h);
      t.cycleVariant();
      return t;
    },
    noPoint: NO_POINT_DRAW,
    drag: DRAG_DRAW,
    step: "Click the other end of the edge, or type its length.",
  },
  { name: "Circle", make: (h) => new CircleTool(h), noPoint: NO_POINT_DRAW, drag: DRAG_DRAW, step: "Click the radius or type it." },
  {
    name: "Polygon",
    make: (h) => {
      const t = new CircleTool(h);
      t.cycleVariant();
      return t;
    },
    noPoint: NO_POINT_DRAW,
    drag: DRAG_DRAW,
    step: "Click the radius or type it.",
  },
  { name: "Arc", make: (h) => new ArcTool(h), noPoint: NO_POINT_DRAW, drag: DRAG_DRAW, step: "Click the end." },
  {
    name: "3-Point Arc",
    make: (h) => {
      const t = new ArcTool(h);
      t.cycleVariant();
      return t;
    },
    noPoint: NO_POINT_DRAW,
    drag: DRAG_DRAW,
    step: "Click a point the arc passes through.",
  },
  {
    name: "Tape",
    make: (h) => new TapeTool(h),
    noPoint: "Nothing to measure there: click the ground or a face, or use Top or Iso.",
    drag: "Click, move, then click again. A drag does not measure.",
    step: "Click the second point. Type a length to drop a guide point.",
  },
  {
    name: "Dimension",
    make: (h) => new DimensionTool(h),
    noPoint: "Nothing to dimension there: click the ground or a face, or use Top or Iso.",
    drag: "Click, move, then click again. A drag does not place a dimension.",
    step: "Click the second point.",
  },
];

describe.each(STROKES)("$name", ({ make, noPoint, drag, step }) => {
  it("a hover over nothing says nothing; a click there draws nothing and says why", () => {
    const h = fakeHost();
    const t = make(h);
    t.onEvent(nothing("move"));
    t.onEvent(nothing("move"));
    expect(h.statuses).toEqual([]);
    t.onEvent(nothing("click"));
    expect(last(h)).toEqual({ text: noPoint, error: true });
    expect(h.ops).toEqual([]);
    expect(t.busy()).toBe(false);
    // A click that finds a point replaces the words with the tool's own.
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "free" }));
    expect(last(h)?.error).toBe(false);
    expect(last(h)?.text?.toLowerCase()).toContain(step.toLowerCase());
  });

  it("a drag before the first click draws nothing and says why", () => {
    const h = fakeHost();
    const t = make(h);
    t.onEvent(ev("down", [0, 0, 0], { snapKind: "free" }));
    t.onEvent(ev("drag", [0.1, 0, 0], { snapKind: "free", start: [0, 0] }));
    expect(h.statuses).toEqual([]);
    t.onEvent(ev("dragend", [0.1, 0, 0], { snapKind: "free", start: [0, 0] }));
    expect(last(h)).toEqual({ text: drag, error: true });
    expect(h.ops).toEqual([]);
    expect(t.busy()).toBe(false);
  });

  it("halfway through, a drag and a click on nothing keep the first point and say the next step", () => {
    const h = fakeHost();
    const t = make(h);
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "free" }));
    expect(t.busy()).toBe(true);
    t.onEvent(ev("drag", [0.1, 0.05, 0], { snapKind: "free", start: [0, 0] }));
    t.onEvent(ev("dragend", [0.1, 0.05, 0], { snapKind: "free", start: [0, 0] }));
    expect(last(h)).toEqual({ text: drag.replace(/^.*?(A drag does not [^.]+\.).*$/, `$1 ${step}`), error: true });
    expect(t.busy()).toBe(true);
    t.onEvent(nothing("click"));
    expect(last(h)).toEqual({ text: `Nothing to click on there. ${step}`, error: true });
    expect(t.busy()).toBe(true);
    expect(h.ops).toEqual([]);
  });
});

describe("a first corner survives a drag", () => {
  it("Rectangle: the corner clicked before the drag is the rectangle's corner", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", regions: [[1, 1]], faces: [] } as never });
    const t = new RectTool(h);
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "free" }));
    t.onEvent(ev("drag", [0.5, 0.5, 0], { snapKind: "free", start: [0, 0] }));
    t.onEvent(ev("dragend", [0.5, 0.5, 0], { snapKind: "free", start: [0, 0] }));
    t.onEvent(nothing("click"));
    t.onEvent(ev("move", [0.3, 0.2, 0], { snapKind: "free" }));
    t.onEvent(ev("click", [0.3, 0.2, 0], { snapKind: "free" }));
    await tick();
    expect(h.ops).toHaveLength(1);
    const op = h.ops[0] as unknown as { op: string; points: V3[] };
    expect(op.op).toBe("draw");
    expect(op.points).toEqual([
      [0, 0, 0],
      [0.3, 0, 0],
      [0.3, 0.2, 0],
      [0, 0.2, 0],
    ]);
    expect(last(h)?.error).toBe(false);
  });

  it("Line: a drag halfway through a chain leaves the chain where it was", async () => {
    const h = fakeHost({ outcome: { kind: "drawn", regions: [], faces: [] } as never });
    const t = new LineTool(h);
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "free" }));
    t.onEvent(ev("dragend", [0.5, 0.5, 0], { snapKind: "free", start: [0, 0] }));
    t.onEvent(ev("move", [0.3, 0, 0], { snapKind: "free" }));
    t.onEvent(ev("click", [0.3, 0, 0], { snapKind: "free" }));
    await tick();
    expect(h.ops).toHaveLength(1);
    expect((h.ops[0] as unknown as { points: V3[] }).points).toEqual([
      [0, 0, 0],
      [0.3, 0, 0],
    ]);
  });
});

describe("the other tools", () => {
  it("Rotate: a click on nothing and a drag turn nothing and say why; halfway the next step stays", async () => {
    const h = fakeHost();
    h.sel = [{ kind: "object", object: 1 }];
    const t = new RotateTool(h);
    t.onEvent(nothing("move"));
    expect(h.statuses).toEqual([]);
    t.onEvent(nothing("click"));
    await tick();
    expect(last(h)).toEqual({ text: "Nothing to turn about there: click the ground or a face, or use Top or Iso.", error: true });
    t.onEvent(ev("dragend", [0.1, 0, 0], { start: [0, 0] }));
    expect(last(h)).toEqual({ text: "Click the centre, then where the angle starts and ends. A drag does not turn.", error: true });
    t.onEvent(ev("click", [0, 0, 0]));
    await tick();
    expect(last(h)?.text).toMatch(/click where the angle starts\.$/);
    t.onEvent(ev("dragend", [0.1, 0, 0], { start: [0, 0] }));
    expect(last(h)).toEqual({ text: "A drag does not turn. Click where the angle starts.", error: true });
    t.onEvent(nothing("click"));
    await tick();
    expect(last(h)).toEqual({ text: "Nothing to click on there. Click where the angle starts.", error: true });
    expect(t.busy()).toBe(true);
    expect(h.ops).toEqual([]);
  });

  it("Move: a press on nothing says why (with something selected, and with nothing)", async () => {
    const h = fakeHost();
    const t = new MoveTool(h);
    t.onEvent(nothing("move"));
    expect(h.statuses).toEqual([]);
    t.onEvent(nothing("down"));
    t.onEvent(nothing("click"));
    await tick();
    // With nothing selected the first step is to pick something (a click on the ground would
    // be refused the same way), so the words say that, not where to find a point.
    expect(last(h)).toEqual({ text: "Click a solid or a sketch shape, or select what to move first.", error: true });
    h.sel = [{ kind: "object", object: 1 }];
    t.onEvent(nothing("down"));
    t.onEvent(nothing("click"));
    await tick();
    expect(last(h)).toEqual({ text: "Nothing to move from there: click the ground or a face, or use Top or Iso.", error: true });
    expect(t.busy()).toBe(false);
    expect(h.ops).toEqual([]);
  });

  it("Rotate with nothing selected: a click on nothing and a drag say to pick something first, as a click on the ground would", async () => {
    const h = fakeHost();
    const t = new RotateTool(h);
    t.onEvent(nothing("click"));
    await tick();
    expect(last(h)).toEqual({ text: "Click a solid or a sketch shape, or select what to move first.", error: true });
    t.onEvent(ev("dragend", [0.1, 0, 0], { start: [0, 0] }));
    expect(last(h)).toEqual({ text: "Click a solid or a sketch shape, or select what to move first. A drag does not turn.", error: true });
    // The same words a click on the ground gives with nothing selected.
    t.onEvent(ev("click", [0, 0, 0], { snapKind: "free" }));
    await tick();
    expect(last(h)).toEqual({ text: "Click a solid or a sketch shape, or select what to move first.", error: true });
  });

  it("Paint with no material chosen: a drag says to choose one first, as a click on a face would", () => {
    const h = fakeHost();
    const ctx = (active: boolean): PaintContext => ({
      active: () => (active ? ({ name: "White oak", color: [200, 180, 140], held: false } as never) : null),
      paint: async () => true,
      sample: async () => null,
      preview: () => undefined,
      faceUvs: async () => new Float32Array(),
      setPartUvs: () => undefined,
      materialOf: () => null,
    });
    new PaintTool(h, ctx(false)).onEvent(ev("dragend", [0, 0, 0], { start: [0, 0] }));
    expect(last(h)).toEqual({ text: "Choose a material in the Materials pane first (or Alt+click a face to pick one up). A drag does not paint.", error: true });
    new PaintTool(h, ctx(true)).onEvent(ev("dragend", [0, 0, 0], { start: [0, 0] }));
    expect(last(h)).toEqual({ text: "Click a face to paint it. A drag does not paint.", error: true });
  });

  it("Push/Pull: a drag that starts on nothing says what a click there says", () => {
    const h = fakeHost();
    const t = new PushTool(h);
    t.onEvent(nothing("down"));
    t.onEvent(nothing("drag"));
    expect(h.statuses).toEqual([]);
    t.onEvent(nothing("dragend"));
    expect(last(h)).toEqual({ text: "Click a face or a region of a sketch to push or pull it.", error: false });
    expect(h.ops).toEqual([]);
  });

  it("Section Plane, Follow Me and Paint: a drag places, sweeps and paints nothing, and says so", () => {
    const h = fakeHost();
    const s = new SectionTool(h);
    s.onEvent(ev("drag", [0, 0, 0], { start: [0, 0] }));
    const before = h.statuses.length;
    s.onEvent(ev("dragend", [0, 0, 0], { start: [0, 0] }));
    expect(h.statuses.length).toBe(before + 1);
    expect(last(h)).toEqual({ text: "Click to place a section plane. A drag does not place one.", error: true });
    expect(h.ops).toEqual([]);

    const f = new FollowTool(h);
    f.onEvent(ev("dragend", [0, 0, 0], { start: [0, 0] }));
    expect(last(h)).toEqual({ text: "Select the path first.", error: true });

    const ctx: PaintContext = {
      active: () => null,
      paint: async () => true,
      sample: async () => null,
      preview: () => undefined,
      faceUvs: async () => new Float32Array(),
      setPartUvs: () => undefined,
      materialOf: () => null,
    };
    const p = new PaintTool(h, ctx);
    p.onEvent(ev("dragend", [0, 0, 0], { start: [0, 0] }));
    expect(last(h)).toEqual({ text: "Choose a material in the Materials pane first (or Alt+click a face to pick one up). A drag does not paint.", error: true });
  });

  it("Follow Me with a path: a drag says to click the profile", async () => {
    const h = fakeHost();
    h.sel = [{ kind: "edge", owner: { kind: "sketch", id: 1 }, a: [0, 0, 0], b: [1, 0, 0], drawn: true }];
    const f = new FollowTool(h);
    f.activate();
    await tick();
    f.onEvent(ev("dragend", [0, 0, 0], { start: [0, 0] }));
    expect(last(h)).toEqual({ text: "Click the profile: a face, or a region of a sketch. A drag does not sweep.", error: true });
  });

  it("Push/Pull's target is not touched by a drag that started on nothing", () => {
    const h = fakeHost();
    const target: Target = { kind: "region", sketch: 1, region: 1 };
    h._target = target;
    const t = new PushTool(h);
    t.onEvent(nothing("down"));
    t.onEvent(nothing("dragend"));
    expect(h._target).toBe(target);
  });
});
