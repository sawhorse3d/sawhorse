// Soft, smooth and hidden edges on the drawing side (lane 15): which edges are lines, which
// only silhouettes, which only dashed; and the smooth blend of the theme's three face tones.
import { describe, expect, it } from "vitest";
import { edgeSets, toneWeights } from "./looks";

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
