// The cut list as paper (lane 7): a report PDF (the table, the totals and the notes, as many
// Letter pages as it takes) and a label sheet PDF (one label per part, laid out for Avery
// 5160-style sheets: 30 labels, 3 across and 10 down, 2 5/8 x 1 in).
import type { CutList } from "../kernel/types";
import { fit, LETTER, PdfDoc, textWidth, type PdfPage } from "./pdf";
import { formatArea, formatBoardFeet, formatSize, formatSteps, GRAIN_WORDS, listStyle, materialText, REPORT_COLUMNS, reportCells, scopeNote, sizeUnit, summary } from "./format";

const IN = 72;

/** The report's columns ({@link REPORT_COLUMNS}): x (left edge), width, right-aligned,
 * font size. The print view has the same columns (CutListReport). Part, Material and Notes
 * wrap onto more lines rather than lose words; the rest are short. */
const LAYOUT: [number, number, boolean, number][] = [
  [40, 100, false, 9.5], // Part
  [140, 26, true, 9.5], // Qty
  [166, 42, true, 9.5], // T
  [208, 46, true, 9.5], // W
  [254, 50, true, 9.5], // L
  [310, 86, false, 9.5], // Material
  [396, 44, false, 8.5], // Grain
  [440, 62, true, 8.5], // Bd ft / area
  [508, 64, false, 7.5], // Notes (wraps)
];

/** Columns that wrap: Part, Material, Notes. */
const WRAPS = new Set([0, 5, 8]);

/** `text` broken into lines that fit `width` points (a word too long for a line is cut). */
export function wrap(text: string, size: number, width: number, bold = false): string[] {
  const lines: string[] = [];
  let cur = "";
  for (const w of text.split(/\s+/).filter(Boolean)) {
    const next = cur ? `${cur} ${w}` : w;
    if (cur && textWidth(next, size, bold) > width) {
      lines.push(cur);
      cur = fit(w, size, width, bold);
    } else cur = textWidth(next, size, bold) > width ? fit(next, size, width, bold) : next;
  }
  if (cur) lines.push(cur);
  return lines;
}

/** The printable report as a PDF. `title`: the model's name. */
export function reportPdf(list: CutList, title: string): Uint8Array {
  const doc = new PdfDoc({ title: `Cut list: ${title}`, creator: "Sawhorse" });
  const [W, H] = LETTER;
  const top = H - 48;
  const bottom = 64;
  const rowH = 16;
  let page: PdfPage = doc.page();
  let y = top;
  const header = (p: PdfPage, first: boolean) => {
    y = top;
    p.text(40, y, first ? `Cut list: ${title}` : `Cut list: ${title} (continued)`, first ? 16 : 11, { bold: true });
    y -= first ? 16 : 12;
    if (first) {
      const s = summary(list);
      p.text(40, y, `${list.totals.parts} parts, ${list.totals.pieces} pieces · ${s.boardFeet} ${s.boardFeetLabel} · ${s.sheets} ${s.sheetsLabel} · sizes in ${sizeUnit(list.rounding)}`, 9, { gray: 0.35 });
      y -= 22;
    } else y -= 8;
    REPORT_COLUMNS.forEach((label, k) => {
      const [x, w, right] = LAYOUT[k];
      p.text(right ? x + w - 4 : x, y, label.toUpperCase(), 7, { bold: true, align: right ? "right" : "left", gray: 0.35 });
    });
    y -= 6;
    p.line(40, y, W - 40, y, 0.75);
    y -= rowH - 4;
  };
  header(page, true);
  for (const row of list.rows) {
    const cells = reportCells(row, list);
    const lines = cells.map((c, k) => (WRAPS.has(k) ? wrap(c, LAYOUT[k][3], LAYOUT[k][1] - 6, k === 0).slice(0, 4) : [c]));
    const h = rowH + Math.max(0, ...lines.map((l) => l.length - 1)) * 10;
    if (y < bottom + h) {
      page = doc.page();
      header(page, false);
    }
    lines.forEach((ls, k) => {
      const [x, w, right, size] = LAYOUT[k];
      ls.forEach((l, i) => page.text(right ? x + w - 4 : x, y - i * 10, fit(l, size, w - 6, k === 0), size, { bold: k === 0, align: right ? "right" : "left" }));
    });
    page.line(40, y - h + 11, W - 40, y - h + 11, 0.25, 0.7);
    y -= h;
  }
  // Totals.
  if (y < bottom + 70) {
    page = doc.page();
    header(page, false);
  }
  y -= 8;
  page.text(40, y, "Totals", 11, { bold: true });
  y -= 15;
  for (const l of list.totals.lumber) {
    page.text(48, y, `${formatBoardFeet(l.board_feet)} board feet, ${l.material ?? "material not set"}`, 9.5);
    y -= 13;
  }
  for (const s of list.totals.sheets) {
    page.text(48, y, `${s.sheets} sheet${s.sheets === 1 ? "" : "s"} 4x8, ${s.material ?? "sheet stock"}, ${formatSteps(s.steps, list.rounding, listStyle(list))} ${sizeUnit(list.rounding)} thick (${formatArea(s.area_m2, list.rounding)}${s.oversize ? "; a piece is larger than a sheet" : ""})`, 9.5);
    y -= 13;
  }
  if (!list.totals.lumber.length && !list.totals.sheets.length) {
    page.text(48, y, "No parts.", 9.5);
    y -= 13;
  }
  // The note, on every page's foot.
  doc.pages.forEach((p, i) => {
    const lines = wrap(scopeNote(list.rounding), 7.5, W - 80);
    lines.forEach((l, k) => p.text(40, 40 - k * 10 + (lines.length - 1) * 10, l, 7.5, { gray: 0.4 }));
    p.text(W - 40, 22, `Page ${i + 1} of ${doc.pages.length} · Sawhorse`, 7.5, { align: "right", gray: 0.4 });
  });
  return doc.bytes();
}

