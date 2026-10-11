import { describe, expect, it } from "vitest";
import {
  AXIS_COLORS,
  equivalentOrthoHalfHeight,
  equivalentPerspectiveDistance,
  gridHalfExtent,
  gridLines,
  gridSpec,
  gridVisibility,
  metresPerPixel,
  projectionForView,
  VIEW_DIRS,
  VIEWS,
} from "./camera";
import { M_PER_IN } from "../lib/units";

describe("standard views", () => {
  it("offers Iso, Top, Front, Right and Persp; only Persp is in perspective", () => {
    expect(VIEWS.map((v) => v.label)).toEqual(["Iso", "Top", "Front", "Right", "Persp"]);
    expect(VIEWS.map((v) => projectionForView(v.id))).toEqual(["parallel", "parallel", "parallel", "parallel", "perspective"]);
  });

  it("looks from where each name says (Z up)", () => {
    expect(VIEW_DIRS.top.dir).toEqual([0, 0, 1]);
    expect(VIEW_DIRS.front.dir).toEqual([0, -1, 0]);
    expect(VIEW_DIRS.right.dir).toEqual([1, 0, 0]);
    const iso = VIEW_DIRS.iso.dir;
    expect(iso[0]).toBeLessThan(0);
    expect(iso[1]).toBeLessThan(0);
    expect(iso[2]).toBeGreaterThan(0);
    expect(Math.hypot(...iso)).toBeCloseTo(1, 12);
  });

  it("keeps the mock's axis colours", () => {
    expect(AXIS_COLORS).toEqual({ x: "#c0392b", y: "#27ae60", z: "#2a63a8" });
  });
});

describe("projection toggle keeps the on-screen size", () => {
  it("round-trips parallel <-> perspective", () => {
    const h = equivalentOrthoHalfHeight(10);
    expect(equivalentPerspectiveDistance(h)).toBeCloseTo(10, 9);
    // 35 degree vertical field of view: half-height = d * tan(17.5 deg).
    expect(h).toBeCloseTo(10 * Math.tan((17.5 * Math.PI) / 180), 12);
  });

  it("pixel size is constant in parallel and grows with depth in perspective", () => {
    expect(metresPerPixel({ ortho: true, scale: 0.002, eye: [0, 0, 10], forward: [0, 0, -1] }, [5, 5, -100])).toBe(0.002);
    const persp = { ortho: false, scale: 0.001, eye: [0, 0, 10] as [number, number, number], forward: [0, 0, -1] as [number, number, number] };
    expect(metresPerPixel(persp, [0, 0, 0])).toBeCloseTo(0.01, 12);
    expect(metresPerPixel(persp, [3, 4, -10])).toBeCloseTo(0.02, 12);
  });
});

describe("ground grid", () => {
  it("is 1 in minor / 1 ft major in imperial units and 10 mm / 100 mm in metric", () => {
    for (const u of ["frac-in", "in", "ft-in"] as const) {
      const g = gridSpec(u);
      expect(g.minor).toBeCloseTo(M_PER_IN, 15);
      expect(g.major).toBeCloseTo(12 * M_PER_IN, 15);
      expect(g.ratio).toBe(12);
    }
    for (const u of ["mm", "cm", "m"] as const) expect(gridSpec(u)).toMatchObject({ minor: 0.01, major: 0.1, ratio: 10 });
  });

  it("hides lines that would be closer than 6 px and fades the minor ones in", () => {
    expect(gridVisibility(3, 12)).toMatchObject({ minor: false, major: true, opacity: 0 });
    expect(gridVisibility(0.4, 12)).toMatchObject({ minor: false, major: false });
    const near = gridVisibility(30, 12);
    expect(near.minor && near.major).toBe(true);
    expect(near.opacity).toBe(1);
    expect(gridVisibility(6, 12).opacity).toBeLessThan(gridVisibility(10, 12).opacity);
  });

  it("covers at least 8 ft and twice the model, in whole feet", () => {
    const g = gridSpec("frac-in");
    expect(gridHalfExtent(g, 0.1) / g.major).toBe(8);
    const big = gridHalfExtent(g, 3);
    expect(big).toBeGreaterThanOrEqual(6);
    expect(Math.abs(big / g.major - Math.round(big / g.major))).toBeLessThan(1e-9);
  });

  it("minor lines skip the positions the major grid and the axes draw", () => {
    const minor = gridLines(1, 24, 12); // 1-unit steps out to 24, majors every 12
    const xs = new Set<number>();
    for (let i = 0; i < minor.length; i += 12) xs.add(Math.round(minor[i]));
    expect(xs.has(0) || xs.has(12) || xs.has(-24)).toBe(false);
    expect(xs.has(1) && xs.has(-23)).toBe(true);
    expect(minor.length / 12).toBe(48 - 4); // 49 positions minus 0, +-12, +-24 ... (4 majors + the axis)
  });
});
