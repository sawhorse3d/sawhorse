// Scale (S). Grips sit on the bounding box of what is selected (or of the solid under the
// cursor): eight corners, twelve edge middles and six face centres. Drag a grip, or click
// it, move and click: a face-centre grip stretches one way, an edge grip two ways, a
// corner grip all three, each about the opposite grip. Shift makes the scale uniform (all
// three ways by the same factor); Ctrl scales about the centre instead of the opposite
// grip. Type a factor (2, 0.5, -1 mirrors, or 2,1,0.5 for red, green and blue) or an
// absolute size with a unit (48in, 1200mm) along the grip's main direction; typed after
// the scale, it changes it.
//
// Non-uniform scales are checked by the kernel like every edit: the solid must keep a
// positive volume, flat faces and a closed shell, or the scale is refused, typed. With
// faces, edges or points of one solid selected, Scale scales that part (a tapered leg is
// its bottom face scaled about its centre).
import type { EditResult } from "../kernel/client";
import type { KernelOp, V3 } from "../kernel/types";
import type { Label } from "../components/Viewport";
import { parseNumber, parseLength } from "../lib/units";
import { dist, dot, sub, unit } from "./draw";
import { EMPTY_VIEW, Tool, markerOf, type InferContext, type Snap, type ToolEvent, type ToolView } from "./tool";
import { applyX, boundsOf, coveredSolids, describeSubject, facesToOutline, objectsOp, previewSegments, scaling, subjectEdges, subjectOf, type Subject } from "./xform";

/** How close (px) the cursor must be to a grip to take it. */
export const GRIP_PX = 12;

export interface Grip {
  /** -1 at the low side, 1 at the high side, 0 in the middle, per axis. */
  code: [number, number, number];
  at: V3;
}

/** The grips of a box: every combination of low / middle / high but the centre; a flat
 * axis (no extent) only has its middle. */
export function grips(lo: V3, hi: V3): Grip[] {
  const out: Grip[] = [];
  const opts = (k: number) => (hi[k] - lo[k] > 1e-9 ? [-1, 0, 1] : [0]);
  for (const x of opts(0))
    for (const y of opts(1))
      for (const z of opts(2)) {
        if (x === 0 && y === 0 && z === 0) continue;
        const code: [number, number, number] = [x, y, z];
        out.push({ code, at: code.map((c, k) => (c < 0 ? lo[k] : c > 0 ? hi[k] : (lo[k] + hi[k]) / 2)) as V3 });
      }
  return out;
}

/** The point a grip scales about: the opposite grip, or the centre. */
export function fixedPoint(g: Grip, lo: V3, hi: V3, aboutCentre: boolean): V3 {
  return g.code.map((c, k) => (aboutCentre || c === 0 ? (lo[k] + hi[k]) / 2 : c < 0 ? hi[k] : lo[k])) as V3;
}

/** Per-axis factors: `t` on the grip's axes (all three when uniform), 1 elsewhere. */
export function factorsFor(g: Grip, t: number, uniform: boolean): V3 {
  return g.code.map((c) => (uniform || c !== 0 ? t : 1)) as V3;
}

/** "a corner grip", "an edge grip", "a face grip". */
function gripName(g: Grip): string {
  const n = g.code.filter((c) => c !== 0).length;
  return n === 3 ? "a corner grip" : n === 2 ? "an edge grip" : "a face grip";
}

/** "x2 along red and green", "x0.6 uniform". */
function factorWords(f: V3): string {
  const r = (x: number) => Math.round(x * 10000) / 10000;
  if (f.every((x) => Math.abs(x - f[0]) < 1e-12)) return `×${r(f[0])} uniform`;
  const names = ["red", "green", "blue"];
  const changed = [0, 1, 2].filter((k) => Math.abs(f[k] - 1) > 1e-12);
  const same = changed.every((k) => Math.abs(f[k] - f[changed[0]]) < 1e-12);
  if (same) return `×${r(f[changed[0]])} along ${changed.map((k) => names[k]).join(" and ")}`;
  return changed.map((k) => `×${r(f[k])} ${names[k]}`).join(", ");
}

/** A typed scale: factors (a bare number, or three), or an absolute size (a unit). */
export type TypedScale = { kind: "factor"; t: number } | { kind: "factors"; f: V3 } | { kind: "size"; m: number } | { kind: "error"; text: string };

