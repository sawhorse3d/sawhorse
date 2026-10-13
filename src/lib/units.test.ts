import { describe, expect, it } from "vitest";
import {
  formatFractionalInches,
  formatLength,
  nextUnitSystem,
  parseDimensions,
  parseLength,
  parseNumber,
  M_PER_IN,
  type UnitSystem,
} from "./units";

const IN = M_PER_IN;
const ok = (text: string, system: UnitSystem = "frac-in") => {
  const r = parseLength(text, system);
  if (!r.ok) throw new Error(r.error);
  return r.metres;
};

describe("parseNumber", () => {
  it.each([
    ["3/4", 0.75],
    ["1 1/2", 1.5],
    ["1-1/2", 1.5],
    ["1 - 1/2", 1.5],
    ["12", 12],
    ["12.5", 12.5],
    [".5", 0.5],
    ["7.", 7],
  ])("%s -> %d", (s, n) => expect(parseNumber(s)).toBeCloseTo(n, 12));

  it.each(["", "abc", "1/0", "1 1/0", "1..2", "1/2/3"])("rejects %j", (s) => expect(parseNumber(s)).toBeNull());
});

describe("parseLength", () => {
  it("reads bare numbers in the display unit", () => {
    expect(ok("3/4")).toBeCloseTo(0.75 * IN, 12);
    expect(ok("24", "in")).toBeCloseTo(24 * IN, 12);
    expect(ok("24", "ft-in")).toBeCloseTo(24 * IN, 12);
    expect(ok("36", "mm")).toBeCloseTo(0.036, 12);
    expect(ok("2.5", "cm")).toBeCloseTo(0.025, 12);
    expect(ok("1.2", "m")).toBeCloseTo(1.2, 12);
  });

  it("lets a typed suffix override the display unit", () => {
    expect(ok("24mm")).toBeCloseTo(0.024, 12);
    expect(ok("24 mm")).toBeCloseTo(0.024, 12);
    expect(ok("2.5cm")).toBeCloseTo(0.025, 12);
    expect(ok("2m", "mm")).toBeCloseTo(2, 12);
    expect(ok('3/4"', "mm")).toBeCloseTo(0.75 * IN, 12);
    expect(ok("3/4in", "mm")).toBeCloseTo(0.75 * IN, 12);
    expect(ok("2ft", "mm")).toBeCloseTo(0.6096, 12);
  });

  it("reads feet and inches", () => {
    expect(ok("5'")).toBeCloseTo(60 * IN, 12);
    expect(ok(`5'6"`)).toBeCloseTo(66 * IN, 12);
    expect(ok(`5' 6 1/2"`)).toBeCloseTo(66.5 * IN, 12);
    expect(ok(`5'-6"`)).toBeCloseTo(66 * IN, 12);
    expect(ok("5' 6")).toBeCloseTo(66 * IN, 12);
    expect(ok("5′ 6″")).toBeCloseTo(66 * IN, 12);
  });

  // Lane 15: every way a woodworker writes one and a half inches reads the same.
  it.each([
    ['1-1/2"', true],
    ["1-1/2", false],
    ["1 1/2", false],
    ['1 1/2"', true],
    ["1.5", false],
    ['1.5"', true],
    ["1.5 in", true],
    ["1-1/2 in", true],
    ["1 1/2in", true],
    ["1 - 1/2", false],
    ["1½", false],
    ['1½"', true],
    ["1 ½", false],
    ["1 1/2''", true],
    ["1.5 inches", true],
    ["1 1/2 inch", true],
    ["  1-1/2” ", true],
    ["3/2", false],
  ] as const)("reads %j as 1 1/2 in (with inches typed: %s)", (s, inch) => {
    expect(ok(s)).toBeCloseTo(1.5 * IN, 12);
    // In a metric model a bare number is millimetres; a typed inch mark still wins.
    expect(ok(s, "mm")).toBeCloseTo(inch ? 1.5 * IN : 0.0015, 12);
  });

  it("reads fractions in feet-and-inches and with every unit", () => {
    expect(ok(`1'-1-1/2"`)).toBeCloseTo(13.5 * IN, 12);
    expect(ok(`1' 1 1/2"`)).toBeCloseTo(13.5 * IN, 12);
    expect(ok("¾")).toBeCloseTo(0.75 * IN, 12);
    expect(ok("23/32\"", "mm")).toBeCloseTo((23 / 32) * IN, 12);
    expect(ok("1 1/2 mm", "frac-in")).toBeCloseTo(0.0015, 12);
    expect(ok("-1-1/2\"")).toBeCloseTo(-1.5 * IN, 12);
  });

  it.each(["1-1/2-1/4", "1 1/2 1/4", "1.5.5", "1/2/4\"", "½½", "1--1/2"])("rejects the malformed %j", (s) => {
    expect(parseLength(s, "frac-in").ok).toBe(false);
  });

  it("keeps the sign", () => {
    expect(ok("-3/4")).toBeCloseTo(-0.75 * IN, 12);
    expect(ok("-1 1/2")).toBeCloseTo(-1.5 * IN, 12);
    expect(ok(`-5'6"`)).toBeCloseTo(-66 * IN, 12);
    expect(ok("+2", "mm")).toBeCloseTo(0.002, 12);
  });

  it.each(["", "abc", "3/0", "12 kg", "5'x", "--1"])("rejects %j", (s) => {
    expect(parseLength(s, "frac-in").ok).toBe(false);
  });

  it("rejects absurd sizes", () => {
    expect(parseLength("20000m", "m").ok).toBe(false);
  });
});

