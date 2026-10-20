// Select (Space). Hover highlights what is under the cursor; a click selects it (face, edge,
// vertex, sketch region); a double-click selects the whole solid. Dragging draws a window
// (left to right: what is wholly inside) or a crossing box (right to left: what it
// touches). Shift adds, Ctrl toggles, Esc clears.
//
// Groups and components (lane 5): a click on anything inside a group or component that is
// not open selects the whole group or component; a double-click opens it for editing (Esc
// closes it). Clicking something outside the open one closes it (back up to where that
// thing is) and selects it.
//
// Section planes (lane 12): a click on a plane's rectangle selects it (when nothing nearer is
// under the cursor; a cut face at the plane selects its solid); a click on the arrow at its
// centre, its reverse control, reverses it.
//
// Curves (lane 23): a circle, an arc, a polygon or a polyline is one curve. Hovering or
// clicking any of its segments takes the whole curve (a solid's rim too); a click exactly on
// one of its points still selects that point. A window takes a curve when all of it is
// inside, a crossing box when it touches any segment.
import type { DocState, Entity, MeshData } from "../kernel/types";
import { curveOf, describeCurve } from "../lib/curves";
import { applySelection, boxSelect, edgeFlagWords, entityKey, modeFor, objectOf } from "../lib/entities";
import { faceOf, regionName, solidName } from "../lib/names";
import { describeNode, graphOf, reachableContext, scopeEntity } from "../lib/scope";
import { describeSection } from "../lib/sections";
import { EMPTY_VIEW, Tool, type ToolEvent, type ToolView } from "./tool";

/** Pixels the pointer must travel with the button down before a click becomes a box. */
export const DRAG_PX = 4;

export class SelectTool extends Tool {
  readonly id = "select" as const;
  private box: { x0: number; y0: number; x1: number; y1: number } | null = null;

  reset(): void {
    this.box = null;
  }

  busy(): boolean {
    return this.box !== null;
  }

  /** The section plane under the cursor, when it is nearer than what the kernel picked. */
  private sectionAt(e: ToolEvent): { section: number; control: boolean } | null {
    const r = this.host.pickSection?.(e.x, e.y);
    if (!r) return null;
    if (r.control) return r;
    const p = e.picked?.point;
    if (!p) return r;
    const t = (p[0] - e.ray.origin[0]) * e.ray.dir[0] + (p[1] - e.ray.origin[1]) * e.ray.dir[1] + (p[2] - e.ray.origin[2]) * e.ray.dir[2];
    // A cut face lies on its plane: the solid wins there (so cut parts stay selectable).
    const cap = e.picked?.entity.kind === "object";
    const slack = 1e-6 * Math.max(1, Math.abs(t));
    return r.t < t - slack || (!cap && r.t <= t + slack) ? r : null;
  }

  /** What a pick means here: the entity in the open context, a curve's segment as the
   * whole curve. */
  private unit(raw: Entity | null): Entity | null {
    return curveOf(scopeEntity(raw, this.host.state()), this.host.mesh());
  }

