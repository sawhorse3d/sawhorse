// How the cut list reads (lane 7): finished sizes from the kernel's rounding steps, so the
// table, the CSV, the report, the PDF and the labels all say exactly the same thing.
import type { CutList, CutRow, Grain, Rounding } from "../kernel/types";
import type { UnitSystem } from "../lib/units";

export const ROUNDINGS: { id: Rounding; label: string; title: string }[] = [
  { id: "sixteenth", label: "1/16 in", title: "Finished sizes to the nearest 1/16 in" },
  { id: "thirty_second", label: "1/32 in", title: "Finished sizes to the nearest 1/32 in" },
  { id: "half_mm", label: "0.5 mm", title: "Finished sizes to the nearest 0.5 mm" },
];

/** The rounding a unit system starts with: 0.5 mm for metric, 1/16 in otherwise. */
export function defaultRounding(units: UnitSystem): Rounding {
  return units === "mm" || units === "cm" || units === "m" ? "half_mm" : "sixteenth";
}

export function isMetric(r: Rounding): boolean {
  return r === "half_mm";
}

function gcd(a: number, b: number): number {
  while (b) [a, b] = [b, a % b];
  return a;
}

/** A size in rounding steps as a woodworker writes it: "11 1/4", "3/4", "72"; "19.5" (mm). */
export function formatSteps(steps: number, r: Rounding): string {
  if (r === "half_mm") {
    const mm = steps / 2;
    return Number.isInteger(mm) ? String(mm) : mm.toFixed(1);
  }
  const den = r === "sixteenth" ? 16 : 32;
  const neg = steps < 0;
  const n = Math.abs(steps);
  const whole = Math.floor(n / den);
  const num = n - whole * den;
  let text: string;
  if (num === 0) text = String(whole);
  else {
    const g = gcd(num, den);
    const frac = `${num / g}/${den / g}`;
    text = whole === 0 ? frac : `${whole} ${frac}`;
  }
  return neg ? `-${text}` : text;
}

/** The unit the sizes are in: "in" or "mm". */
export function sizeUnit(r: Rounding): string {
  return isMetric(r) ? "mm" : "in";
}

/** "3/4 × 11 1/4 × 71 1/4 in". */
export function formatSize(row: CutRow, r: Rounding): string {
  return `${row.steps.map((s) => formatSteps(s, r)).join(" × ")} ${sizeUnit(r)}`;
}

export const SQ_FT = 0.09290304;

/** Area for the totals: square feet in inches, square metres in metric. */
export function formatArea(m2: number, r: Rounding): string {
  return isMetric(r) ? `${m2.toFixed(2)} m²` : `${(m2 / SQ_FT).toFixed(2)} sq ft`;
}

export function formatBoardFeet(bf: number): string {
  return bf.toFixed(2);
}

export const GRAIN_WORDS: Record<Grain, string> = { length: "along L", width: "along W (across)", none: "none" };

/** Plain-language notes for a row: shaped, handed, hidden, scaled. */
export function rowNotes(row: CutRow): string[] {
  const notes: string[] = [];
  if (row.shaped) notes.push("shaped (box it comes out of)");
  if (row.handed) notes.push(row.handed === row.total ? "handed (all mirrored)" : `handed: ${row.handed} of ${row.total} mirrored`);
  if (row.hidden) notes.push(row.hidden === row.total ? "hidden" : `${row.hidden} hidden`);
  if (row.scaled) notes.push("scaled copy");
  return notes;
}

/** The material column: the part's stock, its material's name, or a dash. */
export function materialText(row: CutRow): string {
  return row.material ?? "—";
}

/** What the summary tiles say: board feet (with the species when there is one), and sheets. */
export function summary(list: CutList): { boardFeet: string; boardFeetLabel: string; sheets: string; sheetsLabel: string } {
  const t = list.totals;
  const named = t.lumber.filter((l) => l.board_feet > 0);
  const species = named.length === 1 ? (named[0].material ? `, ${named[0].material}` : "") : named.length > 1 ? `, ${named.length} kinds` : "";
  const r = list.rounding;
  const sheetKinds = t.sheets.filter((s) => s.sheets > 0);
  const sheetsLabel =
    sheetKinds.length === 0
      ? "sheets 4×8"
      : sheetKinds.length === 1
        ? `sheet${sheetKinds[0].sheets === 1 ? "" : "s"} 4×8${sheetKinds[0].material ? ` ${sheetKinds[0].material}` : ""}, ${formatSteps(sheetKinds[0].steps, r)}${isMetric(r) ? " mm" : ""}`
        : `sheets 4×8, ${sheetKinds.length} kinds`;
  return { boardFeet: formatBoardFeet(t.board_feet), boardFeetLabel: `board feet${species}`, sheets: String(t.sheet_count), sheetsLabel };
}

/** The footer every output carries. */
export function scopeNote(r: Rounding): string {
  const step = r === "sixteenth" ? "1/16 in" : r === "thirty_second" ? "1/32 in" : "0.5 mm";
  return `Finished sizes, rounded to the nearest ${step}. Grain runs along L unless noted. Kerf, edge banding and nesting (cutting diagrams) are not included yet; sheet counts are a rough area estimate.`;
}
