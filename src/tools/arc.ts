// Arc (A): click the two ends, then pull out the bulge (or type it: a length is the bulge,
// "12r" a radius, "12s" the number of segments, 12 by default). Press A again for the
// 3-Point Arc: click the start, a point the arc passes through, and the end.
//
// An arc is an open stroke: on a sketch plane it closes regions with the edges it meets;
// on a solid's face it must run from edge to edge (a rounded notch), like a line. Lane 23: in
// a sketch it is one curve, an arc, not loose edges.
import type { EditResult } from "../kernel/client";
import type { V3 } from "../kernel/types";
import { notHere, parseVcb } from "../lib/vcb";
import { arcFromBulge, arcThrough, bulgeForRadius, bulgeFromPoint, circleThrough, dist, onPlane, planeForSegment, type ArcGeom } from "./draw";
import { StrokeTool } from "./stroke";
import { finalTarget } from "./target";
import type { Snap, ToolEvent, ToolView } from "./tool";

type Variant = "arc2" | "arc3";

export const DEFAULT_ARC_SEGMENTS = 12;

export class ArcTool extends StrokeTool {
  readonly id = "arc" as const;
  private variant: Variant = "arc2";
  private pts: V3[] = [];
  private snaps: Snap[] = [];
  private viewDir: V3 = [0, 0, -1];
  segments = DEFAULT_ARC_SEGMENTS;

  start(): V3 | null {
    return this.pts.length ? this.pts[this.pts.length - 1] : null;
  }

  reset(): void {
    super.reset();
    this.pts = [];
    this.snaps = [];
  }

  valueLabel(): string {
    if (this.variant === "arc2" && this.pts.length === 2) return "Bulge";
    return this.pts.length ? "Length" : "Segments";
  }

  cycleVariant(): string | null {
    this.reset();
    this.variant = this.variant === "arc2" ? "arc3" : "arc2";
    this.host.changed();
    return this.variantHint();
  }

  variantHint(): string {
    return this.variant === "arc2"
      ? `Arc (${this.segments} segments): click both ends, then the bulge. Type 12r for a radius, 24s for segments. A again: 3-Point Arc.`
      : `3-Point Arc (${this.segments} segments): click the start, a point on the arc, then the end. A again: Arc.`;
  }

  onEvent(e: ToolEvent): void {
    if (e.type === "move" || e.type === "drag") {
      this.track(e);
      this.viewDir = e.ray.dir;
      const g = this.preview();
      if (this.variant === "arc2" && this.pts.length === 2 && this.plane && this.cursor) this.host.showValue(this.host.fmt(Math.abs(bulgeFromPoint(this.plane.frame, this.pts[0], this.pts[1], this.cursor))));
      else if (this.pts.length === 1 && this.cursor) this.host.showValue(this.host.fmt(dist(this.pts[0], this.cursor)));
      else if (g) this.host.showValue(`r ${this.host.fmt(g.radius)}`);
      this.host.changed();
      return;
    }
    if (e.type === "click" && e.snap) void this.click(e.snap);
  }

  private async click(snap: Snap): Promise<void> {
    const h = this.host;
    const p = snap.point;
    if (!this.pts.length) {
      this.pts = [p];
      this.snaps = [snap];
      this.plane = snap.kind === "on-face" || this.prefer !== "auto" ? this.planeFor(snap) : null;
      h.status(this.variant === "arc2" ? "Arc start set. Click the end." : "Arc start set. Click a point the arc passes through.");
      h.changed();
      return;
    }
    if (this.pts.some((q) => dist(q, p) < 1e-9)) return;
    if (this.pts.length === 1) {
      if (!this.plane) this.plane = this.settle(this.snaps[0], snap, this.pts[0], p);
      if (!onPlane(this.plane.frame, p, 1e-6)) return void h.status("That point is off the arc's plane.", true);
      this.pts.push(p);
      this.snaps.push(snap);
      h.status(this.variant === "arc2" ? "Now the bulge: click, or type a length (12r for a radius)." : "Now the end of the arc.");
      h.changed();
      return;
    }
    if (!this.plane || !onPlane(this.plane.frame, p, 1e-6)) return void h.status("That point is off the arc's plane.", true);
    this.snaps.push(snap);
    const g = this.geometry(p);
    if (!g) return void h.status("Those points are in a line: no arc through them.", true);
    await this.finish(g);
  }

