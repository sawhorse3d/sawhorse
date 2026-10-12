// The cut list as paper (lane 7): a report PDF (the table, the totals and the notes, as many
// Letter pages as it takes) and a label sheet PDF (one label per part, laid out for Avery
// 5160-style sheets: 30 labels, 3 across and 10 down, 2 5/8 x 1 in).
import type { CutList, CutRow } from "../kernel/types";
import { fit, LETTER, PdfDoc, type PdfPage } from "./pdf";
import { formatArea, formatBoardFeet, formatSize, formatSteps, GRAIN_WORDS, listStyle, materialText, rowNotes, scopeNote, sizeUnit, summary } from "./format";

const IN = 72;

/** The report's columns: label, x (left edge), width, right-aligned. */
const COLUMNS: [string, number, number, boolean][] = [
  ["Part", 40, 120, false],
  ["Qty", 160, 28, true],
  ["T", 188, 46, true],
  ["W", 234, 50, true],
  ["L", 284, 56, true],
  ["Material", 346, 94, false],
  ["Grain", 440, 34, false],
  ["Bd ft", 474, 42, true],
  ["Notes", 520, 52, false],
];

function cells(row: CutRow, list: CutList): string[] {
  const r = list.rounding;
  return [
    row.name,
    String(row.qty),
    ...row.steps.map((s) => formatSteps(s, r, listStyle(list))),
    `${materialText(row)}${row.sheet ? " (sheet)" : ""}`,
    row.grain === "length" ? "L" : row.grain === "width" ? "W" : "-",
    row.sheet ? "" : formatBoardFeet(row.board_feet),
    rowNotes(row).join("; "),
  ];
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
    for (const [label, x, w, right] of COLUMNS) p.text(right ? x + w - 4 : x, y, label.toUpperCase(), 8, { bold: true, align: right ? "right" : "left", gray: 0.35 });
    y -= 6;
    p.line(40, y, W - 40, y, 0.75);
    y -= rowH - 4;
  };
  header(page, true);
  for (const row of list.rows) {
    if (y < bottom + rowH) {
      page = doc.page();
      header(page, false);
    }
    cells(row, list).forEach((c, k) => {
      const [, x, w, right] = COLUMNS[k];
      const size = k === 8 ? 7.5 : 9.5;
      const t = fit(c, size, w - 6, k === 0);
      page.text(right ? x + w - 4 : x, y, t, size, { bold: k === 0, align: right ? "right" : "left" });
    });
    page.line(40, y - 5, W - 40, y - 5, 0.25, 0.7);
    y -= rowH;
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
    const words = scopeNote(list.rounding).split(" ");
    const lines: string[] = [];
    let cur = "";
    for (const w of words) {
      const next = cur ? `${cur} ${w}` : w;
      if (fit(next, 7.5, W - 80) !== next) {
        lines.push(cur);
        cur = w;
      } else cur = next;
    }
    if (cur) lines.push(cur);
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
