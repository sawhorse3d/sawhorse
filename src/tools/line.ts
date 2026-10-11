// Line (L): click, click, click... each segment is drawn as it is clicked (one undo step
// each) and the chain carries on from its end; it stops when it closes on its start or a
// face forms, or on Esc. Type a length and press Enter to place the next point at that
// distance along the direction the cursor shows (an axis, a parallel, a locked direction).
//
// Planes: a line started inside a face stays on that face's plane; otherwise the plane is
// settled by the first segment (level, or the upright plane containing it). On a solid's
// face a line must cut it from edge to edge (or close into a loop): until it does, the
// stroke stays a preview, then becomes a split_face cut (lane-2 open question 1: yes).
import type { EditResult } from "../kernel/client";
import type { V3 } from "../kernel/types";
import { notHere, parseVcb } from "../lib/vcb";
import { add, dist, mul, onPlane, planeFromSnap, planeForSegment, sub, unit, type DrawPlane } from "./draw";
import { StrokeTool } from "./stroke";
import { finalTarget, same, snapObject } from "./target";
import type { InferContext, Snap, ToolEvent, ToolView } from "./tool";

export class LineTool extends StrokeTool {
  readonly id = "line" as const;
  /** Points of the current chain (the last one is where the next segment starts). */
  private chain: V3[] = [];
  private snaps: Snap[] = [];
  /** Where the whole chain began (clicking it again closes the chain). */
  private first: V3 | null = null;
  /** A face stroke waiting to reach an edge or close (not in the model yet). */
  private pending = false;
  private viewDir: V3 = [0, 0, -1];

  start(): V3 | null {
    return this.chain.length ? this.chain[this.chain.length - 1] : null;
  }

  reset(): void {
    super.reset();
    this.chain = [];
    this.snaps = [];
    this.first = null;
    this.pending = false;
  }

  valueLabel(): string {
    return "Length";
  }

  inferContext(): InferContext {
    const ctx = super.inferContext();
    const n = this.chain.length;
    // Perpendicular / parallel to the previous segment, as SketchUp users expect.
    if (n >= 2) ctx.refs = [[this.chain[n - 2], this.chain[n - 1]]];
    return ctx;
  }

  onEvent(e: ToolEvent): void {
    if (e.type === "move" || e.type === "drag") {
      this.track(e);
      this.viewDir = e.ray.dir;
      const s = this.start();
      if (s && this.cursor) this.host.showValue(this.host.fmt(dist(s, this.cursor)));
      this.host.changed();
      return;
    }
    if (e.type === "click" && e.snap) void this.click(e.snap);
  }

  onValue(text: string): void {
    const h = this.host;
    const r = parseVcb(text, h.units());
    if (!r.ok) return void h.status(r.error, true);
    if (r.value.kind !== "length") return void h.status(notHere(r.value, "Line", "a length"), true);
    const s = this.start();
    if (!s || !this.cursor || !this.snap) return void h.status("Click where the line starts, point the way, then type its length.", true);
    const dir = unit(sub(this.cursor, s));
    if (Math.hypot(...dir) < 0.5) return void h.status("Move the cursor to show the direction first.", true);
    const end = add(s, mul(dir, r.value.m));
    void this.click({ ...this.snap, point: end });
  }

  private async click(snap: Snap): Promise<void> {
    const h = this.host;
    const p = snap.point;
    if (!this.chain.length) {
      this.chain = [p];
      this.snaps = [snap];
      this.first = p;
      // Inside a face: the line stays on its plane. Elsewhere the first segment decides.
      this.plane = snap.kind === "on-face" ? planeFromSnap(snap, this.prefer) : this.prefer !== "auto" ? planeFromSnap(snap, this.prefer) : null;
      h.status(`Line started${this.plane ? ` ${this.plane.where(h.fmt)}` : ""}. Click the next point or type a length.`);
      h.changed();
      return;
    }
    const last = this.chain[this.chain.length - 1];
    if (same(p, last)) return;
    if (!this.plane) this.plane = this.settlePlane(this.snaps[0], snap, last, p);
    if (!onPlane(this.plane.frame, p, 1e-6)) {
      h.status("That point is off this line's plane. Esc starts a new line.", true);
      return;
    }
    this.chain.push(p);
    this.snaps.push(snap);
    const closes = !!this.first && same(p, this.first) && !same(last, this.first);
    const target = finalTarget(this.plane, this.snaps);
    if (target.target.on === "face") {
      await this.faceStroke(target, closes);
      return;
    }
    const seg: V3[] = [last, p];
    const r = await this.stroke(target.target, seg, false, (res: EditResult) => `Line ${h.fmt(dist(last, p))} ${h.unitLabel()} ${target.where(h.fmt)}${formed(res) ? ": a face formed" : ""}.`);
    if (!r) {
      this.chain.pop();
      this.snaps.pop();
      return;
    }
    // After the first segment on a free plane, later segments stay on it (as a sketch).
    if (this.plane.target.on === "face") this.plane = target;
    if (closes || formed(r)) this.reset();
    else {
      this.chain = [p];
      this.snaps = [snap];
    }
    h.changed();
  }

  /** A stroke on a solid's face: commit when it reaches an edge or closes, else keep it. */
  private async faceStroke(target: DrawPlane, closes: boolean): Promise<void> {
    const h = this.host;
    const pts = closes ? this.chain.slice(0, -1) : this.chain;
    const r = await h.apply(
      { op: "draw", target: target.target, points: pts, closed: closes } as never,
      (res) => `Cut ${target.where(h.fmt)}: ${res.outcome.kind === "drawn" ? res.outcome.faces.length : 0} faces now.`,
    );
    if (r) {
      this.reset();
      h.changed();
      return;
    }
    // A dangling stroke is not an error yet, just not finished; anything else is.
    if ((h.lastError() ?? "").includes("must start and end on its edges")) {
      this.pending = true;
      h.status("Line on a face: carry on to an edge of the face, or close the loop. Esc drops it.", false);
    } else {
      this.chain.pop();
      this.snaps.pop();
    }
    h.changed();
  }

  /** The plane of a line that did not start inside a face. */
  private settlePlane(first: Snap, second: Snap, a: V3, b: V3): DrawPlane {
    for (const s of [first, second]) {
      if (s.surface?.at.kind === "face" || s.surface?.at.kind === "region") {
        const cand = planeFromSnap(s);
        if (onPlane(cand.frame, a) && onPlane(cand.frame, b)) {
          // Both ends on one solid's face plane, on that solid: draw on the face.
          if (cand.target.on === "face" && snapObject(first, cand) !== cand.target.object) continue;
          return cand;
        }
      }
    }
    return planeForSegment(a, b, this.viewDir);
  }

  view(): ToolView {
    const s = this.start();
    const extra = [];
    if (this.chain.length > 1 || this.pending) extra.push({ kind: "strip" as const, points: this.chain, color: "--sel", width: 2 });
    if (s && this.cursor && !same(s, this.cursor)) extra.push({ kind: "strip" as const, points: [s, this.cursor], color: this.bandColor(), width: 2 });
    return this.baseView(extra, s && this.cursor ? this.live(this.host.fmt(dist(s, this.cursor))) : []);
  }
}

function formed(r: EditResult): boolean {
  return r.outcome.kind === "drawn" && r.outcome.regions.length > 0;
}
