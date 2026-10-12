// Push/Pull (P). Press on a face (or a sketch region) and drag: the face follows the cursor
// along its normal with a live preview, and letting go commits. Or click, move, click.
// Type a distance at any point (it goes the way the cursor went; a negative value flips it);
// with no gesture going, a typed distance applies to the last face or region drawn or
// clicked. Double-click a face to repeat the last distance on it. Hold Ctrl when letting go
// (or clicking) to pull out a new face and leave a seam where the old one was.
//
// The face's normal is a locked inference line, so the cursor also snaps to points of
// other geometry projected onto it (push until level with a neighbour's corner). Pushing in
// past the far side goes through (the kernel's boolean path); refusals come back typed and
// show in the status line.
import type { EditResult } from "../kernel/client";
import type { V3 } from "../kernel/types";
import { describeTarget, pushOp, type Target } from "../lib/edit";
import { INFERENCE_STYLES, type InferenceKind } from "../lib/inferenceStyle";
import { notHere, parseVcb } from "../lib/vcb";
import { add, dot, mul, sub } from "./draw";
import { objectEditable, sketchEditable } from "../lib/scope";
import type { DocState } from "../kernel/types";

/** A face or region the open editing context lets Push/Pull act on. */
export function pushable(t: Target, state: DocState | null): boolean {
  return t.kind === "face" ? objectEditable(state, t.object) : sketchEditable(state, t.sketch);
}

const CLOSED = "That is inside a group or component that is not open: double-click it (Select) to edit inside it.";
import { EMPTY_VIEW, Tool, markerOf, type InferContext, type Snap, type ToolEvent, type ToolView } from "./tool";

export function targetOf(e: { kind: string; object?: number; face?: number; sketch?: number; region?: number } | null): Target | null {
  if (!e) return null;
  if (e.kind === "face" && e.object !== undefined && e.face !== undefined) return { kind: "face", object: e.object, face: e.face };
  if (e.kind === "region" && e.sketch !== undefined && e.region !== undefined) return { kind: "region", sketch: e.sketch, region: e.region };
  return null;
}

interface Gesture {
  target: Target;
  /** Where the face was grabbed. */
  origin: V3;
  /** Unit direction a positive distance moves the face (its outward normal). */
  normal: V3;
  rings: V3[][];
  /** Current signed distance. */
  d: number;
  dragging: boolean;
}

/** Tiny moves (under 0.01 mm) are not a push. */
const MIN_PUSH = 1e-5;

function axisKind(n: V3): InferenceKind {
  const a = n.map(Math.abs);
  if (a[0] > 1 - 1e-12) return "axis-x";
  if (a[1] > 1 - 1e-12) return "axis-y";
  if (a[2] > 1 - 1e-12) return "axis-z";
  return "parallel";
}

export class PushTool extends Tool {
  readonly id = "pushpull" as const;
  private snap: Snap | null = null;
  private g: Gesture | null = null;
  /** The press that started the current gesture (its click does not commit). */
  private fresh = false;
  private ctrl = false;
  /** The last committed signed distance (double-click repeats it). */
  last: number | null = null;

  valueLabel(): string {
    return "Distance";
  }

  busy(): boolean {
    return this.g !== null;
  }

  reset(): void {
    this.g = null;
    this.fresh = false;
  }

  activate(): void {
    // A single selected face or region becomes the target, like picking it.
    const sel = this.host.selection();
    if (sel.length === 1) {
      const t = targetOf(sel[0]);
      if (t) this.host.setTarget(t);
    }
  }

