// Rotate (Q). The protractor sits on the plane under the cursor (a face's plane, or level
// on the ground); the arrow keys lock it upright facing red (Right), facing green (Left) or
// level (Up), and holding Shift keeps the plane it is on while the cursor moves away.
// Click the centre, click where the angle starts, then click where it ends, or type the
// angle (90, 22.5, -45°). The cursor's angle snaps to the protractor's 15 degree ticks
// when it is near one, the way a drawn length snaps to the grid; a typed angle is exact,
// mid-gesture or after the turn (it then changes the turn).
//
// Ctrl copies; after a copy x3 makes three copies at one, two and three times the angle,
// and /4 divides the angle into four. With faces, edges or points of one solid selected,
// Rotate turns that part of the solid (faces that stay flat follow; a bend is refused).
//
// Sketch geometry (lane 22): circles, rectangles, polygons, arcs and lines not yet pushed
// into a solid turn the same way. The protractor lies on their sketch's plane unless an
// arrow key locks it, so a turn keeps them in it; a whole shape may turn about any other
// axis (it goes onto the plane it lands on), part of one is turned and then projected back
// into its plane. Ctrl copies, x3 and /4 make arrays.
import type { EditResult } from "../kernel/client";
import type { Entity, KernelOp, V3 } from "../kernel/types";
import { INFERENCE_STYLES } from "../lib/inferenceStyle";
import { parseVcb } from "../lib/vcb";
import { add, alignedFrame, dist, dot, mul, sub, unit } from "./draw";
import { EMPTY_VIEW, Tool, inferenceLine, markerOf, type InferContext, type Snap, type ToolEvent, type ToolView } from "./tool";
import { curvePointRefusal, describeSubject, geometryOp, isCopied, loadOutlines, movedSelection, objectsOp, previewSegments, projectToPlane, rotation, signedAngle, sketchPreviews, sketchSegments, subjectEdges, subjectOf, type SketchPreview, type Subject } from "./xform";
import { scopeEntity } from "../lib/scope";
import { curveOf } from "../lib/curves";

const DEG = Math.PI / 180;
/** The protractor's ticks, and how near (in degrees) a cursor angle must be to snap to one. */
export const TICK_DEG = 15;
export const SNAP_DEG = 3;

const AXIS_KEYS: Record<string, { n: V3; name: string; kind: "axis-x" | "axis-y" | "axis-z" }> = {
  ArrowRight: { n: [1, 0, 0], name: "upright, facing red", kind: "axis-x" },
  ArrowLeft: { n: [0, 1, 0], name: "upright, facing green", kind: "axis-y" },
  ArrowUp: { n: [0, 0, 1], name: "level", kind: "axis-z" },
};

/** Snap a cursor angle (radians) to the nearest tick when it is within SNAP_DEG of it. */
export function snapAngle(a: number): number {
  const deg = a / DEG;
  const tick = Math.round(deg / TICK_DEG) * TICK_DEG;
  return Math.abs(deg - tick) <= SNAP_DEG ? tick * DEG : a;
}

/** A typed angle: "90", "22.5", "-45°", "30deg". Null when the text is not an angle. */
export function parseAngle(text: string): number | null {
  const m = /^\s*(-?(?:\d+(?:\.\d*)?|\.\d+))\s*(?:°|deg|degrees?)?\s*$/i.exec(text);
  return m ? Number(m[1]) : null;
}

/** "the blue axis", "the red axis", or "an axis across the face". */
function axisWords(n: V3): string {
  const a = n.map(Math.abs);
  if (a[0] > 1 - 1e-9) return "the red axis";
  if (a[1] > 1 - 1e-9) return "the green axis";
  if (a[2] > 1 - 1e-9) return "the blue axis";
  return "an axis square to the face";
}

/** The op for turning `subject` by `angle` about the axis through `c` along `n`. */
export function rotateOp(subject: Subject, c: V3, n: V3, angle: number, copy: boolean, count = 1): KernelOp {
  const xform = rotation(c, n, angle);
  if (subject.kind === "geometry") return geometryOp(subject, xform, false);
  return objectsOp(subject, xform, copy, count);
}

/** True when only sketch geometry turns (its protractor then lies on the sketch's plane). */
function sketchOnly(s: Subject): boolean {
  return s.kind === "objects" && !s.objects.length && !s.nodes?.length && !s.sections?.length && !!s.sketches?.length;
}

