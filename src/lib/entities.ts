// Entities the user points at (faces, edges, vertices, regions, whole objects): identity,
// selection sets, highlights, window / crossing selection and the ops that erase them.
// Pure (no DOM, no three.js), so all of it is unit-tested.
import type { DocState, Entity, KernelOp, MeshData, SketchInfo, V3 } from "../kernel/types";
import type { Highlight } from "../viewport/engine";

const r9 = (x: number) => Math.round(x * 1e9) / 1e9;
const pkey = (p: V3) => p.map(r9).join(",");

/** A stable string key for an entity (edges are undirected). */
export function entityKey(e: Entity): string {
  switch (e.kind) {
    case "vertex":
      return `v:${e.owner.kind}${e.owner.id}:${pkey(e.at)}`;
    case "edge": {
      const [a, b] = [pkey(e.a), pkey(e.b)].sort();
      return `e:${e.owner.kind}${e.owner.id}:${a}|${b}`;
    }
    case "face":
      return `f:${e.object}:${e.face}`;
    case "region":
      return `r:${e.sketch}:${e.region}`;
    case "guide":
      return `g:${e.guide}`;
    case "dimension":
      return `d:${e.dimension}`;
    case "object":
      return `o:${e.object}`;
  }
}

export function sameEntity(a: Entity | null, b: Entity | null): boolean {
  if (!a || !b) return a === b;
  return entityKey(a) === entityKey(b);
}

/** The object an entity belongs to, if any. */
export function objectOf(e: Entity): number | null {
  switch (e.kind) {
    case "face":
    case "object":
      return e.object;
    case "edge":
    case "vertex":
      return e.owner.kind === "object" ? e.owner.id : null;
    default:
      return null;
  }
}

export type SelectMode = "replace" | "add" | "toggle";

/** The selection mode for a click: Shift adds, Ctrl (Cmd) toggles, otherwise replace. */
export function modeFor(mods: { shift: boolean; ctrl: boolean }): SelectMode {
  return mods.ctrl ? "toggle" : mods.shift ? "add" : "replace";
}

/** Apply a click or box result to the selection. */
export function applySelection(sel: Entity[], picked: Entity[], mode: SelectMode): Entity[] {
  if (mode === "replace") return dedupe(picked);
  const keys = new Set(sel.map(entityKey));
  if (mode === "add") return dedupe([...sel, ...picked]);
  // toggle
  const flip = new Set(picked.map(entityKey));
  const kept = sel.filter((e) => !flip.has(entityKey(e)));
  const added = picked.filter((e) => !keys.has(entityKey(e)));
  return dedupe([...kept, ...added]);
}

function dedupe(es: Entity[]): Entity[] {
  const seen = new Set<string>();
  const out: Entity[] = [];
  for (const e of es) {
    const k = entityKey(e);
    if (!seen.has(k)) {
      seen.add(k);
      out.push(e);
    }
  }
  return out;
}

/** Mesh part indices an entity covers (a face, a region, or every face of an object). */
export function partsOf(e: Entity, mesh: MeshData | null): number[] {
  if (!mesh) return [];
  const out: number[] = [];
  mesh.parts.forEach((p, i) => {
    if (e.kind === "face" && p.kind === "face" && p.owner === e.object && p.index === e.face) out.push(i);
    else if (e.kind === "region" && p.kind === "region" && p.owner === e.sketch && p.index === e.region) out.push(i);
    else if (e.kind === "object" && p.kind === "face" && p.owner === e.object) out.push(i);
  });
  return out;
}

/** Edge segments of an object in the view mesh. */
export function objectEdges(object: number, mesh: MeshData | null): [V3, V3][] {
  if (!mesh) return [];
  const out: [V3, V3][] = [];
  const e = mesh.edges;
  for (let i = 0; i < mesh.edgeOwner.length; i++) {
    const o = mesh.edgeOwners[mesh.edgeOwner[i]];
    if (o && o.kind === "object" && o.id === object) {
      out.push([
        [e[6 * i], e[6 * i + 1], e[6 * i + 2]],
        [e[6 * i + 3], e[6 * i + 4], e[6 * i + 5]],
      ]);
    }
  }
  return out;
}

