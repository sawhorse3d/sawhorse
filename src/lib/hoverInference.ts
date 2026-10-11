// Coarse hover inference for the viewport, in screen space.
//
// ADR-0001: three.js raycasting is only for coarse hover; the real inference
// engine (BVH + rstar, pixel-to-world tolerance) lands in the inference lane and
// lives in the Rust `inference` crate. This module only decides which of OUR
// four inference colours to show under the cursor.

export type InferenceKind = "endpoint" | "midpoint" | "on-edge" | "on-face";

export interface InferenceStyle {
  label: string;
  /** CSS custom property holding the colour. */
  token: string;
}

export const INFERENCE_STYLES: Record<InferenceKind, InferenceStyle> = {
  endpoint: { label: "Endpoint", token: "--inf-end" },
  midpoint: { label: "Midpoint", token: "--inf-mid" },
  "on-edge": { label: "On Edge", token: "--inf-edge" },
  "on-face": { label: "On Face", token: "--inf-face" },
};

export type Vec2 = readonly [number, number];

export interface ScreenSegment {
  a: Vec2;
  b: Vec2;
}

export interface HoverHit {
  kind: InferenceKind;
  /** Screen position the marker snaps to. */
  at: Vec2;
}

/** Pixel radii; points beat midpoints beat edges beat faces. */
export const SNAP_PX = { point: 10, edge: 6 } as const;

function dist2(p: Vec2, q: Vec2): number {
  const dx = p[0] - q[0];
  const dy = p[1] - q[1];
  return dx * dx + dy * dy;
}

function closestOnSegment(p: Vec2, s: ScreenSegment): Vec2 {
  const vx = s.b[0] - s.a[0];
  const vy = s.b[1] - s.a[1];
  const len2 = vx * vx + vy * vy;
  if (len2 === 0) return s.a;
  let t = ((p[0] - s.a[0]) * vx + (p[1] - s.a[1]) * vy) / len2;
  t = Math.max(0, Math.min(1, t));
  return [s.a[0] + t * vx, s.a[1] + t * vy];
}

/**
 * Classify the cursor against projected edges. `onFace` is the screen point of a
 * face raycast hit, if any. Returns null when nothing is near.
 */
export function classifyHover(cursor: Vec2, segments: readonly ScreenSegment[], onFace: Vec2 | null): HoverHit | null {
  const pointR2 = SNAP_PX.point * SNAP_PX.point;
  const edgeR2 = SNAP_PX.edge * SNAP_PX.edge;
  let best: HoverHit | null = null;
  let bestD = Infinity;
  // Endpoints first.
  for (const s of segments) {
    for (const p of [s.a, s.b]) {
      const d = dist2(cursor, p);
      if (d <= pointR2 && d < bestD) {
        best = { kind: "endpoint", at: p };
        bestD = d;
      }
    }
  }
  if (best) return best;
  for (const s of segments) {
    const mid: Vec2 = [(s.a[0] + s.b[0]) / 2, (s.a[1] + s.b[1]) / 2];
    const d = dist2(cursor, mid);
    if (d <= pointR2 && d < bestD) {
      best = { kind: "midpoint", at: mid };
      bestD = d;
    }
  }
  if (best) return best;
  for (const s of segments) {
    const c = closestOnSegment(cursor, s);
    const d = dist2(cursor, c);
    if (d <= edgeR2 && d < bestD) {
      best = { kind: "on-edge", at: c };
      bestD = d;
    }
  }
  if (best) return best;
  return onFace ? { kind: "on-face", at: onFace } : null;
}
