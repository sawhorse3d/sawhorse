import { describe, expect, it } from "vitest";
import { csvBytes, cutListCsv, parseCsv } from "./csv";
import { labelSlot, labelsPdf, LABEL_SHEET, reportPdf } from "./documents";
import { defaultRounding, formatSteps, rowNotes, summary } from "./format";
import { fit, PdfDoc, readPdf, textWidth, winAnsi } from "./pdf";
import { bookshelf, row } from "./fixture";

describe("cut list formatting", () => {
  it("writes sizes as a woodworker does, from the kernel's rounding steps", () => {
    expect(formatSteps(180, "sixteenth")).toBe("11 1/4");
    expect(formatSteps(12, "sixteenth")).toBe("3/4");
    expect(formatSteps(1152, "sixteenth")).toBe("72");
    expect(formatSteps(25, "thirty_second")).toBe("25/32");
    expect(formatSteps(39, "half_mm")).toBe("19.5");
    expect(formatSteps(38, "half_mm")).toBe("19");
    expect(defaultRounding("mm")).toBe("half_mm");
    expect(defaultRounding("frac-in")).toBe("sixteenth");
  });

  it("the bookshelf: 18.99 board feet of oak and one 4x8 sheet of ply", () => {
    const list = bookshelf();
    // 2735.15625 cubic inches of lumber / 144.
    expect(list.totals.board_feet).toBe(2735.15625 / 144);
    const s = summary(list);
    expect(s).toEqual({ boardFeet: "18.99", boardFeetLabel: "board feet, Oak", sheets: "1", sheetsLabel: "sheet 4×8 1/4 birch ply, 1/4" });
    expect(rowNotes({ ...list.rows[0], handed: 1, total: 2, hidden: 1, shaped: true })).toEqual(["shaped (box it comes out of)", "handed: 1 of 2 mirrored", "1 hidden"]);
  });
});

describe("CSV", () => {
  it("round-trips: every row and the totals read back", () => {
    const list = bookshelf();
    list.rows[0].name = 'Side, "left"';
    const text = cutListCsv(list);
    expect(text.endsWith("\r\n")).toBe(true);
    const rows = parseCsv(text);
    expect(rows[0]).toEqual(["Part", "Qty", "T (in)", "W (in)", "L (in)", "Material", "Grain", "Stock", "Board feet", "Area (sq ft)", "Notes"]);
    expect(rows.slice(1, 6).map((r) => r.slice(0, 7))).toEqual([
      ['Side, "left"', "2", "3/4", "11 1/4", "71 1/4", "Oak", "along L"],
      ["Shelf", "4", "3/4", "11", "34 1/2", "Oak", "along L"],
      ["Top", "1", "3/4", "11 1/4", "36", "Oak", "along W (across)"],
      ["Toe kick", "1", "3/4", "3 1/2", "34 1/2", "Oak", "along L"],
      ["Back panel", "1", "1/4", "34 1/2", "71 1/4", "1/4 birch ply", "along L"],
    ]);
    expect(rows[1][8]).toBe("8.35");
    expect(rows[5].slice(7, 10)).toEqual(["Sheet", "", "17.07"]);
    expect(rows[6].slice(0, 2)).toEqual(["Total", "9"]);
    expect(rows[6][8]).toBe("18.99");
    expect(rows).toHaveLength(7);
    const bytes = csvBytes(list);
    expect([...bytes.slice(0, 3)]).toEqual([0xef, 0xbb, 0xbf]);
    expect(parseCsv(new TextDecoder().decode(bytes.slice(3)))).toEqual(rows);
  });

  it("metric lists say mm and square metres", () => {
    const list = bookshelf();
    list.rounding = "half_mm";
    list.rows = list.rows.map((r) => ({ ...r, steps: r.steps.map((s) => Math.round((s / 16) * 50.8)) as [number, number, number] }));
    const rows = parseCsv(cutListCsv(list));
    expect(rows[0][2]).toBe("T (mm)");
    expect(rows[0][9]).toBe("Area (m²)");
    // 19.05, 285.75 and 1809.75 mm to the nearest 0.5 mm (halves up).
    expect(rows[1].slice(2, 5)).toEqual(["19", "286", "1810"]);
  });
});

