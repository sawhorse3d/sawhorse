// Offset (F). Click a face of a solid (or a sketch region), move inward or outward, click
// again (or press, drag and let go); or type the distance. With edges selected (one solid's
// or one sketch's, joined end to end on one plane), Offset offsets that chain instead: the
// side the cursor is on is the side it goes.
//
// The kernel does the offset (cavalier_contours, sharp corners) and decides where it goes
// the way drawing does: inside the solid's face it imprints (or cuts where it crosses
// edges); reaching off the solid it becomes a new sketch on that plane. A region's offset
// stays in its sketch. The preview is the kernel's own answer, so what you see is what you
// get; an offset too far inward shows the refusal before you click.
import type { EditResult } from "../kernel/client";
import type { Entity, OffsetSource, V3 } from "../kernel/types";
import { faceOf, regionName, solidName } from "../lib/names";
import { parseVcb } from "../lib/vcb";
import { add, cross, dot, len, mul, sub, unit } from "./draw";
import { EMPTY_VIEW, Tool, markerOf, type InferContext, type Snap, type ToolEvent, type ToolView } from "./tool";

const MIN_OFFSET = 1e-5;
const key = (p: V3) => p.map((x) => Math.round(x * 1e8)).join(",");

/** Join edges end to end into one chain: its points in order and whether it closes. */
export function chainOf(edges: [V3, V3][]): { points: V3[]; closed: boolean } | string {
  if (!edges.length) return "Select edges to offset.";
  const adj = new Map<string, { p: V3; next: string[] }>();
  const link = (a: V3, b: V3) => {
    const ka = key(a);
    if (!adj.has(ka)) adj.set(ka, { p: a, next: [] });
    adj.get(ka)!.next.push(key(b));
  };
  for (const [a, b] of edges) {
    link(a, b);
    link(b, a);
  }
  if ([...adj.values()].some((v) => v.next.length > 2)) return "The selected edges branch: select one chain of edges, end to end.";
  const ends = [...adj.entries()].filter(([, v]) => v.next.length === 1);
  const closed = ends.length === 0;
  if (!closed && ends.length !== 2) return "The selected edges are not one chain.";
  // Open chains start at the end that comes first in the selection; closed at the first point.
  const start = closed ? key(edges[0][0]) : ends[0][0];
  const points: V3[] = [];
  const seen = new Set<string>();
  let cur = start;
  let prev = "";
  while (!seen.has(cur)) {
    seen.add(cur);
    const v = adj.get(cur)!;
    points.push(v.p);
    const nxt = v.next.find((n) => n !== prev && !seen.has(n));
    if (!nxt) break;
    prev = cur;
    cur = nxt;
  }
  if (seen.size !== adj.size) return "The selected edges are not one chain.";
  return { points, closed };
}

/** Newell normal of a chain (zero when it is straight). */
export function chainNormal(points: V3[]): V3 {
  let n: V3 = [0, 0, 0];
  const c = points.reduce((a, p) => add(a, mul(p, 1 / points.length)), [0, 0, 0] as V3);
  for (let i = 0; i < points.length; i++) {
    n = add(n, cross(sub(points[i], c), sub(points[(i + 1) % points.length], c)));
  }
  return len(n) > 1e-12 ? unit(n) : [0, 0, 0];
}

/** Distance from `q` to segment a-b. */
function segDist(q: V3, a: V3, b: V3): number {
  const ab = sub(b, a);
  const l2 = dot(ab, ab);
  const t = l2 > 0 ? Math.max(0, Math.min(1, dot(sub(q, a), ab) / l2)) : 0;
  return len(sub(q, add(a, mul(ab, t))));
}

/** Point in polygon (2D, on the plane across `n`). */
function inside(q: V3, ring: V3[], n: V3): boolean {
  // Project to the two axes the normal is least along.
  const k = [0, 1, 2].reduce((a, b) => (Math.abs(n[b]) > Math.abs(n[a]) ? b : a), 0);
  const [i, j] = [0, 1, 2].filter((x) => x !== k);
  let c = false;
  for (let s = 0, t = ring.length - 1; s < ring.length; t = s++) {
    const [a, b] = [ring[s], ring[t]];
    if (a[j] > q[j] !== b[j] > q[j] && q[i] < ((b[i] - a[i]) * (q[j] - a[j])) / (b[j] - a[j]) + a[i]) c = !c;
  }
  return c;
}

