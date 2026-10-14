import { describe, expect, it } from "vitest";
import { clampSideWidth, maxSideWidth, SIDE_WIDE, SIDE_WIDTH } from "./SidePanel";

// Lane 20: the side panel's width (the Cut list's long part names and reasons need room).
describe("the side panel's width", () => {
  it("is the mock's 300 px at least, and leaves the model room beside the rail", () => {
    expect(SIDE_WIDTH).toBe(300);
    expect(clampSideWidth(120, 1440)).toBe(300);
    expect(clampSideWidth(Number.NaN, 1440)).toBe(300);
    expect(clampSideWidth(SIDE_WIDE, 1440)).toBe(600);
    // 1440 - 56 rail - 420 for the model.
    expect(maxSideWidth(1440)).toBe(964);
    expect(clampSideWidth(5000, 1440)).toBe(964);
    expect(maxSideWidth(4000)).toBe(1000);
    expect(clampSideWidth(612.6, 1440)).toBe(613);
  });

  it("stays 300 in a window too narrow to widen it (the Wide button is not offered then)", () => {
    expect(maxSideWidth(700)).toBe(300);
    expect(clampSideWidth(SIDE_WIDE, 700)).toBe(300);
    expect(clampSideWidth(SIDE_WIDE, 1000)).toBe(524);
  });
});