export function parseScale(text: string, units: Parameters<typeof parseLength>[1]): TypedScale {
  const s = text.trim();
  const num = (x: string): number | null => {
    const t = x.trim();
    const neg = t.startsWith("-");
    const n = parseNumber(neg ? t.slice(1) : t);
    return n === null ? null : neg ? -n : n;
  };
  if (s.includes(",")) {
    const parts = s.split(",").map(num);
    if (parts.length === 3 && parts.every((x) => x !== null)) return { kind: "factors", f: parts as V3 };
    return { kind: "error", text: "Type one factor (2), three (2,1,0.5), or a size with a unit (48in)." };
  }
  const n = num(s);
  if (n !== null) return { kind: "factor", t: n };
  const l = parseLength(s, units);
  if (l.ok) return { kind: "size", m: l.metres };
  return { kind: "error", text: l.error };
}

/** The op for scaling `subject` by `f` about `c`. */
export function scaleOp(subject: Subject, c: V3, f: V3): KernelOp {
  const xform = scaling(c, f);
  if (subject.kind === "geometry") return { op: "transform_geometry", object: subject.object, points: subject.points, xform, autofold: false };
  return objectsOp(subject, xform, false);
}

interface Target {
  subject: Subject;
  lo: V3;
  hi: V3;
  edges: [V3, V3][];
}

interface Gesture extends Target {
  grip: Grip;
  /** Current per-axis factors. */
  f: V3;
  dragging: boolean;
}

export class ScaleTool extends Tool {
  readonly id = "scale" as const;
  private snap: Snap | null = null;
  /** What the grips are on (the selection, or the solid last under the cursor). */
  private target: Target | null = null;
  private hot: Grip | null = null;
  private g: Gesture | null = null;
  private fresh = false;
  private uniform = false;
  private aboutCentre = false;
  private last: { target: Target; grip: Grip; f: V3; aboutCentre: boolean } | null = null;

  valueLabel(): string {
    return "Scale";
  }

  busy(): boolean {
    return this.g !== null;
  }

  reset(): void {
    this.g = null;
    this.fresh = false;
  }

  activate(): void {
    this.target = null;
    this.hot = null;
    void this.refreshTarget(null);
    this.host.status("Drag a grip (or click it, move, click). Shift: uniform. Ctrl: about the centre. Type 2, or 48in.");
  }

  inferContext(): InferContext {
    const g = this.g;
    if (!g) return {};
    const f = fixedPoint(g.grip, g.lo, g.hi, this.aboutCentre);
    const d = sub(g.grip.at, f);
    if (Math.hypot(...d) < 1e-12) return {};
    const dir = unit(d);
    const a = dir.map(Math.abs);
    const kind = a[0] > 1 - 1e-12 ? "axis-x" : a[1] > 1 - 1e-12 ? "axis-y" : a[2] > 1 - 1e-12 ? "axis-z" : "parallel";
    return { start: g.grip.at, lock: { dir, kind }, grid: false };
  }

  onShift(down: boolean): void {
    this.uniform = down;
    this.recompute();
    this.host.changed();
  }

  onModifier(key: "ctrl" | "alt"): void {
    if (key !== "ctrl") return;
    this.aboutCentre = !this.aboutCentre;
    this.host.status(this.aboutCentre ? "Scaling about the centre. Tap Ctrl again for the opposite grip." : "Scaling about the opposite grip.");
    this.recompute();
    this.host.changed();
  }

  /** The grips' subject: the selection, or the solid under the cursor (sticky). */
  private async refreshTarget(picked: ToolEvent["picked"]): Promise<void> {
    const h = this.host;
    const sel = h.selection();
    const outlines = new Map<string, V3[][]>();
    for (const f of facesToOutline(sel)) outlines.set(`${f.object}:${f.face}`, await h.outline({ kind: "face", object: f.object, face: f.face }));
    const subject = subjectOf(sel, picked?.entity ?? null, outlines, h.state());
    if (typeof subject === "string") return;
    if (this.target && sel.length === 0 && JSON.stringify(this.target.subject) === JSON.stringify(subject)) return;
    const b = this.bounds(subject);
    if (!b) return;
    this.target = { subject, lo: b[0], hi: b[1], edges: subjectEdges(subject, h.mesh()) };
    h.changed();
  }

