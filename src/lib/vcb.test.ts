import { describe, expect, it } from "vitest";
import { notHere, parseVcb } from "./vcb";
import { M_PER_FT, M_PER_IN, type UnitSystem } from "./units";

const len = (t: string, u: UnitSystem = "frac-in") => {
  const r = parseVcb(t, u);
  if (!r.ok || r.value.kind !== "length") throw new Error(`${t}: ${JSON.stringify(r)}`);
  return r.value.m;
};

describe("Value Control Box grammar", () => {
  it("reads the woodworker's lengths exactly", () => {
    expect(len("3/4")).toBeCloseTo(0.75 * M_PER_IN, 15);
    expect(len("1 1/2")).toBeCloseTo(1.5 * M_PER_IN, 15);
    expect(len("1-1/2")).toBeCloseTo(1.5 * M_PER_IN, 15);
    expect(len("1'6\"")).toBeCloseTo(M_PER_FT + 6 * M_PER_IN, 15);
    expect(len("1' 6 1/2\"")).toBeCloseTo(M_PER_FT + 6.5 * M_PER_IN, 15);
    expect(len("18\"")).toBeCloseTo(18 * M_PER_IN, 15);
    expect(len("457mm")).toBeCloseTo(0.457, 15);
    expect(len("45.7cm")).toBeCloseTo(0.457, 15);
    expect(len("11 1/4")).toBeCloseTo(11.25 * M_PER_IN, 15);
    expect(len("-3/8")).toBeCloseTo(-0.375 * M_PER_IN, 15);
    // A bare number follows the display units.
    expect(len("457", "mm")).toBeCloseTo(0.457, 15);
  });

  it("reads two lengths, counts, radii and angles", () => {
    expect(parseVcb("24,12", "frac-in")).toEqual({ ok: true, value: { kind: "lengths", m: [24 * M_PER_IN, 12 * M_PER_IN] } });
    const mm = parseVcb("600mm; 300mm", "frac-in");
    expect(mm.ok && mm.value.kind === "lengths" && mm.value.m.map((x) => Math.round(x * 1000))).toEqual([600, 300]);
    expect(parseVcb("48s", "frac-in")).toEqual({ ok: true, value: { kind: "count", n: 48 } });
    expect(parseVcb("6S", "frac-in")).toEqual({ ok: true, value: { kind: "count", n: 6 } });
    const r = parseVcb("12r", "frac-in");
    expect(r.ok && r.value.kind === "radius" && r.value.m).toBeCloseTo(12 * M_PER_IN, 15);
    expect(parseVcb("30°", "frac-in")).toEqual({ ok: true, value: { kind: "angle", deg: 30 } });
    expect(parseVcb("-15deg", "frac-in")).toEqual({ ok: true, value: { kind: "angle", deg: -15 } });
  });

  it("reserves array syntax for Move (lane 4)", () => {
    expect(parseVcb("x3", "frac-in")).toEqual({ ok: true, value: { kind: "array", copies: 3 } });
    expect(parseVcb("3x", "frac-in")).toEqual({ ok: true, value: { kind: "array", copies: 3 } });
    expect(parseVcb("*4", "frac-in")).toEqual({ ok: true, value: { kind: "array", copies: 4 } });
    expect(parseVcb("/4", "frac-in")).toEqual({ ok: true, value: { kind: "divide", parts: 4 } });
    const v = parseVcb("x3", "frac-in");
    expect(v.ok && notHere(v.value, "Line", "a length")).toMatch(/lane 4/);
  });

  it("explains what it cannot read", () => {
    for (const bad of ["", "banana", "3/0", "24,", "x0"]) {
      const r = parseVcb(bad, "frac-in");
      expect(r.ok).toBe(false);
    }
  });
});
