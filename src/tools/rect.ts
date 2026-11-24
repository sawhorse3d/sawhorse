// Rectangle (R): click two corners, or one corner and type "width, depth" (a negative size
// goes the other way; without a click the rectangle starts at the origin). Press R again
// for the Rotated Rectangle: click the two ends of one edge, then its width.
//
// The plane is the face or region under the first corner (or level; arrow keys before the
// first click choose level / upright facing red / upright facing green). On a solid's face
// the rectangle cuts the face where it crosses its edges (a dado strip across a board) or
// imprints inside it; a rectangle reaching off that solid (a top board from one side's
// corner to the other's) becomes a new sketch on the plane instead. Started on a corner or
// an edge, the plane follows the cursor until the second click: over a face that holds the
// first corner, the rectangle goes on that face, and a guide line that leaves the face does
// not pull the second corner off it (#130).
import type { EditResult } from "../kernel/client";
import type { V3 } from "../kernel/types";
import { notHere, parseVcb } from "../lib/vcb";
import { GROUND, type Frame } from "../lib/edit";
import { INFERENCE_STYLES, isLineInference } from "../lib/inferenceStyle";
import { add, dirName, dist, dot, fmtPoint, horizontalAt, local, mul, onPlane, planeAt, planeFromSnap, rectangle, rectSides, rotatedRectangle, rotatedWidth, sub, unit, world, type DrawPlane } from "./draw";
import { StrokeTool, faceNote } from "./stroke";
import { finalTarget } from "./target";
import type { Snap, ToolEvent, ToolView } from "./tool";

type Variant = "rect" | "rotated";

/** The kernel's smallest side: it refuses a shape "under 0.01 mm" (crates/kernel error.rs). */
const KERNEL_MIN = 1e-5;

/** #130: added after the kernel's own reason when typed sizes were refused with the cursor on
 * the first corner, on a line through it or not there at all, so the way they ran was a guess. */
export const GUESSED_WAY = "The cursor did not show which way the rectangle runs, so that was a guess: move the cursor onto the face to choose it.";

/**
 * #130: why a rectangle from `a` to `p` on frame `f` has no area, or null when it has one.
 * The kernel refuses such a shape as "too small"; these words say what happened and the
 * way on. Both corners on one line along a side is what a click on an edge through the
 * first corner gives.
 */
export function flatWords(f: Frame, a: V3, p: V3): string | null {
  const [la, lb] = [local(f, a), local(f, p)];
  const wide = Math.abs(lb[0] - la[0]) > 1e-9;
  const deep = Math.abs(lb[1] - la[1]) > 1e-9;
  if (wide && deep) return null;
  if (!wide && !deep) return "On the drawing plane the second corner is on the first. Click the opposite corner, or type width, depth.";
  return "On the drawing plane both corners are on one line, so the rectangle has no width. Click off the line, or type width, depth.";
}

export class RectTool extends StrokeTool {
  readonly id = "rect" as const;
  private variant: Variant = "rect";
  private anchor: V3 | null = null;
  /** Rotated: the far end of the first edge, once clicked. */
  private edgeEnd: V3 | null = null;
  private snaps: Snap[] = [];
  /**
   * Started on a corner, an edge or the grid rather than inside a face: the plane follows
   * the cursor (the face the cursor is on, if the corner is on it too; else the level or
   * upright plane through the corner that the cursor's point lies on; else level).
   */
  private undecided = false;
  /** The snap before `faceFirst` replaced it: the Down arrow and Shift lock to its line. */
  private raw: Snap | null = null;
  /** Bumped by every reset (Esc, a tool switch, a finished shape): a refusal that comes back
   * after one does not bring the first corner back. */
  private epoch = 0;
  /** Typed sizes are on their way to the kernel and the first corner may come back. */
  private inFlight = false;

  start(): V3 | null {
    return this.edgeEnd ?? this.anchor;
  }

  reset(): void {
    super.reset();
    this.anchor = null;
    this.edgeEnd = null;
    this.snaps = [];
    this.undecided = false;
    this.raw = null;
    this.inFlight = false;
    this.epoch++;
  }

