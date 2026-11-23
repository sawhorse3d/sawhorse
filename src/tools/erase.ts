// Erase (E). Click an edge, a line drawn on a face, an imprinted face, a sketch region, a
// guide or a dimension to erase it (#129: a dimension's number and extension lines take the
// click as its line does); drag across several to erase them all at once, guides and
// dimensions included (one undo step). The
// kernel validates every erase: a solid stays closed, so its structural edges and faces are
// refused with the reason in the status line.
//
// Modifiers (lane 15), on a click or a drag, as the tool's hint says: Ctrl softens and
// smooths the edges (a faceted surface reads as one curved surface), Shift hides them,
// Ctrl+Shift unsoftens them (soft, smooth and hidden all cleared; with both keys held, soft
// and hidden edges can be picked). Nothing is erased then: the kernel's `set_edge_flags`,
// one undo step for all of them.
//
// Curves (lane 23): any segment of a circle, an arc, a polygon or a polyline erases the whole
// curve, on a click or across a drag, one undo step (Edit > Explode Curve first to erase one
// segment). With a modifier, the flags go on every segment of the curve.
import type { Entity, KernelOp, MeshData, V3 } from "../kernel/types";
import { curveOf, curveSegments } from "../lib/curves";
import { edgeFlagWords, entityKey, eraseOp, eraseOps } from "../lib/entities";
import { scopeEntity } from "../lib/scope";
import { GUIDE_REACH } from "../lib/annotations";
import { dimensionLines } from "../lib/dimPick";
import { EMPTY_VIEW, Tool, type InferContext, type ToolEvent, type ToolView } from "./tool";

/** Soft (1), smooth (2) and hidden (4), as crates/kernel/src/edges.rs counts them. */
const SOFT = 1;
const SMOOTH = 2;
const HIDDEN = 4;

/** What a modifier does to an edge: the flags to set and to clear, and how to say it. */
export function edgeMode(m: { ctrl: boolean; shift: boolean }): { set: number; clear: number; verb: string } | null {
  if (m.ctrl && m.shift) return { set: 0, clear: SOFT | SMOOTH | HIDDEN, verb: "Unsoftened" };
  if (m.ctrl) return { set: SOFT | SMOOTH, clear: 0, verb: "Softened and smoothed" };
  if (m.shift) return { set: HIDDEN, clear: 0, verb: "Hid" };
  return null;
}

/** The kernel op that sets a modifier's flags on solid edges (sketch strokes have none); a
 * solid's curve (lane 23) is all of its segments. */
export function edgeFlagsOp(edges: Entity[], mode: { set: number; clear: number }, mesh: MeshData | null = null): KernelOp | null {
  const at = edges.flatMap((e) => {
    if (e.kind === "edge" && e.owner.kind === "object") return [{ object: e.owner.id, a: e.a, b: e.b }];
    if (e.kind === "curve" && e.owner.kind === "object") {
      const object = e.owner.id;
      return curveSegments(mesh, e.owner, e.curve).map(([a, b]: [V3, V3]) => ({ object, a, b }));
    }
    return [];
  });
  if (!at.length) return null;
  return { op: "set_edge_flags", edges: at, set: mode.set, clear: mode.clear };
}

export class EraseTool extends Tool {
  readonly id = "erase" as const;
  /** Edges collected by a drag, by key. */
  private swept = new Map<string, Entity>();
  private dragging = false;
  /** The edge words last put in the status line (lane 15). */
  private said = "";

  reset(): void {
    this.swept.clear();
    this.dragging = false;
  }

  busy(): boolean {
    return this.dragging;
  }

