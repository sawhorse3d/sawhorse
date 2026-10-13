// A drawing sheet as the kernel lays it out (lane 12, crates/export/src/sheet.rs): primitives
// in paper metres from the top-left corner, y down. The sheet canvas, the SVG, the PDF, the
// DXF and the PNG all draw exactly these, so what you see is what prints. This module turns
// them into SVG (the canvas and the SVG file share it) and holds the pure geometry the
// canvas needs (viewport mapping, hit tests, grid snapping).
import { fit, textWidth } from "../cutlist/pdf";
import type { DimKind, ModelAnchor } from "../kernel/sheetTypes";
import type { V3 } from "../kernel/types";

export type Anchor = "start" | "middle" | "end";
export type RGB = [number, number, number];

export type Prim =
  | { p: "line"; a: [number, number]; b: [number, number]; width: number; dash?: [number, number]; color: RGB }
  | { p: "fill"; rings: [number, number][][]; color: RGB }
  | { p: "rect"; rect: [number, number, number, number]; width: number; stroke?: RGB; fill?: RGB }
  | { p: "text"; at: [number, number]; text: string; size: number; anchor: Anchor; angle?: number; bold?: boolean; max?: number; color: RGB }
  | { p: "image"; rect: [number, number, number, number]; path: string };

export interface ViewFrame {
  ortho: boolean;
  right: V3;
  up: V3;
  forward: V3;
  eye: V3;
  scale: number;
}

export interface ViewportInfo {
  id: number;
  rect: [number, number, number, number];
  frame: ViewFrame;
  /** Paper metres per drawing unit. */
  factor: number;
  /** The drawing point at the rectangle's centre. */
  centre: [number, number];
  to_scale: boolean;
  problem?: string;
}

export interface ItemInfo {
  id: number;
  kind: "viewport" | "dimension" | "label" | "table" | "image" | "line" | "rect";
  bounds: [number, number, number, number];
  text?: string;
  /** A dimension's model points as drawn: its anchors found on the model as it is now. */
  points?: V3[];
  /** A dimension with an end whose solid or corner is gone: that end stays where it was. */
  detached?: boolean;
  /** The range of `prims` it drew, [start, end). */
  prims: [number, number];
}

export interface SheetRender {
  id: number;
  name: string;
  size: [number, number];
  prims: Prim[];
  items: ItemInfo[];
  viewports: ViewportInfo[];
  drawings: string[];
  hash: string;
}

/** How a viewport would draw before it is placed (the kernel's `viewport_fit`). */
export interface ViewportFit {
  /** Paper metres per drawing metre that fit the drawing in the rectangle (92%). */
  factor: number;
  /** The drawing's width and height, model metres. */
  size: [number, number];
  /** Nothing to draw. */
  empty: boolean;
  problem?: string | null;
}

/** Where a cursor snapped on a viewport's drawing. */
export interface SheetSnap {
  kind: "endpoint" | "midpoint" | "intersection" | "on-edge";
  at: [number, number];
  point: V3;
  /** Where the point sits on a solid (absent: a sketch, a mesh, a section cut). */
  anchor?: ModelAnchor;
}

export const IN = 0.0254;
export const MM = 0.001;

const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

/** The drawing coordinates of a model point in a viewport's view (null: behind the eye). */
export function project(f: ViewFrame, p: V3): [number, number] | null {
  if (f.ortho) return [dot(p, f.right), dot(p, f.up)];
  const d: V3 = [p[0] - f.eye[0], p[1] - f.eye[1], p[2] - f.eye[2]];
  const z = dot(d, f.forward);
  if (z < 5e-4) return null;
  return [(f.scale * dot(d, f.right)) / z, (f.scale * dot(d, f.up)) / z];
}

/** Paper point of a drawing point in a viewport. */
export function toPaper(v: ViewportInfo, d: [number, number]): [number, number] {
  const cx = v.rect[0] + v.rect[2] / 2;
  const cy = v.rect[1] + v.rect[3] / 2;
  return [cx + (d[0] - v.centre[0]) * v.factor, cy - (d[1] - v.centre[1]) * v.factor];
}

/** Drawing point of a paper point in a viewport. */
export function toDrawing(v: ViewportInfo, q: [number, number]): [number, number] {
  const cx = v.rect[0] + v.rect[2] / 2;
  const cy = v.rect[1] + v.rect[3] / 2;
  return [v.centre[0] + (q[0] - cx) / v.factor, v.centre[1] - (q[1] - cy) / v.factor];
}

/** Paper point of a model point seen in a viewport. */
export function modelToPaper(v: ViewportInfo, p: V3): [number, number] | null {
  const d = project(v.frame, p);
  return d ? toPaper(v, d) : null;
}