/** Signed distance from `q` to a face's boundary: negative inside the face, positive out. */
export function faceSide(q: V3, rings: V3[][], n: V3): number {
  let d = Infinity;
  for (const r of rings) for (let k = 0; k < r.length; k++) d = Math.min(d, segDist(q, r[k], r[(k + 1) % r.length]));
  const inFace = rings.length > 0 && inside(q, rings[0], n) && !rings.slice(1).some((h) => inside(q, h, n));
  return inFace ? -d : d;
}

/** Signed distance from `q` to an open chain: positive on its left (seen from `n`). */
export function chainSide(q: V3, points: V3[], n: V3): number {
  let best = Infinity;
  let side = 1;
  for (let k = 0; k + 1 < points.length; k++) {
    const d = segDist(q, points[k], points[k + 1]);
    if (d < best) {
      best = d;
      side = dot(cross(sub(points[k + 1], points[k]), sub(q, points[k])), n) >= 0 ? 1 : -1;
    }
  }
  return side * best;
}

interface Gesture {
  source: OffsetSource;
  what: string;
  plane: { origin: V3; normal: V3 };
  /** Face / region rings (to measure from), or the chain. */
  rings: V3[][];
  chain: V3[] | null;
  /** Signed distance at the press: the offset is measured from there. */
  base: number;
  d: number;
  dragging: boolean;
}

export class OffsetTool extends Tool {
  readonly id = "offset" as const;
  private snap: Snap | null = null;
  private g: Gesture | null = null;
  private fresh = false;
  /** The kernel's preview for the current distance (null while none is back yet). */
  private preview: { d: number; loops: V3[][]; closed: boolean } | null = null;
  private previewError: string | null = null;
  private inFlight = false;
  private wanted: number | null = null;
  last: { source: OffsetSource; what: string; d: number } | null = null;

  valueLabel(): string {
    return "Distance";
  }

  busy(): boolean {
    return this.g !== null;
  }

  reset(): void {
    this.g = null;
    this.fresh = false;
    this.preview = null;
    this.previewError = null;
    this.wanted = null;
  }

  activate(): void {
    this.host.status(this.host.selection().some((e) => e.kind === "edge") ? "Click near the selected edges and move to the side to offset them, or type a distance." : "Click a face or a region, move inward or outward, click again; or type a distance.");
  }

  inferContext(): InferContext {
    const g = this.g;
    return g ? { plane: g.plane, grid: false } : {};
  }

  /** Where the cursor is on the gesture's plane. */
  private onPlane(e: ToolEvent, g: Gesture): V3 | null {
    const n = g.plane.normal;
    const den = dot(e.ray.dir, n);
    if (Math.abs(den) > 1e-12) {
      const t = dot(sub(g.plane.origin, e.ray.origin), n) / den;
      if (t > 0) {
        const hit = add(e.ray.origin, mul(e.ray.dir, t));
        // A snapped point on the plane wins (offset to a corner).
        if (e.snap && Math.abs(dot(sub(e.snap.point, g.plane.origin), n)) < 1e-7) return e.snap.point;
        return hit;
      }
    }
    return null;
  }

  private side(g: Gesture, q: V3): number {
    return g.chain ? chainSide(q, g.chain, g.plane.normal) : faceSide(q, g.rings, g.plane.normal);
  }

