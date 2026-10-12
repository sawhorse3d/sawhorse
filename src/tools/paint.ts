// Paint (B), lane 10. Hover shows what a click would paint in the active material; a click
// paints it:
//
// - a face directly in the open context (a plain solid's face, or a face inside the group or
//   component being edited): that face; when the face is part of a selection of faces,
//   every selected face (one undo step);
// - anything inside a closed group or component: that group or component's default (faces
//   that have a material of their own keep it);
// - Ctrl+click on a face: its whole solid's default.
//
// Alt+click samples: the active material becomes the one the face shows. Double-click a
// textured face to position its texture: three handles on the face move, turn and scale it
// (the texture follows live; letting go is one undo step). Esc or a click off the face ends
// positioning.
import type { Entity, MaterialInfo, MeshPart, PaintTarget, UvXform, V3 } from "../kernel/types";
import { cleanUv } from "../lib/looks";
import { describeNode, graphOf, scopeEntity, solidsIn } from "../lib/scope";
import { faceOf, solidName } from "../lib/names";
import type { OverlayItem } from "../viewport/engine";
import { EMPTY_VIEW, Tool, type ToolEvent, type ToolHost, type ToolView } from "./tool";

/** What the Paint tool needs from the app beyond the tool host. */
export interface PaintContext {
  /** The active material (in the model or the library), null when none is chosen. */
  active(): MaterialInfo | null;
  /** Paint `target` with the active material (a library material is added first, in the
   * same undo step). Resolves true when painted. */
  paint(target: PaintTarget, what: string): Promise<boolean>;
  /** Make the material a face shows the active one (Alt+click). Resolves to its name. */
  sample(object: number, face: number): Promise<string | null>;
  /** Show (or clear) the hover preview: these mesh parts in this material. */
  preview(p: { parts: number[]; material: MaterialInfo } | null): void;
  /** Texture coordinates a face would get with `uv` (live positioning). */
  faceUvs(object: number, face: number, uv: UvXform): Promise<Float32Array>;
  /** Put texture coordinates on one mesh part (the live preview). */
  setPartUvs(part: number, uvs: Float32Array): void;
  /** The material a mesh part shows (with its texture size, rotation and offset). */
  materialOf(part: MeshPart): MaterialInfo | null;
}

type Plan = { target: PaintTarget; parts: number[]; what: string };

type Handle = "move" | "turn" | "scale";

interface Positioning {
  object: number;
  face: number;
  part: number;
  /** The face's texture frame: origin and the axes the texture's x and y run along. */
  origin: V3;
  u: V3;
  v: V3;
  normal: V3;
  /** Centre of the face (frame coordinates) and the handle radius (metres). */
  centre: [number, number];
  radius: number;
  mat: MaterialInfo;
  /** The face's positioning as last applied, when the drag started, and now. */
  start: UvXform;
  base: UvXform;
  now: UvXform;
  drag: { handle: Handle; from: [number, number] } | null;
}

const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const mul = (a: V3, k: number): V3 => [a[0] * k, a[1] * k, a[2] * k];

/** The texture frame of a face: its plane frame, turned a quarter when the face is longer
 * along v than along u (crates/kernel/src/looks.rs `texture_frame`). */
export function textureFrame(part: MeshPart, outer: V3[]): { origin: V3; u: V3; v: V3 } {
  let [ux0, ux1, vy0, vy1] = [Infinity, -Infinity, Infinity, -Infinity];
  for (const p of outer) {
    const d = sub(p, part.origin);
    const x = dot(d, part.u);
    const y = dot(d, part.v);
    ux0 = Math.min(ux0, x);
    ux1 = Math.max(ux1, x);
    vy0 = Math.min(vy0, y);
    vy1 = Math.max(vy1, y);
  }
  const ex = ux1 - ux0;
  const ey = vy1 - vy0;
  if (ey > ex * (1 + 1e-9) + 1e-12) return { origin: part.origin, u: part.v, v: mul(part.u, -1) };
  return { origin: part.origin, u: part.u, v: part.v };
}

/** A new positioning from a handle drag (frame coordinates `from` -> `to`), about `centre`:
 * move slides the texture with the pointer, turn rotates it about the centre, scale grows it
 * about the centre. `mat`: the material's own rotation and offset (the face's add to them). */