/** `x` to the nearest multiple of `grid` (0: no snapping). */
export function snapToGrid(x: number, grid: number): number {
  if (!(grid > 0)) return x;
  return Math.round(x / grid) * grid;
}

export function snapPoint(p: [number, number], grid: number): [number, number] {
  return [snapToGrid(p[0], grid), snapToGrid(p[1], grid)];
}

/** Distance from `p` to the segment `a`-`b`. */
export function segmentDistance(p: [number, number], a: [number, number], b: [number, number]): number {
  const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
  const l2 = dx * dx + dy * dy;
  const t = l2 > 0 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2)) : 0;
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}

/** Even-odd: `p` inside `rings`. */
function insideRings(p: [number, number], rings: [number, number][][]): boolean {
  let c = false;
  for (const r of rings) {
    for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
      const [a, b] = [r[i], r[j]];
      if (a[1] > p[1] !== b[1] > p[1] && p[0] < a[0] + ((p[1] - a[1]) / (b[1] - a[1])) * (b[0] - a[0])) c = !c;
    }
  }
  return c;
}

/** The box a text primitive covers, in its own frame (x along the text from its anchor, y
 * down from the baseline): Helvetica's advance widths, from a full size above the baseline
 * (as the kernel bounds it) to a quarter below (descenders). */
export function textBox(t: Extract<Prim, { p: "text" }>): [number, number, number, number] {
  const w = textWidth(shownText(t), t.size, !!t.bold);
  const x0 = t.anchor === "start" ? 0 : t.anchor === "middle" ? -w / 2 : -w;
  return [x0, -t.size, w, 1.25 * t.size];
}

/** `p` on what primitive `pr` draws, within `slack` paper metres: on a line (half its pen
 * wider), in a text's box, in a filled ring, on a rectangle's outline or in its fill. */
export function primHit(pr: Prim, p: [number, number], slack: number): boolean {
  switch (pr.p) {
    case "line":
      return segmentDistance(p, pr.a, pr.b) <= slack + pr.width / 2;
    case "text": {
      // Into the text's frame: the SVG turns it by -angle (degrees, y down).
      const a = ((pr.angle ?? 0) * Math.PI) / 180;
      const [dx, dy] = [p[0] - pr.at[0], p[1] - pr.at[1]];
      const [x, y] = [dx * Math.cos(a) - dy * Math.sin(a), dx * Math.sin(a) + dy * Math.cos(a)];
      const [bx, by, bw, bh] = textBox(pr);
      return x >= bx - slack && x <= bx + bw + slack && y >= by - slack && y <= by + bh + slack;
    }
    case "fill":
      return insideRings(p, pr.rings) || pr.rings.some((r) => r.some((q, i) => segmentDistance(p, q, r[(i + 1) % r.length]) <= slack));
    case "rect": {
      const [x, y, w, h] = pr.rect;
      const c: [number, number][] = [
        [x, y],
        [x + w, y],
        [x + w, y + h],
        [x, y + h],
      ];
      const onEdge = c.some((q, i) => segmentDistance(p, q, c[(i + 1) % 4]) <= slack + pr.width / 2);
      return onEdge || (!!pr.fill && p[0] >= x && p[0] <= x + w && p[1] >= y && p[1] <= y + h);
    }
    case "image": {
      const [x, y, w, h] = pr.rect;
      return p[0] >= x - slack && p[0] <= x + w + slack && p[1] >= y - slack && p[1] <= y + h + slack;
    }
  }
}

/** Items picked as a whole box (their frame): a viewport, a table, an image. Everything
 * else is picked only on what it draws. */
const BOXED: ReadonlySet<ItemInfo["kind"]> = new Set(["viewport", "table", "image"]);

/** `p` on item `it` within `slack`: inside the frame of a viewport, a table or an image;
 * on the drawn geometry (lines, text boxes, arrowheads) of anything else. `boxed` picks
 * the item anywhere in its box (a selected rectangle or label, which is moved by its box). */
export function itemHit(it: ItemInfo, prims: Prim[], p: [number, number], slack = 0, boxed = false): boolean {
  const [x, y, w, h] = it.bounds;
  if (p[0] < x - slack || p[0] > x + w + slack || p[1] < y - slack || p[1] > y + h + slack) return false;
  if (boxed || BOXED.has(it.kind)) return true;
  for (let i = it.prims[0]; i < it.prims[1]; i++) {
    const pr = prims[i];
    if (pr && primHit(pr, p, slack)) return true;
  }
  return false;
}