interface Gesture {
  subject: Subject;
  centre: V3;
  normal: V3;
  /** Unit direction of the angle's start, once clicked. */
  ref: V3 | null;
  /** Current angle (radians, signed about `normal`). */
  angle: number;
  /** Where the cursor is on the protractor plane. */
  cursor: V3 | null;
  edges: [V3, V3][];
  /** Sketch geometry's preview (lane 22). */
  sk: SketchPreview[];
  /** The selection the turn started from (sketch geometry's follows it). */
  sel: Entity[];
}

interface LastTurn {
  subject: Subject;
  centre: V3;
  normal: V3;
  angle: number;
  copy: boolean;
  count: number;
  sel: Entity[];
  sk: SketchPreview[];
}

export class RotateTool extends Tool {
  readonly id = "rotate" as const;
  private snap: Snap | null = null;
  private g: Gesture | null = null;
  private axisLock: (typeof AXIS_KEYS)[string] | null = null;
  /** Shift held: the plane under the cursor when it went down. */
  private heldNormal: V3 | null = null;
  /** The protractor's plane while hovering. */
  private hoverNormal: V3 = [0, 0, 1];
  private hoverAt: V3 | null = null;
  private copy = false;
  last: LastTurn | null = null;

  valueLabel(): string {
    return "Angle";
  }

  busy(): boolean {
    return this.g !== null;
  }

  reset(): void {
    this.g = null;
    this.copy = false;
  }

  activate(): void {
    this.host.status("Click the centre of the turn (arrow keys choose the protractor's plane), then the start, then the end; or type an angle.");
  }

  /** The protractor normal for the cursor now: locked axis, held plane, or the surface. */
  private normalFor(snap: Snap | null): V3 {
    if (this.axisLock) return this.axisLock.n;
    if (this.heldNormal) return this.heldNormal;
    const s = snap?.surface ?? snap?.adjacent ?? null;
    return s ? s.frame.normal : [0, 0, 1];
  }

  inferContext(): InferContext {
    const g = this.g;
    if (!g) return {};
    // After the centre, points snap on the protractor's plane (the angle is measured there).
    return { start: g.centre, plane: { origin: g.centre, normal: g.normal } };
  }

  onKey(e: KeyboardEvent): boolean {
    const k = AXIS_KEYS[e.key];
    if (!k) return false;
    if (this.g) {
      this.host.status("The protractor's plane is set at the centre; Esc and start again to change it.", true);
      return true;
    }
    this.axisLock = this.axisLock?.kind === k.kind ? null : k;
    this.host.status(this.axisLock ? `Protractor locked ${this.axisLock.name}. Press the same arrow to unlock.` : "Protractor follows the face under the cursor.");
    this.hoverNormal = this.normalFor(this.snap);
    this.host.changed();
    return true;
  }

  onShift(down: boolean): void {
    if (this.g) return;
    this.heldNormal = down ? this.normalFor(this.snap) : null;
    this.host.changed();
  }

  onModifier(key: "ctrl" | "alt"): void {
    if (key !== "ctrl") return;
    this.copy = !this.copy;
    this.host.status(this.copy ? "Copy: the original stays, a turned copy is made. Tap Ctrl again to turn instead." : "Turn (not copying).");
    this.host.changed();
  }

  /** Where the eye ray (or the snap) meets the protractor plane. */
  private onPlane(e: ToolEvent, g: Gesture): V3 | null {
    if (e.snap && Math.abs(dot(sub(e.snap.point, g.centre), g.normal)) < 1e-7) return e.snap.point;
    const den = dot(e.ray.dir, g.normal);
    if (Math.abs(den) > 1e-12) {
      const t = dot(sub(g.centre, e.ray.origin), g.normal) / den;
      if (t > 0) return add(e.ray.origin, mul(e.ray.dir, t));
    }
    return e.snap ? projectToPlane(e.snap.point, g.centre, g.normal) : null;
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    switch (e.type) {
      case "move":
      case "drag": {
        this.snap = e.snap;
        const g = this.g;
        if (!g) {
          this.hoverNormal = this.normalFor(e.snap);
          this.hoverAt = e.snap?.point ?? null;
        } else {
          g.cursor = this.onPlane(e, g);
          if (g.ref && g.cursor && dist(g.cursor, g.centre) > 1e-9) {
            g.angle = snapAngle(signedAngle(g.ref, unit(sub(g.cursor, g.centre)), g.normal));
            h.showValue(`${fmtDeg(g.angle)}`);
          }
        }
        h.changed();
        return;
      }
      case "click":
        void this.click(e);
        return;
      default:
        return;
    }
  }

