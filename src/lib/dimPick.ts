// What of a dimension in the model is under the cursor (#129): its dimension line, the two
// extension lines from the measured points out to it, and the number. The kernel's pick
// (crates/inference/src/pick.rs) knows only the dimension line, so a click on the number or
// on an extension line used to take nothing, or the face behind it.
//
// The lines are picked the way the kernel picks the dimension line: the point on the line
// nearest the eye ray, within `DIM_EDGE_PX` pixels at that point's depth, in front of the
// eye. The number is picked inside the box it is drawn in (centred on the middle of the
// dimension line, as `annotationView` places it and the viewport draws it).
//
// What wins (`withDimension`): on its lines a dimension takes the click only where the
// kernel found no point, no edge and no guide within its radii, as the dimension line always
// has; it beats faces, regions and section caps, as the line does. The number is drawn over
// everything with a near-opaque background, so inside its box the number wins over whatever
// is under it, edges and points included: the click takes what the person sees there. An
// edge under a number is still picked anywhere outside the box.
//
// Pure (no DOM, no three.js): the caller passes the projection and the box size.
import type { DocState, Picked, V3 } from "../kernel/types";

/** The kernel's edge radius for picks (crates/inference/src/infer.rs, `edge_px`). */
export const DIM_EDGE_PX = 5;

/** How the view maps world lengths to pixels (the engine's `pixelScale()`). */
export interface PixelScale {
  ortho: boolean;
  scale: number;
  eye: V3;
  forward: V3;
}

export interface DimHit {
  dimension: number;
  /** The picked point: on the line, or the middle of the dimension line for the number. */
  point: V3;
  /** Pixels from the cursor (0 inside the number's box). */
  px: number;
  /** Ray parameter of the point. */
  t: number;
  part: "line" | "extension" | "number";
}

type Dim = DocState["dimensions"][number];

const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const mul = (a: V3, s: number): V3 => [a[0] * s, a[1] * s, a[2] * s];
const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const len = (a: V3) => Math.hypot(a[0], a[1], a[2]);
const unit = (a: V3): V3 => {
  const l = len(a);
  return l > 0 ? mul(a, 1 / l) : [0, 0, 0];
};

/** The overshoot of the extension lines past the dimension line, as `annotationView` draws it. */
export function overshoot(offset: V3): V3 {
  const ol = len(offset);
  return ol > 0 ? mul(unit(offset), Math.min(0.0127, ol * 0.2)) : [0, 0, 0];
}

/** The three lines of a dimension as drawn: the dimension line, then the two extension lines. */
export function dimensionLines(d: Pick<Dim, "a" | "b" | "offset">): { line: [V3, V3]; extensions: [[V3, V3], [V3, V3]]; middle: V3 } {
  const a2 = add(d.a, d.offset);
  const b2 = add(d.b, d.offset);
  const over = overshoot(d.offset);
  return { line: [a2, b2], extensions: [[d.a, add(a2, over)], [d.b, add(b2, over)]], middle: mul(add(a2, b2), 0.5) };
}

/** Closest points of a ray (origin `o`, unit `dir`) and the segment a..b: the ray parameter
 * (never negative) and the segment parameter in [0, 1]. A port of the kernel's `ray_segment`. */
export function raySegment(o: V3, dir: V3, a: V3, b: V3): [number, number] {
  const d1 = dir;
  const d2 = sub(b, a);
  const r = sub(o, a);
  const aa = dot(d1, d1);
  const e = dot(d2, d2);
  if (e <= 1e-300) return [dot(sub(a, o), dir), 0];
  const f = dot(d2, r);
  const c = dot(d1, r);
  const bb = dot(d1, d2);
  const denom = aa * e - bb * bb;
  let s = Math.abs(denom) > 1e-300 ? Math.max(0, (bb * f - c * e) / denom) : 0;
  let t = (bb * s + f) / e;
  if (t < 0) {
    t = 0;
    s = Math.max(0, -c / aa);
  } else if (t > 1) {
    t = 1;
    s = Math.max(0, (bb - c) / aa);
  }
  return [s, t];
}