describe("parseDimensions", () => {
  it("reads two sizes", () => {
    const r = parseDimensions("24,12", "frac-in");
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.metres[0]).toBeCloseTo(24 * IN, 12);
    expect(r.metres[1]).toBeCloseTo(12 * IN, 12);
    const m = parseDimensions("600mm; 12", "frac-in");
    expect(m.ok).toBe(true);
    if (!m.ok) return;
    expect(m.metres[0]).toBeCloseTo(0.6, 12);
    expect(m.metres[1]).toBeCloseTo(12 * IN, 12);
  });
  it("needs exactly two", () => {
    expect(parseDimensions("24", "in").ok).toBe(false);
    expect(parseDimensions("1,2,3", "in").ok).toBe(false);
    expect(parseDimensions("1,x", "in").ok).toBe(false);
  });
});

describe("formatting", () => {
  it("fractional inches", () => {
    expect(formatFractionalInches(11.25)).toEqual(["11 1/4", false]);
    expect(formatFractionalInches(0.75)).toEqual(["3/4", false]);
    expect(formatFractionalInches(72)).toEqual(["72", false]);
    expect(formatFractionalInches(0)).toEqual(["0", false]);
    expect(formatFractionalInches(-0.5)).toEqual(["-1/2", false]);
    expect(formatFractionalInches(15.999)).toEqual(["16", true]);
    expect(formatFractionalInches(1 / 3)).toEqual(["5/16", true]);
  });

  it("each unit system", () => {
    const m = 11.25 * IN;
    expect(formatLength(m, "frac-in")).toBe("11 1/4");
    expect(formatLength(m, "in")).toBe("11.25");
    expect(formatLength(m, "ft-in")).toBe('11 1/4"');
    expect(formatLength(66.5 * IN, "ft-in")).toBe(`5' 6 1/2"`);
    expect(formatLength(72 * IN, "ft-in")).toBe(`6' 0"`);
    expect(formatLength(m, "mm")).toBe("~ 285.8");
    expect(formatLength(0.024, "mm")).toBe("24");
    expect(formatLength(0.025, "cm")).toBe("2.5");
    expect(formatLength(1.2, "m")).toBe("1.2");
    expect(formatLength(13.125 * IN + 0.0001, "frac-in")).toBe("~ 13 1/8");
  });

  it("round-trips typed values in their own unit", () => {
    for (const s of ["3/4", "11 1/4", "72", "1/16"]) expect(formatLength(ok(s), "frac-in")).toBe(s);
    for (const s of ["24", "600", "12.5"]) expect(formatLength(ok(s, "mm"), "mm")).toBe(s);
    for (const s of [`5' 6 1/2"`, '11 1/4"']) expect(formatLength(ok(s, "ft-in"), "ft-in")).toBe(s);
  });

  it("cycles unit systems", () => {
    expect(nextUnitSystem("frac-in")).toBe("in");
    expect(nextUnitSystem("m")).toBe("frac-in");
  });
});
