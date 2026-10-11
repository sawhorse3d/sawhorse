// How each inference looks: OUR colours (ADR-0003), never SketchUp's. The four point/face
// colours are the approved mock's (endpoint lime, midpoint teal, on-edge amber, on-face
// violet); the rest extend the same palette. Values live in CSS tokens (styles/tokens.css)
// so light and dark themes can differ; the Rust inference crate names the same tokens.

export type InferenceKind =
  | "endpoint"
  | "midpoint"
  | "on-edge"
  | "on-face"
  | "intersection"
  | "centre"
  | "axis-x"
  | "axis-y"
  | "axis-z"
  | "parallel"
  | "perpendicular"
  | "from-point"
  | "on-guide"
  | "guide-point"
  | "grid"
  | "free";

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
  intersection: { label: "Intersection", token: "--inf-int" },
  centre: { label: "Centre", token: "--inf-ctr" },
  "axis-x": { label: "On Red Axis", token: "--axis-x" },
  "axis-y": { label: "On Green Axis", token: "--axis-y" },
  "axis-z": { label: "On Blue Axis", token: "--axis-z" },
  parallel: { label: "Parallel to Edge", token: "--inf-par" },
  perpendicular: { label: "Perpendicular to Edge", token: "--inf-par" },
  "from-point": { label: "From Point", token: "--inf-from" },
  "on-guide": { label: "On Guide", token: "--inf-guide" },
  "guide-point": { label: "Guide Point", token: "--inf-guide" },
  grid: { label: "Grid", token: "--inf-grid" },
  free: { label: "", token: "--muted" },
};

/** The legend under the viewport shows the mock's four. */
export const LEGEND_KINDS: InferenceKind[] = ["endpoint", "midpoint", "on-edge", "on-face"];

/** Inferences that constrain direction (drawn as a line from the gesture start). */
export function isLineInference(k: InferenceKind): boolean {
  return k === "axis-x" || k === "axis-y" || k === "axis-z" || k === "parallel" || k === "perpendicular" || k === "from-point";
}