export function dragUv(handle: Handle, start: UvXform, mat: Pick<MaterialInfo, "rotation" | "offset">, centre: [number, number], from: [number, number], to: [number, number]): UvXform {
  const scale = start.scale ?? [1, 1];
  const rot = start.rotation ?? 0;
  const off = start.offset ?? [0, 0];
  const mo = mat.offset ?? [0, 0];
  // The total offset (material + face), where the texture's origin sits on the face.
  const T: [number, number] = [mo[0] + off[0], mo[1] + off[1]];
  const [cx, cy] = centre;
  if (handle === "move") return { scale, rotation: rot, offset: [off[0] + to[0] - from[0], off[1] + to[1] - from[1]] };
  if (handle === "turn") {
    const d = Math.atan2(to[1] - cy, to[0] - cx) - Math.atan2(from[1] - cy, from[0] - cx);
    const c = Math.cos(d);
    const s = Math.sin(d);
    const rx = cx - T[0];
    const ry = cy - T[1];
    const t2: [number, number] = [cx - (c * rx - s * ry), cy - (s * rx + c * ry)];
    return { scale, rotation: rot + (d * 180) / Math.PI, offset: [t2[0] - mo[0], t2[1] - mo[1]] };
  }
  const r0 = Math.hypot(from[0] - cx, from[1] - cy);
  const r1 = Math.hypot(to[0] - cx, to[1] - cy);
  const k = r0 > 1e-9 ? Math.min(50, Math.max(0.02, r1 / r0)) : 1;
  const t2: [number, number] = [cx - k * (cx - T[0]), cy - k * (cy - T[1])];
  return { scale: [scale[0] * k, scale[1] * k], rotation: rot, offset: [t2[0] - mo[0], t2[1] - mo[1]] };
}

export class PaintTool extends Tool {
  readonly id = "paint" as const;
  private pos: Positioning | null = null;
  private previewSeq = 0;

  constructor(
    host: ToolHost,
    private ctx: PaintContext,
  ) {
    super(host);
  }

  reset(): void {
    if (this.pos) this.restoreUvs();
    this.pos = null;
    this.ctx.preview(null);
  }

  deactivate(): void {
    this.reset();
    this.host.hover(null);
  }

  busy(): boolean {
    return this.pos !== null;
  }

  activate(): void {
    const m = this.ctx.active();
    this.host.status(m ? `Paint with ${m.name}: click a face. Alt+click picks a material up. Double-click a textured face to position its texture.` : "Choose a material in the Materials pane, or Alt+click a face to pick one up.");
  }

  /** Which mesh parts belong to an object. */
  private partsOf(object: number, onlyInherited = false): number[] {
    const parts = this.host.mesh()?.parts ?? [];
    const out: number[] = [];
    parts.forEach((p, i) => {
      if (p.kind === "face" && p.owner === object && !(onlyInherited && p.own)) out.push(i);
    });
    return out;
  }

  private partOf(object: number, face: number): number {
    return (this.host.mesh()?.parts ?? []).findIndex((p) => p.kind === "face" && p.owner === object && p.index === face);
  }

  /** The face under the cursor (on a solid), whatever the pick landed on. */
  private faceUnder(e: ToolEvent): { object: number; face: number } | null {
    const ent = e.picked?.entity;
    if (ent?.kind === "face") return { object: ent.object, face: ent.face };
    const at = e.picked?.surface?.at;
    if (at?.kind === "face") return { object: at.object, face: at.face };
    return null;
  }