  inferContext(): InferContext {
    const g = this.g;
    if (!g) return {};
    // Snapping along the face's normal: the line is locked, other geometry projects onto it.
    return { start: g.origin, lock: { dir: g.normal, kind: axisKind(g.normal) }, grid: true };
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    this.ctrl = e.ctrl;
    switch (e.type) {
      case "move":
      case "drag": {
        this.snap = e.snap;
        const g = this.g;
        if (g) {
          if (e.type === "drag") g.dragging = true;
          if (e.snap) g.d = dot(sub(e.snap.point, g.origin), g.normal);
          h.showValue(h.fmt(Math.abs(g.d)));
        } else {
          const s = e.picked?.surface;
          const t = s ? targetOf(s.at) : null;
          h.hover(s && t && pushable(t, h.state()) ? (s.at.kind === "face" ? { kind: "face", object: s.at.object, face: s.at.face } : { kind: "region", sketch: s.at.sketch, region: s.at.region }) : null);
        }
        h.changed();
        return;
      }
      case "down": {
        if (this.g) {
          this.fresh = false;
          return;
        }
        const s = e.picked?.surface;
        const t = s ? targetOf(s.at) : null;
        if (!s || !t) {
          this.fresh = false;
          return;
        }
        if (!pushable(t, h.state())) {
          this.fresh = false;
          h.status(CLOSED, true);
          return;
        }
        this.fresh = true;
        h.setTarget(t);
        this.g = { target: t, origin: s.point, normal: s.frame.normal, rings: [], d: 0, dragging: false };
        const g = this.g;
        void h.outline(t).then((rings) => {
          if (this.g === g) {
            g.rings = rings;
            h.changed();
          }
        });
        h.status(`Pushing ${describeTarget(t)}: drag, or move and click, or type a distance. Hold Ctrl to pull out a new face.`);
        h.changed();
        return;
      }
      case "dragend": {
        const g = this.g;
        if (g?.dragging) this.commit(g.d, e.ctrl);
        return;
      }
      case "click": {
        const g = this.g;
        if (!g) {
          h.status("Click a face or a region of a sketch to push or pull it.", false);
          return;
        }
        if (this.fresh) {
          // The press that started the gesture: keep going (click, move, click).
          this.fresh = false;
          return;
        }
        if (Math.abs(g.d) < MIN_PUSH) {
          // A second click without moving (part of a double-click): not a push.
          this.reset();
          h.changed();
          return;
        }
        this.commit(g.d, e.ctrl);
        return;
      }
      case "dblclick": {
        const s = e.picked?.surface;
        const t = s ? targetOf(s.at) : null;
        this.reset();
        h.changed();
        if (!t || this.last === null) return;
        h.setTarget(t);
        this.apply(t, this.last, e.ctrl, "Repeated");
        return;
      }
      case "leave":
        h.hover(null);
        return;
      default:
        return;
    }
  }

  onValue(text: string): void {
    const h = this.host;
    const r = parseVcb(text, h.units());
    if (!r.ok) return void h.status(r.error, true);
    if (r.value.kind !== "length") return void h.status(notHere(r.value, "Push/Pull", "a distance (negative pushes in)"), true);
    const typed = r.value.m;
    const g = this.g;
    if (g) {
      // Mid-gesture: the typed distance goes the way the cursor went.
      // (Typed values go to the kernel as they are, so a zero is refused there, typed.)
      const sign = g.d < 0 ? -1 : 1;
      this.reset();
      this.host.changed();
      this.apply(g.target, sign * typed, this.ctrl);
      return;
    }
    const t = h.target();
    if (!t) return void h.status("Click a face or a region first, then type the distance.", true);
    this.apply(t, typed, false);
  }

  private commit(d: number, ctrl: boolean): void {
    const g = this.g;
    this.reset();
    this.host.changed();
    if (!g || Math.abs(d) < MIN_PUSH) return;
    this.apply(g.target, d, ctrl);
  }

  private apply(t: Target, d: number, ctrl: boolean, verb?: string): void {
    const h = this.host;
    const op = t.kind === "face" && ctrl && d > 0 ? { op: "push_pull_new_face" as const, object: t.object, face: t.face, distance: d } : pushOp(t, d);
    void h
      .apply(op, (res: EditResult) => {
        const s = res.scene.state.stats;
        const what =
          verb ?? (res.outcome.kind === "object_created" ? "Extruded" : res.outcome.kind === "object_collapsed" ? "Collapsed" : d >= 0 ? (op.op === "push_pull_new_face" ? "Pulled a new face" : "Pulled") : "Pushed");
        return `${what} ${h.fmt(Math.abs(d))} ${h.unitLabel()}. ${s.objects} solid(s): ${s.faces} faces, ${s.edges} edges, ${s.vertices} vertices, genus ${s.genus}, ${s.watertight ? "watertight" : "OPEN"}.`;
      })
      .then((ok) => {
        if (ok) {
          this.last = d;
          h.showValue(h.fmt(Math.abs(d)));
        }
      });
  }

  view(): ToolView {
    const g = this.g;
    const marker = markerOf(this.snap);
    if (!g) return { ...EMPTY_VIEW, marker };
    const off = mul(g.normal, g.d);
    const overlay = [];
    const color = g.d < 0 ? "--warn" : "--sel";
    for (const ring of g.rings) {
      overlay.push({ kind: "loop" as const, points: ring.map((p) => add(p, off)), color, width: 2 });
      if (Math.abs(g.d) > MIN_PUSH) overlay.push({ kind: "segments" as const, points: ring.flatMap((p) => [p, add(p, off)]), color, width: 1.25 });
    }
    overlay.push({ kind: "segments" as const, points: [g.origin, add(g.origin, off)], color: INFERENCE_STYLES[axisKind(g.normal)].token, width: 1.25, dashed: true });
    const at = add(g.origin, off);
    return { ...EMPTY_VIEW, marker, overlay, labels: [{ at, text: `${g.d < 0 ? "−" : ""}${this.host.fmt(Math.abs(g.d))}${this.ctrl && g.d > 0 ? " +" : ""}`, className: "live" }] };
  }
}