/** The viewport highlight for a selection plus the entity under the cursor. */
export function highlightFor(selection: Entity[], hover: Entity | null, mesh: MeshData | null): Highlight {
  const h: Highlight = { hoverParts: [], selectedParts: [], hoverEdges: [], selectedEdges: [], selectedPoints: [] };
  for (const e of selection) {
    h.selectedParts.push(...partsOf(e, mesh));
    if (e.kind === "edge") h.selectedEdges.push([e.a, e.b]);
    if (e.kind === "vertex") h.selectedPoints.push(e.at);
    if (e.kind === "object") h.selectedEdges.push(...objectEdges(e.object, mesh));
  }
  if (hover && !selection.some((s) => sameEntity(s, hover))) {
    h.hoverParts.push(...partsOf(hover, mesh));
    if (hover.kind === "edge") h.hoverEdges.push([hover.a, hover.b]);
    if (hover.kind === "object") h.hoverEdges.push(...objectEdges(hover.object, mesh));
  }
  return h;
}

/** World point to a sketch's 2D frame. */
export function toSketch(s: SketchInfo, p: V3): [number, number] {
  const d: V3 = [p[0] - s.origin[0], p[1] - s.origin[1], p[2] - s.origin[2]];
  return [d[0] * s.u[0] + d[1] * s.u[1] + d[2] * s.u[2], d[0] * s.v[0] + d[1] * s.v[1] + d[2] * s.v[2]];
}

/** Sketch 2D point to world. */
export function fromSketch(s: SketchInfo, x: number, y: number): V3 {
  return [s.origin[0] + s.u[0] * x + s.v[0] * y, s.origin[1] + s.u[1] * x + s.v[1] * y, s.origin[2] + s.u[2] * x + s.v[2] * y];
}

/** The op that erases one entity, or a reason it cannot be erased. */
export function eraseOp(e: Entity, state: DocState | null): KernelOp | string {
  switch (e.kind) {
    case "object":
      return { op: "delete_object", object: e.object };
    case "face":
      return { op: "erase_face", object: e.object, face: e.face };
    case "region":
      return { op: "erase_region", sketch: e.sketch, region: e.region };
    case "edge": {
      if (e.owner.kind === "object") return { op: "erase_edge", object: e.owner.id, a: e.a, b: e.b };
      if (e.owner.kind === "sketch") {
        const s = state?.sketches.find((x) => x.id === e.owner.id);
        if (!s) return "That sketch no longer exists.";
        return { op: "erase_segment", sketch: s.id, a: toSketch(s, e.a), b: toSketch(s, e.b) };
      }
      return "Guides are erased from the Tape tool's guide list.";
    }
    case "vertex":
      return "A point goes with its edges: erase an edge instead.";
    case "guide":
      return { op: "erase_guide", guide: e.guide };
    case "dimension":
      return { op: "erase_dimension", dimension: e.dimension };
  }
}

/**
 * Ops that erase a selection as one transaction: whole objects first (their own faces and
 * edges in the selection are then moot), then faces, regions and edges.
 */
export function eraseOps(sel: Entity[], state: DocState | null): { ops: KernelOp[]; skipped: string[] } {
  const deleted = new Set(sel.filter((e) => e.kind === "object").map((e) => (e as { object: number }).object));
  const ops: KernelOp[] = [];
  const skipped: string[] = [];
  const order = (e: Entity) => ({ object: 0, face: 1, region: 2, edge: 3, guide: 4, dimension: 4, vertex: 5 })[e.kind];
  for (const e of [...sel].sort((a, b) => order(a) - order(b))) {
    const o = objectOf(e);
    if (e.kind !== "object" && o !== null && deleted.has(o)) continue;
    const r = eraseOp(e, state);
    if (typeof r === "string") skipped.push(r);
    else ops.push(r);
  }
  return { ops, skipped };
}

// ---- window / crossing selection -----------------------------------------------------

export interface Box2 {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

type P2 = [number, number];

function norm(b: Box2): Box2 {
  return { x0: Math.min(b.x0, b.x1), y0: Math.min(b.y0, b.y1), x1: Math.max(b.x0, b.x1), y1: Math.max(b.y0, b.y1) };
}

const inBox = (b: Box2, p: P2) => p[0] >= b.x0 && p[0] <= b.x1 && p[1] >= b.y0 && p[1] <= b.y1;

function segsCross(a: P2, b: P2, c: P2, d: P2): boolean {
  const o = (p: P2, q: P2, r: P2) => (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
  const d1 = o(c, d, a);
  const d2 = o(c, d, b);
  const d3 = o(a, b, c);
  const d4 = o(a, b, d);
  return ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0));
}

