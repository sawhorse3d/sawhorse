// Rectangle (R): click two corners, or one corner and type "width, depth" (a negative size
// goes the other way; without a click the rectangle starts at the origin). Press R again
// for the Rotated Rectangle: click the two ends of one edge, then its width.
//
// The plane is the face or region under the first corner (or level; arrow keys before the
// first click choose level / upright facing red / upright facing green). On a solid's face
// the rectangle cuts the face where it crosses its edges (a dado strip across a board) or
// imprints inside it; a rectangle reaching off that solid (a top board from one side's
// corner to the other's) becomes a new sketch on the plane instead.
import type { EditResult } from "../kernel/client";
import type { V3 } from "../kernel/types";
import { notHere, parseVcb } from "../lib/vcb";
import { GROUND } from "../lib/edit";
import { add, dist, horizontalAt, local, mul, rectangle, rotatedRectangle, rotatedWidth, sub, unit } from "./draw";
import { StrokeTool, faceNote } from "./stroke";
import { finalTarget } from "./target";
import type { Snap, ToolEvent, ToolView } from "./tool";

type Variant = "rect" | "rotated";

export class RectTool extends StrokeTool {
  readonly id = "rect" as const;
  private variant: Variant = "rect";
  private anchor: V3 | null = null;
  /** Rotated: the far end of the first edge, once clicked. */
  private edgeEnd: V3 | null = null;
  private snaps: Snap[] = [];

  start(): V3 | null {
    return this.edgeEnd ?? this.anchor;
  }

  reset(): void {
    super.reset();
    this.anchor = null;
    this.edgeEnd = null;
    this.snaps = [];
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

  onEvent(e: ToolEvent): void {
    if (e.type === "move" || e.type === "drag") {
      this.track(e);
      this.liveValue();
      this.host.changed();
      return;
    }
    if (e.type === "click" && e.snap) void this.click(e.snap);
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
      this.plane = this.planeFor(snap);
      this.anchor = snap.point;
      this.snaps = [snap];
      h.status(
        `First corner set ${this.plane.where(h.fmt)}. ${this.variant === "rect" ? "Click the opposite corner, or type width, depth." : "Click the other end of the edge, or type its length."}`,
      );
      h.changed();
      return;
    }
    if (this.variant === "rotated" && !this.edgeEnd) {
      if (dist(snap.point, this.anchor) < 1e-9) return;
      this.edgeEnd = snap.point;
      this.snaps.push(snap);
      h.status("Now the width: click, or type it.");
      h.changed();
      return;
    }
    this.snaps.push(snap);
    await this.finish(snap.point);
  }

  /** Draw with the cursor (or typed) far corner / width point. */
  private async finish(p: V3, size?: [number, number], width?: number): Promise<void> {
    const h = this.host;
    const plane = this.plane;
    const a = this.anchor;
    if (!plane || !a) return;
    const f = plane.frame;
    let pts: V3[];
    let what: string;
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
    }
    const target = finalTarget(plane, this.snaps);
    this.reset();
    h.changed();
    await this.stroke(target.target, pts, true, (r: EditResult) => `${what} ${target.where(h.fmt)}${faceNote(r)}. Push/Pull (P) and type a distance to raise it.`);
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
    void this.finish(this.anchor, [w * sx, d * sy]);
  }

  view(): ToolView {
    const f = this.plane?.frame;
    const a = this.anchor;
    const c = this.cursor;
    const extra = [];
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
      }
    }
    return this.baseView(extra, label ? this.live(label) : []);
  }
}