  /** Esc while typed sizes are on their way: the first corner does not come back. */
  cancel(): boolean {
    if (!this.inFlight) return super.cancel();
    this.reset();
    this.host.changed();
    return true;
  }

  /** The Down arrow and Shift lock to the guide line the engine found, as before #130's face
   * rule hid it: the lock then shows the line, and the click follows it. */
  onKey(e: KeyboardEvent): boolean {
    return this.withRaw(() => super.onKey(e));
  }

  onShift(down: boolean): void {
    this.withRaw(() => super.onShift(down));
  }

  private withRaw<T>(f: () => T): T {
    const shown = this.snap;
    if (this.raw) this.snap = this.raw;
    try {
      return f();
    } finally {
      this.snap = shown;
    }
  }

  inferContext() {
    const ctx = super.inferContext();
    // While the plane is open, snap in 3D (the cursor may be choosing an upright plane).
    if (this.undecided && !this.edgeEnd) ctx.plane = null;
    // In a 3D view the blue axis through the first corner lies over a square's diagonal and
    // took the far corner, standing the rectangle up with no width (issue #9). Skip it while
    // the plane is open. Red and green stay (they say the rectangle is square to an axis),
    // and on a set plane the line that would leave it is already dropped.
    if (this.variant === "rect" && this.anchor && this.undecided && !this.edgeEnd) ctx.skipStartLines = [[0, 0, 1]];
    return ctx;
  }

  /**
   * #130: while the plane is open and the cursor is over a solid's face that holds the first
   * corner, the rectangle goes on that face. A guide line (an axis line through the first
   * corner, a From Point line of a remembered point, parallel or perpendicular) whose point
   * leaves the face's plane gives way to the point on the face under the cursor: such a line
   * can run through the air or behind the solid, and took the corner off the face onto a new
   * sketch beside it. Every snap that lies on the face's plane is kept (the face's corners,
   * edges and midpoints, guide lines in the plane), and so is a real point or edge of other
   * geometry off the plane, which the person sees and may mean. A lock already set (arrow
   * keys, Shift) is the person's own choice and is kept too; the Down arrow and Shift lock to
   * the line this hides (`onKey`, `onShift` read the snap from before it).
   */
  private faceFirst(snap: Snap | null): Snap | null {
    const a = this.anchor;
    if (!snap || !a || !this.undecided || this.edgeEnd || this.lock.lock) return snap;
    const s = snap.surface;
    if (!s || s.at.kind !== "face" || !onPlane(s.frame, a) || onPlane(s.frame, snap.point)) return snap;
    if (!isLineInference(snap.kind)) return snap;
    const n = s.frame.normal;
    const point = sub(s.point, mul(n, dot(sub(s.point, s.frame.origin), n)));
    const { object, face } = s.at;
    return { point, kind: "on-face", tip: INFERENCE_STYLES["on-face"].label, surface: s, adjacent: null, entity: { kind: "face", object, face }, lines: [] };
  }

  /** The plane a rectangle started at the anchor takes for the cursor at `snap`. */
  private resolvePlane(snap: Snap | null, ray: { origin: V3; dir: V3 }): { plane: DrawPlane; point: V3 | null } {
    const a = this.anchor!;
    const start = this.snaps[0];
    if (snap) {
      const p = snap.point;
      // The face or region the cursor is on, when the corner lies on its plane too.
      for (const s of [snap.surface, snap.adjacent]) {
        if (s && onPlane(s.frame, a) && onPlane(s.frame, p)) return { plane: planeFromSnap({ ...snap, surface: s, kind: "on-face" }), point: p };
      }
      // The start's own face, when the cursor's point lies on it.
      for (const s of [start?.surface, start?.adjacent]) {
        if (s && onPlane(s.frame, p) && onPlane(s.frame, a)) return { plane: planeFromSnap({ ...start, surface: s, kind: "on-face" }), point: p };
      }
      // A level or upright plane through the corner that holds the cursor's point.
      for (const n of [[0, 0, 1], [1, 0, 0], [0, 1, 0]] as V3[]) {
        if (Math.abs(dot(sub(p, a), n)) <= 1e-7 && dist(p, a) > 1e-9) return { plane: n[2] ? horizontalAt(a) : planeAt(a, n), point: p };
      }
    }
    // Otherwise level, where the eye ray meets it.
    const plane = horizontalAt(a);
    const den = ray.dir[2];
    const t = Math.abs(den) > 1e-12 ? (a[2] - ray.origin[2]) / den : -1;
    return { plane, point: t > 0 ? add(ray.origin, mul(ray.dir, t)) : null };
  }

