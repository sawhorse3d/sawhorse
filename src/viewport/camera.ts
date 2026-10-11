// Pure camera and grid helpers for the viewport (no three.js, no DOM), so the navigation
// rules are unit-tested. Z-up, metres, exactly the kernel's frame (ADR-0001).
import type { UnitSystem } from "../lib/units";
import { M_PER_IN } from "../lib/units";

export type Vec3 = [number, number, number];
export type ViewId = "iso" | "top" | "front" | "right" | "persp";
export type Projection = "parallel" | "perspective";

export const VIEWS: { id: ViewId; label: string; title: string }[] = [
  { id: "iso", label: "Iso", title: "Isometric view (parallel projection)" },
  { id: "top", label: "Top", title: "Top view, looking down -Z" },
  { id: "front", label: "Front", title: "Front view, looking along +Y" },
  { id: "right", label: "Right", title: "Right view, looking along -X" },
  { id: "persp", label: "Persp", title: "Three-quarter perspective view" },
];

/** Camera direction (from the target toward the eye) and up vector per standard view. */
export const VIEW_DIRS: Record<ViewId, { dir: Vec3; up: Vec3 }> = {
  // Isometric feel: eye in the -X/-Y/+Z octant so +X runs right and +Y recedes left, Z up.
  iso: { dir: norm([-1, -1, 1]), up: [0, 0, 1] },
  top: { dir: [0, 0, 1], up: [0, 1, 0] },
  front: { dir: [0, -1, 0], up: [0, 0, 1] },
  right: { dir: [1, 0, 0], up: [0, 0, 1] },
  persp: { dir: norm([-0.9, -1.6, 1]), up: [0, 0, 1] },
};

/** The projection a standard view uses: the three-quarter view is the perspective one. */
export function projectionForView(v: ViewId): Projection {
  return v === "persp" ? "perspective" : "parallel";
}

export function norm(v: Vec3): Vec3 {
  const l = Math.hypot(v[0], v[1], v[2]) || 1;
  return [v[0] / l, v[1] / l, v[2] / l];
}

export const PERSPECTIVE_FOV_DEG = 35;

/**
 * Half-height of a parallel frustum that shows the same thing at distance `d` as a
 * perspective camera with vertical field of view `fovDeg`: switching projection keeps
 * the target the same size on screen.
 */
export function equivalentOrthoHalfHeight(d: number, fovDeg = PERSPECTIVE_FOV_DEG): number {
  return d * Math.tan((fovDeg * Math.PI) / 360);
}

/** Distance a perspective camera needs to show a parallel frame of half-height `h`. */
export function equivalentPerspectiveDistance(h: number, fovDeg = PERSPECTIVE_FOV_DEG): number {
  return h / Math.tan((fovDeg * Math.PI) / 360);
}

/**
 * World size of one CSS pixel. Parallel: the visible height over the viewport height.
 * Perspective: grows with depth along the view direction, so it is given per unit depth.
 */
export interface PixelScale {
  ortho: boolean;
  /** Parallel: metres per pixel. Perspective: metres per pixel per metre of depth. */
  scale: number;
  eye: Vec3;
  /** Unit view direction (from the eye into the scene). */
  forward: Vec3;
}

export function metresPerPixel(px: PixelScale, at: Vec3): number {
  if (px.ortho) return px.scale;
  const d = (at[0] - px.eye[0]) * px.forward[0] + (at[1] - px.eye[1]) * px.forward[1] + (at[2] - px.eye[2]) * px.forward[2];
  return px.scale * Math.max(d, 1e-6);
}

// ---- grid ------------------------------------------------------------------------------

export interface GridSpec {
  /** Minor spacing, metres (1 in, or 10 mm). */
  minor: number;
  /** Major spacing, metres (1 ft, or 100 mm). */
  major: number;
  /** Minor cells per major cell. */
  ratio: number;
  label: string;
}

/** The ground grid follows the display units: 1 in / 1 ft imperial, 10 mm / 100 mm metric. */
export function gridSpec(units: UnitSystem): GridSpec {
  switch (units) {
    case "mm":
    case "cm":
    case "m":
      return { minor: 0.01, major: 0.1, ratio: 10, label: "10 mm / 100 mm" };
    default:
      return { minor: M_PER_IN, major: 12 * M_PER_IN, ratio: 12, label: "1 in / 1 ft" };
  }
}

/** Grid lines closer than this on screen are hidden (they would be a grey wash). */
export const GRID_MIN_PX = 6;

/** Which grid levels are worth drawing when one minor cell spans `minorPx` pixels. */
export function gridVisibility(minorPx: number, ratio: number): { minor: boolean; major: boolean; opacity: number } {
  const majorPx = minorPx * ratio;
  const minor = minorPx >= GRID_MIN_PX;
  // Fade minor lines in between 6 and 14 px so zooming does not pop.
  const opacity = minor ? Math.min(1, (minorPx - GRID_MIN_PX) / 8 + 0.35) : 0;
  return { minor, major: majorPx >= GRID_MIN_PX, opacity };
}

/**
 * Half-size of the square grid, metres: at least 8 ft (or 2.5 m), and at least twice the
 * model's extent, rounded up to whole major cells.
 */
export function gridHalfExtent(spec: GridSpec, contentRadius: number): number {
  const floor = spec.ratio === 12 ? 8 * 12 * M_PER_IN : 2.5;
  const want = Math.max(floor, 2 * contentRadius);
  return Math.ceil(want / spec.major) * spec.major;
}

/**
 * Line segments of one grid level on z = 0, as flat xyz pairs. `skipEvery` drops lines
 * that the coarser level already draws (every 12th minor line is a major line).
 */
export function gridLines(step: number, half: number, skipEvery = 0): Float32Array {
  const n = Math.round(half / step);
  const out: number[] = [];
  for (let i = -n; i <= n; i++) {
    if (i === 0) continue; // the axes run here
    if (skipEvery > 0 && i % skipEvery === 0) continue;
    const t = i * step;
    out.push(t, -half, 0, t, half, 0, -half, t, 0, half, t, 0);
  }
  return new Float32Array(out);
}

/** Axis colours, the conventional red / green / blue, same values as the mock. */
export const AXIS_COLORS = { x: "#c0392b", y: "#27ae60", z: "#2a63a8" } as const;