/** Avery 5160-style sheet geometry, points. */
export const LABEL_SHEET = { columns: 3, rows: 10, width: 2.625 * IN, height: 1 * IN, left: 0.1875 * IN, top: 0.5 * IN, pitchX: 2.75 * IN, pitchY: 1 * IN };

/** Where label `k` (0-based) goes: its page and its lower-left corner. */
export function labelSlot(k: number): { page: number; x: number; y: number } {
  const per = LABEL_SHEET.columns * LABEL_SHEET.rows;
  const i = k % per;
  const col = i % LABEL_SHEET.columns;
  const row = Math.floor(i / LABEL_SHEET.columns);
  return { page: Math.floor(k / per), x: LABEL_SHEET.left + col * LABEL_SHEET.pitchX, y: LETTER[1] - LABEL_SHEET.top - (row + 1) * LABEL_SHEET.pitchY };
}

/** One label per part: name and quantity, size, material, and the grain as words and an arrow. */
export function labelsPdf(list: CutList, title: string, opts: { outlines?: boolean } = {}): Uint8Array {
  const doc = new PdfDoc({ title: `Labels: ${title}`, creator: "Sawhorse" });
  const { width: w, height: h } = LABEL_SHEET;
  list.rows.forEach((row, k) => {
    const slot = labelSlot(k);
    while (doc.pages.length <= slot.page) doc.page();
    const p = doc.pages[slot.page];
    const { x, y } = slot;
    if (opts.outlines) p.rect(x, y, w, h, { stroke: 0.8, width: 0.25 });
    const pad = 9;
    p.text(x + pad, y + h - 17, fit(row.name, 11, w - 2 * pad - 34, true), 11, { bold: true });
    p.text(x + w - pad, y + h - 17, `×${row.qty}`, 11, { bold: true, align: "right" });
    p.text(x + pad, y + h - 31, fit(formatSize(row, list.rounding, listStyle(list)), 9, w - 2 * pad), 9);
    p.text(x + pad, y + h - 44, fit(`${materialText(row)}${row.sheet ? " (sheet)" : ""}`, 8, w - 2 * pad), 8, { gray: 0.25 });
    const grain = `Grain ${GRAIN_WORDS[row.grain]}`;
    p.text(x + pad, y + 10, fit(grain, 7.5, w - 2 * pad - 50), 7.5, { gray: 0.25 });
    if (row.grain !== "none") {
      // The arrow runs along the label for L, up the label for W (the board's width).
      if (row.grain === "length") p.arrow(x + w - pad - 40, y + 13, x + w - pad, y + 13, 0.8, 4);
      else p.arrow(x + w - pad - 6, y + 6, x + w - pad - 6, y + 26, 0.8, 4);
    }
  });
  if (!doc.pages.length) doc.page().text(LETTER[0] / 2, LETTER[1] / 2, "No parts to label.", 12, { align: "center" });
  return doc.bytes();
}
