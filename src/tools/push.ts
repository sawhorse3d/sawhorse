// Push/Pull (P): click a face or a sketch region, then type a distance (negative pushes in).
// A region extrudes into a new solid; a face of a solid moves (the kernel picks the exact
// path or the boolean path). Lane 3 step 8 adds the drag gesture and repeats.
import type { EditResult } from "../kernel/client";
import { describeTarget, pushOp, type Target } from "../lib/edit";
import { parseLength } from "../lib/units";
import { EMPTY_VIEW, Tool, markerOf, type Snap, type ToolEvent, type ToolView } from "./tool";

export function targetOf(e: { kind: string; object?: number; face?: number; sketch?: number; region?: number } | null): Target | null {
  if (!e) return null;
  if (e.kind === "face" && e.object !== undefined && e.face !== undefined) return { kind: "face", object: e.object, face: e.face };
  if (e.kind === "region" && e.sketch !== undefined && e.region !== undefined) return { kind: "region", sketch: e.sketch, region: e.region };
  return null;
}

export class PushTool extends Tool {
  readonly id = "pushpull" as const;
  private snap: Snap | null = null;

  valueLabel(): string {
    return "Distance";
  }

  activate(): void {
    // A single selected face or region becomes the target, like picking it.
    const sel = this.host.selection();
    if (sel.length === 1) {
      const t = targetOf(sel[0]);
      if (t) this.host.setTarget(t);
    }
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    if (e.type === "move") {
      this.snap = e.snap;
      const s = e.picked?.surface;
      h.hover(s ? (s.at.kind === "face" ? { kind: "face", object: s.at.object, face: s.at.face } : { kind: "region", sketch: s.at.sketch, region: s.at.region }) : null);
      h.changed();
      return;
    }
    if (e.type === "click") {
      const s = e.picked?.surface;
      const t = s ? targetOf(s.at) : null;
      h.setTarget(t);
      h.status(t ? `Selected ${describeTarget(t)}. Type a distance and press Enter (negative pushes in).` : null);
      h.changed();
    }
  }

  onValue(text: string): void {
    const h = this.host;
    const r = parseLength(text, h.units());
    if (!r.ok) {
      h.status(r.error, true);
      return;
    }
    const t = h.target();
    if (!t) {
      h.status("Click a face or a region first, then type the distance.", true);
      return;
    }
    const d = r.metres;
    void h
      .apply(pushOp(t, d), (res: EditResult) => {
        const s = res.scene.state.stats;
        const what = res.outcome.kind === "object_created" ? "Extruded" : res.outcome.kind === "object_collapsed" ? "Collapsed" : d >= 0 ? "Pulled" : "Pushed";
        return `${what} ${h.fmt(Math.abs(d))} ${h.unitLabel()}. ${s.objects} solid(s): ${s.faces} faces, ${s.edges} edges, ${s.vertices} vertices, genus ${s.genus}, ${s.watertight ? "watertight" : "OPEN"}.`;
      })
      .then((ok) => {
        if (ok) h.showValue(h.fmt(Math.abs(d)));
      });
  }

  view(): ToolView {
    return { ...EMPTY_VIEW, marker: markerOf(this.snap) };
  }
}