/** The item under `p` (within `slack` paper metres): the topmost (drawn last) whose drawn
 * geometry it is on. A dimension's lines and text, a label's text and leader, a line or a
 * rectangle's outline take a click only on themselves, so a click in a viewport between a
 * dimension's lines picks the viewport (lane 12c; it used to pick by bounding box). A
 * selected item other than a dimension is also taken anywhere in its box (the box drawn
 * around it), so a selected rectangle drags from its middle; a dimension's box spans the
 * drawing it measures, so it never is. */
export function hitItem(r: Pick<SheetRender, "items" | "prims">, p: [number, number], slack = 0, selected: readonly number[] = []): ItemInfo | null {
  for (let i = r.items.length - 1; i >= 0; i--) {
    const it = r.items[i];
    if (itemHit(it, r.prims, p, slack, it.kind !== "dimension" && selected.includes(it.id))) return it;
  }
  return null;
}

/** The viewport under paper point `p`, if any (topmost). */
export function viewportAt(r: SheetRender, p: [number, number]): ViewportInfo | null {
  for (let i = r.viewports.length - 1; i >= 0; i--) {
    const v = r.viewports[i];
    const [x, y, w, h] = v.rect;
    if (p[0] >= x && p[0] <= x + w && p[1] >= y && p[1] <= y + h) return v;
  }
  return null;
}

// ---- dimensions -----------------------------------------------------------------------
//
// How the kernel stands a dimension off its points (crates/export/src/sheet.rs,
// `dimension`), turned around: the offset that puts the dimension line under a cursor.

type P2 = [number, number];
const sub = (a: P2, b: P2): P2 => [a[0] - b[0], a[1] - b[1]];
const len = (a: P2) => Math.hypot(a[0], a[1]);