  private bounds(s: Subject): [V3, V3] | null {
    if (s.kind === "geometry") return boundsOf(s.points);
    const solids = this.host.state()?.solids ?? [];
    const covered = coveredSolids(s);
    const pts = solids.filter((x) => covered.includes(x.id)).flatMap((x) => [x.min, x.max]);
    return boundsOf(pts);
  }

  private nearestGrip(x: number, y: number): Grip | null {
    const t = this.target;
    if (!t) return null;
    let best: Grip | null = null;
    let bd = GRIP_PX;
    for (const g of grips(t.lo, t.hi)) {
      const [sx, sy] = this.host.toScreen(g.at);
      const d = Math.hypot(sx - x, sy - y);
      if (d <= bd) {
        bd = d;
        best = g;
      }
    }
    return best;
  }

  /** Factor along the grip's line for the cursor at `p`. */
  private factorAt(g: Gesture, p: V3): number {
    const f = fixedPoint(g.grip, g.lo, g.hi, this.aboutCentre);
    const d = sub(g.grip.at, f);
    const l2 = dot(d, d);
    return l2 > 1e-18 ? dot(sub(p, f), d) / l2 : 1;
  }

  private cursorFactor = 1;

  private recompute(): void {
    const g = this.g;
    if (!g) return;
    g.f = factorsFor(g.grip, this.cursorFactor, this.uniform);
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
          if (e.snap) {
            this.cursorFactor = this.factorAt(g, e.snap.point);
            this.recompute();
            h.showValue(String(Math.round(this.cursorFactor * 10000) / 10000));
          }
        } else {
          if (h.selection().length === 0 && e.picked) void this.refreshTarget(e.picked);
          const hot = this.nearestGrip(e.x, e.y);
          if (hot !== this.hot) {
            this.hot = hot;
            if (hot) h.status(`${gripName(hot)[0].toUpperCase()}${gripName(hot).slice(1)}: drag it, or click it and move. Shift: uniform. Ctrl: about the centre.`);
          }
        }
        h.changed();
        return;
      }
      case "down": {
        if (this.g) {
          this.fresh = false;
          return;
        }
        const grip = this.nearestGrip(e.x, e.y);
        const t = this.target;
        if (!grip || !t) {
          this.fresh = false;
          h.status("Press on a grip (the small squares on the box around the selection).", true);
          return;
        }
        this.fresh = true;
        this.cursorFactor = 1;
        this.g = { ...t, grip, f: [1, 1, 1], dragging: false };
        h.status(`Scaling ${describeSubject(t.subject)} from ${gripName(grip)}: move, then click (or let go), or type a factor or a size.`);
        h.changed();
        return;
      }
      case "dragend": {
        if (this.g?.dragging) this.commit(this.g.f);
        return;
      }
      case "click": {
        const g = this.g;
        if (!g) return;
        if (this.fresh) {
          this.fresh = false;
          return;
        }
        if (g.f.every((x) => Math.abs(x - 1) < 1e-9)) {
          this.reset();
          h.changed();
          return;
        }
        this.commit(g.f);
        return;
      }
      default:
        return;
    }
  }

  onValue(text: string): void {
    const h = this.host;
    const v = parseScale(text, h.units());
    if (v.kind === "error") return void h.status(v.text, true);
    const g = this.g;
    const base = g ?? (this.last ? { ...this.last.target, grip: this.last.grip } : null);
    const grip = g?.grip ?? this.last?.grip ?? null;
    let f: V3;
    if (v.kind === "factors") f = v.f;
    else {
      let t: number;
      if (v.kind === "factor") t = v.t;
      else {
        if (!base || !grip) return void h.status("Pick a grip first: the size is measured along it.", true);
        // The grip's main direction: its scaled axis with the largest extent.
        const axes = [0, 1, 2].filter((k) => grip.code[k] !== 0);
        const k = axes.reduce((a, b) => (base.hi[b] - base.lo[b] > base.hi[a] - base.lo[a] ? b : a), axes[0]);
        const extent = base.hi[k] - base.lo[k];
        if (!(extent > 1e-12)) return void h.status("That grip has no length to scale.", true);
        t = v.m / extent;
      }
      if (!grip) {
        // No grip yet: uniform about the centre of the selection.
        const tg = this.target;
        if (!tg) return void h.status("Select something (or hover a solid) to scale first.", true);
        f = [t, t, t];
        void this.apply(tg, { code: [1, 1, 1], at: tg.hi }, f, true, false);
        return;
      }
      f = factorsFor(grip, t, this.uniform);
    }
    if (g) {
      this.commit(f);
      return;
    }
    const last = this.last;
    if (!last) return void h.status("Pick a grip first, then type the factor.", true);
    void this.apply(last.target, last.grip, f, last.aboutCentre, true);
  }

  private commit(f: V3): void {
    const g = this.g;
    this.reset();
    this.host.changed();
    if (!g) return;
    void this.apply(g, g.grip, f, this.aboutCentre, false);
  }

  private async apply(t: Target, grip: Grip, f: V3, aboutCentre: boolean, again: boolean): Promise<void> {
    const h = this.host;
    if (f.some((x) => Math.abs(x) < 1e-9)) return void h.status("A scale of zero would flatten it.", true);
    if (again && !(await h.apply({ op: "undo" }))) return;
    const c = fixedPoint(grip, t.lo, t.hi, aboutCentre);
    const r = await h.apply(scaleOp(t.subject, c, f), (_res: EditResult) => `Scaled ${describeSubject(t.subject)} ${factorWords(f)} about ${aboutCentre ? "its centre" : "the opposite grip"}.`);
    if (r) {
      this.last = { target: t, grip, f, aboutCentre };
      // The box shows what was applied (the largest change), not the cursor's last reading.
      const shown = f.reduce((a, b) => (Math.abs(b - 1) > Math.abs(a - 1) ? b : a), 1);
      h.showValue(String(Math.round(shown * 10000) / 10000));
      // The grips move to the new bounds of the same subject.
      const x = scaling(c, f);
      const subject: Subject = t.subject.kind === "geometry" ? { ...t.subject, points: t.subject.points.map((p) => applyX(x, p)) } : t.subject;
      const b = this.bounds(subject);
      this.target = b ? { subject, lo: b[0], hi: b[1], edges: subjectEdges(subject, h.mesh()) } : null;
      h.changed();
    }
  }

  view(): ToolView {
    const marker = markerOf(this.snap);
    const t = this.g ?? this.target;
    if (!t) return { ...EMPTY_VIEW, marker };
    const overlay = [];
    const labels: Label[] = [];
    const box = (lo: V3, hi: V3) => {
      const c = (x: number, y: number, z: number): V3 => [x ? hi[0] : lo[0], y ? hi[1] : lo[1], z ? hi[2] : lo[2]];
      const pts: V3[] = [];
      for (const [a, b] of [
        [c(0, 0, 0), c(1, 0, 0)], [c(1, 0, 0), c(1, 1, 0)], [c(1, 1, 0), c(0, 1, 0)], [c(0, 1, 0), c(0, 0, 0)],
        [c(0, 0, 1), c(1, 0, 1)], [c(1, 0, 1), c(1, 1, 1)], [c(1, 1, 1), c(0, 1, 1)], [c(0, 1, 1), c(0, 0, 1)],
        [c(0, 0, 0), c(0, 0, 1)], [c(1, 0, 0), c(1, 0, 1)], [c(1, 1, 0), c(1, 1, 1)], [c(0, 1, 0), c(0, 1, 1)],
      ] as [V3, V3][]) {
        if (dist(a, b) > 1e-12) pts.push(a, b);
      }
      return pts;
    };
    overlay.push({ kind: "segments" as const, points: box(t.lo, t.hi), color: "--sel", width: 1, dashed: true });
    const g = this.g;
    for (const gr of grips(t.lo, t.hi)) {
      const hot = (g && g.grip.at === gr.at) || (!g && this.hot && dist(this.hot.at, gr.at) < 1e-12);
      labels.push({ at: gr.at, text: "", className: hot ? "grip hot" : "grip" });
    }
    if (g) {
      const c = fixedPoint(g.grip, g.lo, g.hi, this.aboutCentre);
      const x = scaling(c, g.f);
      overlay.push({ kind: "segments" as const, points: previewSegments(g.subject, g.edges, x), color: "--sel", width: 1.5 });
      labels.push({ at: g.grip.at, text: factorWords(g.f), className: "live" });
      labels.push({ at: c, text: "", className: "grip fixed" });
    }
    return { ...EMPTY_VIEW, marker, overlay, labels };
  }
}
