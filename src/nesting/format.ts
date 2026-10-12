// How the nesting reads (lane 9): lengths in the cut list's rounding, the cut sequence in
// shop words, why a part is not on any stock, and the totals tiles. The pane, the SVG and
// DXF files and the PDF booklet all say exactly the same thing.
import type { NestAlgorithm, NestCut, Nesting, Rounding, StockDef, StockPiece, Unplaced } from "../kernel/types";
import { formatArea, formatSteps, sizeUnit as unitOf, type SizeStyle } from "../cutlist/format";

/** How lengths are written: a rounding (fractions for inches, mm for metric), or a rounding
 * and the cut list's size style (decimal inches when the model works in decimal inches). */
export type Units = Rounding | { r: Rounding; style: SizeStyle };

export function roundingOf(u: Units): Rounding {
  return typeof u === "string" ? u : u.r;
}

function styleOf(u: Units): SizeStyle {
  return typeof u === "string" ? (u === "half_mm" ? "mm" : "fraction") : u.style;
}

/** "in" or "mm". */
export function sizeUnit(u: Units): string {
  return unitOf(roundingOf(u));
}

export const M_PER_IN = 0.0254;

/** One rounding step, metres. */
export function stepOf(r: Rounding): number {
  return r === "half_mm" ? 0.0005 : r === "thirty_second" ? M_PER_IN / 32 : M_PER_IN / 16;
}

/** A length as the cut list writes it: "11 1/4", "3/4", "285.5" (no unit). */
export function len(m: number, u: Units): string {
  const r = roundingOf(u);
  return formatSteps(Math.round(m / stepOf(r)), r, styleOf(u));
}

/** The same with its unit: "11 1/4 in", "285.5 mm". */
export function lenU(m: number, r: Units): string {
  return `${len(m, r)} ${sizeUnit(r)}`;
}

/** "11 1/4 × 34 1/2" (width × length of a placed part, or of a stock). */
export function dims(a: number, b: number, r: Units): string {
  return `${len(a, r)} × ${len(b, r)}`;
}

/** A fraction 0..1 as a percentage with one decimal: "62.4%". */
export function pct(x: number): string {
  return `${(Math.round(x * 1000) / 10).toFixed(1)}%`;
}

export function area(m2: number, u: Units): string {
  return formatArea(m2, roundingOf(u));
}

export const ALGORITHMS: { id: NestAlgorithm; label: string; title: string }[] = [
  { id: "guillotine", label: "Table saw", title: "Guillotine: every sheet comes apart in straight cuts from edge to edge" },
  { id: "maxrects", label: "Tightest (CNC)", title: "Maximal rectangles: packs tighter; some layouts need a track saw or a CNC" },
];

/** "Sheet", "Board". */
export function kindWord(p: { kind: "sheet" | "board" }): string {
  return p.kind === "sheet" ? "Sheet" : "Board";
}

/** "Sheet 1 of 2". */
export function pieceNumber(p: StockPiece): string {
  return `${kindWord(p)} ${p.seq} of ${p.count}`;
}

/** "Sheet 1 of 2 · 4 x 8 sheet, 3/4 · Oak". */
export function pieceTitle(p: StockPiece): string {
  return `${pieceNumber(p)} · ${p.stock}${p.material ? ` · ${p.material}` : ""}`;
}

/** "62.4% used · 12.71 sq ft waste". */
export function pieceYield(p: StockPiece, r: Units): string {
  return `${pct(p.efficiency)} used · ${area(p.waste_area, r)} waste`;
}

/** A stock's size as a shop writes it: "48 × 96 × 3/4 in" (width × length × thickness). */
export function stockSize(s: Pick<StockDef, "width" | "length" | "thickness">, r: Units): string {
  return `${len(s.width, r)} × ${len(s.length, r)} × ${len(s.thickness, r)} ${sizeUnit(r)}`;
}

/** One step of the cut sequence: "Rip to 11 1/4 in (96 in long)". */
export function cutText(c: NestCut, r: Units): string {
  const run = len(c.to - c.from, r);
  if (c.trim) return `Trim ${c.dir === "rip" ? "a long edge" : "an end"} (${lenU(c.size, r)} off)`;
  return c.dir === "rip" ? `Rip to ${lenU(c.size, r)} (cut ${run} long)` : `Crosscut to ${lenU(c.size, r)} (cut ${run} across)`;
}

/** Why a part is not on any stock, in plain words. */
export function reasonText(u: Unplaced): string {
  switch (u.reason) {
    case "no_stock":
      return `no ${u.sheet ? "sheet" : "board"} in the stock library is ${u.sheet ? "this thickness" : "this thick"}; add one (Stock…)`;
    case "thickness":
      return `${u.stock} is not its thickness`;
    case "too_large":
      return `larger than ${u.stock ?? "any stock"}; glue it up or add larger stock`;
    case "grain":
      return `fits ${u.stock} only with its grain across; turn its grain or pick other stock`;
  }
}

export interface NestSummary {
  sheets: string;
  sheetsLabel: string;
  boards: string;
  boardsLabel: string;
  yieldText: string;
  yieldLabel: string;
}

/** What the three tiles say: sheets by stock, boards by nominal size, overall yield. */
export function nestSummary(n: Nesting, r: Units): NestSummary {
  const t = n.totals;
  const sheetGroups = t.groups.filter((g) => g.kind === "sheet");
  const stocks = [...new Set(sheetGroups.map((g) => g.stock))];
  const sheetsLabel = stocks.length === 0 ? "sheets" : stocks.length === 1 ? `sheet${t.sheets === 1 ? "" : "s"}: ${stocks[0]}` : `sheets, ${stocks.length} kinds`;
  const nominal = t.by_nominal.map((b) => `${b.count} × ${b.nominal}`).join(", ");
  const bf = t.by_nominal.reduce((s, b) => s + b.board_feet, 0);
  const boardsLabel = t.boards === 0 ? "boards" : `board${t.boards === 1 ? "" : "s"}: ${nominal}, ${bf.toFixed(1)} bd ft`;
  return {
    sheets: String(t.sheets),
    sheetsLabel,
    boards: String(t.boards),
    boardsLabel,
    yieldText: t.stock_area > 0 ? pct(t.efficiency) : "—",
    yieldLabel: `yield · ${t.placed} of ${t.parts_in} part${t.parts_in === 1 ? "" : "s"} placed${t.stock_area > 0 ? ` · ${area(t.stock_area - t.parts_area, r)} waste` : ""}`,
  };
}

/** The settings in one line: "Kerf 1/8 in · trim 0 · table saw". */
export function settingsText(n: Nesting, r: Units): string {
  const alg = n.algorithm === "guillotine" ? "table saw" : "tightest layout (CNC)";
  return `Kerf ${lenU(n.settings.kerf, r)} · trim ${n.settings.trim > 0 ? `${lenU(n.settings.trim, r)} per edge` : "none"} · ${alg}`;
}

/** The footer every nesting output carries. */
export function nestNote(n: Nesting, r: Units): string {
  return `${settingsText(n, r)}. Parts at their exact finished sizes; grain runs along each sheet's and board's length. Sizes in ${sizeUnit(r)}.`;
}
