// Follow Me (W). Select the path first: edges joined end to end (Shift-click to add them),
// or one face (its edge, all the way round). Then press W and click the profile: a face of
// a solid or a region of a sketch. The profile travels along the path, mitered at every
// corner (straight segments; round paths come later). A sketch region becomes a new solid;
// a face adds to its solid when the path leaves it outward and cuts the solid when the
// path runs into it, so a moulding drawn on a box's end and swept round its top edge is
// routed in.
import type { EditResult } from "../kernel/client";
import type { Entity, KernelOp, Profile, V3 } from "../kernel/types";
import { faceOf, regionName, solidName } from "../lib/names";
import { cross, dot, len, sub } from "./draw";
import { chainOf } from "./offset";
import { EMPTY_VIEW, Tool, markerOf, type Snap, type ToolEvent, type ToolView } from "./tool";

export interface Path {
  points: V3[];
  closed: boolean;
  what: string;
}

/** The path from the selection: a chain of edges, or a face's (region's) outer edge. */
export async function pathOf(sel: Entity[], outline: (e: Entity) => Promise<V3[][]>): Promise<Path | string> {
  if (!sel.length) return "Select the path first (edges end to end, or a face to go round its edge), then press W and click the profile.";
  const edges = sel.filter((e): e is Extract<Entity, { kind: "edge" }> => e.kind === "edge");
  if (edges.length === sel.length) {
    const c = chainOf(edges.map((e) => [e.a, e.b]));
    if (typeof c === "string") return c;
    return { ...c, what: `${edges.length} edge${edges.length === 1 ? "" : "s"}` };
  }
  if (sel.length === 1 && (sel[0].kind === "face" || sel[0].kind === "region")) {
    const rings = await outline(sel[0]);
    if (!rings.length) return "That face is gone.";
    const e = sel[0];
    return { points: rings[0], closed: true, what: `the edge of ${e.kind === "face" ? faceOf(e.object, e.face) : regionName(e.sketch, e.region)}` };
  }
  return "The path is edges joined end to end, or one face: select just those.";
}

/** The path's corners: points where it runs straight on are not corners (a face's edge
 * may carry a vertex where a line was drawn up to it). */
export function corners(points: V3[], closed: boolean): V3[] {
  const n = points.length;
  return points.filter((p, i) => {
    if (!closed && (i === 0 || i === n - 1)) return true;
    const a = points[(i + n - 1) % n];
    const b = points[(i + 1) % n];
    const u = sub(p, a);
    const v = sub(b, p);
    return len(cross(u, v)) > 1e-9 * len(u) * len(v) || dot(u, v) < 0;
  });
}

/** An open path starts at the end nearest the profile's plane (a closed one anywhere). */
export function orientPath(points: V3[], closed: boolean, plane: { origin: V3; normal: V3 }): V3[] {
  if (closed || points.length < 2) return points;
  const d = (p: V3) => Math.abs(dot(sub(p, plane.origin), plane.normal));
  return d(points[points.length - 1]) < d(points[0]) ? [...points].reverse() : points;
}

export function followOp(profile: Profile, path: Path): KernelOp {
  return { op: "follow_me", profile, path: path.points, closed: path.closed };
}

export class FollowTool extends Tool {
  readonly id = "follow" as const;
  private snap: Snap | null = null;
  private path: Path | null = null;
  private pathError: string | null = null;

  activate(): void {
    void this.loadPath();
  }

  private async loadPath(): Promise<void> {
    const h = this.host;
    const p = await pathOf(h.selection(), async (e) => (e.kind === "face" ? h.outline({ kind: "face", object: e.object, face: e.face }) : e.kind === "region" ? h.outline({ kind: "region", sketch: e.sketch, region: e.region }) : []));
    if (typeof p === "string") {
      this.path = null;
      this.pathError = p;
      h.status(p, h.selection().length > 0);
    } else {
      this.path = p;
      this.pathError = null;
      const c = corners(p.points, p.closed).length;
      h.status(`Path: ${p.what} (${p.closed ? "closed" : "open"}, ${p.closed ? c : c - 1} segments). Click the profile: a face, or a region of a sketch.`);
    }
    h.changed();
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    switch (e.type) {
      case "move": {
        this.snap = e.snap;
        const s = e.picked?.surface;
        h.hover(s ? (s.at.kind === "face" ? { kind: "face", object: s.at.object, face: s.at.face } : { kind: "region", sketch: s.at.sketch, region: s.at.region }) : null);
        h.changed();
        return;
      }
      case "click": {
        void this.click(e);
        return;
      }
      case "leave":
        h.hover(null);
        return;
      default:
        return;
    }
  }

  private async click(e: ToolEvent): Promise<void> {
    const h = this.host;
    const path = this.path;
    if (!path) return void h.status(this.pathError ?? "Select the path first.", true);
    const s = e.picked?.surface;
    if (!s) return void h.status("Click the profile: a face of a solid, or a region of a sketch.", true);
    const profile: Profile = s.at.kind === "face" ? { of: "face", object: s.at.object, face: s.at.face } : { of: "region", sketch: s.at.sketch, region: s.at.region };
    const what = s.at.kind === "face" ? faceOf(s.at.object, s.at.face) : regionName(s.at.sketch, s.at.region);
    const oriented: Path = { ...path, points: orientPath(path.points, path.closed, { origin: s.frame.origin, normal: s.frame.normal }) };
    const r = await h.apply(followOp(profile, oriented), (res: EditResult) => {
      const o = res.outcome;
      const c = corners(oriented.points, oriented.closed).length;
      const segs = oriented.closed ? c : c - 1;
      if (o.kind === "object_created") return `Followed ${path.what} with ${what} (${segs} segments): a new solid, ${solidName(o.object)}.`;
      if (o.kind === "swept") {
        const more = o.objects.length > 1 ? ` It came apart into ${o.objects.length} solids.` : "";
        return `Followed ${path.what} with ${what} (${segs} segments): ${o.cut ? "cut from" : "added to"} ${solidName(o.objects[0])}.${more}`;
      }
      return `Followed ${path.what}.`;
    });
    if (r) {
      // The path's edges may have changed; the selection is spent.
      h.setSelection([]);
      this.path = null;
      this.pathError = "Select the next path, then click a profile.";
      h.changed();
    }
  }

  view(): ToolView {
    const marker = markerOf(this.snap);
    const p = this.path;
    if (!p) return { ...EMPTY_VIEW, marker };
    return { ...EMPTY_VIEW, marker, overlay: [{ kind: p.closed ? "loop" : "strip", points: p.points, color: "--accent", width: 3 }] };
  }
}
