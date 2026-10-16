// The style on the UI side (lane 21, issue #4): the grid switch fills in as on, and is
// written to the document only when it is off (a file without it shows the grid).
import { describe, expect, it } from "vitest";
import { compactStyle, fullStyle, STYLE_PRESETS } from "./looks";

describe("the grid switch in the style", () => {
  it("defaults on, for no style and for a style written before it existed", () => {
    expect(fullStyle(null).grid).toBe(true);
    expect(fullStyle({ face: "shaded", section_planes: false }).grid).toBe(true);
    expect(fullStyle({ grid: false }).grid).toBe(false);
  });

  it("round-trips: off is written, on is left out", () => {
    expect(compactStyle(fullStyle(null))).toBeNull();
    const off = compactStyle({ ...fullStyle(null), grid: false });
    expect(off).toEqual({ grid: false });
    expect(fullStyle(off).grid).toBe(false);
    expect(compactStyle(fullStyle(off))).toEqual({ grid: false });
    expect(compactStyle({ ...fullStyle(off), grid: true })).toBeNull();
  });

  it("every preset shows the grid (the Styles pane keeps the current switch when one is chosen)", () => {
    for (const p of STYLE_PRESETS) expect(p.style.grid, p.name).toBe(true);
  });
});