/** Metres per pixel at `p`, and its depth in front of the eye (the kernel's `PixelModel::at`). */
function depthOf(px: PixelScale, p: V3): number {
  return dot(sub(p, px.eye), px.forward);
}

function metresPerPixel(px: PixelScale, p: V3): number {
  const s = Math.max(px.scale, 1e-12);
  return px.ortho ? s : s * Math.max(depthOf(px, p), 1e-6);
}

/** Pixels between the eye ray and `p`, at p's depth (the kernel's `ray_px`). */
export function rayPx(o: V3, dir: V3, px: PixelScale, p: V3): number {
  const v = sub(p, o);
  const along = dot(v, dir);
  const off = sub(v, mul(dir, along));
  return len(off) / metresPerPixel(px, p);
}

export interface DimPickInput {
  dims: Dim[];
  ray: { origin: V3; dir: V3 };
  px: PixelScale;
  /** The cursor, in canvas pixels. */
  cursor: [number, number];
  /** World point to canvas pixels. */
  toScreen: (p: V3) => [number, number];
  /** The number's text for a dimension (the display units). */
  text: (d: Dim) => string;
  /** The number's box, width and height in pixels, for its text. */
  size: (text: string) => [number, number];
  edgePx?: number;
}

/** The dimension under the cursor and which part of it, or null. */
export function dimensionAt(q: DimPickInput): DimHit | null {
  const edgePx = q.edgePx ?? DIM_EDGE_PX;
  const o = q.ray.origin;
  const dir = unit(q.ray.dir);
  let best: DimHit | null = null;
  const better = (c: DimHit) => !best || (Math.abs(c.px - best.px) <= 0.25 ? c.t < best.t : c.px < best.px);
  for (const d of q.dims) {
    const g = dimensionLines(d);
    const segs: [V3, V3, DimHit["part"]][] = [[...g.line, "line"], [...g.extensions[0], "extension"], [...g.extensions[1], "extension"]];
    for (const [a, b, part] of segs) {
      const [t, u] = raySegment(o, dir, a, b);
      const p = add(a, mul(sub(b, a), u));
      // In front of the eye: the ray parameter and the depth both.
      if (!(t >= 0) || !(depthOf(q.px, p) > 0)) continue;
      const dpx = rayPx(o, dir, q.px, p);
      if (!(dpx <= edgePx)) continue;
      const c: DimHit = { dimension: d.id, point: p, px: dpx, t, part };
      if (better(c)) best = c;
    }
    // The number: drawn centred on the middle of the dimension line, over everything.
    const m = g.middle;
    if (depthOf(q.px, m) > 0 && dot(sub(m, o), dir) >= 0) {
      const [cx, cy] = q.toScreen(m);
      const [w, h] = q.size(q.text(d));
      if (Math.abs(q.cursor[0] - cx) <= w / 2 && Math.abs(q.cursor[1] - cy) <= h / 2) {
        const c: DimHit = { dimension: d.id, point: m, px: 0, t: dot(sub(m, o), dir), part: "number" };
        if (!best || best.part !== "number" || c.t < best.t) best = c;
      }
    }
  }
  return best;
}

/** The kernel's pick with a dimension's hit folded in (see the rules at the top). */
export function withDimension(picked: Picked | null, hit: DimHit | null): Picked | null {
  if (!hit) return picked;
  const k = picked?.entity.kind;
  const nearer = k === "vertex" || k === "edge" || k === "guide" || k === "dimension";
  if (nearer && hit.part !== "number") return picked;
  return { entity: { kind: "dimension", dimension: hit.dimension }, point: hit.point, surface: picked?.surface ?? null } as Picked;
}

/** A number's box for its text when nothing can measure it: the viewport's label style
 * (11 px monospace, 1 px by 5 px padding, a 1 px border). */
export function estimatedLabelSize(text: string): [number, number] {
  return [text.length * 6.6 + 12, 17];
}
