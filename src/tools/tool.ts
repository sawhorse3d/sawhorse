// The tool framework. Every tool is a small state machine fed with pointer events that
// already carry what is under the cursor (a pick) and where the cursor snaps (an
// inference), keyboard events, and Value Control Box entries. Tools change the document
// only through the host's `apply` (one kernel op, transactional, undoable) and describe
// what they show (overlay lines, the inference marker, labels) through `view()`.
//
// Tools are plain classes with no React and no three.js, so their logic is unit-tested
// with a fake host (src/tools/*.test.ts).
import type { EditResult } from "../kernel/client";
import type { DocState, Entity, KernelOp, MeshData, Picked, SceneData, SurfaceHit, V3 } from "../kernel/types";
import { INFERENCE_STYLES, type InferenceKind } from "../lib/inferenceStyle";
import type { ToolId } from "../lib/tools";
import type { UnitSystem } from "../lib/units";
import type { Label, Marker, SelectionBox } from "../components/Viewport";
import type { OverlayItem } from "../viewport/engine";
import type { Target } from "../lib/edit";

/** Where the cursor snaps (an inference result; see crates/inference). */
export interface Snap {
  point: V3;
  kind: InferenceKind;
  /** Tooltip text ("Endpoint", "On Red Axis", "From Point" ...). */
  tip: string;
  /** The surface under the cursor, if any. */
  surface: SurfaceHit | null;
  /** The entity the snap came from (edge, vertex, face ...), if any. */
  entity: Entity | null;
  /** Lines to draw for the inference (axis from the start, dotted from-point alignment). */
  lines: { from: V3; to: V3; kind: InferenceKind }[];
}

export type ToolEventType = "move" | "down" | "click" | "dblclick" | "drag" | "dragend" | "leave";

export interface ToolEvent {
  type: ToolEventType;
  /** Canvas CSS pixels. */
  x: number;
  y: number;
  /** For drag / dragend: where the left button went down. */
  start?: [number, number];
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
  ray: { origin: V3; dir: V3 };
  picked: Picked | null;
  snap: Snap | null;
}

/** What a tool wants drawn right now. */
export interface ToolView {
  overlay: OverlayItem[];
  marker: Marker | null;
  labels: Label[];
  box: SelectionBox | null;
  cursor?: string;
}

export const EMPTY_VIEW: ToolView = { overlay: [], marker: null, labels: [], box: null };

/** Constraints a tool puts on inference (see crates/inference `InferQuery`). */
export interface InferContext {
  /** The gesture's start point: axis, parallel and perpendicular inferences run from it. */
  start?: V3;
  /** Points to project: snaps are made on this plane (a face being drawn on). */
  plane?: { origin: V3; normal: V3 } | null;
  /** A locked direction from `start` (arrow keys, Shift-held inference). */
  lock?: { dir: V3; kind: InferenceKind } | null;
  /** Reference edges for parallel / perpendicular inference. */
  refs?: [V3, V3][];
  /** Skip snapping to this object (the solid being pushed). */
  exclude?: number | null;
  /** Allow grid snapping on the ground when nothing else is near (default true). */
  grid?: boolean;
}

/** What tools may use from the app. */
export interface ToolHost {
  units(): UnitSystem;
  /** A length in the display units (exact when the value is, "~" only when rounded). */
  fmt(m: number): string;
  /** The unit label ("in", "mm" ...). */
  unitLabel(): string;
  scene(): SceneData | null;
  mesh(): MeshData | null;
  state(): DocState | null;
  /** Apply one op (a batch for several). Resolves to the result, or null when refused. */
  apply(op: KernelOp, describe?: (r: EditResult) => string): Promise<EditResult | null>;
  /** Status line text (null restores the tool hint). */
  status(text: string | null, error?: boolean): void;
  /** Show a value in the Value Control Box (live measurement or the default). */
  showValue(text: string): void;
  selection(): Entity[];
  setSelection(es: Entity[]): void;
  hover(e: Entity | null): void;
  /** Request a re-render of `view()`. */
  changed(): void;
  /** The group being edited (lane 5 hook), or null for the model. */
  context(): number | null;
  enterGroup(group: number): void;
  /** World point to canvas pixels (for box selection). */
  toScreen(p: V3): [number, number];
  /**
   * What Push/Pull acts on next when nothing is clicked: the region a rectangle just drew,
   * the sub-face an imprint made, the cap of a new solid, the face just pushed.
   */
  target(): Target | null;
  setTarget(t: Target | null): void;
}

export abstract class Tool {
  abstract readonly id: ToolId;
  constructor(protected host: ToolHost) {}

  /** Called when the tool becomes active. */
  activate(): void {}
  /** Called when another tool takes over: drop any gesture. */
  deactivate(): void {
    this.reset();
  }
  /** Forget the current gesture. */
  reset(): void {}
  /** A gesture is in progress (Esc cancels it instead of clearing the selection). */
  busy(): boolean {
    return false;
  }
  onEvent(_e: ToolEvent): void {}
  /** A key the app did not consume. Return true when handled. */
  onKey(_e: KeyboardEvent): boolean {
    return false;
  }
  /** Shift went down or up (some tools lock the current inference while it is held). */
  onShift(_down: boolean): void {}
  /** Enter in the Value Control Box. */
  onValue(_text: string): void {}
  /** Esc. Return true when it cancelled something. */
  cancel(): boolean {
    if (!this.busy()) return false;
    this.reset();
    this.host.changed();
    return true;
  }
  view(): ToolView {
    return EMPTY_VIEW;
  }
  inferContext(): InferContext {
    return {};
  }
  /** The Value Control Box label while this tool is active (null: read-only). */
  valueLabel(): string | null {
    return null;
  }
}

/** A marker for a snap. */
export function markerOf(s: Snap | null): Marker | null {
  if (!s || s.kind === "free") return null;
  return { at: s.point, kind: s.kind, tip: s.tip };
}

/** The overlay lines an inference wants drawn: solid in the axis or parallel colour from
 * the gesture start, dotted for "from point" alignments. */
export function inferenceLine(s: Snap | null): OverlayItem[] {
  if (!s?.lines?.length) return [];
  return s.lines.map((l) => ({
    kind: "segments" as const,
    points: [l.from, l.to],
    color: INFERENCE_STYLES[l.kind].token,
    width: l.kind === "from-point" ? 1.25 : 1.75,
    dashed: l.kind === "from-point",
  }));
}
