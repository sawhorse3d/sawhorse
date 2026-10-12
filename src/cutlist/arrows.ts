// Grain arrows in the viewport (lane 7), shown while the Cut list pane is open: on each
// drawn piece, an arrow along the grain on both broad faces (the faces across its
// thickness), drawn with depth testing so only the face you can see shows its arrow.
import type { CutList, V3 } from "../kernel/types";
import type { OverlayItem } from "../viewport/engine";

const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const mul = (a: V3, k: number): V3 => [a[0] * k, a[1] * k, a[2] * k];

/** Lift off the face so the arrow is not buried in it (metres). */
const LIFT = 0.0008;

/** Segments (pairs of points) for every grain arrow. `selected`: pieces to draw stronger. */
export function grainArrows(list: CutList | null): { normal: V3[]; across: V3[] } {
  const normal: V3[] = [];
  const across: V3[] = [];
  if (!list) return { normal, across };
  for (const row of list.rows) {
    if (row.grain === "none") continue;
    const along = row.grain === "length" ? 2 : 1;
    const other = along === 2 ? 1 : 2;
    const out = row.grain === "length" ? normal : across;
    for (const o of row.occurrences) {
      if (!o.visible) continue;
      const [t, w, l] = o.size;
      const size = [t, w, l];
      const d = o.axes[along];
      const s = o.axes[other];
      // Long enough to read, short enough to sit inside the face.
      const len = Math.min(size[along] * 0.6, Math.max(size[along] * 0.3, 0.3));
      const head = Math.min(len * 0.18, size[other] * 0.3, 0.04);
      for (const side of [1, -1]) {
        const c = add(o.center, mul(o.axes[0], side * (t / 2 + LIFT)));
        const a = add(c, mul(d, -len / 2));
        const b = add(c, mul(d, len / 2));
        out.push(a, b);
        out.push(b, add(add(b, mul(d, -head)), mul(s, head * 0.6)));
        out.push(b, add(add(b, mul(d, -head)), mul(s, -head * 0.6)));
        // A short tail bar marks the arrow's start, so it reads at any angle.
        out.push(add(a, mul(s, head * 0.4)), add(a, mul(s, -head * 0.4)));
      }
    }
  }
  return { normal, across };
}

export function grainOverlay(list: CutList | null): OverlayItem[] {
  const { normal, across } = grainArrows(list);
  const items: OverlayItem[] = [];
  if (normal.length) items.push({ kind: "segments", points: normal, color: "--grain", width: 2, depthTest: true });
  if (across.length) items.push({ kind: "segments", points: across, color: "--grain-across", width: 2.5, depthTest: true });
  return items;
}
