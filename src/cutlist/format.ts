// How the cut list reads (lane 7): finished sizes from the kernel's rounding steps, so the
// table, the CSV, the report, the PDF and the labels all say exactly the same thing.
import type { CutList, CutRow, Grain, Rounding } from "../kernel/types";
import type { UnitSystem } from "../lib/units";

/** What the Round to menu offers: follow the display units (the default), or a fixed step. */
export type RoundingChoice = Rounding | "follow";

export const ROUNDINGS: { id: RoundingChoice; label: string; title: string }[] = [
  { id: "follow", label: "Follow units", title: "1/16 in for fractional, decimal and ft-in units; 0.5 mm for metric" },
  { id: "sixteenth", label: "1/16 in", title: "Finished sizes to the nearest 1/16 in" },
  { id: "thirty_second", label: "1/32 in", title: "Finished sizes to the nearest 1/32 in" },
  { id: "half_mm", label: "0.5 mm", title: "Finished sizes to the nearest 0.5 mm" },
];

export function isMetricUnits(units: UnitSystem): boolean {
  return units === "mm" || units === "cm" || units === "m";
}

/** The rounding "Follow units" means: 0.5 mm for metric, 1/16 in otherwise (fractional,
 * decimal and ft-in inches). */
export function followRounding(units: UnitSystem): Rounding {
  return isMetricUnits(units) ? "half_mm" : "sixteenth";
}

/** @deprecated the old name of {@link followRounding}. */
export const defaultRounding = followRounding;

/** The rounding a choice comes to under the current units. */
export function resolveRounding(choice: RoundingChoice, units: UnitSystem): Rounding {
  return choice === "follow" ? followRounding(units) : choice;
}

export function isRoundingChoice(v: unknown): v is RoundingChoice {
  return v === "follow" || v === "sixteenth" || v === "thirty_second" || v === "half_mm";
}

/** How sizes are written: shop fractions ("11 1/4"), decimal inches ("11.25") or mm. */
export type SizeStyle = "fraction" | "decimal" | "mm";

/** Metric roundings write mm; inch roundings write decimal inches when the display units
 * are decimal inches or ft-in, and fractions otherwise. */
export function sizeStyle(r: Rounding, units: UnitSystem): SizeStyle {
  if (r === "half_mm") return "mm";
  return units === "in" || units === "ft-in" ? "decimal" : "fraction";
}

/** The style a list is written in: what the app set on it, or the rounding's natural one. */
export function listStyle(list: CutList): SizeStyle {
  return list.rounding === "half_mm" ? "mm" : (list.style ?? "fraction");
}

export function isMetric(r: Rounding): boolean {
  return r === "half_mm";
}

function gcd(a: number, b: number): number {
  while (b) [a, b] = [b, a % b];
  return a;
}

/** A size in rounding steps as a woodworker writes it: "11 1/4", "3/4", "72"; "19.5" (mm);
 * "11.25" in the decimal style. */