/** The centre of the circle through three paper points (null: in a line). */
export function circumcentre(a: P2, b: P2, c: P2): P2 | null {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
  if (Math.abs(d) < 1e-18) return null;
  const a2 = a[0] ** 2 + a[1] ** 2;
  const b2 = b[0] ** 2 + b[1] ** 2;
  const c2 = c[0] ** 2 + c[1] ** 2;
  return [(a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d, (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d];
}

/** The offset that puts a dimension of `kind` on paper points `p` through `cursor`. */
export function dimOffset(kind: DimKind, p: P2[], cursor: P2): number {
  switch (kind) {
    case "horizontal":
      return p[0][1] - cursor[1];
    case "vertical":
      return cursor[0] - p[0][0];
    case "aligned": {
      const d = sub(p[1], p[0]);
      const l = len(d) || 1;
      const n: P2 = [d[1] / l, -d[0] / l];
      const w = sub(cursor, p[0]);
      return w[0] * n[0] + w[1] * n[1];
    }
    case "angular":
      return len(sub(cursor, p[0]));
    case "radius":
    case "diameter": {
      const c = circumcentre(p[0], p[1], p[2]);
      if (!c) return 0;
      const r = len(sub(p[0], c));
      return Math.max(0, len(sub(cursor, c)) - r);
    }
  }
}

/** The dimension line a placement shows before it is made: two paper points. */
export function dimPreview(kind: DimKind, p: P2[], offset: number): [P2, P2] | null {
  switch (kind) {
    case "horizontal":
      return [
        [p[0][0], p[0][1] - offset],
        [p[1][0], p[0][1] - offset],
      ];
    case "vertical":
      return [
        [p[0][0] + offset, p[0][1]],
        [p[0][0] + offset, p[1][1]],
      ];
    case "aligned": {
      const d = sub(p[1], p[0]);
      const l = len(d) || 1;
      const n: P2 = [(d[1] / l) * offset, (-d[0] / l) * offset];
      return [
        [p[0][0] + n[0], p[0][1] + n[1]],
        [p[1][0] + n[0], p[1][1] + n[1]],
      ];
    }
    default:
      return null;
  }
}

// ---- SVG ------------------------------------------------------------------------------
//
// SVG coordinates are paper metres times `k`: text a couple of millimetres tall written in
// metres (0.0024) renders badly in some browsers once scaled up, so the canvas draws in
// 1/10 mm (k = 1e4) and files in millimetres or points.

/** User units per paper metre: the canvas's tenths of a millimetre. */
export const CANVAS_K = 1e4;
/** Points (1/72 in) per metre. */
export const POINTS_K = 72 / IN;
/** Millimetres per metre. */
export const MM_K = 1000;

const num = (x: number) => {
  const r = Math.round(x * 1e4) / 1e4;
  return Object.is(r, -0) ? "0" : String(r);
};
const rgb = (c: RGB) => `rgb(${c[0]},${c[1]},${c[2]})`;
export const escapeXml = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** The text a primitive shows, shortened to its `max` width. */
export function shownText(t: Extract<Prim, { p: "text" }>): string {
  return t.max !== undefined ? fit(t.text, t.size, t.max, !!t.bold) : t.text;
}

/** Resolves an image path to something an <image> can show (null: draw a placeholder). */
export type Href = (path: string) => string | null;

/** One primitive as an SVG element, at `k` user units per paper metre. */
export function primSvg(pr: Prim, href: Href, k: number): string {
  const n = (x: number) => num(x * k);
  switch (pr.p) {
    case "line":
      return `<line x1="${n(pr.a[0])}" y1="${n(pr.a[1])}" x2="${n(pr.b[0])}" y2="${n(pr.b[1])}" stroke="${rgb(pr.color)}" stroke-width="${n(pr.width)}" stroke-linecap="round"${pr.dash ? ` stroke-dasharray="${n(pr.dash[0])} ${n(pr.dash[1])}"` : ""}/>`;
    case "fill":
      return `<path d="${pr.rings.map((r) => `M${r.map((q) => `${n(q[0])} ${n(q[1])}`).join("L")}Z`).join("")}" fill="${rgb(pr.color)}" fill-rule="evenodd"/>`;
    case "rect":
      return `<rect x="${n(pr.rect[0])}" y="${n(pr.rect[1])}" width="${n(pr.rect[2])}" height="${n(pr.rect[3])}" fill="${pr.fill ? rgb(pr.fill) : "none"}"${pr.stroke ? ` stroke="${rgb(pr.stroke)}" stroke-width="${n(pr.width)}"` : ""}/>`;
    case "text": {
      const rot = pr.angle ? ` transform="rotate(${num(-pr.angle)} ${n(pr.at[0])} ${n(pr.at[1])})"` : "";
      return `<text x="${n(pr.at[0])}" y="${n(pr.at[1])}" font-family="Helvetica, Arial, sans-serif" font-size="${n(pr.size)}"${pr.bold ? ' font-weight="bold"' : ""} text-anchor="${pr.anchor}" fill="${rgb(pr.color)}"${rot}>${escapeXml(shownText(pr))}</text>`;
    }
    case "image": {
      const h = href(pr.path);
      if (!h) return `<rect x="${n(pr.rect[0])}" y="${n(pr.rect[1])}" width="${n(pr.rect[2])}" height="${n(pr.rect[3])}" fill="none" stroke="rgb(170,174,170)" stroke-width="${n(0.0002)}"/>`;
      return `<image href="${escapeXml(h)}" x="${n(pr.rect[0])}" y="${n(pr.rect[1])}" width="${n(pr.rect[2])}" height="${n(pr.rect[3])}" preserveAspectRatio="none"/>`;
    }
  }
}

/** A run of primitives as SVG, one element per line. */
export function primsSvg(prims: Prim[], href: Href, k: number): string {
  return prims.map((p) => primSvg(p, href, k)).join("\n");
}

/** The sheet in layers for the canvas: what is under every item (the margin frame), each
 * item's own primitives (so a dragged item moves alone), and what is over them (the title
 * block). */
export interface SheetLayers {
  under: string;
  items: { id: number; svg: string }[];
  over: string;
}

export function sheetLayers(r: SheetRender, href: Href, k = CANVAS_K): SheetLayers {
  const first = r.items.length ? Math.min(...r.items.map((i) => i.prims[0])) : r.prims.length;
  const last = r.items.length ? Math.max(...r.items.map((i) => i.prims[1])) : r.prims.length;
  return {
    under: primsSvg(r.prims.slice(0, first), href, k),
    items: r.items.map((i) => ({ id: i.id, svg: primsSvg(r.prims.slice(i.prims[0], i.prims[1]), href, k) })),
    over: primsSvg(r.prims.slice(last), href, k),
  };
}

/** The sheet as an SVG file at its real paper size. Coordinates are paper lengths: points
 * (1/72 in) on an inch sheet, millimetres on a metric one, so one user unit is a real
 * length on the paper; the viewBox spans the paper and width and height say its size. */
export function sheetSvg(r: SheetRender, href: Href, units: "inches" | "millimetres" = "inches"): string {
  const [w, h] = r.size;
  const k = units === "inches" ? POINTS_K : MM_K;
  const size = (m: number) => (units === "inches" ? `${num(m / IN)}in` : `${num(m / MM)}mm`);
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="${size(w)}" height="${size(h)}" viewBox="0 0 ${num(w * k)} ${num(h * k)}">
<title>${escapeXml(r.name)}</title>
<desc>Sawhorse drawing sheet. User units: ${units === "inches" ? "points (1/72 in)" : "millimetres"} of paper.</desc>
<rect x="0" y="0" width="${num(w * k)}" height="${num(h * k)}" fill="#ffffff"/>
${primsSvg(r.prims, href, k)}
</svg>
`;
}
