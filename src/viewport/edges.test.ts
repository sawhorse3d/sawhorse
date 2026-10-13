// Soft, smooth and hidden edges on the drawing side (lane 15): which edges are lines, which
// only silhouettes, which only dashed; and the smooth blend of the theme's three face tones.
import { describe, expect, it } from "vitest";
import { edgeInk, edgeSets, gridTones, toneWeights } from "./looks";

const seg = (k: number) => [k, 0, 0, k, 0, 1];
const nrm = (k: number) => [1, 0, 0, 0, 1, k];

describe("edge sets", () => {
  it("no flags: the edges themselves, untouched", () => {
    const e = new Float32Array([...seg(0), ...seg(1)]);
    const n = new Float32Array([...nrm(0), ...nrm(1)]);
    const s = edgeSets(e, n, new Uint8Array([0, 0]));
    expect(s.hard).toBe(e);
    expect(s.hardNormals).toBe(n);
    expect(s.soft.length + s.hidden.length).toBe(0);
    expect(edgeSets(e, n, undefined).hard).toBe(e);
    // Smooth alone does not change how an edge is drawn.
    expect(edgeSets(e, n, new Uint8Array([2, 2])).hard).toBe(e);
  });

  it("soft edges leave the lines (their normals go with them); hidden ones leave everything", () => {
    const e = new Float32Array([...seg(0), ...seg(1), ...seg(2), ...seg(3)]);
    const n = new Float32Array([...nrm(0), ...nrm(1), ...nrm(2), ...nrm(3)]);
    const s = edgeSets(e, n, new Uint8Array([0, 3, 4, 5]));
    expect([...s.hard]).toEqual(seg(0));
    expect([...(s.hardNormals ?? [])]).toEqual(nrm(0));
    expect([...s.soft]).toEqual(seg(1));
    expect([...s.softNormals]).toEqual(nrm(1));
    expect([...s.hidden]).toEqual([...seg(2), ...seg(3)]);
  });
});

describe("tone weights", () => {
  it("an axis normal gets exactly one tone; the weights always sum to one", () => {
    expect(toneWeights(0, 0, 1)).toEqual([1, 0, 0]);
    expect(toneWeights(0, -1, 0)).toEqual([0, 1, 0]);
    expect(toneWeights(1, 0, 0)).toEqual([0, 0, 1]);
    expect(toneWeights(0, 0, -1)).toEqual([0, 0, 1]);
    for (const [x, y, z] of [
      [0.6, 0.8, 0],
      [0.3, 0.3, 0.9],
      [-0.5, 0.5, -0.7],
    ]) {
      const w = toneWeights(x, y, z);
      expect(w[0] + w[1] + w[2]).toBeCloseTo(1, 12);
      for (const v of w) expect(v).toBeGreaterThanOrEqual(0);
    }
    // Round a cylinder the front tone turns into the side tone smoothly.
    const a = toneWeights(Math.sin(0.1), Math.cos(0.1), 0);
    const b = toneWeights(Math.sin(0.2), Math.cos(0.2), 0);
    expect(a[1]).toBeGreaterThan(b[1]);
    expect(a[1] - b[1]).toBeLessThan(0.05);
  });
});

describe("grid tones (lane 15)", () => {
  const theme = { grid: "#d4d8cf", gridMajor: "#9aa39a" };
  it("the theme's colours without a background of the style's own", () => {
    expect(gridTones(null, theme)).toEqual({ minor: "#d4d8cf", major: "#9aa39a" });
  });
  it("on a dark background the lines turn lighter, on a light one darker", () => {
    const dark = gridTones([20, 24, 30], theme);
    const light = gridTones([250, 248, 241], theme);
    const lum = (h: string) => parseInt(h.slice(1, 3), 16) + parseInt(h.slice(3, 5), 16) + parseInt(h.slice(5, 7), 16);
    expect(lum(dark.minor)).toBeGreaterThan(20 + 24 + 30);
    expect(lum(dark.major)).toBeGreaterThan(lum(dark.minor));
    expect(lum(light.minor)).toBeLessThan(250 + 248 + 241);
    expect(lum(light.major)).toBeLessThan(lum(light.minor));
    expect(light.minor).toMatch(/^#[0-9a-f]{6}$/);
  });
});

describe("edge ink (lane 15)", () => {
  it("keeps the theme's edge colour where it reads, and inks it where it would vanish", () => {
    expect(edgeInk(null, "#e8eae5")).toBe("#e8eae5");
    // The dark theme's light edges on the Shop drawing's paper: dark ink.
    expect(edgeInk([250, 248, 241], "#e8eae5")).toBe("#2b2e2b");
    // The light theme's dark edges on paper: as they are.
    expect(edgeInk([250, 248, 241], "#1d211f")).toBe("#1d211f");
    // Dark edges on a dark background: light ink.
    expect(edgeInk([20, 22, 24], "#1d211f")).toBe("#e8eae5");
  });
});
