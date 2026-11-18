// The nesting's words and files (lane 9) on a small fixed nesting: lengths, the cut sequence
// and the reasons in shop words; the SVG read back by the DOM's own XML parser, the DXF by
// the independent `dxf-parser` library (both flavours), the PDF booklet by our PDF reader.
// Every part must be in every file, where the kernel put it. The same checks run on real
// kernel output in `nesting.test.ts`.
import { describe, expect, it } from "vitest";
import { checkDxf, checkPdf, checkSvg } from "./checks";
import { formatArea } from "../cutlist/format";
import { builtInStockText, cutText, fineU, KERF_DEFAULT, len, lenU, nestSummary, pct, pieceTitle, pieceYield, reasonText, settingsText } from "./format";
import { gluedNesting, smallNesting } from "./fixture";
import { pieceFileName } from "./svg";
import { parseLength } from "../lib/units";

const IN = 0.0254;

describe("nesting in shop words", () => {
  it("writes lengths, cuts, reasons and tiles from the kernel's exact sizes", () => {
    const n = smallNesting();
    expect(len(11.25 * IN, "sixteenth")).toBe("11 1/4");
    expect(lenU((3 / 32) * IN, "thirty_second")).toBe("3/32 in");
    expect(len(1219.2 / 1000, "half_mm")).toBe("1219");
    expect(pct(0.6234)).toBe("62.3%");
    // Kerf and trim keep the precision a saw is set to, whatever the cut list's rounding.
    expect(fineU((3 / 32) * IN, "sixteenth")).toBe("3/32 in");
    expect(fineU(IN / 8, "sixteenth")).toBe("1/8 in");
    expect(fineU(1.5 * IN, "sixteenth")).toBe("1 1/2 in");
    expect(fineU(2 * IN, "sixteenth")).toBe("2 in");
    expect(fineU(0, "sixteenth")).toBe("0 in");
    expect(fineU(0.0023812, "half_mm")).toBe("2.38 mm");
    expect(fineU((3 / 32) * IN, { r: "sixteenth", style: "decimal" })).toBe("0.094 in");
    expect(cutText(n.pieces[0].cuts[0], "sixteenth")).toBe("Crosscut to 34 1/2 in (cut 12 across)");
    expect(cutText(n.pieces[0].cuts[1], "sixteenth")).toBe("Rip to 11 in (cut 34 1/2 long)");
    expect(cutText({ ...n.pieces[0].cuts[0], trim: true, size: 0.5 * IN }, "sixteenth")).toBe("Trim an end (1/2 in off)");
    expect(reasonText(n.unplaced[0])).toBe("fits 4/4 x 12 x 96 only with its grain across; turn its grain or pick other stock");
    expect(reasonText({ ...n.unplaced[0], reason: "too_large" })).toContain("larger than 4/4 x 12 x 96");
    expect(reasonText({ ...n.unplaced[0], reason: "no_stock", sheet: true })).toContain("no sheet in the stock library is this thickness");
    const s = nestSummary(n, "sixteenth");
    expect([s.sheets, s.boards]).toEqual(["1", "1"]);
    expect(s.sheetsLabel).toBe("sheet: 4 x 8 sheet, 1/4");
    expect(s.boardsLabel).toBe("board: 1 × 4/4, 8.0 bd ft");
    expect(s.yieldText).toBe(pct(n.totals.efficiency));
    expect(s.yieldLabel).toContain("3 of 4 parts placed");
    expect(settingsText(n, "sixteenth")).toBe("Kerf 1/8 in · trim none · table saw");
    expect(pieceTitle(n.pieces[0])).toBe("Board 1 of 1 · 4/4 x 12 x 96 · Oak");
    expect(pieceFileName("Bookshelf", n.pieces[1])).toBe("Bookshelf 02 sheet 1 of 1, 4 x 8 sheet, 1-4.svg");
  });

  it("#58: what Built-in stock puts back is written in the document's units, as the Kerf field writes it", () => {
    expect(builtInStockText("sixteenth")).toBe("kerf 1/8 in, no trim, joint loss 1/8 in, minimum strip auto, table saw");
    expect(builtInStockText({ r: "sixteenth", style: "decimal" })).toBe("kerf 0.125 in, no trim, joint loss 0.125 in, minimum strip auto, table saw");
    // Metric: the built-in set is still 1/8 in; metric defaults are open on #58.
    expect(builtInStockText("half_mm")).toBe("kerf 3.18 mm, no trim, joint loss 3.18 mm, minimum strip auto, table saw");
    expect(builtInStockText("half_mm")).toContain(`kerf ${fineU(KERF_DEFAULT, "half_mm")}`);
    expect(KERF_DEFAULT).toBe(IN / 8);
  });

  it("rounds half-hundredths of a millimetre without lifting shorter lengths", () => {
    expect(fineU(IN / 8, "half_mm")).toBe("3.18 mm");
    expect(fineU(0.00317499, "half_mm")).toBe("3.17 mm");
    expect(fineU(0.00317501, "half_mm")).toBe("3.18 mm");
    expect(fineU(0.0031749999999999, "half_mm")).toBe("3.17 mm");
    expect(fineU((3 / 8) * IN, "half_mm")).toBe("9.53 mm");
    expect(fineU(0.003125, { r: "sixteenth", style: "mm" })).toBe("3.13 mm");
  });

  it.each([
    [3 / 16, "0.188 in"],
    [29 / 16, "1.813 in"],
    [53 / 16, "3.313 in"],
    [3 / 16 - 1e-14, "0.187 in"],
  ])("rounds %s inches to the nearest displayed thousandth", (inches, displayed) => {
    expect(fineU(inches * IN, { r: "sixteenth", style: "decimal" })).toBe(displayed);
  });

  it("rounds a decimal inch midpoint also reached through a millimetre conversion", () => {
    expect(fineU(2.0075 * 25.4 / 1000, { r: "sixteenth", style: "decimal" })).toBe("2.008 in");
    // Built this way, 8.1055 in lands far enough under its halfway that half the nudge reads
    // it 8.105 in. No length the parser reads gives this double ("8.1055", "205.8797 mm" and
    // the rest multiply by 0.0254 or 0.001); a length worked out in millimetres does.
    expect(fineU(8.1055 * 25.4 / 1000, { r: "sixteenth", style: "decimal" })).toBe("8.106 in");
  });

  it.each([
    [3 / 128, "1/32 in"],
    [29 / 128, "15/64 in"],
    [127 / 128, "1 in"],
    [3 / 128 - 1e-14, "1/64 in"],
  ])("rounds %s inches to the nearest displayed sixty-fourth", (inches, displayed) => {
    expect(fineU(inches * IN, "sixteenth")).toBe(displayed);
  });

  it("#92: typed as the issue has them, 3/16 and 1 13/16 in read 0.188 and 1.813 in, 3/128 and 29/128 in read 1/32 and 15/64 in", () => {
    const typed = (t: string) => {
      const r = parseLength(t, "frac-in");
      if (!r.ok) throw new Error(r.error);
      return r.metres;
    };
    const decimal = { r: "sixteenth", style: "decimal" } as const;
    expect(fineU(typed("3/16"), decimal)).toBe("0.188 in");
    expect(fineU(typed("1 13/16"), decimal)).toBe("1.813 in");
    expect(fineU(typed("3/128"), "sixteenth")).toBe("1/32 in");
    expect(fineU(typed("29/128"), "sixteenth")).toBe("15/64 in");
    // Near the top of the fields' range, where the nudge is largest in metres: a halfway
    // length typed as a decimal reads up, and one a hair under it still reads down.
    expect(fineU(typed("11.9995"), decimal)).toBe("12 in");
    expect(fineU((11.9995 - 1e-13) * IN, decimal)).toBe("11.999 in");
    expect(fineU(typed("11 127/128"), "sixteenth")).toBe("12 in");
    expect(fineU((11 + 127 / 128 - 1e-13) * IN, "sixteenth")).toBe("11 63/64 in");
  });

  it("#57: the footer names a set minimum strip beside the joint loss, only when something is glued up", () => {
    const g = gluedNesting();
    expect(settingsText(g, "sixteenth")).toBe("Kerf 1/8 in · trim none · joint loss 1/8 in · table saw");
    g.settings = { ...g.settings, minStrip: 2 * 0.0254 };
    expect(settingsText(g, "sixteenth")).toBe("Kerf 1/8 in · trim none · joint loss 1/8 in · min strip 2 in · table saw");
    g.settings = { ...g.settings, minStrip: 0 };
    expect(settingsText(g, "sixteenth")).toBe("Kerf 1/8 in · trim none · joint loss 1/8 in · min strip 0 in · table saw");
    // Nothing glued up: the footer reads as before, set or not.
    const n = smallNesting();
    n.settings = { ...n.settings, minStrip: 2 * 0.0254 };
    expect(settingsText(n, "sixteenth")).toBe("Kerf 1/8 in · trim none · table saw");
  });

  // Lane 51: nine 16 x 32 in parts fill a 4 x 8 sheet with no kerf; before the kernel
  // reported such a sheet as exactly full, its waste came to -4.44e-16 m² and its efficiency
  // to 1.0000000000000002, which the panel wrote as "-0.00 sq ft waste".
  it("a waste that rounds to zero is never written with a minus, and a real one keeps it", () => {
    for (const r of ["sixteenth", "thirty_second", "half_mm"] as const) {
      const unit = r === "half_mm" ? "m²" : "sq ft";
      for (const m2 of [-4.440892098500626e-16, -0, 0, 1e-17, -1e-9, -0.0004]) expect(formatArea(m2, r)).toBe(`0.00 ${unit}`);
      expect(formatArea(-0.0929, r)).toBe(r === "half_mm" ? "-0.09 m²" : "-1.00 sq ft");
      expect(formatArea(-0.02, r)).toBe(r === "half_mm" ? "-0.02 m²" : "-0.22 sq ft");
      expect(formatArea(2.9728972799999993, r)).toBe(r === "half_mm" ? "2.97 m²" : "32.00 sq ft");
    }
    const n = smallNesting();
    const sheet = n.pieces[1];
    const full = { ...sheet, parts_area: 2.9728972799999998, stock_area: 2.9728972799999993, waste_area: -4.440892098500626e-16, efficiency: 1.0000000000000002 };
    expect(pieceYield(full, "sixteenth")).toBe("100.0% used · 0.00 sq ft waste");
    expect(pieceYield(full, "half_mm")).toBe("100.0% used · 0.00 m² waste");
    // As the kernel reports it now: exactly full.
    expect(pieceYield({ ...full, parts_area: full.stock_area, waste_area: 0, efficiency: 1 }, "sixteenth")).toBe("100.0% used · 0.00 sq ft waste");
    const totals = { ...n.totals, parts_area: full.parts_area, stock_area: full.stock_area, efficiency: full.efficiency };
    expect(nestSummary({ ...n, pieces: [full], totals }, "sixteenth").yieldLabel).toBe("yield · 3 of 4 parts placed · 0.00 sq ft waste");
    expect(nestSummary({ ...n, pieces: [full], totals }, "sixteenth").yieldText).toBe("100.0%");
  });

  it("the SVG of each piece holds every part where the kernel put it", () => checkSvg(smallNesting(), "Bookshelf"));
  it("the DXF holds a polyline per part on its piece's layer, in both flavours", () => checkDxf(smallNesting()));
  it("the PDF booklet has a summary and a page per piece with every part and cut", () => checkPdf(smallNesting(), "Bookshelf"));
});
