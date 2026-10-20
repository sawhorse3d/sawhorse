// Circle (C): click the centre, then the radius (or type it). "48s" sets the number of
// segments (24 by default). Press C again for Polygon: same gesture, "6s" sets the sides
// (6 by default). Like every shape, it goes on the face or region under the centre (an
// imprint inside a face, cuts where it crosses edges) or on the level plane through it.
// Lane 23: in a sketch it is one curve (a circle, or a polygon), not 24 loose edges.
import type { EditResult } from "../kernel/client";
import type { V3 } from "../kernel/types";
import { notHere, parseVcb } from "../lib/vcb";
import { add, dist, mul, onPlane, regularPolygon } from "./draw";
import { StrokeTool, faceNote } from "./stroke";
import { finalTarget } from "./target";
import type { Snap, ToolEvent, ToolView } from "./tool";

type Variant = "circle" | "polygon";

export const DEFAULT_SEGMENTS = 24;
export const DEFAULT_SIDES = 6;

export class CircleTool extends StrokeTool {
  readonly id = "circle" as const;
  private variant: Variant = "circle";
  private centre: V3 | null = null;
  private snaps: Snap[] = [];
  segments = DEFAULT_SEGMENTS;
  sides = DEFAULT_SIDES;

  start(): V3 | null {
    return this.centre;
  }

  reset(): void {
    super.reset();
    this.centre = null;
    this.snaps = [];
  }

  private n(): number {
    return this.variant === "circle" ? this.segments : this.sides;
  }

  valueLabel(): string {
    return this.centre ? "Radius" : this.variant === "circle" ? "Segments" : "Sides";
  }

  cycleVariant(): string | null {
    this.reset();
    this.variant = this.variant === "circle" ? "polygon" : "circle";
    this.host.changed();
    return this.variantHint();
  }

  variantHint(): string {
    return this.variant === "circle"
      ? `Circle (${this.segments} segments): click the centre, then the radius. Type 48s for 48 segments. C again: Polygon.`
      : `Polygon (${this.sides} sides): click the centre, then the radius. Type 8s for 8 sides. C again: Circle.`;
  }

  onEvent(e: ToolEvent): void {
    if (e.type === "move" || e.type === "drag") {
      this.track(e);
      if (this.centre && this.cursor) this.host.showValue(this.host.fmt(dist(this.centre, this.cursor)));
      this.host.changed();
      return;
    }
    if (e.type === "click" && e.snap) void this.click(e.snap);
  }

  private async click(snap: Snap): Promise<void> {
    const h = this.host;
    if (!this.centre) {
      this.plane = this.planeFor(snap);
      this.centre = snap.point;
      this.snaps = [snap];
      h.status(`Centre set ${this.plane.where(h.fmt)}. Click the radius or type it.`);
      h.changed();
      return;
    }
    const p = snap.point;
    if (!this.plane || !onPlane(this.plane.frame, p, 1e-6)) return;
    const r = dist(this.centre, p);
    if (r < 1e-9) return;
    this.snaps.push(snap);
    await this.finish(p, r);
  }

  private async finish(toward: V3, r: number): Promise<void> {
    const h = this.host;
    const plane = this.plane;
    const c = this.centre;
    if (!plane || !c) return;
    const n = this.n();
    const pts = regularPolygon(plane.frame, c, toward, r, n);
    const target = finalTarget(plane, this.snaps.length > 1 ? this.snaps : [this.snaps[0], this.snaps[0]]);
    const kind = this.variant === "circle" ? `Circle r ${h.fmt(r)} ${h.unitLabel()}, ${n} segments` : `Polygon r ${h.fmt(r)} ${h.unitLabel()}, ${n} sides`;
    this.reset();
    h.changed();
    await this.stroke(target.target, pts, true, (res: EditResult) => `${kind} ${target.where(h.fmt)}${faceNote(res)}.`, { kind: this.variant === "circle" ? "circle" : "polygon", centre: c });
  }

  onValue(text: string): void {
    const h = this.host;
    const r = parseVcb(text, h.units());
    if (!r.ok) return void h.status(r.error, true);
    const v = r.value;
    if (v.kind === "count") {
      const min = this.variant === "circle" ? 3 : 3;
      if (v.n < min || v.n > 999) return void h.status(`Use 3 to 999 ${this.variant === "circle" ? "segments" : "sides"}.`, true);
      if (this.variant === "circle") this.segments = v.n;
      else this.sides = v.n;
      h.status(this.variantHint());
      h.changed();
      return;
    }
    if (v.kind !== "length" && v.kind !== "radius") return void h.status(notHere(v, this.variant === "circle" ? "Circle" : "Polygon", "a radius, or a count like 48s"), true);
    if (!this.centre || !this.plane) return void h.status("Click the centre first, then type the radius.", true);
    const radius = Math.abs(v.m);
    const toward = this.cursor && dist(this.cursor, this.centre) > 1e-9 ? this.cursor : add(this.centre, mul(this.plane.frame.u, 1));
    void this.finish(toward, radius);
  }

  view(): ToolView {
    const c = this.centre;
    const f = this.plane?.frame;
    if (!c || !f || !this.cursor) return this.baseView();
    const r = dist(c, this.cursor);
    if (r < 1e-9) return this.baseView();
    const pts = regularPolygon(f, c, this.cursor, r, this.n());
    return this.baseView(
      [
        { kind: "loop", points: pts, color: "--sel", width: 2 },
        { kind: "segments", points: [c, this.cursor], color: "--sel", width: 1, dashed: true },
      ],
      this.live(`r ${this.host.fmt(r)}`),
    );
  }
}
