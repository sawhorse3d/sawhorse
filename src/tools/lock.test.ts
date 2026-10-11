import { describe, expect, it } from "vitest";
import type { Inference } from "../kernel/types";
import { InferenceMemory, snapOf } from "./driver";
import { LockState } from "./lock";
import type { Snap } from "./tool";

const start: [number, number, number] = [1, 1, 0];
const along = (kind: Snap["kind"], to: [number, number, number]): Snap => ({
  point: to,
  kind,
  tip: "",
  surface: null,
  entity: null,
  lines: [{ from: start, to, kind }],
});

describe("axis locks", () => {
  it("arrow keys lock red, green and blue; the same key unlocks", () => {
    const l = new LockState();
    expect(l.onKey("ArrowRight", null, start)).toBe(true);
    expect(l.lock).toEqual({ dir: [1, 0, 0], kind: "axis-x" });
    l.onKey("ArrowLeft", null, start);
    expect(l.lock?.kind).toBe("axis-y");
    l.onKey("ArrowUp", null, start);
    expect(l.lock?.kind).toBe("axis-z");
    l.onKey("ArrowUp", null, start);
    expect(l.lock).toBeNull();
    // No start point (nothing being drawn): arrows do nothing.
    expect(l.onKey("ArrowRight", null, null)).toBe(false);
  });

  it("Down arrow locks the parallel or perpendicular direction on show", () => {
    const l = new LockState();
    l.onKey("ArrowDown", along("parallel", [2, 2, 0]), start);
    expect(l.lock?.kind).toBe("parallel");
    expect(l.lock?.dir[0]).toBeCloseTo(Math.SQRT1_2, 12);
    l.onKey("ArrowDown", along("parallel", [2, 2, 0]), start);
    expect(l.lock).toBeNull();
  });

  it("holding Shift locks the line inference until released", () => {
    const l = new LockState();
    expect(l.onShift(true, along("axis-y", [1, 4, 0]), start)).toBe(true);
    expect(l.lock).toEqual({ dir: [0, 1, 0], kind: "axis-y" });
    expect(l.onShift(false, null, start)).toBe(true);
    expect(l.lock).toBeNull();
    // Shift over a plain point snap locks nothing.
    expect(l.onShift(true, { ...along("endpoint", [3, 3, 0]), lines: [] }, start)).toBe(false);
    // An arrow lock is not released by Shift.
    l.onKey("ArrowRight", null, start);
    l.onShift(false, null, start);
    expect(l.lock?.kind).toBe("axis-x");
  });
});

describe("inference memory", () => {
  const inf = (kind: Inference["kind"], point: [number, number, number], edge: Inference["edge"] = null): Inference => ({
    kind,
    point,
    tip: "",
    entity: null,
    surface: null,
    lines: [],
    edge,
    picked: null,
  });

  it("keeps the two most recent snapped points as from-point anchors and the last edge as a reference", () => {
    const m = new InferenceMemory();
    m.observe(inf("endpoint", [0, 0, 0]));
    m.observe(inf("on-face", [5, 5, 0]));
    m.observe(inf("midpoint", [1, 0, 0], [[0, 0, 0], [2, 0, 0]]));
    m.observe(inf("endpoint", [0, 0, 0]));
    m.observe(inf("centre", [3, 3, 0]));
    expect(m.anchors).toEqual([
      [3, 3, 0],
      [0, 0, 0],
    ]);
    expect(m.refs).toEqual([
      [
        [0, 0, 0],
        [2, 0, 0],
      ],
    ]);
    expect(snapOf(inf("grid", [1, 2, 0])).kind).toBe("grid");
  });
});