  valueLabel(): string {
    if (this.variant === "rotated") return this.edgeEnd ? "Width" : this.anchor ? "Length" : "Dimensions";
    return "Dimensions";
  }

  cycleVariant(): string | null {
    this.reset();
    this.variant = this.variant === "rect" ? "rotated" : "rect";
    this.host.changed();
    return this.variantHint();
  }

  variantHint(): string {
    return this.variant === "rect"
      ? "Rectangle: click two corners, or type width, depth. R again: Rotated Rectangle."
      : "Rotated Rectangle: click both ends of one edge, then the width. R again: Rectangle.";
  }

  onEvent(ev: ToolEvent): void {
    // The preview (marker, guide lines, rubber band) and the click see the same snap.
    const e = ev.snap ? { ...ev, snap: this.faceFirst(ev.snap) } : ev;
    if (e.type === "move" || e.type === "drag") {
      this.track(e);
      this.raw = e.snap !== ev.snap ? ev.snap : null;
      if (this.anchor && this.undecided && !this.edgeEnd) {
        const r = this.resolvePlane(e.snap, e.ray);
        this.plane = r.plane;
        this.cursor = r.point;
      }
      this.liveValue();
      this.host.changed();
      return;
    }
    if (this.dropped(e)) return;
    if (e.type === "click" && e.snap) {
      let snap = e.snap;
      let plane = this.plane;
      const open = !!this.anchor && this.undecided && !this.edgeEnd;
      if (open) {
        const r = this.resolvePlane(e.snap, e.ray);
        plane = r.plane;
        if (r.point && !onPlane(r.plane.frame, e.snap.point)) snap = { ...e.snap, point: r.point, kind: "free", entity: null, surface: null, adjacent: null };
      }
      // #130: both corners on one line along the rectangle's sides draw nothing; say so
      // before the kernel does, and keep the first corner (and an open plane open).
      if (this.variant === "rect" && this.anchor && plane) {
        const why = flatWords(plane.frame, this.anchor, snap.point);
        if (why) return void this.host.status(why, true);
      }
      if (open && plane) {
        this.plane = plane;
        this.undecided = false;
      }
      void this.click(snap);
    }
  }

  protected stepHint(): string | null {
    if (!this.anchor) return null;
    if (this.variant === "rect") return "Click the opposite corner, or type width, depth.";
    return this.edgeEnd ? "Now the width: click, or type it." : "Click the other end of the edge, or type its length.";
  }

  private liveValue(): void {
    const h = this.host;
    const f = this.plane?.frame;
    if (!f || !this.anchor || !this.cursor) return;
    if (this.variant === "rotated") {
      if (this.edgeEnd) h.showValue(h.fmt(Math.abs(rotatedWidth(f, this.anchor, this.edgeEnd, this.cursor))));
      else h.showValue(h.fmt(dist(this.anchor, this.cursor)));
      return;
    }
    const [a, b] = [local(f, this.anchor), local(f, this.cursor)];
    h.showValue(`${h.fmt(Math.abs(b[0] - a[0]))}, ${h.fmt(Math.abs(b[1] - a[1]))}`);
  }

