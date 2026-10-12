// Erase (E). Click an edge, a line drawn on a face, an imprinted face or a sketch region to
// erase it; drag across several edges to erase them all at once (one undo step). The
// kernel validates every erase: a solid stays closed, so its structural edges and faces are
// refused with the reason in the status line.
import type { Entity, KernelOp } from "../kernel/types";
import { entityKey, eraseOp, eraseOps } from "../lib/entities";
import { scopeEntity } from "../lib/scope";
import { EMPTY_VIEW, Tool, type ToolEvent, type ToolView } from "./tool";

export class EraseTool extends Tool {
  readonly id = "erase" as const;
  /** Edges collected by a drag, by key. */
  private swept = new Map<string, Entity>();
  private dragging = false;

  reset(): void {
    this.swept.clear();
    this.dragging = false;
  }

  busy(): boolean {
    return this.dragging;
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    // In the open context: a group or component that is not open erases as a whole; what
    // is outside the open one is not touched.
    const raw = e.picked?.entity ?? null;
    const ent = scopeEntity(raw, h.state());
    switch (e.type) {
      case "move":
        h.hover(ent && ent.kind !== "vertex" ? ent : null);
        return;
      case "leave":
        h.hover(null);
        return;
      case "click": {
        if (!ent) {
          if (raw) h.status("That is outside the group or component you are editing: press Esc to close it first.", true);
          return;
        }
        // A whole solid is deleted with Select + Delete, never by a stray eraser click.
        const op = eraseOp(ent, h.state());
        if (typeof op === "string") {
          h.status(op, true);
          return;
        }
        void this.run([op], 1);
        return;
      }
      case "drag":
        this.dragging = true;
        if (ent && (ent.kind === "edge" || ent.kind === "region")) this.swept.set(entityKey(ent), ent);
        h.hover(ent);
        h.changed();
        return;
      case "dragend": {
        this.dragging = false;
        const all = [...this.swept.values()];
        this.swept.clear();
        h.changed();
        if (!all.length) return;
        const { ops, skipped } = eraseOps(all, h.state());
        if (!ops.length) {
          h.status(skipped[0] ?? "Nothing to erase there.", true);
          return;
        }
        void this.run(ops, all.length);
        return;
      }
      default:
        return;
    }
  }

  private async run(ops: KernelOp[], n: number): Promise<void> {
    const op: KernelOp = ops.length === 1 ? ops[0] : { op: "batch", ops };
    const r = await this.host.apply(op, () => `Erased ${n} item${n === 1 ? "" : "s"}.`);
    if (r) this.host.setSelection([]);
  }

  view(): ToolView {
    if (!this.swept.size) return EMPTY_VIEW;
    const pts = [...this.swept.values()].flatMap((e) => (e.kind === "edge" ? [e.a, e.b] : []));
    return { ...EMPTY_VIEW, overlay: pts.length ? [{ kind: "segments", points: pts, color: "--warn", width: 3 }] : [] };
  }
}