  /** With Ctrl+Shift held, soft and hidden edges can be picked (to unsoften them); a
   * dimension's number and extension lines are picked as its line is (#129). */
  inferContext(): InferContext {
    return { pickSoftWithCtrlShift: true, pickDimensions: true };
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    // In the open context: a group or component that is not open erases as a whole; what
    // is outside the open one is not touched.
    const raw = e.picked?.entity ?? null;
    const edge = scopeEntity(raw, h.state());
    // Lane 23: a curve's segment is the whole curve.
    const ent = curveOf(edge, h.mesh());
    const mode = edgeMode(e);
    switch (e.type) {
      case "move": {
        h.hover(ent && ent.kind !== "vertex" && (!mode || ent.kind === "edge" || ent.kind === "curve") ? ent : null);
        // The edge's tooltip (lane 15): what it is now, so the modifier's effect is plain.
        const words = edge?.kind === "edge" ? edgeFlagWords(edge.flags) : "";
        if (words !== this.said) {
          this.said = words;
          h.status(words ? `This edge is ${words}. ${words.includes("soft") || words.includes("hidden") ? "Ctrl+Shift+click unsoftens it." : "Ctrl+click softens and smooths it."}` : null);
        }
        return;
      }
      case "leave":
        h.hover(null);
        return;
      case "down":
        // A new press starts a new sweep. A sweep whose release never reached the view (let go
        // outside it) is dropped here and erases nothing.
        if (this.swept.size || this.dragging) {
          this.swept.clear();
          this.dragging = false;
          h.changed();
        }
        return;
      case "click": {
        if (!ent) {
          if (raw) h.status("That is outside the group or component you are editing: press Esc to close it first.", true);
          return;
        }
        if (mode) {
          void this.flag([ent], mode);
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
        // Edges and curves with a modifier too (their flags); regions, guides and dimensions
        // only when erasing (#129: a drag across a dimension or a guide takes it).
        if (ent && (ent.kind === "edge" || ent.kind === "curve" || (!mode && (ent.kind === "region" || ent.kind === "guide" || ent.kind === "dimension")))) this.swept.set(entityKey(ent), ent);
        h.hover(ent);
        h.changed();
        return;
      case "dragend": {
        this.dragging = false;
        const all = [...this.swept.values()];
        this.swept.clear();
        h.changed();
        if (!all.length) return;
        if (mode) {
          void this.flag(all, mode);
          return;
        }
        const { ops, skipped } = eraseOps(all, h.state(), h.mesh());
        if (!ops.length) {
          h.status(skipped[0] ?? "Nothing to erase there.", true);
          return;
        }
        void this.run(ops, ops.length);
        return;
      }
      default:
        return;
    }
  }

  private async flag(edges: Entity[], mode: { set: number; clear: number; verb: string }): Promise<void> {
    const op = edgeFlagsOp(edges, mode, this.host.mesh());
    if (!op) {
      this.host.status("Only the edges of solids soften, smooth or hide: click an edge of a solid.", true);
      return;
    }
    const n = op.op === "set_edge_flags" ? op.edges.length : 0;
    await this.host.apply(op, () => `${mode.verb} ${n} edge${n === 1 ? "" : "s"}.`);
  }

  private async run(ops: KernelOp[], n: number): Promise<void> {
    const op: KernelOp = ops.length === 1 ? ops[0] : { op: "batch", ops };
    const r = await this.host.apply(op, () => `Erased ${n} item${n === 1 ? "" : "s"}.`);
    if (r) this.host.setSelection([]);
  }

  view(): ToolView {
    if (!this.swept.size) return EMPTY_VIEW;
    const mesh = this.host.mesh();
    const st = this.host.state();
    const pts = [...this.swept.values()].flatMap((e): V3[] => {
      if (e.kind === "edge") return [e.a, e.b];
      if (e.kind === "curve") return curveSegments(mesh, e.owner, e.curve).flat();
      if (e.kind === "dimension") {
        const d = st?.dimensions?.find((x) => x.id === e.dimension);
        if (!d) return [];
        const g = dimensionLines(d);
        return [...g.line, ...g.extensions[0], ...g.extensions[1]];
      }
      if (e.kind === "guide") {
        const g = st?.guides?.find((x) => x.id === e.guide);
        if (!g?.dir) return [];
        const r = GUIDE_REACH;
        return [
          [g.point[0] - g.dir[0] * r, g.point[1] - g.dir[1] * r, g.point[2] - g.dir[2] * r],
          [g.point[0] + g.dir[0] * r, g.point[1] + g.dir[1] * r, g.point[2] + g.dir[2] * r],
        ];
      }
      return [];
    });
    return { ...EMPTY_VIEW, overlay: pts.length ? [{ kind: "segments", points: pts, color: "--warn", width: 3 }] : [] };
  }
}