  private settle(first: Snap, second: Snap, a: V3, b: V3) {
    for (const s of [first, second]) {
      if (s.surface) {
        const cand = this.planeFor({ ...s });
        if (onPlane(cand.frame, a) && onPlane(cand.frame, b)) return cand;
      }
    }
    return planeForSegment(a, b, this.viewDir);
  }

  /** The arc for the third point (bulge point, or the end for a 3-point arc). */
  private geometry(p: V3, bulge?: number): ArcGeom | null {
    const f = this.plane?.frame;
    if (!f || this.pts.length < 2) return null;
    const [a, b] = this.pts;
    if (this.variant === "arc2") return arcFromBulge(f, a, b, bulge ?? bulgeFromPoint(f, a, b, p), this.segments);
    const c = circleThrough(a, b, p);
    if (!c) return null;
    return { centre: c, radius: dist(c, a), points: arcThrough(f, c, a, p, b, this.segments) };
  }

  private preview(): ArcGeom | null {
    return this.cursor && this.pts.length === 2 ? this.geometry(this.cursor) : null;
  }

  private async finish(g: ArcGeom): Promise<void> {
    const h = this.host;
    const plane = this.plane;
    if (!plane) return;
    const target = finalTarget(plane, this.snaps);
    this.reset();
    h.changed();
    await this.stroke(target.target, g.points, false, (r: EditResult) => {
      const o = r.outcome;
      const formed = o.kind === "drawn" && (o.regions.length > 0 || o.faces.length > 0) ? ": closed a shape" : "";
      return `Arc r ${h.fmt(g.radius)} ${h.unitLabel()}, ${g.points.length - 1} segments ${target.where(h.fmt)}${formed}.`;
    }, { kind: "arc", centre: g.centre });
  }

  onValue(text: string): void {
    const h = this.host;
    const r = parseVcb(text, h.units());
    if (!r.ok) return void h.status(r.error, true);
    const v = r.value;
    if (v.kind === "count") {
      if (v.n < 1 || v.n > 999) return void h.status("Use 1 to 999 segments.", true);
      this.segments = v.n;
      h.status(this.variantHint());
      h.changed();
      return;
    }
    const f = this.plane?.frame;
    if (this.variant === "arc2" && this.pts.length === 2 && f) {
      const [a, b] = this.pts;
      const toward = this.cursor ?? a;
      let bulge: number | null;
      if (v.kind === "radius") bulge = bulgeForRadius(f, a, b, v.m, toward);
      else if (v.kind === "length") bulge = (bulgeFromPoint(f, a, b, toward) >= 0 ? 1 : -1) * Math.abs(v.m);
      else return void h.status(notHere(v, "Arc", "a bulge, a radius like 12r, or segments like 12s"), true);
      if (bulge === null) return void h.status("That radius is shorter than half the chord.", true);
      const g = this.geometry(toward, bulge);
      if (!g) return void h.status("No arc with that bulge.", true);
      this.snaps.push(this.snaps[this.snaps.length - 1]);
      void this.finish(g);
      return;
    }
    h.status(this.pts.length ? "Click the next point of the arc." : "Click where the arc starts.", true);
  }

  view(): ToolView {
    const extra = [];
    const g = this.preview();
    if (g) extra.push({ kind: "strip" as const, points: g.points, color: "--sel", width: 2 });
    else if (this.pts.length && this.cursor) extra.push({ kind: "strip" as const, points: [...this.pts, this.cursor], color: "--sel", width: 1.5, dashed: true });
    return this.baseView(extra, g ? this.live(`r ${this.host.fmt(g.radius)}`) : []);
  }
}
