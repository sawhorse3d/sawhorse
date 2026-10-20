// Base for the draw tools: snapping state, axis locks, the drawing plane, region hover,
// live labels, and committing a stroke through the kernel's Draw op.
import type { EditResult } from "../kernel/client";
import type { CurveSpec, V3 } from "../kernel/types";
import { drawOp, planeFromSnap, type DrawPlane, type DrawTarget } from "./draw";
import { LockState } from "./lock";
import { EMPTY_VIEW, Tool, inferenceLine, markerOf, type InferContext, type Snap, type ToolEvent, type ToolView } from "./tool";
import type { Label } from "../components/Viewport";
import { INFERENCE_STYLES } from "../lib/inferenceStyle";
import type { OverlayItem } from "../viewport/engine";

export const PLANE_KEYS: Record<string, "z" | "x" | "y"> = { ArrowUp: "z", ArrowRight: "x", ArrowLeft: "y" };

export abstract class StrokeTool extends Tool {
  protected snap: Snap | null = null;
  protected lock = new LockState();
  /** The plane the current shape is drawn on (set by the first click). */
  protected plane: DrawPlane | null = null;
  /** Before the first click, arrow keys choose the plane: horizontal, facing red or green. */
  protected prefer: "auto" | "z" | "x" | "y" = "auto";
  /** Where the cursor is on the drawing plane (or in space for a free line). */
  protected cursor: V3 | null = null;

  /** The gesture's current start point (axis lines run from it), or null before a click. */
  abstract start(): V3 | null;

  reset(): void {
    this.plane = null;
    this.cursor = null;
    this.lock.clear();
  }

  busy(): boolean {
    return this.start() !== null;
  }

  onKey(e: KeyboardEvent): boolean {
    const s = this.start();
    if (!s && PLANE_KEYS[e.key]) {
      const want = PLANE_KEYS[e.key];
      this.prefer = this.prefer === want ? "auto" : want;
      this.host.status(this.prefer === "auto" ? "Drawing plane: the face under the cursor, or level." : `Drawing plane locked: ${this.prefer === "z" ? "level (blue up)" : this.prefer === "x" ? "upright, facing red" : "upright, facing green"}. Press the arrow again to unlock.`);
      return true;
    }
    if (this.lock.onKey(e.key, this.snap, s)) {
      const names: Record<string, string> = { "axis-x": "the red axis", "axis-y": "the green axis", "axis-z": "the blue axis", parallel: "the parallel direction", perpendicular: "the perpendicular direction" };
      this.host.status(this.lock.lock ? `Locked to ${names[this.lock.lock.kind] ?? this.lock.lock.kind}. Press the same arrow to unlock.` : "Unlocked.");
      return true;
    }
    return false;
  }

  onShift(down: boolean): void {
    if (this.lock.onShift(down, this.snap, this.start())) this.host.changed();
  }

  inferContext(): InferContext {
    const s = this.start();
    if (!s) return {};
    const f = this.plane?.frame;
    return { start: s, plane: f ? { origin: f.origin, normal: f.normal } : null, lock: this.lock.lock };
  }

  /** Track the cursor; regions under it highlight (they are what the stroke may close). */
  protected track(e: ToolEvent): void {
    this.snap = e.snap;
    this.cursor = e.snap?.point ?? null;
    const s = e.picked?.surface;
    this.host.hover(s && s.at.kind === "region" ? { kind: "region", sketch: s.at.sketch, region: s.at.region } : null);
  }

  /** The plane a shape starting at this snap goes on. */
  protected planeFor(snap: Snap): DrawPlane {
    return planeFromSnap(snap, this.prefer);
  }

  /** Commit a stroke; `curve` (lane 23) makes it one curve (a circle, an arc, a polygon). */
  protected async stroke(target: DrawTarget, points: V3[], closed: boolean, describe: (r: EditResult) => string, curve?: CurveSpec): Promise<EditResult | null> {
    return this.host.apply(drawOp(target, points, closed, curve), describe);
  }

  protected baseView(extra: OverlayItem[] = [], labels: Label[] = []): ToolView {
    return { ...EMPTY_VIEW, marker: markerOf(this.snap), overlay: [...inferenceLine(this.snap), ...extra], labels };
  }

  /** The rubber band's colour: the axis or parallel colour while on one, else selection blue. */
  protected bandColor(): string {
    const k = this.snap?.kind;
    return k === "axis-x" || k === "axis-y" || k === "axis-z" || k === "parallel" || k === "perpendicular" ? INFERENCE_STYLES[k].token : "--sel";
  }

  /** A live measurement pinned beside the cursor. */
  protected live(text: string): Label[] {
    return this.cursor ? [{ at: this.cursor, text, className: "live" }] : [];
  }
}

export { EMPTY_VIEW };

/** "(imprinted)" or "(cut: N faces)" for a stroke drawn on a solid's face. */
export function faceNote(r: { outcome: { kind: string; faces?: number[]; inside?: number | null } }): string {
  const o = r.outcome;
  if (o.kind !== "drawn" || !o.faces?.length) return "";
  return o.faces.length === 2 && o.inside !== null && o.inside !== undefined ? " (imprinted)" : ` (cut: ${o.faces.length} faces)`;
}
