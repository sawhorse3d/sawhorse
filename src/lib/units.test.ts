import { describe, expect, it } from "vitest";
import {
  formatFractionalInches,
  formatLength,
  formatVolume,
  nextUnitSystem,
  UNIT_CHOICES,
  UNIT_SYSTEMS,
  unitInfo,
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

  // Lane 20: feet, then inches with a fraction, every way they are typed: 13 1/2 in each
  // (the release notes once said these read "like 1-1/2"", which they never did).
  it.each([`1'-1-1/2"`, `1' 1 1/2"`, `1'1.5"`, `1' 1.5"`, `1'-1.5"`, `1'1-1/2"`, `1'-1 1/2"`, `1'1½"`, `1' 1 1/2`, `1′ 1 1/2″`, `1'1 1/2''`, `13 1/2"`, `1.125'`])("reads %j as 13 1/2 in", (s) => {
    expect(ok(s)).toBeCloseTo(13.5 * IN, 12);
    // Feet or an inch mark: the same in a metric model.
    expect(ok(s, "mm")).toBeCloseTo(13.5 * IN, 12);
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

describe("the status bar's volume follows the units (#26)", () => {
  it("the inch systems read cubic inches to one decimal, as before", () => {
    const m3 = 2761.03125 * IN ** 3; // the bookshelf of e2e/bookshelf.spec.ts
    for (const s of ["frac-in", "in", "ft-in"] as const) expect(formatVolume(m3, s)).toBe("2761.0 in³");
    expect(formatVolume(0, "frac-in")).toBe("0.0 in³");
  });

  it("the metric systems read cubic metres, never cubic inches", () => {
    // A 300 mm cube, and a 19 x 90 board 2400 long.
    for (const s of ["mm", "cm", "m"] as const) {
      expect(formatVolume(0.3 ** 3, s)).toBe("0.027 m³");
      expect(formatVolume(0.019 * 0.09 * 2.4, s)).toBe("0.004104 m³");
      expect(formatVolume(0, s)).toBe("0 m³");
    }
    // Six decimals: a cubic centimetre is the smallest step shown.
    expect(formatVolume(1.5, "mm")).toBe("1.5 m³");
    expect(formatVolume(0.0000016, "mm")).toBe("0.000002 m³");
    // A 20 mm cube.
    expect(formatVolume(0.02 ** 3, "mm")).toBe("0.000008 m³");
  });

  it("a real solid smaller than the step never reads as 0 (from the squash)", () => {
    // A 7 mm cube (0.343 cm³), a 5 mm dowel 20 long (0.39 cm³), half a cubic centimetre
    // (5e-7 is a hair under it in binary), and the smallest volume a double holds.
    for (const m3 of [0.007 ** 3, Math.PI * 0.0025 ** 2 * 0.02, 5e-7, 4.9e-7, Number.MIN_VALUE])
      for (const s of ["mm", "cm", "m"] as const) expect(formatVolume(m3, s)).toBe("< 0.000001 m³");
    // Just over half a cubic centimetre rounds up to the step.
    expect(formatVolume(5.0001e-7, "mm")).toBe("0.000001 m³");
    expect(formatVolume(1e-6, "mm")).toBe("0.000001 m³");
    // Nothing (an empty or open model) is 0, and so is a tiny negative from an open mesh.
    expect(formatVolume(0, "m")).toBe("0 m³");
    expect(formatVolume(-0, "m")).toBe("0 m³");
    expect(formatVolume(-4e-7, "m")).toBe("0 m³");
    expect(formatVolume(-0.027, "m")).toBe("-0.027 m³");
  });

  it("a real solid smaller than the inch step never reads as 0.0 either (lane 38)", () => {
    // The same rule in cubic inches, whose step is a tenth: a 7 mm cube (0.0209 in³), a
    // 1/4 in cube (0.0156 in³), a hair under half a tenth, and the smallest volume a double
    // holds all read "< 0.1 in³" in every inch system, never "0.0 in³".
    for (const m3 of [0.007 ** 3, (0.25 * IN) ** 3, 0.0499 * IN ** 3, 1e-12, Number.MIN_VALUE])
      for (const s of ["frac-in", "in", "ft-in"] as const) expect(formatVolume(m3, s)).toBe("< 0.1 in³");
    // Half a tenth and more rounds to the step, as before; a 20 mm cube is 0.488 in³.
    expect(formatVolume(0.051 * IN ** 3, "in")).toBe("0.1 in³");
    expect(formatVolume(0.02 ** 3, "frac-in")).toBe("0.5 in³");
    // Nothing (an empty or open model) is still 0.0, and a tiny negative from an open mesh
    // reads as it always did.
    expect(formatVolume(0, "in")).toBe("0.0 in³");
    expect(formatVolume(-0, "in")).toBe("0.0 in³");
    expect(formatVolume(-1e-12, "in")).toBe("-0.0 in³");
  });

  it("a large model reads in whole cubic metres and their decimals (from the squash)", () => {
    expect(formatVolume(1234.5678901, "m")).toBe("1234.56789 m³");
    expect(formatVolume(1e6, "mm")).toBe("1000000 m³");
  });

  it("the inch systems print exactly what the badge printed before #46 (from the squash)", () => {
    // The badge's expression at a6792ea, whatever the units were. One change since lane 38:
    // a real solid that expression printed as "0.0 in³" (1e-12 here) reads "< 0.1 in³".
    const before = (m3: number) => `${(m3 / M_PER_IN ** 3).toFixed(1)} in³`;
    const spread = [0, -0, 1e-12, 0.05 * IN ** 3, 0.3 ** 3, 2761.03125 * IN ** 3, 26967.5 * IN ** 3, 12345.678, -0.027, NaN, Infinity];
    const now = (m3: number) => (m3 > 0 && before(m3) === "0.0 in³" ? "< 0.1 in³" : before(m3));
    for (const s of ["frac-in", "in", "ft-in"] as const) for (const m3 of spread) expect(formatVolume(m3, s)).toBe(now(m3));
    expect(spread.filter((m3) => now(m3) !== before(m3))).toEqual([1e-12]);
    expect(formatVolume(0.3 ** 3, "frac-in")).toBe("1647.6 in³");
  });

  it("the metric badge and the inch badge give the same volume, each to its own step (from the squash)", () => {
    // The kernel's volume in m³, read back from both texts: the m³ text within half a cubic
    // centimetre, the in³ text within half a tenth of a cubic inch.
    for (const m3 of [0.3 ** 3, 0.019 * 0.09 * 2.4, 0.441918, 2761.03125 * IN ** 3, 0.000123456, 3.75]) {
      const metric = Number(formatVolume(m3, "mm").replace(/ m³$/, ""));
      const inch = Number(formatVolume(m3, "frac-in").replace(/ in³$/, ""));
      expect(Math.abs(metric - m3)).toBeLessThanOrEqual(0.5e-6 + 1e-12);
      expect(Math.abs(inch * IN ** 3 - m3)).toBeLessThanOrEqual(0.05 * IN ** 3 + 1e-12);
    }
  });

  it("every unit system has a volume", () => {
    for (const u of UNIT_SYSTEMS) expect(formatVolume(1, u.id)).toMatch(/ (in³|m³)$/);
    expect(UNIT_SYSTEMS.map((u) => u.id).sort()).toEqual(["cm", "frac-in", "ft-in", "in", "m", "mm"]);
  });
});

describe("the unit picker's choices (lane 24)", () => {
  it("lists every unit system once, metric first, fractional inches last", () => {
    expect(UNIT_CHOICES).toEqual(["mm", "cm", "m", "in", "ft-in", "frac-in"]);
    expect([...UNIT_CHOICES].sort()).toEqual(UNIT_SYSTEMS.map((u) => u.id).sort());
  });

  it("names each in words", () => {
    expect(UNIT_CHOICES.map((id) => unitInfo(id).name)).toEqual(["millimetres", "centimetres", "metres", "decimal inches", "feet and inches", "fractional inches"]);
  });
});