  private async click(snap: Snap): Promise<void> {
    const h = this.host;
    if (!this.anchor) {
      // A new start: typed sizes still on their way do not bring their corner back over it.
      this.epoch++;
      this.inFlight = false;
      this.plane = this.planeFor(snap);
      this.anchor = snap.point;
      this.snaps = [snap];
      // Inside a face (or with an arrow-key plane) the plane is set; otherwise the cursor
      // chooses it.
      this.undecided = snap.kind !== "on-face" && this.prefer === "auto";
      h.status(`First corner set ${this.plane.where(h.fmt)}. ${this.stepHint()}`);
      h.changed();
      return;
    }
    if (this.variant === "rotated" && !this.edgeEnd) {
      if (dist(snap.point, this.anchor) < 1e-9) return;
      this.edgeEnd = snap.point;
      this.snaps.push(snap);
      h.status(this.stepHint() ?? "");
      h.changed();
      return;
    }
    this.snaps.push(snap);
    await this.finish(snap.point);
  }

  /** Draw with the cursor (or typed) far corner / width point. */
  private async finish(p: V3, size?: [number, number], width?: number, aimless = false): Promise<void> {
    const h = this.host;
    const plane = this.plane;
    const a = this.anchor;
    if (!plane || !a) return;
    const f = plane.frame;
    let pts: V3[];
    let what: string;
    let extents = "";
    if (this.variant === "rotated") {
      const e = this.edgeEnd;
      if (!e) return;
      const w = width ?? rotatedWidth(f, a, e, p);
      pts = rotatedRectangle(f, a, e, w);
      what = `Rotated rectangle ${h.fmt(dist(a, e))} × ${h.fmt(Math.abs(w))} ${h.unitLabel()}`;
    } else {
      const la = local(f, a);
      const lb: [number, number] = size ? [la[0] + size[0], la[1] + size[1]] : local(f, p);
      pts = rectangle(f, la, lb);
      what = `Rectangle ${h.fmt(Math.abs(lb[0] - la[0]))} × ${h.fmt(Math.abs(lb[1] - la[1]))} ${h.unitLabel()}`;
      // Say which way it went: each side's length and direction, from the first corner.
      const s = rectSides(f, la, lb);
      extents = `: ${h.fmt(s.u.len)} ${h.unitLabel()} along ${dirName(s.u.dir)} and ${h.fmt(s.v.len)} ${h.unitLabel()} along ${dirName(s.v.dir)} from ${fmtPoint(a, h.fmt)}`;
    }
    const target = finalTarget(plane, this.snaps);
    const kept = aimless ? { anchor: a, snaps: this.snaps, plane } : null;
    this.reset();
    this.inFlight = !!kept;
    const epoch = this.epoch;
    h.changed();
    const done = await this.stroke(target.target, pts, true, (r: EditResult) => `${what} ${target.where(h.fmt)}${faceNote(r)}${extents}. Push/Pull (P) and type a distance to raise it.`);
    if (this.epoch !== epoch || this.anchor) return; // Esc, a tool switch or a new start came first.
    this.inFlight = false;
    if (done || !kept) return;
    // Refused with the way a guess: the first corner stays and the plane stays open. The
    // kernel's own reason stays on the status line; when every typed size is one the kernel
    // takes (not too small), a sentence after it says the way was guessed.
    this.anchor = kept.anchor;
    this.snaps = kept.snaps;
    this.plane = kept.plane;
    this.undecided = true;
    const why = h.lastError();
    if (why && size && Math.abs(size[0]) > KERNEL_MIN && Math.abs(size[1]) > KERNEL_MIN) h.status(`${why.replace(/\.?\s*$/, ".")} ${GUESSED_WAY}`, true);
    h.changed();
  }

