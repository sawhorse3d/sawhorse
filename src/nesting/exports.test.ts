// The nesting's words and files (lane 9) on a small fixed nesting: lengths, the cut sequence
// and the reasons in shop words; the SVG read back by the DOM's own XML parser, the DXF by
// the independent `dxf-parser` library (both flavours), the PDF booklet by our PDF reader.
// Every part must be in every file, where the kernel put it. The same checks run on real
// kernel output in `nesting.test.ts`.
import { describe, expect, it } from "vitest";
import { checkDxf, checkPdf, checkSvg } from "./checks";
import { cutText, fineU, len, lenU, nestSummary, pct, pieceTitle, reasonText, settingsText } from "./format";
import { smallNesting } from "./fixture";
import { pieceFileName } from "./svg";

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

  it("the SVG of each piece holds every part where the kernel put it", () => checkSvg(smallNesting(), "Bookshelf"));
  it("the DXF holds a polyline per part on its piece's layer, in both flavours", () => checkDxf(smallNesting()));
  it("the PDF booklet has a summary and a page per piece with every part and cut", () => checkPdf(smallNesting(), "Bookshelf"));
});
