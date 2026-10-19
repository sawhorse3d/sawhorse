// Move (M). Click a solid (or select what to move first), move the cursor, click again;
// or press, drag and let go. The cursor snaps through the inference engine from the start
// point, so a move from one corner to another is exact; the arrow keys lock red / green /
// blue (Down locks the parallel or perpendicular shown) and Shift holds the line showing.
// Type a distance at any point: it goes the way the cursor went (negative flips it); typed
// after the move, it changes the move.
//
// Ctrl (tap it, or hold it on the last click) copies instead of moving. After a copy, type
// x3 (or 3x, *3) for three copies at one, two and three times the move, or /4 for four
// copies dividing the move into quarters; another array or distance replaces the last one.
// Copies of solids are separate solids; copies of a component instance are instances of
// the same component, and a copied group is a new group (lane 5).
//
// With faces, edges or points of one solid selected, Move moves that part of the solid:
// faces that stay flat follow, and a face that would bend out of its plane is refused,
// typed. Alt toggles autofold (off by default): bent faces then fold along new edges.
//
// Sketch geometry (lane 22): a circle, rectangle, polygon, arc or line not yet pushed into
// a solid moves the same way, selected or clicked (a region, an edge, a point). A point or
// an edge of a shape moves within the shape's plane and the edges beside it follow; a
// whole shape moves anywhere (lifted off its plane, it goes onto the plane it lands on).
// What it lands on welds and splits as drawing there would. Ctrl copies it, and x3 / /4
// make arrays of it, as for solids.
import type { EditResult } from "../kernel/client";
import type { Entity, KernelOp, V3, Xform } from "../kernel/types";
import { INFERENCE_STYLES } from "../lib/inferenceStyle";
import { parseVcb } from "../lib/vcb";
import { dirName, dist, len, mul, sub, unit } from "./draw";
import { LockState } from "./lock";
import { scopeEntity } from "../lib/scope";
import { EMPTY_VIEW, Tool, inferenceLine, markerOf, type InferContext, type Snap, type ToolEvent, type ToolView } from "./tool";
import { describeSubject, geometryOp, isCopied, isSketchEntity, loadOutlines, movedSelection, objectsOp, previewSegments, sketchPreviews, sketchSegments, subjectEdges, subjectOf, translation, type SketchPreview, type Subject } from "./xform";

interface Gesture {
  subject: Subject;
  from: V3;
  to: V3 | null;
  edges: [V3, V3][];
  /** Sketch geometry's preview (lane 22). */
  sk: SketchPreview[];
  dragging: boolean;
}

/** What the last committed move did, so a typed value or an array can redo it. */
interface LastMove {
  subject: Subject;
  delta: V3;
  copy: boolean;
  /** Array count when the last edit was an array (else 1). */
  count: number;
  /** The selection before the move and the sketch previews (lane 22): what a typed
   * distance moves again from, and how the selection follows sketch geometry. */
  sel: Entity[];
  sk: SketchPreview[];
}

/** Under 0.01 mm is not a move. */
const MIN_MOVE = 1e-5;

/** The ops for a move of `subject` by `delta`, as a copy or an array of `count` copies. */
export function moveOp(subject: Subject, delta: V3, copy: boolean, count = 1, autofold = false): KernelOp {
  const xform: Xform = translation(delta);
  if (subject.kind === "geometry") return geometryOp(subject, xform, autofold);
  return objectsOp(subject, xform, copy, count);
}

export class MoveTool extends Tool {
  readonly id = "move" as const;
  private snap: Snap | null = null;
  private g: Gesture | null = null;
  private lock = new LockState();
  /** Ctrl was tapped during this gesture: copy instead of move. */
  private copy = false;
  /** Alt toggles autofold for moving part of a solid. */
  private autofold = false;
  /** The press that started the gesture (its click does not commit). */
  private fresh = false;
  /** The selection the gesture started from (lane 22: sketch geometry's follows it). */
  private sel: Entity[] = [];
  last: LastMove | null = null;

  valueLabel(): string {
    return "Distance";
  }

  busy(): boolean {
    return this.g !== null;
  }

  reset(): void {
    this.g = null;
    this.fresh = false;
    this.copy = false;
    this.lock.clear();
  }

  activate(): void {
    this.host.status(this.hint());
  }

  private hint(): string {
    const sel = this.host.selection();
    return sel.length
      ? "Click a start point, move, click the end (or drag). Ctrl copies; type a distance, then x3 or /4 after a copy."
      : "Click a solid or a sketch shape to pick it up (or select first), move, click again. Ctrl copies.";
  }