describe("PDF writer", () => {
  it("writes a valid file whose offsets, pages and text read back", () => {
    const d = new PdfDoc({ title: "Test" });
    d.page().text(72, 700, "Hello (world) \\ 3/4 × 11 1/4", 12).line(72, 690, 300, 690).rect(72, 600, 100, 50, { stroke: 0 });
    d.page().text(72, 700, "Page two", 12, { bold: true });
    const back = readPdf(d.bytes());
    expect(back.pages).toBe(2);
    expect(back.text).toEqual([["Hello (world) \\ 3/4 × 11 1/4"], ["Page two"]]);
    // Deterministic.
    expect(d.bytes()).toEqual(d.bytes());
  });

  it("measures and fits text", () => {
    expect(textWidth("0000", 10)).toBeCloseTo(22.24, 6);
    expect(winAnsi("a×b☃")).toEqual([97, 0xd7, 98, 63]);
    const t = fit("A very long part name that does not fit", 10, 80);
    expect(t.endsWith("...")).toBe(true);
    expect(textWidth(t, 10)).toBeLessThanOrEqual(80);
  });

  it("the report lists every part, the totals and the note", () => {
    const back = readPdf(reportPdf(bookshelf(), "Bookshelf"));
    expect(back.pages).toBe(1);
    const text = back.text[0];
    expect(text[0]).toBe("Cut list: Bookshelf");
    for (const want of ["Side", "Shelf", "Top", "Toe kick", "Back panel", "71 1/4", "34 1/2", "18.99 board feet, Oak"]) expect(text.some((t) => t.includes(want)), want).toBe(true);
    expect(text.some((t) => t.startsWith("1 sheet 4x8, 1/4 birch ply"))).toBe(true);
    expect(text.join(" ")).toContain("nesting");
  });

  it("a long list runs onto more pages", () => {
    const list = bookshelf();
    list.rows = Array.from({ length: 90 }, (_, k) => row(`Part ${k + 1}`, 1, [12, 64, 320]));
    const back = readPdf(reportPdf(list, "Many"));
    expect(back.pages).toBeGreaterThan(1);
    const all = back.text.flat();
    for (let k = 1; k <= 90; k++) expect(all).toContain(`Part ${k}`);
  });

  it("labels: one per part on an Avery 5160 grid, with size, material and grain", () => {
    expect(labelSlot(0)).toEqual({ page: 0, x: LABEL_SHEET.left, y: 792 - 36 - 72 });
    expect(labelSlot(2).x).toBeCloseTo(13.5 + 2 * 198, 9);
    expect(labelSlot(29).y).toBeCloseTo(792 - 36 - 720, 9);
    expect(labelSlot(30)).toEqual({ page: 1, x: LABEL_SHEET.left, y: 792 - 36 - 72 });
    // The grid fits on Letter: 3 x 2 5/8 + 2 x 1/8 + 2 x 3/16 = 8 1/2 in; 10 x 1 + 2 x 1/2 = 11 in.
    expect(LABEL_SHEET.left * 2 + LABEL_SHEET.pitchX * 2 + LABEL_SHEET.width).toBeCloseTo(612, 9);
    const back = readPdf(labelsPdf(bookshelf(), "Bookshelf"));
    expect(back.pages).toBe(1);
    const t = back.text[0];
    expect(t).toEqual([
      "Side", "×2", "3/4 × 11 1/4 × 71 1/4 in", "Oak", "Grain along L",
      "Shelf", "×4", "3/4 × 11 × 34 1/2 in", "Oak", "Grain along L",
      "Top", "×1", "3/4 × 11 1/4 × 36 in", "Oak", "Grain along W (across)",
      "Toe kick", "×1", "3/4 × 3 1/2 × 34 1/2 in", "Oak", "Grain along L",
      "Back panel", "×1", "1/4 × 34 1/2 × 71 1/4 in", "1/4 birch ply (sheet)", "Grain along L",
    ]);
  });
});
