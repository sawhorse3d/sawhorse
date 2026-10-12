import { describe, expect, it } from "vitest";
import type { SurfaceHit } from "../kernel/types";
import { ev, fakeHost } from "./fakeHost";
import { SectionTool, keptSide, previewFor } from "./section";
import type { Snap } from "./tool";

/** The top face of a box (normal +Z) under the cursor at `p`. */
function topFace(p: [number, number, number]): SurfaceHit {
  return { at: { kind: "face", object: 1, face: 2 }, frame: { origin: [0, 0, 1], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }, point: p, t: 9 };
}

function snapOn(p: [number, number, number]): Snap {
  return { point: p, kind: "on-face", tip: "On Face", surface: topFace(p), entity: { kind: "face", object: 1, face: 2 }, lines: [] };
}

describe("Section Plane tool", () => {
  it("lays the plane on the face under the cursor, keeping the side the eye looks into", () => {
    const p = previewFor(snapOn([0.5, 0.5, 1]), [0, 0, -1], null, false);
    expect(p?.normal).toEqual([0, 0, -1]);
    expect(keptSide(p!.normal)).toBe("the side towards -Z");
    // Reversed: the side towards the eye.
    expect(previewFor(snapOn([0.5, 0.5, 1]), [0, 0, -1], null, true)?.normal).toEqual([0, 0, 1]);
    // Locked square to red: the plane faces along X, away from an eye looking along +X.
    expect(previewFor(snapOn([0.5, 0.5, 1]), [0.8, 0.2, -0.5], [1, 0, 0], false)?.normal).toEqual([1, 0, 0]);
    // Away from any surface: square to the axis the eye looks along most.
    const free: Snap = { point: [0, 0, 0], kind: "free", tip: "", surface: null, entity: null, lines: [] };
    expect(previewFor(free, [0.2, -0.9, -0.3], null, false)?.normal).toEqual([0, -1, 0]);
  });

  it("places an active plane with a click, Alt-click reverses it, Enter places the preview", async () => {
    const h = fakeHost({ outcome: { kind: "section_added", section: 4 } });
    const t = new SectionTool(h);
    t.activate();
    t.onEvent({ ...ev("move", [0.5, 0.5, 1]), snap: snapOn([0.5, 0.5, 1]) });
    expect(t.view().overlay.length).toBeGreaterThan(0);
    t.onEvent({ ...ev("click", [0.5, 0.5, 1]), snap: snapOn([0.5, 0.5, 1]) });
    await Promise.resolve();
    expect(h.ops[0]).toEqual({ op: "add_section", origin: [0.5, 0.5, 1], normal: [0, 0, -1] });
    await new Promise((r) => setTimeout(r, 0));
    expect(h.sel).toEqual([{ kind: "section", section: 4 }]);
    t.onEvent({ ...ev("click", [0.5, 0.5, 1]), snap: snapOn([0.5, 0.5, 1]), alt: true });
    await Promise.resolve();
    expect(h.ops[1]).toMatchObject({ op: "add_section", normal: [0, 0, 1] });
    // A tap of Alt reverses the preview; Enter places it.
    t.onModifier("alt");
    t.onKey(new KeyboardEvent("keydown", { key: "Enter" }));
    await Promise.resolve();
    expect(h.ops[2]).toMatchObject({ op: "add_section", normal: [0, 0, 1] });
    // Arrow keys lock it; Down unlocks.
    expect(t.onKey(new KeyboardEvent("keydown", { key: "ArrowRight" }))).toBe(true);
    expect(t.onKey(new KeyboardEvent("keydown", { key: "ArrowDown" }))).toBe(true);
    expect(t.onKey(new KeyboardEvent("keydown", { key: "x" }))).toBe(false);
  });
});