  private async click(e: ToolEvent): Promise<void> {
    const h = this.host;
    if (!e.snap) return;
    const g = this.g;
    if (!g) {
      const sel = h.selection();
      // Lane 23: a curve's segment under the cursor is the whole curve.
      const picked = curveOf(e.picked?.entity ?? null, h.mesh());
      const outlines = await loadOutlines((t) => h.outline(t), sel, sel.length ? null : curveOf(scopeEntity(picked, h.state()), h.mesh()));
      const subject = subjectOf(sel, picked, outlines, h.state());
      if (typeof subject === "string") return void h.status(subject, true);
      const refused = curvePointRefusal(subject, sel, picked, h.mesh());
      if (refused) return void h.status(refused, true);
      // Sketch geometry turns on its own plane unless an arrow key or Shift chose another.
      const own = sketchOnly(subject) && !this.axisLock && !this.heldNormal ? subject.sketches?.[0]?.normal : undefined;
      const normal = unit(own ?? this.normalFor(e.snap));
      this.copy = this.copy || e.ctrl;
      this.g = { subject, centre: e.snap.point, normal, ref: null, angle: 0, cursor: null, edges: subjectEdges(subject, h.mesh()), sk: sketchPreviews(subject, h.mesh()), sel };
      h.status(`Turning ${describeSubject(subject)} about ${axisWords(normal)}: click where the angle starts.`);
      h.changed();
      return;
    }
    const p = this.onPlane(e, g);
    if (!g.ref) {
      if (!p || dist(p, g.centre) < 1e-6) return;
      g.ref = unit(sub(p, g.centre));
      g.cursor = p;
      h.status("Now click where the angle ends, or type it (degrees).");
      h.changed();
      return;
    }
    if (Math.abs(g.angle) < 1e-9) return;
    this.commit(g.angle, e.ctrl);
  }

  onValue(text: string): void {
    const h = this.host;
    const deg = parseAngle(text);
    if (deg !== null) {
      const g = this.g;
      if (g) {
        if (!g.ref) g.ref = alignedFrame(g.centre, g.normal).u;
        const sign = g.angle < 0 ? -1 : 1;
        this.commit(sign * deg * DEG, false);
        return;
      }
      const last = this.last;
      if (!last) return void h.status("Turn something first (or click its centre), then type the angle.", true);
      void this.redo(last, deg * DEG * Math.sign(last.angle || 1), last.count);
      return;
    }
    const r = parseVcb(text, h.units());
    if (r.ok && (r.value.kind === "array" || r.value.kind === "divide")) {
      const last = this.last;
      if (!last || !last.copy) return void h.status("Arrays follow a copy: Rotate with Ctrl first, then type x3 or /4.", true);
      const total = last.angle * last.count;
      const n = r.value.kind === "array" ? r.value.copies : r.value.parts;
      const step = r.value.kind === "array" ? last.angle : total / n;
      void this.redo(last, step, n);
      return;
    }
    h.status("Rotate takes an angle in degrees (90, -22.5), or x3 / /4 after a copy.", true);
  }

  private async redo(last: LastTurn, angle: number, count: number): Promise<void> {
    if (!(await this.host.apply({ op: "undo" }))) return;
    await this.apply(last.subject, last.centre, last.normal, angle, last.copy, count, last.sel, last.sk);
  }

  private commit(angle: number, ctrl: boolean): void {
    const g = this.g;
    const copy = (this.copy || ctrl) && g?.subject.kind === "objects";
    this.reset();
    this.host.changed();
    if (!g || Math.abs(angle) < 1e-9) return;
    void this.apply(g.subject, g.centre, g.normal, angle, copy, 1, g.sel, g.sk);
  }

  private async apply(subject: Subject, centre: V3, normal: V3, angle: number, copy: boolean, count: number, sel: Entity[] = [], sk: SketchPreview[] = []): Promise<void> {
    const h = this.host;
    const op = rotateOp(subject, centre, normal, angle, copy, count);
    // Named before the edit (lane 22): a region lifted onto another plane is another one after it.
    const what = describeSubject(subject);
    const how = `${fmtDeg(angle)} about ${axisWords(normal)}`;
    const r = await h.apply(op, (res: EditResult) => {
      const o = res.outcome;
      if (isCopied(o) && count > 1) return `Array: ${count} turned copies of ${what}, ${fmtDeg(angle)} apart. Type another count, or an angle.`;
      if (isCopied(o)) return `Copied ${what} turned ${how}. Type x3 for more copies, or /4 to divide the angle.`;
      return `Turned ${what} ${how}.`;
    });
    if (r) {
      this.last = { subject, centre, normal, angle, copy, count, sel, sk };
      h.showValue(fmtDeg(angle));
      // Selected sketch geometry stays selected where it went (lane 22).
      if (!copy && subject.sketches?.length && sel.length) h.setSelection(movedSelection(sel, subject, sk, rotation(centre, normal, angle), r.outcome));
    }
  }

