// How guides and dimensions draw: guide lines as long dashed lines, guide points as small
// crosses, dimensions as extension lines, a dimension line with ticks, and unit-aware text.
import type { DocState, Entity, V3 } from "../kernel/types";
import type { Label } from "../components/Viewport";
import type { OverlayItem } from "../viewport/engine";

/** Half-length a guide line is drawn to on each side of its point, metres. */
export const GUIDE_REACH = 100;

const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const mul = (a: V3, s: number): V3 => [a[0] * s, a[1] * s, a[2] * s];
const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const len = (a: V3) => Math.hypot(a[0], a[1], a[2]);
const unit = (a: V3): V3 => {
  const l = len(a);
  return l > 0 ? mul(a, 1 / l) : [0, 0, 0];
};

export function annotationView(state: DocState | null, fmt: (m: number) => string, selection: Entity[]): { overlay: OverlayItem[]; labels: Label[] } {
  const overlay: OverlayItem[] = [];
  const labels: Label[] = [];
  if (!state) return { overlay, labels };
  const selGuides = new Set(selection.filter((e) => e.kind === "guide").map((e) => (e as { guide: number }).guide));
  const selDims = new Set(selection.filter((e) => e.kind === "dimension").map((e) => (e as { dimension: number }).dimension));
  for (const g of state.guides ?? []) {
    const color = selGuides.has(g.id) ? "--sel" : "--inf-guide";
    if (g.dir) {
      overlay.push({ kind: "segments", points: [add(g.point, mul(g.dir, -GUIDE_REACH)), add(g.point, mul(g.dir, GUIDE_REACH))], color, width: selGuides.has(g.id) ? 2 : 1.25, dashed: true, depthTest: true });
    } else {
      const s = 0.0127; // a 1/2 in cross
      const p = g.point;
      overlay.push({
        kind: "segments",
        points: [add(p, [-s, 0, 0]), add(p, [s, 0, 0]), add(p, [0, -s, 0]), add(p, [0, s, 0]), add(p, [0, 0, -s]), add(p, [0, 0, s])],
        color,
        width: 1.5,
        depthTest: true,
      });
    }
  }
  for (const d of state.dimensions ?? []) {
    const color = selDims.has(d.id) ? "--sel" : "--accent";
    const off = d.offset;
    const ol = len(off);
    const over = ol > 0 ? mul(unit(off), Math.min(0.0127, ol * 0.2)) : ([0, 0, 0] as V3);
    const a2 = add(d.a, off);
    const b2 = add(d.b, off);
    const along = unit(sub(d.b, d.a));
    const tick = mul(add(along, unit(off)), 0.006);
    overlay.push({
      kind: "segments",
      points: [
        d.a,
        add(a2, over),
        d.b,
        add(b2, over),
        a2,
        b2,
        sub(a2, tick),
        add(a2, tick),
        sub(b2, tick),
        add(b2, tick),
      ],
      color,
      width: 1.25,
    });
    labels.push({ at: mul(add(a2, b2), 0.5), text: fmt(d.length), className: "dim" });
  }
  return { overlay, labels };
}
