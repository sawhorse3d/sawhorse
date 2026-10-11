import { describe, expect, it } from "vitest";
import { classifyHover, INFERENCE_STYLES, type ScreenSegment } from "./hoverInference";

const seg: ScreenSegment = { a: [0, 0], b: [100, 0] };

describe("classifyHover", () => {
  it("prefers endpoints", () => {
    expect(classifyHover([3, 4], [seg], [3, 4])).toEqual({ kind: "endpoint", at: [0, 0] });
  });
  it("then midpoints", () => {
    expect(classifyHover([52, 3], [seg], null)).toEqual({ kind: "midpoint", at: [50, 0] });
  });
  it("then on-edge, snapped onto the segment", () => {
    expect(classifyHover([25, 4], [seg], null)).toEqual({ kind: "on-edge", at: [25, 0] });
  });
  it("then on-face", () => {
    expect(classifyHover([25, 40], [seg], [25, 40])).toEqual({ kind: "on-face", at: [25, 40] });
  });
  it("or nothing", () => {
    expect(classifyHover([25, 40], [seg], null)).toBeNull();
  });
  it("uses our four colours, never SketchUp's", () => {
    expect(Object.values(INFERENCE_STYLES).map((s) => s.token)).toEqual(["--inf-end", "--inf-mid", "--inf-edge", "--inf-face"]);
  });
});
