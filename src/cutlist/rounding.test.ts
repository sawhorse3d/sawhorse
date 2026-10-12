// Rounding and units (lane 7b): "Follow units" is the default, decimal-inch and ft-in units
// write decimal inches, a pick lasts for the session unless pinned, and no choice of units
// or rounding changes what is counted (the kernel test `every_rounding_classifies_and_
// totals_the_same` proves the totals; this proves every unit system maps onto those).
import { afterEach, describe, expect, it } from "vitest";
import { UNIT_SYSTEMS, type UnitSystem } from "../lib/units";
import { cutListCsv, parseCsv, csvCellText } from "./csv";
import { bookshelf } from "./fixture";
import { followRounding, formatSize, formatSteps, resolveRounding, ROUNDINGS, sizeStyle, summary } from "./format";
import { CUT_ROUNDING_KEY, loadCutRounding, saveCutRounding } from "./rounding-store";

afterEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});

describe("Follow units", () => {
  it("is the first choice and follows the units: 1/16 in for inches, 0.5 mm for metric", () => {
    expect(ROUNDINGS[0].id).toBe("follow");
    const want: Record<UnitSystem, string> = { "frac-in": "sixteenth", in: "sixteenth", "ft-in": "sixteenth", mm: "half_mm", cm: "half_mm", m: "half_mm" };
    for (const u of UNIT_SYSTEMS) {
      expect(followRounding(u.id)).toBe(want[u.id]);
      expect(resolveRounding("follow", u.id)).toBe(want[u.id]);
      expect(resolveRounding("thirty_second", u.id)).toBe("thirty_second");
    }
  });

  it("decimal-inch and ft-in units write decimal inches; fractional units write fractions", () => {
    expect(sizeStyle("sixteenth", "in")).toBe("decimal");
    expect(sizeStyle("thirty_second", "ft-in")).toBe("decimal");
    expect(sizeStyle("sixteenth", "frac-in")).toBe("fraction");
    expect(sizeStyle("sixteenth", "mm")).toBe("fraction");
    expect(sizeStyle("half_mm", "in")).toBe("mm");
    expect(formatSteps(180, "sixteenth", "decimal")).toBe("11.25");
    expect(formatSteps(1, "thirty_second", "decimal")).toBe("0.03125");
    expect(formatSteps(39, "half_mm", "decimal")).toBe("19.5");
    const list = { ...bookshelf(), style: "decimal" as const };
    expect(formatSize(list.rows[0], "sixteenth", "decimal")).toBe("0.75 × 11.25 × 71.25 in");
    expect(summary(list).sheetsLabel).toBe("sheet 4×8 1/4 birch ply, 0.25");
    const csv = parseCsv(cutListCsv(list)).map((r) => r.map(csvCellText));
    expect(csv[1].slice(2, 8)).toEqual(["0.75", "11.25", "71.25", "0.75", "11.25", "71.25"]);
  });

  it("every unit system gives the same counts: the units only pick a rounding and a style", () => {
    const base = summary(bookshelf());
    for (const u of UNIT_SYSTEMS) {
      const r = followRounding(u.id);
      const s = summary({ ...bookshelf(), rounding: r, style: sizeStyle(r, u.id) });
      expect([s.boardFeet, s.sheets], u.id).toEqual([base.boardFeet, base.sheets]);
    }
  });
});

describe("the Round to choice between visits", () => {
  it("defaults to Follow units", () => {
    expect(loadCutRounding()).toEqual({ choice: "follow", pinned: false });
  });

  it("a pick lasts for the session only", () => {
    saveCutRounding("thirty_second", false);
    expect(sessionStorage.getItem(CUT_ROUNDING_KEY)).toBe("thirty_second");
    expect(localStorage.getItem(CUT_ROUNDING_KEY)).toBeNull();
    expect(loadCutRounding()).toEqual({ choice: "thirty_second", pinned: false });
    sessionStorage.clear(); // a new session
    expect(loadCutRounding()).toEqual({ choice: "follow", pinned: false });
  });

  it("a pinned pick is kept for next time; unpinning forgets it", () => {
    saveCutRounding("half_mm", true);
    sessionStorage.clear();
    expect(loadCutRounding()).toEqual({ choice: "half_mm", pinned: true });
    saveCutRounding("half_mm", false);
    sessionStorage.clear();
    expect(loadCutRounding()).toEqual({ choice: "follow", pinned: false });
  });

  it("this session's pick wins over the pinned one; storage it cannot read is Follow units", () => {
    saveCutRounding("half_mm", true);
    sessionStorage.setItem(CUT_ROUNDING_KEY, "thirty_second");
    expect(loadCutRounding()).toEqual({ choice: "thirty_second", pinned: true });
    localStorage.setItem(CUT_ROUNDING_KEY, "nonsense");
    sessionStorage.clear();
    expect(loadCutRounding()).toEqual({ choice: "follow", pinned: false });
  });
});