  /** What a click here would paint. */
  plan(e: ToolEvent): Plan | null {
    const state = this.host.state();
    const hit = this.faceUnder(e);
    if (!hit) return null;
    const scoped = scopeEntity({ kind: "face", object: hit.object, face: hit.face }, state);
    if (!scoped) return null;
    // A click on something selected paints the whole selection of solids, groups and
    // components (one undo step).
    const sel = this.host.selection();
    const items = sel.filter((s) => s.kind === "node" || s.kind === "object");
    const key = (e: Entity) => (e.kind === "node" ? `n${e.node}` : e.kind === "object" ? `o${e.object}` : "");
    const clicked = scoped.kind === "node" ? `n${scoped.node}` : `o${hit.object}`;
    if (items.length > 1 && items.some((s) => key(s) === clicked)) {
      const g = graphOf(state);
      const parts = items.flatMap((s) => (s.kind === "node" ? solidsIn(g, s.node).flatMap((o) => this.partsOf(o, true)) : s.kind === "object" ? this.partsOf(s.object, true) : []));
      return {
        target: { on: "items", items: items.map((s) => (s.kind === "node" ? { node: s.node } : { object: (s as { object: number }).object })) },
        parts,
        what: `${items.length} selected items`,
      };
    }
    if (scoped.kind === "node") {
      const g = graphOf(state);
      const parts = solidsIn(g, scoped.node).flatMap((o) => this.partsOf(o, true));
      return { target: { on: "items", items: [{ node: scoped.node }] }, parts, what: describeNode(g, scoped.node) };
    }
    if (scoped.kind !== "face") return null;
    if (e.ctrl) return { target: { on: "items", items: [{ object: hit.object }] }, parts: this.partsOf(hit.object, true), what: solidName(hit.object) };
    // A face of a selection of faces: all of them.
    const faces = sel.filter((s): s is Extract<Entity, { kind: "face" }> => s.kind === "face");
    const inSel = faces.some((s) => s.object === hit.object && s.face === hit.face);
    if (inSel && faces.length > 1 && faces.every((s) => s.object === hit.object)) {
      return {
        target: { on: "faces", object: hit.object, faces: faces.map((s) => s.face) },
        parts: faces.map((s) => this.partOf(s.object, s.face)).filter((p) => p >= 0),
        what: `${faces.length} faces`,
      };
    }
    return { target: { on: "faces", object: hit.object, faces: [hit.face] }, parts: [this.partOf(hit.object, hit.face)].filter((p) => p >= 0), what: faceOf(hit.object, hit.face) };
  }

  onEvent(e: ToolEvent): void {
    if (this.pos) {
      this.positioning(e);
      return;
    }
    const h = this.host;
    switch (e.type) {
      case "move": {
        const p = this.plan(e);
        const m = this.ctx.active();
        h.hover(p ? (scopeEntity(e.picked?.entity ?? null, h.state()) ?? null) : null);
        // Faces that already show the material need no preview (they keep their texture).
        const mesh = h.mesh();
        const parts = p && m ? p.parts.filter((i) => mesh?.parts[i]?.material !== m.name) : [];
        this.ctx.preview(p && m && !e.alt && parts.length ? { parts, material: m } : null);
        return;
      }
      case "leave":
        h.hover(null);
        this.ctx.preview(null);
        return;
      case "click": {
        const hit = this.faceUnder(e);
        if (e.alt) {
          if (!hit) {
            h.status("Alt+click a face to pick up its material.");
            return;
          }
          void this.ctx.sample(hit.object, hit.face).then((name) => {
            h.status(name ? `Picked up ${name}: click to paint with it.` : "That face has no material: painting with it takes paint off.");
          });
          return;
        }
        const p = this.plan(e);
        if (!p) {
          h.status(e.picked?.entity?.kind === "region" ? "Paint goes on the faces of solids: push or pull the region first." : "Click a face to paint it.");
          return;
        }
        const m = this.ctx.active();
        if (!m) {
          h.status("Choose a material in the Materials pane first (or Alt+click a face to pick one up).", true);
          return;
        }
        this.ctx.preview(null);
        void this.ctx.paint(p.target, p.what);
        return;
      }
      case "dblclick": {
        const hit = this.faceUnder(e);
        if (!hit) return;
        void this.startPositioning(hit.object, hit.face);
        return;
      }
      default:
        return;
    }
  }

