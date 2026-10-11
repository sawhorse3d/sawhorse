// Select (Space). Hover highlights what is under the cursor; a click selects it (face, edge,
// vertex, sketch region); a double-click selects the whole solid, or enters its group (the
// lane-5 hook); dragging draws a window (left to right: what is wholly inside) or a
// crossing box (right to left: what it touches). Shift adds, Ctrl toggles, Esc clears.
import type { Entity } from "../kernel/types";
import { applySelection, boxSelect, modeFor, objectOf } from "../lib/entities";
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

  onEvent(e: ToolEvent): void {
    const h = this.host;
    switch (e.type) {
      case "move":
        h.hover(this.scoped(e.picked?.entity ?? null));
        return;
      case "leave":
        h.hover(null);
        return;
      case "click": {
        const ent = this.scoped(e.picked?.entity ?? null);
        const mode = modeFor(e);
        if (!ent) {
          if (mode === "replace") h.setSelection([]);
          return;
        }
        h.setSelection(applySelection(h.selection(), [ent], mode));
        h.status(describe(ent));
        return;
      }
      case "dblclick": {
        const ent = e.picked?.entity ?? null;
        const object = ent ? objectOf(ent) : null;
        if (object === null) return;
        const group = h.state()?.groups.find((g) => g.objects.includes(object));
        if (group && h.context() !== group.id) {
          // Lane 5 hook: groups are edited in place. For now this sets the editing context.
          h.enterGroup(group.id);
          h.status(`Editing group "${group.name}". Esc leaves it.`);
          return;
        }
        h.setSelection(applySelection(h.selection(), [{ kind: "object", object }], modeFor(e)));
        h.status(`Selected solid ${object}.`);
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
        const found = boxSelect(h.mesh(), (p) => h.toScreen(p), b, crossing);
        h.setSelection(applySelection(h.selection(), found, modeFor(e)));
        h.status(`${crossing ? "Crossing" : "Window"} selection: ${found.length} item${found.length === 1 ? "" : "s"}.`);
        return;
      }
      default:
        return;
    }
  }

  /** Inside a group's editing context, only that group's solids respond. */
  private scoped(e: Entity | null): Entity | null {
    const ctx = this.host.context();
    if (!e || ctx === null) return e;
    const o = objectOf(e);
    const g = this.host.state()?.groups.find((x) => x.id === ctx);
    return o !== null && g?.objects.includes(o) ? e : null;
  }

  view(): ToolView {
    if (!this.box) return EMPTY_VIEW;
    return { ...EMPTY_VIEW, box: { ...this.box, crossing: this.box.x1 < this.box.x0 } };
  }
}

export function describe(e: Entity): string {
  switch (e.kind) {
    case "face":
      return `Selected face ${e.face} of solid ${e.object}. Delete erases it if it was drawn on a face.`;
    case "edge":
      return e.owner.kind === "sketch"
        ? "Selected a sketch edge. Delete erases it."
        : e.drawn
          ? `Selected a line drawn on solid ${e.owner.id}. Delete erases it.`
          : `Selected an edge of solid ${e.owner.id}.`;
    case "vertex":
      return "Selected a point.";
    case "region":
      return `Selected region ${e.region}. Push/Pull (P) raises it; Delete erases it.`;
    case "object":
      return `Selected solid ${e.object}.`;
    case "guide":
      return "Selected a guide. Delete erases it.";
    case "dimension":
      return "Selected a dimension. Delete erases it.";
  }
}
