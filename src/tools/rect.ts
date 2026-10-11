// Rectangle (R): click two corners, or one corner and type "width, depth". On the ground it
// draws into the ground sketch; on an object face it imprints in the face's frame.
import type { EditResult } from "../kernel/client";
import { parseDimensions } from "../lib/units";
import { GROUND, rectCorners, rectangleOp, toLocal, toWorld, type RectAnchor, type Vec3 } from "../lib/edit";
import { EMPTY_VIEW, Tool, inferenceLine, markerOf, type Snap, type ToolEvent, type ToolView } from "./tool";
import { rayPlane } from "../lib/edit";

export class RectTool extends Tool {
  readonly id = "rect" as const;
  private anchor: RectAnchor | null = null;
  private corner: Vec3 | null = null;
  private snap: Snap | null = null;

  reset(): void {
    this.anchor = null;
    this.corner = null;
  }

  busy(): boolean {
    return this.anchor !== null;
  }

  valueLabel(): string {
    return "Dimensions";
  }

  inferContext() {
    return this.anchor ? { start: this.anchor.point, plane: { origin: this.anchor.frame.origin, normal: this.anchor.frame.normal } } : {};
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    if (e.type === "move" || e.type === "drag") {
      this.snap = e.snap;
      if (this.anchor) {
        const p = rayPlane(e.ray.origin, e.ray.dir, this.anchor.frame);
        this.corner = e.snap && this.onPlane(e.snap.point) ? e.snap.point : p;
        if (this.corner) {
          const a = toLocal(this.anchor.frame, this.anchor.point);
          const b = toLocal(this.anchor.frame, this.corner);
          h.showValue(`${h.fmt(Math.abs(b[0] - a[0]))}, ${h.fmt(Math.abs(b[1] - a[1]))}`);
        }
      }
      h.changed();
      return;
    }
    if (e.type !== "click" || !e.snap) return;
    if (!this.anchor) {
      const s = e.snap;
      const face = s.surface && s.surface.at.kind === "face" ? s.surface : null;
      this.anchor = face
        ? { frame: face.frame, point: s.point, face: { object: face.at.kind === "face" ? face.at.object : 0, face: face.at.kind === "face" ? face.at.face : 0 } }
        : { frame: GROUND, point: [s.point[0], s.point[1], 0], face: null };
      h.status(`First corner set ${this.anchor.face ? `on face ${this.anchor.face.face}` : "on the ground"}. Click the opposite corner, or type width, depth.`);
      h.changed();
      return;
    }
    const end = this.onPlane(e.snap.point) ? e.snap.point : rayPlane(e.ray.origin, e.ray.dir, this.anchor.frame);
    if (!end) return;
    void this.draw(toLocal(this.anchor.frame, end));
  }

  private onPlane(p: Vec3): boolean {
    if (!this.anchor) return false;
    const f = this.anchor.frame;
    const d = (p[0] - f.origin[0]) * f.normal[0] + (p[1] - f.origin[1]) * f.normal[1] + (p[2] - f.origin[2]) * f.normal[2];
    return Math.abs(d) < 1e-7;
  }

  onValue(text: string): void {
    const h = this.host;
    const r = parseDimensions(text, h.units());
    if (!r.ok) {
      h.status(r.error, true);
      return;
    }
    if (!this.anchor) this.anchor = { frame: GROUND, point: [0, 0, 0], face: null };
    void this.draw(null, r.metres);
  }

  private async draw(end: [number, number] | null, size?: [number, number]): Promise<void> {
    const anchor = this.anchor;
    if (!anchor) return;
    const h = this.host;
    const op = rectangleOp(anchor, end, size);
    const a = toLocal(anchor.frame, anchor.point);
    const b = size ? [a[0] + size[0], a[1] + size[1]] : (end ?? a);
    const [w, d] = [Math.abs(b[0] - a[0]), Math.abs(b[1] - a[1])];
    this.reset();
    h.changed();
    const r = await h.apply(op, (res: EditResult) => {
      const where = op.op === "imprint" ? `on face ${op.face} (imprinted, face ${res.outcome.kind === "imprinted" ? res.outcome.face : "?"})` : "on the ground";
      return `Rectangle ${h.fmt(w)} × ${h.fmt(d)} ${h.unitLabel()} ${where}. Push/Pull (P) and type a distance to raise it.`;
    });
    if (r) h.showValue(`${h.fmt(w)}, ${h.fmt(d)}`);
  }

  view(): ToolView {
    const v: ToolView = { ...EMPTY_VIEW, marker: markerOf(this.snap), overlay: inferenceLine(this.snap) };
    if (this.anchor && this.corner) {
      const a = toLocal(this.anchor.frame, this.anchor.point);
      const pts = rectCorners(a, toLocal(this.anchor.frame, this.corner)).map(([x, y]) => toWorld(this.anchor!.frame, x, y));
      v.overlay = [...v.overlay, { kind: "loop", points: pts, color: "--sel", width: 2 }];
    }
    return v;
  }
}
