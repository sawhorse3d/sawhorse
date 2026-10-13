// Paper, scales and the fields of the Sheets pane (lane 12): the TypeScript side of
// crates/kernel/src/paper.rs and sheets.rs that the UI needs before it asks the kernel
// anything (the kernel stays the judge: every edit is an op it validates).
import type { CutColumn, ItemKind, LengthFormat, PaperSize, PaperSizeKind, PaperUnits, Scale, SheetSetup, StdView, ViewportSource } from "../kernel/sheetTypes";
import { parseNumber } from "../lib/units";

/** What the pane needs of a scene. */
export interface SceneLike {
  name: string;
  parallel?: boolean;
}

export const IN = 0.0254;
export const MM = 0.001;

export const PAPERS: { kind: PaperSizeKind; label: string; size: [number, number] }[] = [
  { kind: "letter", label: "Letter (8.5 x 11 in)", size: [8.5 * IN, 11 * IN] },
  { kind: "legal", label: "Legal (8.5 x 14 in)", size: [8.5 * IN, 14 * IN] },
  { kind: "tabloid", label: "Tabloid (11 x 17 in)", size: [11 * IN, 17 * IN] },
  { kind: "a4", label: "A4 (210 x 297 mm)", size: [210 * MM, 297 * MM] },
  { kind: "a3", label: "A3 (297 x 420 mm)", size: [297 * MM, 420 * MM] },
  { kind: "a2", label: "A2 (420 x 594 mm)", size: [420 * MM, 594 * MM] },
  { kind: "custom", label: "Custom", size: [0, 0] },
];

/** Portrait width and height of a paper size, metres. */
export function portrait(p: PaperSize): [number, number] {
  if (p.kind === "custom") return [p.width, p.height];
  return PAPERS.find((x) => x.kind === p.kind)!.size;
}

/** Width and height as laid out. */
export function paperSize(s: SheetSetup): [number, number] {
  const [w, h] = portrait(s.paper);
  return s.landscape ? [h, w] : [w, h];
}

/** A new sheet's setup: the kernel's default (Letter landscape, inches, half-inch margins,
 * a 1/8 in grid, fractions to 1/16 in, the title block on), or A4 landscape in millimetres
 * for a metric model. */
export function newSetup(name: string, metric: boolean): SheetSetup {
  if (metric) {
    return { name, paper: { kind: "a4" }, landscape: true, margins: [10 * MM, 10 * MM, 10 * MM, 10 * MM], units: "millimetres", grid: 2.5 * MM, format: { kind: "millimetres", places: 0 }, title: { show: true } };
  }
  return { name, paper: { kind: "letter" }, landscape: true, margins: [0.5 * IN, 0.5 * IN, 0.5 * IN, 0.5 * IN], units: "inches", grid: IN / 8, format: { kind: "fraction", den: 16 }, title: { show: true } };
}

// ---- scales -----------------------------------------------------------------------------

/** The kernel's standard scales (Scale::standard), largest first within each family. */
export const STANDARD_SCALES: Scale[] = [
  ...[1, 2, 4, 8, 12, 16, 24, 48].map((n): Scale => ({ kind: "ratio", paper: 1, model: n })),
  ...[3, 1.5, 1, 0.75, 0.5, 0.25].map((i): Scale => ({ kind: "architect", inches_per_foot: i })),
];

/** Paper metres per model metre (null: not to scale). */
export function scaleFactor(s: Scale): number | null {
  if (s.kind === "ratio") return s.paper / s.model;
  if (s.kind === "architect") return s.inches_per_foot / 12;
  return null;
}

const plain = (x: number) => {
  const s = x.toFixed(4).replace(/0+$/, "").replace(/\.$/, "");
  return s === "" || s === "-0" ? "0" : s;
};

function fraction(x: number, den: number): string | null {
  const n = Math.round(x * den);
  if (n === 0 || Math.abs(x * den - n) > 1e-9) return null;
  let a = n;
  let b = den;
  while (a % 2 === 0 && b % 2 === 0) {
    a /= 2;
    b /= 2;
  }
  return `${a}/${b}`;
}

/** "1:8", "3\" = 1'-0\"", "1-1/2\" = 1'-0\"", "NTS" (as the kernel writes them). */
export function scaleLabel(s: Scale): string {
  if (s.kind === "ratio") return `${plain(s.paper)}:${plain(s.model)}`;
  if (s.kind === "nts") return "NTS";
  const whole = Math.trunc(s.inches_per_foot);
  const frac = fraction(s.inches_per_foot - whole, 64);
  const text = whole === 0 && frac ? frac : frac ? `${whole}-${frac}` : `${whole}`;
  return `${text}" = 1'-0"`;
}

export const scaleKey = (s: Scale) => scaleLabel(s);

/** The largest standard scale whose factor is at most `factor` (what a fitted viewport
 * shows at to scale), in the sheet's family (architectural scales for inch sheets showing
 * buildings-size things is rare in a shop: ratios first, then the architect scales). */
export function fittingScale(factor: number): Scale {
  let best: Scale | null = null;
  let bestF = 0;
  for (const s of STANDARD_SCALES) {
    const f = scaleFactor(s)!;
    if (f <= factor * (1 + 1e-9) && f > bestF) {
      best = s;
      bestF = f;
    }
  }
  return best ?? { kind: "ratio", paper: 1, model: Math.max(1, Math.ceil(1 / Math.max(factor, 1e-4))) };
}

