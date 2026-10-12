// The cut list as CSV (RFC 4180: comma separated, CRLF line ends, fields with commas,
// quotes or line breaks quoted, quotes doubled). One row per part, then a Total row.
import type { CutList } from "../kernel/types";
import { formatBoardFeet, formatSteps, GRAIN_WORDS, isMetric, materialText, rowNotes, SQ_FT, sizeUnit } from "./format";

export const CSV_TYPE = { description: "Cut list (CSV)", mime: "text/csv", extensions: [".csv"] };

function field(v: string | number): string {
  const s = String(v);
  return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

export function csvHeader(list: CutList): string[] {
  const u = sizeUnit(list.rounding);
  const area = isMetric(list.rounding) ? "Area (m²)" : "Area (sq ft)";
  return ["Part", "Qty", `T (${u})`, `W (${u})`, `L (${u})`, "Material", "Grain", "Stock", "Board feet", area, "Notes"];
}

export function cutListCsv(list: CutList): string {
  const r = list.rounding;
  const area = (m2: number) => (m2 ? (isMetric(r) ? m2 : m2 / SQ_FT).toFixed(2) : "");
  const lines: string[][] = [csvHeader(list)];
  for (const row of list.rows) {
    lines.push([
      row.name,
      String(row.qty),
      ...row.steps.map((s) => formatSteps(s, r)),
      materialText(row),
      GRAIN_WORDS[row.grain],
      row.sheet ? "Sheet" : "Lumber",
      row.sheet ? "" : formatBoardFeet(row.board_feet),
      area(row.area_m2),
      rowNotes(row).join("; "),
    ]);
  }
  const totalArea = list.totals.sheets.reduce((s, x) => s + x.area_m2, 0);
  lines.push(["Total", String(list.totals.pieces), "", "", "", "", "", `${list.totals.sheet_count} sheet${list.totals.sheet_count === 1 ? "" : "s"} 4x8`, formatBoardFeet(list.totals.board_feet), area(totalArea), ""]);
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
