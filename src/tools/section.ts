// Section Plane (lane 12, ADR-0010; on the rail and Tools menu, no default key, as in
// SketchUp 2017). Hover a face (or a cut, or a sketch region) and the preview lies on it,
// facing away from the eye: the side kept is the one you look into, so a plane laid on a
// face cuts nothing until it is moved in. Away from any surface the preview stands on the
// point under the cursor square to the view's main axis. The arrow keys lock it square to
// red (Right), green (Left) or blue (Up); Down unlocks. A tap of Alt reverses it, and
// Alt+click places it reversed. A click (or Enter) places it, active, in the open group or
// component; it is then selected.
import type { KernelOp, V3 } from "../kernel/types";
import { awayFromEye, reversed as turned } from "../lib/sections";
import { add, cross, mul, unit } from "./draw";
import { EMPTY_VIEW, Tool, inferenceLine, markerOf, type Snap, type ToolEvent, type ToolView } from "./tool";

const LOCKS: Record<string, { n: V3; name: string }> = {
  ArrowRight: { n: [1, 0, 0], name: "square to red" },
  ArrowLeft: { n: [0, 1, 0], name: "square to green" },
  ArrowUp: { n: [0, 0, 1], name: "square to blue (level)" },
};

/** The preview: where the plane would go and the side it would keep. */
export interface SectionPreview {
  origin: V3;
  /** The side kept. */
  normal: V3;
  /** What it lies on, for the status line. */
  on: string;
}

/** The plane for the cursor now: on the surface (or cap) under it, facing away from the eye,
 * or square to the locked axis or the view's main axis; reversed when asked. */
export function previewFor(snap: Snap | null, viewDir: V3, lock: V3 | null, reversed: boolean): SectionPreview | null {
  if (!snap) return null;
  let normal: V3;
  let on: string;
  const surface = snap.surface ?? snap.adjacent ?? null;
  if (lock) {
    normal = awayFromEye(lock, viewDir);
    on = "locked";
  } else if (surface) {
    normal = awayFromEye(surface.frame.normal, viewDir);
    on = surface.at.kind === "face" ? "on the face" : "on the sketch";
  } else if (snap.cap) {
    normal = awayFromEye(snap.cap.frame.normal, viewDir);
    on = "on the cut";
  } else {
    // Square to the axis the eye looks along most.
    const a = viewDir.map(Math.abs);
    const k = a[0] >= a[1] && a[0] >= a[2] ? 0 : a[1] >= a[2] ? 1 : 2;
    const n: V3 = [0, 0, 0];
    n[k] = 1;
    normal = awayFromEye(n, viewDir);
    on = "square to the view";
  }
  if (reversed) normal = turned(normal);
  return { origin: snap.point, normal, on };
}

/** A square of side `size` around `p` on the plane across `n`, as a loop. */
export function squareOn(p: V3, n: V3, size: number): V3[] {
  const helper: V3 = Math.abs(n[2]) < 0.9 ? [0, 0, 1] : [0, 1, 0];
  const u = unit(cross(helper, n));
  const v = cross(n, u);
  const h = size / 2;
  return [add(add(p, mul(u, -h)), mul(v, -h)), add(add(p, mul(u, h)), mul(v, -h)), add(add(p, mul(u, h)), mul(v, h)), add(add(p, mul(u, -h)), mul(v, h))];
}

export class SectionTool extends Tool {
  readonly id = "section" as const;
  private snap: Snap | null = null;
  private viewDir: V3 = [0, 0, -1];
  private lock: (typeof LOCKS)[string] | null = null;
  private reversed = false;
  private preview: SectionPreview | null = null;
  /** The preview's size in metres (a fifth of the model, at least 8 in). */
  private size = 0.3;

  activate(): void {
    this.host.status("Section Plane: hover a face and click to cut there. Alt reverses it; the arrow keys lock it square to an axis; Enter places it.");
  }