function segmentTouches(b: Box2, p: P2, q: P2): boolean {
  if (inBox(b, p) || inBox(b, q)) return true;
  const c: P2[] = [
    [b.x0, b.y0],
    [b.x1, b.y0],
    [b.x1, b.y1],
    [b.x0, b.y1],
  ];
  return [0, 1, 2, 3].some((i) => segsCross(p, q, c[i], c[(i + 1) % 4]));
}

function pointInTri(p: P2, a: P2, b: P2, c: P2): boolean {
  const s = (q: P2, r: P2) => (r[0] - q[0]) * (p[1] - q[1]) - (r[1] - q[1]) * (p[0] - q[0]);
  const [d1, d2, d3] = [s(a, b), s(b, c), s(c, a)];
  return !((d1 < 0 || d2 < 0 || d3 < 0) && (d1 > 0 || d2 > 0 || d3 > 0));
}

function triangleTouches(b: Box2, a: P2, c: P2, d: P2): boolean {
  if (segmentTouches(b, a, c) || segmentTouches(b, c, d) || segmentTouches(b, d, a)) return true;
  return pointInTri([b.x0, b.y0], a, c, d);
}

/**
 * Window (left to right: entities wholly inside) or crossing (right to left: entities the
 * box touches) selection in screen space. Solids select as whole objects; sketch regions
 * and sketch strokes select individually.
 */
export function boxSelect(mesh: MeshData | null, toScreen: (p: V3) => P2, box: Box2, crossing: boolean): Entity[] {
  if (!mesh) return [];
  const b = norm(box);
  const pos = mesh.positions;
  const pt = (i: number): P2 => toScreen([pos[3 * i], pos[3 * i + 1], pos[3 * i + 2]]);
  // Per part: all triangles inside (window) / any triangle touching (crossing).
  const inside = new Map<number, boolean>();
  const touched = new Set<number>();
  for (let t = 0; t < mesh.faceIds.length; t++) {
    const part = mesh.faceIds[t];
    const [a, c, d] = [pt(mesh.indices[3 * t]), pt(mesh.indices[3 * t + 1]), pt(mesh.indices[3 * t + 2])];
    const all = inBox(b, a) && inBox(b, c) && inBox(b, d);
    inside.set(part, (inside.get(part) ?? true) && all);
    if (crossing && !touched.has(part) && triangleTouches(b, a, c, d)) touched.add(part);
  }
  const out: Entity[] = [];
  const objIn = new Map<number, boolean>();
  const objTouched = new Set<number>();
  mesh.parts.forEach((p, i) => {
    const whole = inside.get(i) ?? false;
    const any = touched.has(i);
    if (p.kind === "face") {
      objIn.set(p.owner, (objIn.get(p.owner) ?? true) && whole);
      if (any) objTouched.add(p.owner);
    } else if (crossing ? any : whole) {
      out.push({ kind: "region", sketch: p.owner, region: p.index });
    }
  });
  for (const [o, whole] of objIn) if (crossing ? objTouched.has(o) || whole : whole) out.push({ kind: "object", object: o });
  // Sketch strokes.
  const e = mesh.edges;
  for (let i = 0; i < mesh.edgeOwner.length; i++) {
    const o = mesh.edgeOwners[mesh.edgeOwner[i]];
    if (!o || o.kind !== "sketch") continue;
    const a: V3 = [e[6 * i], e[6 * i + 1], e[6 * i + 2]];
    const c: V3 = [e[6 * i + 3], e[6 * i + 4], e[6 * i + 5]];
    const [sa, sc] = [toScreen(a), toScreen(c)];
    const hit = crossing ? segmentTouches(b, sa, sc) : inBox(b, sa) && inBox(b, sc);
    if (hit) out.push({ kind: "edge", owner: { kind: "sketch", id: o.id }, a, b: c, drawn: true });
  }
  return out;
}