  /** Start positioning the texture on a face (double-click, or the Materials pane). */
  async startPositioning(object: number, face: number): Promise<boolean> {
    const h = this.host;
    const scoped = scopeEntity({ kind: "face", object, face }, h.state());
    if (scoped?.kind !== "face") {
      h.status("Open the group or component first (double-click it with Select) to position a texture on one of its faces.");
      return false;
    }
    const part = this.partOf(object, face);
    const info = h.mesh()?.parts[part];
    const mat = info ? this.ctx.materialOf(info) : null;
    if (!info || !mat?.held) {
      h.status("That face shows no texture to position. Paint it with a textured material first.");
      return false;
    }
    const rings = await h.outline({ kind: "face", object, face });
    if (!rings.length) return false;
    const frame = textureFrame(info, rings[0]);
    const local = (p: V3): [number, number] => [dot(sub(p, frame.origin), frame.u), dot(sub(p, frame.origin), frame.v)];
    const pts = rings[0].map(local);
    const cx = pts.reduce((s, p) => s + p[0], 0) / pts.length;
    const cy = pts.reduce((s, p) => s + p[1], 0) / pts.length;
    const ext = Math.min(Math.max(...pts.map((p) => p[0])) - Math.min(...pts.map((p) => p[0])), Math.max(...pts.map((p) => p[1])) - Math.min(...pts.map((p) => p[1])));
    const start = info.uv ?? {};
    this.pos = { object, face, part, ...frame, normal: info.normal, centre: [cx, cy], radius: Math.max(ext * 0.3, 1e-3), mat, start, base: start, now: start, drag: null };
    this.ctx.preview(null);
    h.status("Drag the centre to move the texture, the round handle to turn it, the square one to scale it. Enter or Esc when done.");
    h.changed();
    return true;
  }

  private world(q: [number, number]): V3 {
    const p = this.pos!;
    return add(add(p.origin, mul(p.u, q[0])), mul(p.v, q[1]));
  }

  private handles(): Record<Handle, [number, number]> {
    const p = this.pos!;
    const [cx, cy] = p.centre;
    return { move: [cx, cy], turn: [cx + p.radius, cy], scale: [cx + p.radius * 0.7, cy + p.radius * 0.7] };
  }

  /** Where the cursor ray meets the face's plane, in frame coordinates. */
  private onPlane(e: ToolEvent): [number, number] | null {
    const p = this.pos!;
    const { origin, dir } = e.ray;
    const den = dot(dir, p.normal);
    if (Math.abs(den) < 1e-9) return null;
    const t = dot(sub(p.origin, origin), p.normal) / den;
    const hit = add(origin, mul(dir, t));
    return [dot(sub(hit, p.origin), p.u), dot(sub(hit, p.origin), p.v)];
  }

  private handleAt(x: number, y: number): Handle | null {
    const hs = this.handles();
    let best: Handle | null = null;
    let bestD = 14;
    for (const k of ["turn", "scale", "move"] as Handle[]) {
      const [sx, sy] = this.host.toScreen(this.world(hs[k]));
      const d = Math.hypot(sx - x, sy - y);
      if (d < bestD) {
        bestD = d;
        best = k;
      }
    }
    return best;
  }

  private positioning(e: ToolEvent): void {
    const p = this.pos!;
    const grab = (x: number, y: number): boolean => {
      const handle = this.handleAt(x, y);
      if (!handle) return false;
      p.drag = { handle, from: this.handles()[handle] };
      p.base = p.now;
      return true;
    };
    switch (e.type) {
      case "down":
        // Pressing on a handle grabs it.
        if (grab(e.x, e.y)) this.host.changed();
        return;
      case "drag":
      case "move": {
        if (!p.drag) {
          if (e.type !== "drag") return;
          const [x0, y0] = e.start ?? [e.x, e.y];
          if (!grab(x0, y0)) return;
        }
        const to = this.onPlane(e);
        if (!to || !p.drag) return;
        p.now = dragUv(p.drag.handle, p.base, p.mat, p.centre, p.drag.from, to);
        this.livePreview();
        this.host.changed();
        return;
      }
      case "dragend":
      case "click": {
        if (p.drag) {
          // Where the button came up is where the texture goes (moves may have been merged).
          const to = this.onPlane(e);
          if (to) p.now = dragUv(p.drag.handle, p.base, p.mat, p.centre, p.drag.from, to);
          p.drag = null;
          this.livePreview();
          if (JSON.stringify(p.now) !== JSON.stringify(p.base)) {
            p.start = p.now;
            void this.host.apply({ op: "set_face_uv", object: p.object, face: p.face, uv: cleanUv(p.now) }, () => "Texture positioned. Ctrl+Z puts it back.");
          }
          return;
        }
        if (e.type === "click") {
          const hit = this.faceUnder(e);
          if (!hit || hit.object !== p.object || hit.face !== p.face) this.finish();
        }
        return;
      }
      default:
        return;
    }
  }