  onEvent(e: ToolEvent): void {
    const h = this.host;
    switch (e.type) {
      case "move":
      case "drag": {
        this.snap = e.snap;
        const g = this.g;
        if (g) {
          if (e.type === "drag") g.dragging = true;
          const q = this.onPlane(e, g);
          if (q) {
            g.d = this.side(g, q) - g.base;
            h.showValue(h.fmt(Math.abs(g.d)));
            this.requestPreview(g.d);
          }
        } else {
          const s = e.picked?.surface;
          h.hover(s ? (s.at.kind === "face" ? { kind: "face", object: s.at.object, face: s.at.face } : { kind: "region", sketch: s.at.sketch, region: s.at.region }) : null);
        }
        h.changed();
        return;
      }
      case "down": {
        if (this.g) {
          this.fresh = false;
          return;
        }
        this.fresh = true;
        void this.start(e);
        return;
      }
      case "dragend": {
        const g = this.g;
        if (g?.dragging && Math.abs(g.d) >= MIN_OFFSET) this.commit(g.d);
        return;
      }
      case "click": {
        const g = this.g;
        if (!g) return;
        if (this.fresh) {
          this.fresh = false;
          return;
        }
        if (Math.abs(g.d) < MIN_OFFSET) {
          this.reset();
          h.changed();
          return;
        }
        this.commit(g.d);
        return;
      }
      case "leave":
        h.hover(null);
        return;
      default:
        return;
    }
  }

  private async start(e: ToolEvent): Promise<void> {
    const h = this.host;
    const edges = h.selection().filter((x): x is Extract<Entity, { kind: "edge" }> => x.kind === "edge");
    let g: Gesture | null = null;
    if (edges.length) {
      const owners = new Set(edges.map((x) => `${x.owner.kind}:${x.owner.id}`));
      if (owners.size !== 1 || edges[0].owner.kind === "guide") {
        this.fresh = false;
        return void h.status("Offset edges of one solid or one sketch at a time.", true);
      }
      const chain = chainOf(edges.map((x) => [x.a, x.b]));
      if (typeof chain === "string") {
        this.fresh = false;
        return void h.status(chain, true);
      }
      const owner = edges[0].owner;
      let n = chainNormal(chain.points);
      if (len(n) === 0) {
        // A straight chain: the plane of the face (or sketch) it lies on.
        if (owner.kind === "sketch") n = h.state()?.sketches.find((s) => s.id === owner.id)?.normal ?? [0, 0, 1];
        else {
          const parts = (h.mesh()?.parts ?? []).filter((p) => p.kind === "face" && p.owner === owner.id && chain.points.every((q) => Math.abs(dot(sub(q, p.origin), p.normal)) < 1e-7));
          const under = e.picked?.surface?.at;
          const pick = parts.find((p) => under?.kind === "face" && under.object === p.owner && under.face === p.index) ?? parts[0];
          n = pick?.normal ?? [0, 0, 1];
        }
      }
      // An open chain's "left" is seen from the normal; keep the normal facing the eye.
      if (dot(n, e.ray.dir) > 0) n = mul(n, -1);
      const source: OffsetSource = { of: "edges", points: chain.points, closed: chain.closed, normal: n, object: owner.kind === "object" ? owner.id : null };
      const plane = { origin: chain.points[0], normal: n };
      const what = `${edges.length} edge${edges.length === 1 ? "" : "s"}${owner.kind === "object" ? ` of ${solidName(owner.id)}` : " of a sketch"}`;
      g = { source, what, plane, rings: chain.closed ? [chain.points] : [], chain: chain.closed ? null : chain.points, base: 0, d: 0, dragging: false };
    } else {
      // The face (or region) under the press, or beside the edge it was pressed on.
      const s = e.picked?.surface ?? e.snap?.surface ?? e.snap?.adjacent ?? null;
      if (!s) {
        this.fresh = false;
        return void h.status("Click a face of a solid or a region of a sketch to offset its edges.", true);
      }
      const t = s.at.kind === "face" ? ({ kind: "face", object: s.at.object, face: s.at.face } as const) : ({ kind: "region", sketch: s.at.sketch, region: s.at.region } as const);
      const rings = await h.outline(t);
      const source: OffsetSource = t.kind === "face" ? { of: "face", object: t.object, face: t.face } : { of: "region", sketch: t.sketch, region: t.region };
      const what = t.kind === "face" ? faceOf(t.object, t.face) : regionName(t.sketch, t.region);
      g = { source, what, plane: { origin: s.frame.origin, normal: s.frame.normal }, rings, chain: null, base: 0, d: 0, dragging: false };
    }
    const q = this.onPlane(e, g) ?? e.snap?.point ?? null;
    g.base = q ? this.side(g, q) : 0;
    // Pressed right on an edge: measure from the edge.
    if (Math.abs(g.base) < 1e-6) g.base = 0;
    this.g = g;
    h.hover(null);
    h.status(`Offsetting ${g.what}: move inward or outward and click, or type a distance.`);
    h.changed();
  }