  onValue(text: string): void {
    const h = this.host;
    const r = parseVcb(text, h.units());
    if (!r.ok) return void h.status(r.error, true);
    const v = r.value;
    if (this.variant === "rotated" && this.anchor && this.plane) {
      if (v.kind !== "length") return void h.status(notHere(v, "Rotated Rectangle", "a length"), true);
      const f = this.plane.frame;
      if (!this.edgeEnd) {
        const dir = this.cursor ? unit(sub(this.cursor, this.anchor)) : f.u;
        this.edgeEnd = add(this.anchor, mul(Math.hypot(...dir) > 0.5 ? dir : f.u, v.m));
        this.snaps.push(this.snaps[0]);
        h.changed();
        return;
      }
      const side = this.cursor ? Math.sign(rotatedWidth(f, this.anchor, this.edgeEnd, this.cursor)) || 1 : 1;
      void this.finish(this.edgeEnd, undefined, side * v.m);
      return;
    }
    if (v.kind !== "lengths" || v.m.length !== 2) return void h.status(notHere(v, "Rect", "two sizes separated by a comma, like 24,12"), true);
    if (!this.anchor || !this.plane) {
      // No corner clicked: start at the origin on the ground, as before.
      this.plane = { ...horizontalAt([0, 0, 0]), frame: { ...GROUND } };
      this.anchor = [0, 0, 0];
      this.snaps = [{ point: [0, 0, 0], kind: "free", tip: "", surface: null, entity: null, lines: [] }];
    }
    // Typed sizes run toward the quadrant the cursor is in; a negative size flips that.
    const f = this.plane.frame;
    const la = local(f, this.anchor);
    const lc = this.cursor ? local(f, this.cursor) : [la[0] + 1, la[1] + 1];
    const sx = Math.sign(lc[0] - la[0]) || 1;
    const sy = Math.sign(lc[1] - la[1]) || 1;
    const [w, d] = v.m;
    // #130: with the plane still open and the cursor on the first corner (or on a line
    // through it), the cursor chose no face and no direction. The sizes are tried as before
    // (toward + where the cursor gave no sign); if the kernel refuses them, the first corner
    // stays and the words say what to do.
    const aimless = this.undecided && !this.edgeEnd && (!this.cursor || Math.abs(lc[0] - la[0]) <= 1e-9 || Math.abs(lc[1] - la[1]) <= 1e-9);
    void this.finish(this.anchor, [w * sx, d * sy], undefined, aimless);
  }

  view(): ToolView {
    const f = this.plane?.frame;
    const a = this.anchor;
    const c = this.cursor;
    const extra = [];
    const sideLabels: { at: V3; text: string; className?: string }[] = [];
    let label = "";
    if (f && a && c) {
      if (this.variant === "rotated") {
        if (this.edgeEnd) {
          const w = rotatedWidth(f, a, this.edgeEnd, c);
          extra.push({ kind: "loop" as const, points: rotatedRectangle(f, a, this.edgeEnd, w), color: "--sel", width: 2 });
          label = this.host.fmt(Math.abs(w));
        } else {
          extra.push({ kind: "strip" as const, points: [a, c], color: this.bandColor(), width: 2 });
          label = this.host.fmt(dist(a, c));
        }
      } else {
        const la = local(f, a);
        const lc = local(f, c);
        extra.push({ kind: "loop" as const, points: rectangle(f, la, lc), color: "--sel", width: 2 });
        label = `${this.host.fmt(Math.abs(lc[0] - la[0]))} × ${this.host.fmt(Math.abs(lc[1] - la[1]))}`;
        // Direction and extents: each side from the first corner, labelled at its middle with
        // its length and the way it runs; the first corner carries two short direction ticks.
        const s = rectSides(f, la, lc);
        const fmt = this.host.fmt;
        if (s.u.len > 1e-9) sideLabels.push({ at: world(f, (la[0] + lc[0]) / 2, la[1]), text: `${fmt(s.u.len)} ${dirName(s.u.dir)}`, className: "dim" });
        if (s.v.len > 1e-9) sideLabels.push({ at: world(f, la[0], (la[1] + lc[1]) / 2), text: `${fmt(s.v.len)} ${dirName(s.v.dir)}`, className: "dim" });
        const tick = Math.max(s.u.len, s.v.len) * 0.08;
        if (tick > 0) {
          extra.push({ kind: "segments" as const, points: [a, add(a, mul(s.u.dir, tick)), a, add(a, mul(s.v.dir, tick))], color: "--accent", width: 3 });
        }
      }
    }
    return this.baseView(extra, [...sideLabels, ...(label ? this.live(label) : [])]);
  }
}