export function formatSteps(steps: number, r: Rounding, style: SizeStyle = "fraction"): string {
  if (style === "decimal" && r !== "half_mm") return decimalSteps(steps, r);
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

/** A size in rounding steps as a plain decimal number in the size unit: "0.75", "11.25",
 * "0.0625" (inches; exact, since sixteenths and thirty-seconds end within five places);
 * "19.5" (mm). For spreadsheets and anyone who wants numbers, not fractions. */
export function decimalSteps(steps: number, r: Rounding): string {
  const v = r === "half_mm" ? steps / 2 : steps / (r === "sixteenth" ? 16 : 32);
  return String(Number(v.toFixed(5)));
}

/** The unit the sizes are in: "in" or "mm". */
export function sizeUnit(r: Rounding): string {
  return isMetric(r) ? "mm" : "in";
}

/** "3/4 × 11 1/4 × 71 1/4 in" ("0.75 × 11.25 × 71.25 in" in the decimal style). */
export function formatSize(row: CutRow, r: Rounding, style: SizeStyle = "fraction"): string {
  return `${row.steps.map((s) => formatSteps(s, r, style)).join(" × ")} ${sizeUnit(r)}`;
}

export const SQ_FT = 0.09290304;

/** Area for the totals: square feet in inches, square metres in metric. */
export function formatArea(m2: number, r: Rounding): string {
  return isMetric(r) ? `${m2.toFixed(2)} m²` : `${(m2 / SQ_FT).toFixed(2)} sq ft`;
}

export function formatBoardFeet(bf: number): string {
  return bf.toFixed(2);
}

export const GRAIN_WORDS: Record<Grain, string> = { length: "along L", width: "across W", none: "none" };

/** What a part that is not a watertight solid says, everywhere it is written (lane 15). */
export const NOT_SOLID_NOTE = "not solid (sizes from bounds)";
/** What a part dropped from the totals says (lane 15). */
export const LEFT_OUT_NOTE = "left out of the totals";

/** Plain-language notes for a row, the same in the pane, the CSV, the print view, the PDF
 * and the labels: left out of the totals, not solid (sized from its bounds), a grain set
 * on the part ("grain across W", "no grain"; the default, along L, set explicitly too),
 * shaped, handed, hidden, scaled. */
export function rowNotes(row: CutRow): string[] {
  const notes: string[] = [];
  if (row.included === false) notes.push(LEFT_OUT_NOTE);
  if (row.not_solid) notes.push(NOT_SOLID_NOTE);
  if (row.grain_set) notes.push(row.grain === "none" ? "no grain" : `grain ${GRAIN_WORDS[row.grain]}`);
  if (row.shaped) notes.push("shaped");
  if (row.handed) notes.push(row.handed === row.total ? "handed" : `${row.handed} of ${row.total} handed`);
  if (row.hidden) notes.push(row.hidden === row.total ? "hidden" : `${row.hidden} hidden`);
  if (row.scaled) notes.push("scaled");
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
  const style = listStyle(list);
  const sheetKinds = t.sheets.filter((s) => s.sheets > 0);
  const sheetsLabel =
    sheetKinds.length === 0
      ? "sheets 4×8"
      : sheetKinds.length === 1
        ? `sheet${sheetKinds[0].sheets === 1 ? "" : "s"} 4×8${sheetKinds[0].material ? ` ${sheetKinds[0].material}` : ""}, ${formatSteps(sheetKinds[0].steps, r, style)}${isMetric(r) ? " mm" : ""}`
        : `sheets 4×8, ${sheetKinds.length} kinds`;
  return { boardFeet: formatBoardFeet(t.board_feet), boardFeetLabel: `board feet${species}`, sheets: String(t.sheet_count), sheetsLabel };
}

/** Board feet for a lumber row, area for a sheet row, with its unit: "8.35 bd ft",
 * "17.07 sq ft", "1.59 m²". */
export function amountText(row: CutRow, r: Rounding): string {
  return row.sheet ? formatArea(row.area_m2, r) : `${formatBoardFeet(row.board_feet)} bd ft`;
}

/** The report's columns, the same in the print view and the PDF. */
export const REPORT_COLUMNS = ["Part", "Qty", "T", "W", "L", "Material", "Grain", "Bd ft / area", "Notes"] as const;

/** One report row's cells, in {@link REPORT_COLUMNS} order: the print view and the PDF
 * both write exactly these. */
export function reportCells(row: CutRow, list: CutList): string[] {
  const r = list.rounding;
  const style = listStyle(list);
  return [
    row.name,
    String(row.qty),
    ...row.steps.map((s) => formatSteps(s, r, style)),
    `${materialText(row)}${row.sheet ? " (sheet)" : ""}`,
    GRAIN_WORDS[row.grain],
    amountText(row, r),
    rowNotes(row).join("; "),
  ];
}

/** The footer every output carries. */
export function scopeNote(r: Rounding): string {
  const step = r === "sixteenth" ? "1/16 in" : r === "thirty_second" ? "1/32 in" : "0.5 mm";
  return `Finished sizes, rounded to the nearest ${step}. Grain runs along L unless noted. ${BOARD_FEET_NOTE} Edge banding is not included; the sheet count here is a rough area estimate (Nesting lays the parts out on real sheets and boards, kerf included).`;
}

/** Lane 15: board feet are bought on the rough thickness, not the finished one. */
export const BOARD_FEET_NOTE = "Board feet are counted on the rough (nominal) thickness each part is cut from: a 3/4 in part from 4/4 lumber counts as 1 in.";

/** Lane 15: the nominal board a lumber row's board feet are counted on: "4/4 (1 in rough)",
 * or "" for sheet goods and parts thicker than any board in the library. */
export function nominalText(row: CutRow): string {
  if (row.sheet || !row.nominal) return "";
  const inches = Math.round(((row.nominal_thickness ?? 0) / 0.0254) * 10000) / 10000;
  return `${row.nominal} (${String(inches)} in rough)`;
}

/** Lane 15: what the list says about geometry that is not a solid: "2 parts not solid
 * (sizes from bounds) · 312 faces outside parts · 1 part left out". Empty when all is solid. */
export function geometryNote(list: CutList): string {
  const t = list.totals;
  const bits: string[] = [];
  const n = (k: number, one: string, many: string) => `${k} ${k === 1 ? one : many}`;
  if (t.not_solid) bits.push(`${n(t.not_solid, "part", "parts")} not solid (sizes from bounds)`);
  if (t.loose_faces) bits.push(`${n(t.loose_faces, "face", "faces")} outside parts`);
  if (t.excluded) bits.push(`${n(t.excluded, "part", "parts")} left out of the totals`);
  return bits.join(" · ");
}