  inferContext(): InferContext {
    const g = this.g;
    if (!g) return {};
    return { start: g.from, lock: this.lock.lock, grid: true };
  }

  onKey(e: KeyboardEvent): boolean {
    if (this.lock.onKey(e.key, this.snap, this.g?.from ?? null)) {
      const names: Record<string, string> = { "axis-x": "the red axis", "axis-y": "the green axis", "axis-z": "the blue axis", parallel: "the parallel direction", perpendicular: "the perpendicular direction" };
      this.host.status(this.lock.lock ? `${this.copy ? "Copying" : "Moving"} along ${names[this.lock.lock.kind] ?? this.lock.lock.kind}. Press the same arrow to unlock.` : "Unlocked.");
      return true;
    }
    return false;
  }

  onShift(down: boolean): void {
    if (this.lock.onShift(down, this.snap, this.g?.from ?? null)) this.host.changed();
  }

  onModifier(key: "ctrl" | "alt"): void {
    if (key === "ctrl") {
      this.copy = !this.copy;
      const g = this.g;
      if (g) this.host.status(this.gestureStatus(g.subject));
      else this.host.status(this.copy ? "Copying: the original stays, a copy moves. Tap Ctrl again to move instead." : "Moving (not copying).");
    } else {
      this.autofold = !this.autofold;
      this.host.status(this.autofold ? "Autofold on: faces that would bend fold along new edges." : "Autofold off: a move that would bend a face is refused.");
    }
    this.host.changed();
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    switch (e.type) {
      case "move":
      case "drag": {
        this.snap = e.snap;
        const g = this.g;
        if (g) {
          if (e.type === "drag") g.dragging = true;
          if (e.snap) g.to = e.snap.point;
          if (g.to) h.showValue(h.fmt(dist(g.from, g.to)));
        } else {
          const ent = e.picked?.entity ?? null;
          const o = ent && h.selection().length === 0 ? objectEntity(scopeEntity(ent, h.state())) : null;
          h.hover(o);
        }
        h.changed();
        return;
      }
      case "down": {
        if (this.g) {
          this.fresh = false;
          return;
        }
        this.fresh = true;
        void this.start(e);
        return;
      }
      case "dragend": {
        const g = this.g;
        if (g?.dragging && g.to) this.commit(sub(g.to, g.from), e.ctrl);
        return;
      }
      case "click": {
        const g = this.g;
        if (!g) return;
        if (this.fresh) {
          this.fresh = false;
          return;
        }
        if (!g.to || dist(g.from, g.to) < MIN_MOVE) {
          // A second click without moving: not a move.
          this.reset();
          h.changed();
          return;
        }
        this.commit(sub(g.to, g.from), e.ctrl);
        return;
      }
      case "leave":
        h.hover(null);
        return;
      default:
        return;
    }
  }

  /** Pick up the subject at the press. */
  private async start(e: ToolEvent): Promise<void> {
    const h = this.host;
    if (!e.snap) return;
    const sel = h.selection();
    const picked = e.picked?.entity ?? null;
    const outlines = await loadOutlines((t) => h.outline(t), sel, sel.length ? null : scopeEntity(picked, h.state()));
    const subject = subjectOf(sel, picked, outlines, h.state());
    if (typeof subject === "string") {
      this.fresh = false;
      h.status(subject, true);
      return;
    }
    this.copy = this.copy || e.ctrl;
    this.g = { subject, from: e.snap.point, to: null, edges: subjectEdges(subject, h.mesh()), sk: sketchPreviews(subject, h.mesh()), dragging: false };
    this.sel = sel;
    h.hover(null);
    h.status(this.gestureStatus(subject));
    h.changed();
  }

  /** The status line during a gesture: says plainly whether it moves or copies. */
  gestureStatus(subject: Subject): string {
    if (this.copy && subject.kind === "objects") return `Copying ${describeSubject(subject)}: the original stays. Move and click (or drag), or type a distance. Tap Ctrl to move it instead.`;
    return `Moving ${describeSubject(subject)}: move and click (or drag), or type a distance. ${subject.kind === "objects" ? "Ctrl copies." : "Alt toggles autofold."}`;
  }

