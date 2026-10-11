// The Value Control Box grammar: what can be typed mid-gesture, for every tool.
//
//   3/4   1 1/2   1-1/2   18"   1'6"   1' 6 1/2"   457mm   45.7cm   0.457m   -2
//                                           a length (units: the display units unless typed)
//   24,12   24, 12   600mm,300mm             two lengths (rectangles: width, depth)
//   48s                                      a count: circle / arc segments, polygon sides
//   12r                                      an arc radius
//   30°   30deg                              an angle, reserved for Rotate (lane 4)
//   x3   3x   *3                             copies: an array, reserved for Move (lane 4)
//   /4                                       divide: an array, reserved for Move (lane 4)
//
// Every tool takes the forms that make sense for it and says so when given another.
import { parseLength, type UnitSystem } from "./units";

export type VcbValue =
  | { kind: "length"; m: number }
  | { kind: "lengths"; m: number[] }
  | { kind: "count"; n: number }
  | { kind: "radius"; m: number }
  | { kind: "angle"; deg: number }
  | { kind: "array"; copies: number }
  | { kind: "divide"; parts: number };

export type VcbResult = { ok: true; value: VcbValue } | { ok: false; error: string };

const COUNT = /^(\d+)\s*s$/i;
const RADIUS = /^(.+?)\s*r$/i;
const ANGLE = /^(-?\d+(?:\.\d+)?|-?\.\d+)\s*(?:°|deg|degrees?)$/i;
const ARRAY = /^(?:[x*]\s*(\d+)|(\d+)\s*x)$/i;
const DIVIDE = /^\/\s*(\d+)$/;

/** Parse one VCB entry. */
export function parseVcb(text: string, units: UnitSystem): VcbResult {
  const s = text.trim();
  if (s === "") return { ok: false, error: "Type a value." };
  let m = ARRAY.exec(s);
  if (m) {
    const n = Number(m[1] ?? m[2]);
    return n >= 1 ? { ok: true, value: { kind: "array", copies: n } } : { ok: false, error: "An array needs at least one copy." };
  }
  m = DIVIDE.exec(s);
  if (m) {
    const n = Number(m[1]);
    return n >= 2 ? { ok: true, value: { kind: "divide", parts: n } } : { ok: false, error: "Divide into at least 2 parts." };
  }
  m = COUNT.exec(s);
  if (m) {
    const n = Number(m[1]);
    return { ok: true, value: { kind: "count", n } };
  }
  m = ANGLE.exec(s);
  if (m) return { ok: true, value: { kind: "angle", deg: Number(m[1]) } };
  if (s.includes(",") || s.includes(";")) {
    const parts = s.split(/[,;]/);
    const ms: number[] = [];
    for (const p of parts) {
      if (p.trim() === "") return { ok: false, error: `Missing a value in "${s}".` };
      const r = parseLength(p, units);
      if (!r.ok) return r;
      ms.push(r.metres);
    }
    return { ok: true, value: { kind: "lengths", m: ms } };
  }
  m = RADIUS.exec(s);
  if (m) {
    // "12r", "300mm r": an arc radius.
    const r = parseLength(m[1], units);
    if (r.ok) return { ok: true, value: { kind: "radius", m: r.metres } };
  }
  const r = parseLength(s, units);
  if (!r.ok) return r;
  return { ok: true, value: { kind: "length", m: r.metres } };
}

/** The message for a form a tool does not take. */
export function notHere(v: VcbValue, tool: string, wants: string): string {
  if (v.kind === "array" || v.kind === "divide") return "Arrays (x3, /4) arrive with the Move tool in lane 4.";
  return `${tool} takes ${wants}.`;
}