  /** Ask the kernel for the preview at `d`, one request at a time (the latest wins). */
  private requestPreview(d: number): void {
    const g = this.g;
    if (!g || Math.abs(d) < MIN_OFFSET) return;
    if (this.inFlight) {
      this.wanted = d;
      return;
    }
    this.inFlight = true;
    void this.host.offsetPreview(g.source, d).then((r) => {
      this.inFlight = false;
      if (this.g === g) {
        if (typeof r === "string") {
          this.previewError = r;
          this.preview = null;
        } else {
          this.previewError = null;
          this.preview = { d, ...r };
        }
        this.host.changed();
      }
      const next = this.wanted;
      this.wanted = null;
      if (next !== null && this.g === g) this.requestPreview(next);
    });
  }

  onValue(text: string): void {
    const h = this.host;
    const r = parseVcb(text, h.units());
    if (!r.ok) return void h.status(r.error, true);
    if (r.value.kind !== "length") return void h.status("Offset takes a distance (negative goes the other way).", true);
    const typed = r.value.m;
    const g = this.g;
    if (g) {
      const sign = g.d > 0 ? 1 : g.d < 0 ? -1 : g.chain ? 1 : -1;
      this.commit(sign * typed);
      return;
    }
    const last = this.last;
    if (!last) return void h.status("Offset something first (click a face), then type a distance to change it.", true);
    void (async () => {
      if (!(await h.apply({ op: "undo" }))) return;
      await this.apply(last.source, last.what, Math.sign(last.d || -1) * typed);
    })();
  }

  private commit(d: number): void {
    const g = this.g;
    this.reset();
    this.host.changed();
    if (!g) return;
    void this.apply(g.source, g.what, d);
  }

  private async apply(source: OffsetSource, what: string, d: number): Promise<void> {
    const h = this.host;
    const way = source.of === "edges" && !source.closed ? (d > 0 ? "to the left" : "to the right") : d > 0 ? "outward" : "inward";
    const r = await h.apply({ op: "offset", source, distance: d }, (res: EditResult) => {
      const o = res.outcome;
      const where = o.kind === "drawn" ? (o.sketch !== null ? "into a sketch on its plane" : "on the solid") : "";
      return `Offset ${what} ${h.fmt(Math.abs(d))} ${h.unitLabel()} ${way}, ${where}. Push/Pull (P) the new face next.`;
    });
    if (r) {
      this.last = { source, what, d };
      h.showValue(h.fmt(Math.abs(d)));
    }
  }

  view(): ToolView {
    const marker = markerOf(this.snap);
    const g = this.g;
    if (!g) return { ...EMPTY_VIEW, marker };
    const overlay = [];
    const labels = [];
    const p = this.preview;
    if (p && Math.abs(p.d - g.d) < 1e-3 + Math.abs(g.d) * 0.5) {
      for (const loop of p.loops) overlay.push({ kind: p.closed ? ("loop" as const) : ("strip" as const), points: loop, color: g.d < 0 ? "--warn" : "--sel", width: 2 });
    }
    const at = this.snap?.point ?? g.plane.origin;
    const text = this.previewError ? `${this.host.fmt(Math.abs(g.d))}: too far` : `${g.d < 0 ? "−" : ""}${this.host.fmt(Math.abs(g.d))}`;
    labels.push({ at, text, className: "live" });
    return { ...EMPTY_VIEW, marker, overlay, labels };
  }
}