  /** The status line for a selected entity. */
  private say(ent: Entity): string {
    const h = this.host;
    return describe(ent, h.state(), { mesh: h.mesh(), fmt: (m) => h.fmt(m), unit: h.unitLabel() });
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    switch (e.type) {
      case "move": {
        const sec = this.sectionAt(e);
        h.hover(sec ? scopeEntity({ kind: "section", section: sec.section }, h.state()) : this.unit(e.picked?.entity ?? null));
        return;
      }
      case "leave":
        h.hover(null);
        return;
      case "click": {
        const sec = this.sectionAt(e);
        if (sec) {
          const ent = scopeEntity({ kind: "section", section: sec.section }, h.state());
          if (sec.control && ent?.kind === "section") {
            void h.apply({ op: "flip_section", section: sec.section }, () => `Reversed the ${describeSection(h.state()?.graph, sec.section)}: it keeps the other side now.`);
            return;
          }
          if (ent) {
            h.setSelection(applySelection(h.selection(), [ent], modeFor(e)));
            h.status(this.say(ent));
            return;
          }
        }
        const raw = e.picked?.entity ?? null;
        const ent = this.unit(raw);
        const mode = modeFor(e);
        if (!ent && raw) {
          // Outside the open group or component: close back to where it is, select it.
          const target = reachableContext(raw, h.state());
          void h.apply({ op: "set_context", node: target }).then((r) => {
            if (!r) return;
            const again = this.unit(raw);
            h.setSelection(again ? [again] : []);
            if (again) h.status(this.say(again));
          });
          return;
        }
        if (!ent) {
          if (mode === "replace") h.setSelection([]);
          return;
        }
        h.setSelection(applySelection(h.selection(), [ent], mode));
        h.status(this.say(ent));
        return;
      }
      case "dblclick": {
        const ent = scopeEntity(e.picked?.entity ?? null, h.state());
        if (!ent) return;
        if (ent.kind === "node") {
          h.enterGroup(ent.node);
          return;
        }
        const object = objectOf(ent);
        if (object === null) return;
        h.setSelection(applySelection(h.selection(), [{ kind: "object", object }], modeFor(e)));
        h.status(`Selected ${solidName(object)}.`);
        return;
      }
      case "drag": {
        const [x0, y0] = e.start ?? [e.x, e.y];
        this.box = { x0, y0, x1: e.x, y1: e.y };
        h.hover(null);
        h.changed();
        return;
      }
      case "dragend": {
        const b = this.box;
        this.box = null;
        h.changed();
        if (!b) return;
        const crossing = b.x1 < b.x0;
        // Solids inside closed groups and components come back as those (once each);
        // anything outside the open one is left out.
        const seen = new Set<string>();
        const found: Entity[] = [];
        for (const raw of boxSelect(h.mesh(), (p) => h.toScreen(p), b, crossing)) {
          const ent = scopeEntity(raw, h.state());
          if (!ent || seen.has(entityKey(ent))) continue;
          seen.add(entityKey(ent));
          found.push(ent);
        }
        h.setSelection(applySelection(h.selection(), found, modeFor(e)));
        h.status(`${crossing ? "Crossing" : "Window"} selection: ${found.length} item${found.length === 1 ? "" : "s"}.`);
        return;
      }
      default:
        return;
    }
  }

  view(): ToolView {
    if (!this.box) return EMPTY_VIEW;
    return { ...EMPTY_VIEW, box: { ...this.box, crossing: this.box.x1 < this.box.x0 } };
  }
}

/** What the status line says of a curve: the view mesh it is in and the display units. */
export interface DescribeContext {
  mesh: MeshData | null;
  fmt: (m: number) => string;
  unit: string;
}

export function describe(e: Entity, state?: DocState | null, ctx?: DescribeContext): string {
  switch (e.kind) {
    case "curve":
      return describeCurve(e, ctx?.mesh ?? null, ctx?.fmt ?? ((m) => `${m}`), ctx?.unit ?? "m");
    case "face":
      return `Selected ${faceOf(e.object, e.face)}. Delete erases it if it was drawn on a face.`;
    case "edge":
      return e.owner.kind === "sketch"
        ? "Selected a sketch edge. Delete erases it."
        : e.drawn
          ? `Selected a line drawn on ${solidName(e.owner.id)}. Delete erases it.`
          : `Selected an edge of ${solidName(e.owner.id)}${edgeFlagWords(e.flags) ? ` (${edgeFlagWords(e.flags)})` : ""}.`;
    case "vertex":
      return "Selected a point.";
    case "region":
      return `Selected ${regionName(e.sketch, e.region)}. Push/Pull (P) raises it; Delete erases it.`;
    case "object":
      return `Selected ${solidName(e.object)}.`;
    case "guide":
      return "Selected a guide. Delete erases it.";
    case "dimension":
      return "Selected a dimension. Delete erases it.";
    case "node":
      return `Selected ${describeNode(graphOf(state), e.node)}. Double-click to edit inside it.`;
    case "section":
      return `Selected the ${describeSection(state?.graph, e.section)}. Move and Rotate move it; right-click (or the Outliner) to activate, reverse or slice it; Delete erases it.`;
  }
}