  reset(): void {
    this.preview = null;
    this.snap = null;
  }

  variantHint(): string {
    return "Section Plane: hover a face and click to cut there. Alt reverses it; the arrow keys lock it square to an axis; Enter places it.";
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    this.viewDir = e.ray.dir;
    if (e.type === "move" || e.type === "click" || e.type === "drag") {
      this.snap = e.snap;
      this.preview = previewFor(e.snap, e.ray.dir, this.lock?.n ?? null, this.reversed !== e.alt);
      const st = h.state();
      const lo = st?.solids.length ? [0, 1, 2].map((k) => Math.min(...st.solids.map((s) => s.min[k]))) : [0, 0, 0];
      const hi = st?.solids.length ? [0, 1, 2].map((k) => Math.max(...st.solids.map((s) => s.max[k]))) : [0.3, 0.3, 0.3];
      this.size = Math.max(0.2, 0.35 * Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]));
      h.changed();
    }
    // Alt held at the click reverses it (the preview already shows that).
    if (e.type === "click") void this.place();
    if (e.type === "leave") {
      this.preview = null;
      h.changed();
    }
  }

  onKey(e: KeyboardEvent): boolean {
    if (e.key === "Enter") {
      void this.place();
      return true;
    }
    if (e.key === "ArrowDown") {
      this.lock = null;
      this.host.status("Section Plane: unlocked; it follows the face under the cursor.");
      this.refresh();
      return true;
    }
    const l = LOCKS[e.key];
    if (!l) return false;
    this.lock = this.lock === l ? null : l;
    this.host.status(this.lock ? `Section Plane: locked ${l.name}. Down unlocks.` : "Section Plane: unlocked.");
    this.refresh();
    return true;
  }

  onModifier(key: "ctrl" | "alt"): void {
    if (key !== "alt") return;
    this.reversed = !this.reversed;
    this.host.status(this.reversed ? "Section Plane: reversed (it keeps the side towards you). Tap Alt again to turn it back." : "Section Plane: keeps the side you look into.");
    this.refresh();
  }

  private refresh(): void {
    this.preview = previewFor(this.snap, this.viewDir, this.lock?.n ?? null, this.reversed);
    this.host.changed();
  }

  /** Place the plane previewed now. */
  private async place(): Promise<void> {
    const h = this.host;
    const p = this.preview;
    if (!p) return void h.status("Hover a face (or anywhere in the model) to place a section plane there.", true);
    const op: KernelOp = { op: "add_section", origin: p.origin, normal: p.normal };
    const r = await h.apply(op, () => "Section plane placed: it cuts away the side its arrows point from. Move it with Move (M); right-click to reverse it or make a group from the slice.");
    if (r?.outcome.kind === "section_added") h.setSelection([{ kind: "section", section: r.outcome.section }]);
  }

  view(): ToolView {
    const p = this.preview;
    if (!p) return EMPTY_VIEW;
    const sq = squareOn(p.origin, p.normal, this.size);
    const tip = add(p.origin, mul(p.normal, this.size * 0.2));
    const arrows: V3[] = [p.origin, tip, ...sq.flatMap((c) => [c, add(c, mul(p.normal, this.size * 0.12))])];
    return {
      ...EMPTY_VIEW,
      overlay: [
        { kind: "loop", points: sq, color: "--section", width: 2 },
        { kind: "segments", points: arrows, color: "--section", width: 1.75 },
        ...inferenceLine(this.snap),
      ],
      marker: markerOf(this.snap),
      cursor: "crosshair",
    };
  }
}

/** The side a plane keeps, in words ("the side towards -X"), for status lines and tests. */
export function keptSide(n: V3): string {
  const a = n.map(Math.abs);
  const k = a[0] >= a[1] && a[0] >= a[2] ? 0 : a[1] >= a[2] ? 1 : 2;
  return `the side towards ${n[k] >= 0 ? "+" : "-"}${"XYZ"[k]}`;
}