  onValue(text: string): void {
    const h = this.host;
    const r = parseVcb(text, h.units());
    if (!r.ok) return void h.status(r.error, true);
    const v = r.value;
    const g = this.g;
    if (v.kind === "length") {
      if (g) {
        const dir = this.direction();
        if (!dir) return void h.status("Move the cursor to show the direction first, then type the distance.", true);
        this.commit(mul(dir, v.m), false);
        return;
      }
      const last = this.last;
      if (!last) return void h.status("Move something first, then type a distance to change the move.", true);
      // Typed after the move: change it.
      const dir = unit(last.delta);
      void this.redo(last, mul(dir, v.m), last.count);
      return;
    }
    if (v.kind === "array" || v.kind === "divide") {
      const last = this.last;
      if (!last || !last.copy || last.subject.kind !== "objects") return void h.status("Arrays follow a copy: Move with Ctrl first, then type x3 or /4.", true);
      const n = v.kind === "array" ? v.copies : v.parts;
      const full = mul(last.delta, last.count);
      const step = v.kind === "array" ? mul(full, 1 / last.count) : mul(full, 1 / n);
      void this.redo(last, step, n);
      return;
    }
    h.status("Move takes a distance, or x3 / /4 after a copy.", true);
  }

  /** The way the cursor went (or the locked axis), unit length. */
  private direction(): V3 | null {
    const g = this.g;
    if (!g) return null;
    if (g.to && dist(g.from, g.to) > 1e-9) return unit(sub(g.to, g.from));
    return this.lock.lock?.dir ?? null;
  }

  /** Undo the last move / copy / array and do it again differently. */
  private async redo(last: LastMove, delta: V3, count: number): Promise<void> {
    const h = this.host;
    const undone = await h.apply({ op: "undo" });
    if (!undone) return;
    await this.apply(last.subject, delta, last.copy, count, last.sel, last.sk);
  }

  private commit(delta: V3, ctrl: boolean): void {
    const g = this.g;
    const copy = this.copy || ctrl;
    this.reset();
    this.host.changed();
    if (!g || len(delta) < MIN_MOVE) return;
    void this.apply(g.subject, delta, copy, 1, this.sel, g.sk);
  }

  private async apply(subject: Subject, delta: V3, copy: boolean, count: number, sel: Entity[] = [], sk: SketchPreview[] = []): Promise<void> {
    const h = this.host;
    const op = moveOp(subject, delta, copy, count, this.autofold);
    // Named before the edit (lane 22): a region lifted onto another plane is another one after it.
    const what = describeSubject(subject);
    const how = `${h.fmt(len(delta))} ${h.unitLabel()} along ${dirName(delta)}`;
    const r = await h.apply(op, (res: EditResult) => {
      const o = res.outcome;
      if (isCopied(o) && count > 1) return `Array: ${count} copies of ${what}, ${how} apart. Type another count, or a distance.`;
      if (isCopied(o)) return `Copied ${what} ${how}. Type x3 for more copies, or /4 to divide the distance.`;
      if (o.kind === "geometry_moved") return `Moved ${what} ${how}${o.faces.length ? ` (${o.faces.length} faces changed)` : ""}.`;
      return `Moved ${what} ${how}.`;
    });
    if (r) {
      this.last = { subject, delta, copy: copy && subject.kind === "objects", count, sel, sk };
      h.showValue(h.fmt(len(delta)));
      // Selected sketch geometry stays selected where it went (lane 22).
      if (!copy && subject.sketches?.length && sel.length) h.setSelection(movedSelection(sel, subject, sk, translation(delta), r.outcome));
    }
  }

  view(): ToolView {
    const g = this.g;
    const marker = markerOf(this.snap);
    if (!g) return { ...EMPTY_VIEW, marker };
    const overlay = [...inferenceLine(this.snap)];
    const label = [];
    if (g.to) {
      const d = sub(g.to, g.from);
      const k = this.snap?.kind;
      const color = k === "axis-x" || k === "axis-y" || k === "axis-z" || k === "parallel" || k === "perpendicular" ? INFERENCE_STYLES[k].token : "--sel";
      overlay.push({ kind: "segments" as const, points: [...previewSegments(g.subject, g.edges, translation(d)), ...sketchSegments(g.sk, translation(d), this.copy)], color: this.copy ? "--ok" : "--sel", width: 1.5 });
      overlay.push({ kind: "segments" as const, points: [g.from, g.to], color, width: 1.75, dashed: true });
      label.push({ at: g.to, text: `${this.host.fmt(len(d))}${this.copy ? " +" : ""}`, className: "live" });
    }
    return { ...EMPTY_VIEW, marker, overlay, labels: label };
  }
}

/** The whole solid (or group or component) an entity belongs to, as a hover target; sketch
 * geometry (a region, a sketch edge or point) is its own. */
function objectEntity(e: Entity | null): Entity | null {
  if (!e) return null;
  if (e.kind === "node" || isSketchEntity(e)) return e;
  if (e.kind === "face") return { kind: "object", object: e.object };
  if ((e.kind === "edge" || e.kind === "vertex") && e.owner.kind === "object") return { kind: "object", object: e.owner.id };
  return null;
}