/** Whether a viewport of `source` may be to scale (square views and parallel cameras). */
export function canBeToScale(source: ViewportSource, scenes: SceneLike[]): boolean {
  if (source.from === "standard") return source.view !== "iso";
  if (source.from === "camera") return !!source.camera.parallel;
  return !!scenes.find((s) => s.name === source.name)?.parallel;
}

export const STD_VIEWS: { view: StdView; label: string }[] = [
  { view: "front", label: "Front" },
  { view: "back", label: "Back" },
  { view: "left", label: "Left" },
  { view: "right", label: "Right" },
  { view: "top", label: "Top (plan)" },
  { view: "bottom", label: "Bottom" },
  { view: "iso", label: "Isometric" },
];

export function sourceLabel(s: ViewportSource): string {
  if (s.from === "scene") return `Scene ${s.name}`;
  if (s.from === "standard") return STD_VIEWS.find((v) => v.view === s.view)?.label ?? s.view;
  return "Saved camera";
}

/** A caption for a new viewport: "FRONT ELEVATION 1:8". */
export function captionFor(source: ViewportSource, scale: Scale): string {
  const what = source.from === "scene" ? source.name : source.from === "standard" ? ({ front: "Front elevation", back: "Back elevation", left: "Left elevation", right: "Right elevation", top: "Plan", bottom: "Bottom view", iso: "Isometric" } as const)[source.view] : "View";
  return `${what.toUpperCase()} ${scaleLabel(scale)}`;
}

// ---- cut-list columns -------------------------------------------------------------------

export const CUT_COLUMNS: { id: CutColumn; label: string }[] = [
  { id: "part", label: "Part" },
  { id: "qty", label: "Qty" },
  { id: "thickness", label: "Thickness" },
  { id: "width", label: "Width" },
  { id: "length", label: "Length" },
  { id: "size", label: "T x W x L" },
  { id: "material", label: "Material" },
  { id: "grain", label: "Grain" },
  { id: "amount", label: "Board feet / area" },
  { id: "notes", label: "Notes" },
];
export const DEFAULT_COLUMNS: CutColumn[] = ["part", "qty", "thickness", "width", "length", "material", "amount", "notes"];

// ---- lengths on paper -------------------------------------------------------------------

/** A paper length for a field: inches to 1/1000 or millimetres to 1/10. */
export function paperText(m: number, units: PaperUnits): string {
  return units === "millimetres" ? `${plain(Math.round((m / MM) * 10) / 10)} mm` : `${plain(Math.round((m / IN) * 1000) / 1000)}"`;
}

/** Read a typed paper length ("1/2", "0.5in", "12 mm", "1 1/4\""), bare numbers in the
 * sheet's units. Null when it cannot be read. */
export function parsePaper(text: string, units: PaperUnits): number | null {
  const s = text.trim().toLowerCase().replace(/[″”]/g, '"');
  if (!s) return null;
  const m = /^(.*?)\s*(mm|cm|in|")$/.exec(s);
  const n = parseNumber(m ? m[1] : s);
  if (n === null || !Number.isFinite(n)) return null;
  const unit = m ? m[2] : units === "millimetres" ? "mm" : "in";
  return n * (unit === "mm" ? MM : unit === "cm" ? 10 * MM : IN);
}

export const FORMATS: { label: string; format: LengthFormat }[] = [
  { label: 'Fractions to 1/16"', format: { kind: "fraction", den: 16 } },
  { label: 'Fractions to 1/32"', format: { kind: "fraction", den: 32 } },
  { label: 'Fractions to 1/8"', format: { kind: "fraction", den: 8 } },
  { label: "Decimal inches (0.00)", format: { kind: "decimal", places: 2 } },
  { label: "Decimal inches (0.000)", format: { kind: "decimal", places: 3 } },
  { label: "Feet and inches", format: { kind: "feet_inches", den: 16 } },
  { label: "Millimetres", format: { kind: "millimetres", places: 0 } },
  { label: "Millimetres (0.0)", format: { kind: "millimetres", places: 1 } },
];

export const formatKey = (f: LengthFormat) => JSON.stringify(f);

// ---- items ------------------------------------------------------------------------------

/** An item's rectangle, for those that have one. */
export function itemRect(k: ItemKind): [number, number, number, number] | null {
  return k.kind === "viewport" || k.kind === "table" || k.kind === "image" || k.kind === "rect" ? k.rect : null;
}

/** The item with its rectangle replaced. */
export function withRect(k: ItemKind, r: [number, number, number, number]): ItemKind {
  return itemRect(k) ? ({ ...k, rect: r } as ItemKind) : k;
}

/** A normalised rectangle from two corners. */
export function rectFrom(a: [number, number], b: [number, number]): [number, number, number, number] {
  return [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.abs(b[0] - a[0]), Math.abs(b[1] - a[1])];
}

export const ITEM_NAMES: Record<ItemKind["kind"], string> = {
  viewport: "Viewport",
  dimension: "Dimension",
  label: "Label",
  table: "Cut-list table",
  image: "Image",
  line: "Line",
  rect: "Rectangle",
};

/** The date on the person's own calendar, as a title block writes it ("2026-10-12"); never
 *  the UTC date, which is already tomorrow on a Pacific evening. */
export function localDate(d: Date = new Date()): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
}
