import { describe, expect, it } from "vitest";
import { csvBytes, csvCellText, cutListCsv, parseCsv, textCell } from "./csv";
import { labelSlot, labelsPdf, LABEL_SHEET, reportPdf } from "./documents";
import { decimalSteps, defaultRounding, formatSteps, GRAIN_WORDS, rowNotes, summary } from "./format";
import { parseNumber } from "../lib/units";
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
  it("round-trips: every row and the totals read back, fractions as text, decimals as numbers", () => {
    const list = bookshelf();
    list.rows[0].name = 'Side, "left"';
    list.rows[1].material = null;
    const text = cutListCsv(list);
    expect(text.endsWith("\r\n")).toBe(true);
    const rows = parseCsv(text);
    expect(rows[0]).toEqual(["Part", "Qty", "T", "W", "L", "T (in)", "W (in)", "L (in)", "Material", "Grain", "Stock", "Board feet", "Area (sq ft)", "Notes", "Summary"]);
    // Every line has every column.
    for (const r of rows) expect(r).toHaveLength(15);
    // Fractions are text formulas, so spreadsheets do not read 3/4 as a date or 11 1/4 as a number.
    expect(rows[1].slice(2, 5)).toEqual(['="3/4"', '="11 1/4"', '="71 1/4"']);
    expect(rows[2][3]).toBe("11");
    expect(text).toContain('"=""3/4"""');
    const read = rows.map((r) => r.map(csvCellText));
    expect(read.slice(1, 6).map((r) => r.slice(0, 10))).toEqual([
      ['Side, "left"', "2", "3/4", "11 1/4", "71 1/4", "0.75", "11.25", "71.25", "Oak", "along L"],
      ["Shelf", "4", "3/4", "11", "34 1/2", "0.75", "11", "34.5", "", "along L"],
      ["Top", "1", "3/4", "11 1/4", "36", "0.75", "11.25", "36", "Oak", GRAIN_WORDS.width],
      ["Toe kick", "1", "3/4", "3 1/2", "34 1/2", "0.75", "3.5", "34.5", "Oak", "along L"],
      ["Back panel", "1", "1/4", "34 1/2", "71 1/4", "0.25", "34.5", "71.25", "1/4 birch ply", "along L"],
    ]);
    // No dash placeholders anywhere.
    expect(text).not.toContain("—");
    // The decimals are the shown sizes, exactly.
    for (const r of read.slice(1, 6)) for (let k = 0; k < 3; k++) expect(parseNumber(r[2 + k])).toBe(Number(r[5 + k]));
    expect(read[1][11]).toBe("8.35");
    expect(read[5].slice(10, 13)).toEqual(["Sheet", "", "17.07"]);
    // The Total row: pieces, board feet, sheet area; the sheet count in Summary, not Stock.
    expect(read[6]).toEqual(["Total", "9", "", "", "", "", "", "", "", "", "", "18.99", "17.07", "", "1 sheet 4x8"]);
    expect(rows).toHaveLength(7);
    const bytes = csvBytes(list);
    expect([...bytes.slice(0, 3)]).toEqual([0xef, 0xbb, 0xbf]);
    expect(parseCsv(new TextDecoder().decode(bytes.slice(3)))).toEqual(rows);
  });

  it("text cells unwrap and quote safely", () => {
    expect(textCell("3/4")).toBe('="3/4"');
    expect(textCell("72")).toBe("72");
    expect(csvCellText('="11 1/4"')).toBe("11 1/4");
    expect(csvCellText("plain")).toBe("plain");
    expect(decimalSteps(1, "sixteenth")).toBe("0.0625");
    expect(decimalSteps(1, "thirty_second")).toBe("0.03125");
    expect(decimalSteps(39, "half_mm")).toBe("19.5");
  });

  it("metric lists say mm and square metres", () => {
    const list = bookshelf();
    list.rounding = "half_mm";
    list.rows = list.rows.map((r) => ({ ...r, steps: r.steps.map((s) => Math.round((s / 16) * 50.8)) as [number, number, number] }));
    const rows = parseCsv(cutListCsv(list));
    expect(rows[0].slice(5, 8)).toEqual(["T (mm)", "W (mm)", "L (mm)"]);
    expect(rows[0][12]).toBe("Area (m²)");
    // 19.05, 285.75 and 1809.75 mm to the nearest 0.5 mm (halves up).
    expect(rows[1].slice(2, 8)).toEqual(["19", "286", "1810", "19", "286", "1810"]);
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
