// Length parsing and formatting for the Value Control Box.
//
// The kernel works in f64 metres (ADR-0001); units convert only at the edges.
// Fractional inches are first-class: "3/4", "1 1/2", "1-1/2", 5'6", 5' 6 1/2".
// A typed unit suffix always wins over the document's display units.

export type UnitSystem = "frac-in" | "in" | "ft-in" | "mm" | "cm" | "m";

export interface UnitInfo {
  id: UnitSystem;
  /** Long name for the viewport HUD ("Units: fractional in"). */
  label: string;
  /** Short label beside the VCB input. */
  short: string;
}

export const UNIT_SYSTEMS: UnitInfo[] = [
  { id: "frac-in", label: "fractional in", short: "in" },
  { id: "in", label: "decimal in", short: "in" },
  { id: "ft-in", label: "ft-in", short: "ft-in" },
  { id: "mm", label: "mm", short: "mm" },
  { id: "cm", label: "cm", short: "cm" },
  { id: "m", label: "m", short: "m" },
];

export const M_PER_IN = 0.0254;
export const M_PER_FT = 0.3048;
/** Largest magnitude accepted from the VCB, in metres (matches kernel tolerance::MAX_COORD). */
export const MAX_LENGTH_M = 10_000;

export function unitInfo(id: UnitSystem): UnitInfo {
  const u = UNIT_SYSTEMS.find((x) => x.id === id);
  if (!u) throw new Error(`unknown unit system ${id}`);
  return u;
}

export function nextUnitSystem(id: UnitSystem): UnitSystem {
  const i = UNIT_SYSTEMS.findIndex((u) => u.id === id);
  return UNIT_SYSTEMS[(i + 1) % UNIT_SYSTEMS.length].id;
}

export type ParseResult = { ok: true; metres: number } | { ok: false; error: string };

const MIXED = /^(\d+)(?:\s+|\s*-\s*)(\d+)\s*\/\s*(\d+)$/;
const FRACTION = /^(\d+)\s*\/\s*(\d+)$/;
const DECIMAL = /^(\d+\.?\d*|\.\d+)$/;

/** Parse an unsigned number: decimal, simple fraction or mixed number. */
export function parseNumber(text: string): number | null {
  const s = text.trim();
  let m = MIXED.exec(s);
  if (m) {
    const den = Number(m[3]);
    if (den === 0) return null;
    return Number(m[1]) + Number(m[2]) / den;
  }
  m = FRACTION.exec(s);
  if (m) {
    const den = Number(m[2]);
    if (den === 0) return null;
    return Number(m[1]) / den;
  }
  if (DECIMAL.test(s)) return Number(s);
  return null;
}

const SUFFIX: Record<string, number> = { mm: 0.001, cm: 0.01, m: 1, in: M_PER_IN, '"': M_PER_IN, ft: M_PER_FT };

function defaultScale(system: UnitSystem): number {
  switch (system) {
    case "mm":
      return 0.001;
    case "cm":
      return 0.01;
    case "m":
      return 1;
    default:
      return M_PER_IN; // in, frac-in and ft-in all read a bare number as inches
  }
}

function normalise(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[″”]/g, '"')
    .replace(/[′’]/g, "'")
    .replace(/\s+/g, " ");
}