  private livePreview(): void {
    const p = this.pos;
    if (!p) return;
    const seq = ++this.previewSeq;
    void this.ctx.faceUvs(p.object, p.face, p.now).then((uvs) => {
      if (seq === this.previewSeq && this.pos) this.ctx.setPartUvs(p.part, uvs);
    });
  }

  /** Put back the coordinates of the last applied positioning (a drag cancelled midway). */
  private restoreUvs(): void {
    const p = this.pos;
    if (!p?.drag) return;
    const seq = ++this.previewSeq;
    void this.ctx.faceUvs(p.object, p.face, p.start).then((uvs) => {
      if (seq === this.previewSeq) this.ctx.setPartUvs(p.part, uvs);
    });
  }

  private finish(): void {
    this.pos = null;
    this.host.status("Done positioning.");
    this.host.changed();
  }

  onKey(e: KeyboardEvent): boolean {
    if (this.pos && (e.key === "Enter" || e.key === "Escape")) {
      if (e.key === "Escape") this.restoreUvs();
      this.finish();
      return true;
    }
    return false;
  }

  cancel(): boolean {
    if (!this.pos) return false;
    this.restoreUvs();
    this.finish();
    return true;
  }

  view(): ToolView {
    const p = this.pos;
    if (!p) return { ...EMPTY_VIEW, cursor: "crosshair" };
    const hs = this.handles();
    const square = (q: [number, number], r: number): V3[] => [
      this.world([q[0] - r, q[1] - r]),
      this.world([q[0] + r, q[1] - r]),
      this.world([q[0] + r, q[1] + r]),
      this.world([q[0] - r, q[1] + r]),
    ];
    const ring = (q: [number, number], r: number): V3[] => Array.from({ length: 16 }, (_, k) => this.world([q[0] + r * Math.cos((k * Math.PI) / 8), q[1] + r * Math.sin((k * Math.PI) / 8)]));
    const s = p.radius * 0.08;
    const overlay: OverlayItem[] = [
      { kind: "segments", points: [this.world(hs.move), this.world(hs.turn), this.world(hs.move), this.world(hs.scale)], color: "--accent", width: 1.25, dashed: true },
      { kind: "loop", points: square(hs.move, s), color: "--accent", width: 2.5 },
      { kind: "loop", points: ring(hs.turn, s), color: "--inf-face", width: 2.5 },
      { kind: "loop", points: square(hs.scale, s * 0.8), color: "--inf-end", width: 2.5 },
    ];
    const rot = Math.round(((p.now.rotation ?? 0) % 360) * 10) / 10;
    const sc = Math.round((p.now.scale?.[0] ?? 1) * 100);
    return {
      ...EMPTY_VIEW,
      overlay,
      labels: [{ at: this.world(hs.turn), text: `${rot}°`, className: "uvlabel" }, { at: this.world(hs.scale), text: `${sc}%`, className: "uvlabel" }],
      cursor: p.drag ? "grabbing" : "grab",
    };
  }

  /** The face being positioned (tests and captures). */
  positioningFace(): { object: number; face: number; uv: UvXform } | null {
    return this.pos ? { object: this.pos.object, face: this.pos.face, uv: this.pos.now } : null;
  }

  /** Handle positions in the world, and the face's normal (tests and captures). */
  handleWorld(): (Record<Handle, V3> & { normal: V3 }) | null {
    if (!this.pos) return null;
    const hs = this.handles();
    return { move: this.world(hs.move), turn: this.world(hs.turn), scale: this.world(hs.scale), normal: this.pos.normal };
  }

  /** Handle positions on screen (tests and captures drive them). */
  handleScreen(): Record<Handle, [number, number]> | null {
    if (!this.pos) return null;
    const hs = this.handles();
    return {
      move: this.host.toScreen(this.world(hs.move)),
      turn: this.host.toScreen(this.world(hs.turn)),
      scale: this.host.toScreen(this.world(hs.scale)),
    };
  }
}