  view(): ToolView {
    const g = this.g;
    const marker = markerOf(this.snap);
    const centre = g?.centre ?? this.hoverAt;
    const normal = g?.normal ?? this.hoverNormal;
    if (!centre) return { ...EMPTY_VIEW, marker };
    const overlay = [...inferenceLine(this.snap)];
    const r = this.radius(centre);
    const f = alignedFrame(centre, normal);
    const at = (a: number, rr = r) => add(centre, add(mul(f.u, Math.cos(a) * rr), mul(f.v, Math.sin(a) * rr)));
    const a = normal.map(Math.abs);
    const color = a[0] > 1 - 1e-9 ? INFERENCE_STYLES["axis-x"].token : a[1] > 1 - 1e-9 ? INFERENCE_STYLES["axis-y"].token : a[2] > 1 - 1e-9 ? INFERENCE_STYLES["axis-z"].token : "--sel";
    // The protractor: a ring with a tick every 15 degrees (longer every 45).
    overlay.push({ kind: "loop" as const, points: Array.from({ length: 48 }, (_, k) => at((k / 48) * 2 * Math.PI)), color, width: 1.5 });
    const ticks: V3[] = [];
    for (let k = 0; k < 24; k++) {
      const t = k * TICK_DEG * DEG;
      ticks.push(at(t), at(t, r * (k % 3 === 0 ? 0.8 : 0.9)));
    }
    overlay.push({ kind: "segments" as const, points: ticks, color, width: 1 });
    const labels = [];
    if (g?.ref) {
      const start = add(centre, mul(g.ref, r));
      overlay.push({ kind: "segments" as const, points: [centre, start], color: "--sel", width: 1.75 });
      if (Math.abs(g.angle) > 1e-9) {
        const steps = Math.max(2, Math.ceil(Math.abs(g.angle) / (5 * DEG)));
        const arc: V3[] = [];
        for (let k = 0; k <= steps; k++) arc.push(add(centre, mul(rotate(g.ref, g.normal, (g.angle * k) / steps), r * 0.6)));
        overlay.push({ kind: "strip" as const, points: arc, color: "--sel", width: 2 });
        const end = add(centre, mul(rotate(g.ref, g.normal, g.angle), r));
        overlay.push({ kind: "segments" as const, points: [centre, end], color: "--sel", width: 1.75, dashed: true });
        overlay.push({ kind: "segments" as const, points: [...previewSegments(g.subject, g.edges, rotation(centre, g.normal, g.angle)), ...sketchSegments(g.sk, rotation(centre, g.normal, g.angle), this.copy)], color: this.copy ? "--ok" : "--sel", width: 1.5 });
        labels.push({ at: end, text: `${fmtDeg(g.angle)}${this.copy ? " +" : ""}`, className: "live" });
      }
    } else if (g?.cursor) {
      overlay.push({ kind: "segments" as const, points: [centre, g.cursor], color: "--sel", width: 1.25, dashed: true });
    }
    return { ...EMPTY_VIEW, marker, overlay, labels };
  }

  /** About 56 px on screen at the centre (from the host's projection). */
  private radius(c: V3): number {
    const a = this.host.toScreen(c);
    const b = this.host.toScreen(add(c, [0.1, 0, 0]));
    const c2 = this.host.toScreen(add(c, [0, 0.1, 0]));
    const pxPerM = Math.max(Math.hypot(b[0] - a[0], b[1] - a[1]), Math.hypot(c2[0] - a[0], c2[1] - a[1])) / 0.1;
    return pxPerM > 1e-6 ? 56 / pxPerM : 0.1;
  }
}

/** Rodrigues: `v` turned by `a` about unit `n`. */
function rotate(v: V3, n: V3, a: number): V3 {
  const c = Math.cos(a);
  const s = Math.sin(a);
  const k = dot(n, v) * (1 - c);
  return [v[0] * c + (n[1] * v[2] - n[2] * v[1]) * s + n[0] * k, v[1] * c + (n[2] * v[0] - n[0] * v[2]) * s + n[1] * k, v[2] * c + (n[0] * v[1] - n[1] * v[0]) * s + n[2] * k];
}

/** "90°", "-22.5°" (up to three decimals). */
export function fmtDeg(a: number): string {
  const d = Math.round((a / DEG) * 1000) / 1000;
  return `${Object.is(d, -0) ? 0 : d}°`;
}