/** Parse one typed length into metres. */
export function parseLength(text: string, system: UnitSystem): ParseResult {
  let s = normalise(text);
  if (s === "") return { ok: false, error: "Type a length." };
  let sign = 1;
  if (s[0] === "-" || s[0] === "+") {
    if (s[0] === "-") sign = -1;
    s = s.slice(1).trim();
  }
  let metres: number | null = null;
  const tick = s.indexOf("'");
  if (tick >= 0) {
    // feet-inches: 5'  5'6"  5' 6 1/2"  5'-6"
    const feet = parseNumber(s.slice(0, tick));
    let rest = s.slice(tick + 1).trim();
    if (rest.startsWith("-")) rest = rest.slice(1).trim();
    rest = rest.replace(/(?:"|in)$/, "").trim();
    const inches = rest === "" ? 0 : parseNumber(rest);
    if (feet !== null && inches !== null) metres = feet * M_PER_FT + inches * M_PER_IN;
  } else {
    const m = /^(.*?)\s*(mm|cm|m|in|ft|")$/.exec(s);
    if (m) {
      const n = parseNumber(m[1]);
      if (n !== null) metres = n * SUFFIX[m[2]];
    } else {
      const n = parseNumber(s);
      if (n !== null) metres = n * defaultScale(system);
    }
  }
  if (metres === null || !Number.isFinite(metres)) {
    return { ok: false, error: `Can't read "${text.trim()}" as a length.` };
  }
  metres *= sign;
  if (Math.abs(metres) > MAX_LENGTH_M) return { ok: false, error: "That length is larger than 10 km." };
  return { ok: true, metres };
}

export type DimsResult = { ok: true; metres: [number, number] } | { ok: false; error: string };

/** Parse "24,12" (or "24;12", "600mm, 300mm") into two lengths. */
export function parseDimensions(text: string, system: UnitSystem): DimsResult {
  const parts = text.split(/[,;]/);
  if (parts.length !== 2) return { ok: false, error: "Type two sizes separated by a comma, like 24,12." };
  const a = parseLength(parts[0], system);
  if (!a.ok) return a;
  const b = parseLength(parts[1], system);
  if (!b.ok) return b;
  return { ok: true, metres: [a.metres, b.metres] };
}

function gcd(a: number, b: number): number {
  while (b) [a, b] = [b, a % b];
  return a;
}

/** Inches as "11 1/4", "3/4", "72" at 1/denominator precision. Returns [text, wasRounded]. */
export function formatFractionalInches(inches: number, denominator = 16): [string, boolean] {
  const neg = inches < 0;
  const abs = Math.abs(inches);
  let whole = Math.floor(abs);
  let num = Math.round((abs - whole) * denominator);
  if (num === denominator) {
    whole += 1;
    num = 0;
  }
  const rounded = whole + num / denominator;
  const approx = Math.abs(rounded - abs) > 1e-6;
  let text: string;
  if (num === 0) text = String(whole);
  else {
    const g = gcd(num, denominator);
    const frac = `${num / g}/${denominator / g}`;
    text = whole === 0 ? frac : `${whole} ${frac}`;
  }
  if (neg && text !== "0") text = `-${text}`;
  return [text, approx];
}

function trimDecimals(n: number, places: number): [string, boolean] {
  // Snap away binary noise first (0.28575 m * 1000 = 285.74999999999994).
  const clean = Number(n.toPrecision(12));
  const fixed = clean.toFixed(places);
  const approx = Math.abs(Number(fixed) - clean) > Math.pow(10, -places - 3);
  let text = fixed.includes(".") ? fixed.replace(/0+$/, "").replace(/\.$/, "") : fixed;
  if (text === "-0") text = "0";
  return [text, approx];
}

/** Format metres for display in a unit system. Rounded values get a leading "~ " like a tape reading. */
export function formatLength(metres: number, system: UnitSystem): string {
  let text: string;
  let approx: boolean;
  switch (system) {
    case "frac-in":
      [text, approx] = formatFractionalInches(metres / M_PER_IN);
      break;
    case "in":
      [text, approx] = trimDecimals(metres / M_PER_IN, 3);
      break;
    case "ft-in": {
      const totalIn = metres / M_PER_IN;
      const neg = totalIn < 0;
      const abs = Math.abs(totalIn);
      let feet = Math.floor(abs / 12 + 1e-9);
      let [inText, a] = formatFractionalInches(Math.max(0, abs - feet * 12));
      if (inText === "12") {
        feet += 1;
        inText = "0";
      }
      text = feet > 0 ? `${feet}' ${inText}"` : `${inText}"`;
      if (neg && abs > 0) text = `-${text}`;
      approx = a;
      break;
    }
    case "mm":
      [text, approx] = trimDecimals(metres * 1000, 1);
      break;
    case "cm":
      [text, approx] = trimDecimals(metres * 100, 2);
      break;
    case "m":
      [text, approx] = trimDecimals(metres, 4);
      break;
  }
  return approx ? `~ ${text}` : text;
}
