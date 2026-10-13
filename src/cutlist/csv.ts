// The cut list as CSV (RFC 4180: comma separated, CRLF line ends, fields with commas,
// quotes or line breaks quoted, quotes doubled). One row per part, then a Total row.
//
// Columns: Part, Qty; T, W, L as shown (the shop's fractions, "11 1/4", or mm); T, W, L as
// decimal numbers in the size unit (inches or mm) for formulas; Material, Grain, Stock
// (Lumber / Sheet), Board feet, Area, Notes; and Summary, filled only on the Total row
// (the rough 4 x 8 sheet count); Board feet on (lane 15: the nominal board a lumber row's
// board feet are counted on, "4/4 (1 in rough)"). Empty cells are empty: no dash placeholders.
//
// Spreadsheet programs read "3/4" as a date and "11 1/4" as a number, so a shown size that
// holds a fraction is written as the text formula ="3/4" (Excel, LibreOffice Calc and
// Google Sheets all show it as the text 3/4). A plain CSV reader sees ="3/4";
// `csvCellText` unwraps it. The decimal columns are plain numbers.
import type { CutList } from "../kernel/types";
import { decimalSteps, formatBoardFeet, formatSteps, GRAIN_WORDS, isMetric, listStyle, nominalText, rowNotes, SQ_FT, sizeUnit } from "./format";

export const CSV_TYPE = { description: "Cut list (CSV)", mime: "text/csv", extensions: [".csv"] };

function field(v: string | number): string {
  const s = String(v);
  return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

/** A shown size as a spreadsheet keeps it: text when it holds a fraction. */
export function textCell(s: string): string {
  return s.includes("/") ? `="${s.replace(/"/g, '""')}"` : s;
}

/** A cell as the person reads it: `="3/4"` (a text formula) is 3/4. */
export function csvCellText(s: string): string {
  const m = /^="(.*)"$/s.exec(s);
  return m ? m[1].replace(/""/g, '"') : s;
}

export function csvHeader(list: CutList): string[] {
  const u = sizeUnit(list.rounding);
  const area = isMetric(list.rounding) ? "Area (m²)" : "Area (sq ft)";
  return ["Part", "Qty", "T", "W", "L", `T (${u})`, `W (${u})`, `L (${u})`, "Material", "Grain", "Stock", "Board feet", area, "Notes", "Summary", "Board feet on"];
}

export function cutListCsv(list: CutList): string {
  const r = list.rounding;
  const style = listStyle(list);
  const area = (m2: number) => (m2 ? (isMetric(r) ? m2 : m2 / SQ_FT).toFixed(2) : "");
  const lines: string[][] = [csvHeader(list)];
  for (const row of list.rows) {
    lines.push([
      row.name,
      String(row.qty),
      ...row.steps.map((s) => textCell(formatSteps(s, r, style))),
      ...row.steps.map((s) => decimalSteps(s, r)),
      row.material ?? "",
      GRAIN_WORDS[row.grain],
      row.uncounted ? "Not counted" : row.sheet ? "Sheet" : "Lumber",
      row.sheet || row.uncounted ? "" : formatBoardFeet(row.board_feet),
      area(row.area_m2),
      rowNotes(row).join("; "),
      "",
      nominalText(row),
    ]);
  }
  const t = list.totals;
  const totalArea = t.sheets.reduce((s, x) => s + x.area_m2, 0);
  const sheets = t.sheet_count ? `${t.sheet_count} sheet${t.sheet_count === 1 ? "" : "s"} 4x8` : "";
  lines.push(["Total", String(t.pieces), "", "", "", "", "", "", "", "", "", formatBoardFeet(t.board_feet), area(totalArea), "", sheets, ""]);
  return lines.map((l) => l.map(field).join(",")).join("\r\n") + "\r\n";
}

/** Parse CSV (RFC 4180) back into rows of fields: for tests and for anyone re-reading a saved list. */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cur = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        cur += '"';
        i++;
      } else if (c === '"') quoted = false;
      else cur += c;
      continue;
    }
    if (c === '"') quoted = true;
    else if (c === ",") {
      row.push(cur);
      cur = "";
    } else if (c === "\r" && text[i + 1] === "\n") {
      // CRLF: handled on the \n
    } else if (c === "\n") {
      row.push(cur);
      rows.push(row);
      row = [];
      cur = "";
    } else cur += c;
  }
  if (cur !== "" || row.length) {
    row.push(cur);
    rows.push(row);
  }
  return rows;
}

/** UTF-8 bytes with a byte-order mark, so spreadsheet programs read "×" and "²" right. */
export function csvBytes(list: CutList): Uint8Array {
  const body = new TextEncoder().encode(cutListCsv(list));
  const out = new Uint8Array(body.length + 3);
  out.set([0xef, 0xbb, 0xbf]);
  out.set(body, 3);
  return out;
}
