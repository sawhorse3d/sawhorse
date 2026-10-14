// Entities the user points at (faces, edges, vertices, regions, whole objects): identity,
// selection sets, highlights, window / crossing selection and the ops that erase them.
// Pure (no DOM, no three.js), so all of it is unit-tested.
import type { DocState, Entity, KernelOp, MeshData, SceneGraph, SketchInfo, V3 } from "../kernel/types";
import type { Highlight } from "../viewport/engine";
import { meshIndex } from "./meshIndex";
import { contentsOf, graphIndex } from "./scope";

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
    case "node":
      return `n:${e.node}`;
    case "section":
      return `s:${e.section}`;
  }
}

export function sameEntity(a: Entity | null, b: Entity | null): boolean {
  if (!a || !b) return a === b;
  return entityKey(a) === entityKey(b);
}

/** The object an entity belongs to, if any. */
export function objectOf(e: Entity): number | null {
  switch (e.kind) {
    case "node":
      return null;
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

/**
 * Mesh part indices an entity covers (a face, a region, every face of an object, or every
 * face of every solid and mesh in a group or component, which needs the scene graph).
 * Lane 16b: read from the mesh's index (meshIndex), the entity's own parts and no pass over
 * the whole mesh; the app asks this of every selected entity after each edit.
 */
export function partsOf(e: Entity, mesh: MeshData | null, graph?: SceneGraph | null): number[] {
  if (!mesh) return [];
  const ix = meshIndex(mesh);
  const own = (l: number[] | undefined, index?: number) => (l ? (index === undefined ? [...l] : l.filter((i) => mesh.parts[i].index === index)) : []);
  switch (e.kind) {
    case "face":
      return own(ix.objectParts.get(e.object), e.face);
    case "region":
      return own(ix.regionParts.get(e.sketch), e.region);
    case "object":
      return own(ix.objectParts.get(e.object));
    case "node": {
      if (!graph) return [];
      const inside = contentsOf(graph, [e.node]);
      const out: number[] = [];
      for (const o of inside.objects) for (const i of ix.objectParts.get(o) ?? []) out.push(i);
      for (const m of inside.meshes) for (const i of ix.meshParts.get(m) ?? []) out.push(i);
      // In the mesh's order, as before.
      return Array.from(Uint32Array.from(out).sort());
    }
    default:
      return [];
  }
}

/** Edge segments of an object in the view mesh. */
export function objectEdges(object: number, mesh: MeshData | null): [V3, V3][] {
  if (!mesh) return [];
  const ix = meshIndex(mesh);
  const k = ix.objectOwner.get(object);
  if (k === undefined) return [];
  const out: [V3, V3][] = [];
  const e = mesh.edges;
  for (let j = ix.ownerStart[k]; j < ix.ownerStart[k + 1]; j++) {
    const i = ix.ownerEdges[j];
    out.push([
      [e[6 * i], e[6 * i + 1], e[6 * i + 2]],
      [e[6 * i + 3], e[6 * i + 4], e[6 * i + 5]],
    ]);
  }
  return out;
}

/** What a set of entities covers in the view mesh. Lane 16: gathered once, not solid by
 * solid with a pass over the whole mesh each. Lane 16b: from the mesh's index, so a
 * selection costs its own size; whole solids' edges are named by their owner (an index
 * into `edgeOwners`), and the viewport decides how to draw them (it draws a million
 * segments without building a million pairs here). */
function covered(es: Entity[], mesh: MeshData | null, graph: SceneGraph | null | undefined): { parts: number[]; edges: [V3, V3][]; edgeOwners: number[]; points: V3[] } {
  const out = { parts: [] as number[], edges: [] as [V3, V3][], edgeOwners: [] as number[], points: [] as V3[] };
  if (!es.length) return out;
  const objects = new Set<number>();
  const meshes = new Set<number>();
  const faces = new Map<number, Set<number>>();
  const regions = new Map<number, Set<number>>();
  const nodes: number[] = [];
  for (const e of es) {
    if (e.kind === "edge") out.edges.push([e.a, e.b]);
    else if (e.kind === "vertex") out.points.push(e.at);
    else if (e.kind === "object") objects.add(e.object);
    else if (e.kind === "node") nodes.push(e.node);
    else if (e.kind === "face") (faces.get(e.object) ?? faces.set(e.object, new Set()).get(e.object)!).add(e.face);
    else if (e.kind === "region") (regions.get(e.sketch) ?? regions.set(e.sketch, new Set()).get(e.sketch)!).add(e.region);
  }
  if (nodes.length && graph) {
    const inside = contentsOf(graph, nodes);
    for (const o of inside.objects) objects.add(o);
    for (const m of inside.meshes) meshes.add(m);
  }
  if (!mesh) return out;
  const ix = meshIndex(mesh);
  for (const o of objects) {
    for (const i of ix.objectParts.get(o) ?? []) out.parts.push(i);
    const k = ix.objectOwner.get(o);
    if (k !== undefined) out.edgeOwners.push(k);
  }
  for (const [o, fs] of faces) {
    if (objects.has(o)) continue;
    for (const i of ix.objectParts.get(o) ?? []) if (fs.has(mesh.parts[i].index)) out.parts.push(i);
  }
  for (const [s, rs] of regions) for (const i of ix.regionParts.get(s) ?? []) if (rs.has(mesh.parts[i].index)) out.parts.push(i);
  for (const m of meshes) for (const i of ix.meshParts.get(m) ?? []) out.parts.push(i);
  return out;
}

/**
 * The viewport highlight for a selection plus the entity under the cursor. With the scene
 * graph, a group or component highlights as a whole, and while one is open everything
 * outside it is faded.
 */
export function highlightFor(selection: Entity[], hover: Entity | null, mesh: MeshData | null, graph?: SceneGraph | null): Highlight {
  return withHover(selectionHighlight(selection, mesh, graph), selection, hover, mesh, graph);
}

/** Parts outside the open context, once per mesh and context (lane 16b: a selection
 * change inside an open group no longer passes over the whole mesh). */
const fadedCache = new WeakMap<MeshData, { graph: SceneGraph; ctx: number; parts: number[] }>();
function fadedParts(mesh: MeshData, graph: SceneGraph, ctx: number): number[] {
  const had = fadedCache.get(mesh);
  if (had && had.graph === graph && had.ctx === ctx) return had.parts;
  const inside = contentsOf(graph, [ctx]);
  const sketches = new Set(graph.sketches.filter(([, p]) => p !== null && solidsInChain(graph, p, ctx)).map(([s]) => s));
  const parts: number[] = [];
  mesh.parts.forEach((p, i) => {
    const keep = p.kind === "face" ? inside.objects.has(p.owner) : p.kind === "mesh" ? inside.meshes.has(p.owner) : sketches.has(p.owner);
    if (!keep) parts.push(i);
  });
  fadedCache.set(mesh, { graph, ctx, parts });
  return parts;
}

/** The highlight of a selection alone (what changes with the selection and the model). */
export function selectionHighlight(selection: Entity[], mesh: MeshData | null, graph?: SceneGraph | null): Highlight {
  const h: Highlight = { hoverParts: [], selectedParts: [], hoverEdges: [], selectedEdges: [], selectedPoints: [], fadedParts: [] };
  h.sections = { selected: selection.flatMap((e) => (e.kind === "section" ? [e.section] : [])), hover: null };
  const sel = covered(selection, mesh, graph);
  h.selectedParts = sel.parts;
  h.selectedEdges = sel.edges;
  h.selectedEdgeOwners = sel.edgeOwners;
  h.selectedPoints = sel.points;
  const ctx = graph?.context ?? null;
  if (graph && ctx !== null && mesh) h.fadedParts = fadedParts(mesh, graph, ctx);
  return h;
}

/** Membership in a selection, built once per selection array (lane 16b): solids and groups
 * by number, everything else by its key. The pointer, the Outliner's rows and the cut list
 * ask often; a select-all of 100,000 solids used to make 100,000 keys for each. */
export interface SelectionLookup {
  has(e: Entity): boolean;
  readonly objects: ReadonlySet<number>;
  readonly nodes: ReadonlySet<number>;
}
const lookups = new WeakMap<Entity[], SelectionLookup>();
export function selectionLookup(selection: Entity[]): SelectionLookup {
  const had = lookups.get(selection);
  if (had) return had;
  const objects = new Set<number>();
  const nodes = new Set<number>();
  const others = new Set<string>();
  for (const e of selection) {
    if (e.kind === "object") objects.add(e.object);
    else if (e.kind === "node") nodes.add(e.node);
    else others.add(entityKey(e));
  }
  const l: SelectionLookup = { objects, nodes, has: (e) => (e.kind === "object" ? objects.has(e.object) : e.kind === "node" ? nodes.has(e.node) : others.has(entityKey(e))) };
  lookups.set(selection, l);
  return l;
}

/** A selection's highlight with the entity under the cursor added (what changes as the
 * pointer moves: one entity's parts, never the selection's again). */
export function withHover(base: Highlight, selection: Entity[], hover: Entity | null, mesh: MeshData | null, graph?: SceneGraph | null): Highlight {
  const h: Highlight = { ...base, hoverParts: [], hoverEdges: [], hoverEdgeOwners: [], sections: { selected: base.sections?.selected ?? [], hover: hover?.kind === "section" ? hover.section : null } };
  if (hover && !selectionLookup(selection).has(hover)) {
    const c = covered([hover], mesh, graph);
    h.hoverParts = c.parts;
    h.hoverEdges = c.edges;
    h.hoverEdgeOwners = c.edgeOwners;
  }
  return h;
}

/** True when node `n` is `ancestor` or inside it. */
function solidsInChain(graph: SceneGraph, n: number, ancestor: number): boolean {
  let at: number | null = n;
  const seen = new Set<number>();
  while (at !== null && !seen.has(at)) {
    if (at === ancestor) return true;
    seen.add(at);
    at = graphIndex(graph).node.get(at)?.parent ?? null;
  }
  return false;
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
      return "Erase the guide itself, or use Edit > Delete Guides.";
    }
    case "vertex":
      return "A point goes with its edges: erase an edge instead.";
    case "guide":
      return { op: "erase_guide", guide: e.guide };
    case "dimension":
      return { op: "erase_dimension", dimension: e.dimension };
    case "node":
      return { op: "delete_node", node: e.node };
    case "section":
      return { op: "delete_section", section: e.section };
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
  const order = (e: Entity) => ({ node: 0, object: 0, face: 1, region: 2, edge: 3, guide: 4, dimension: 4, section: 4, vertex: 5 })[e.kind];
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

/** Lane 15: an edge's soft (1), smooth (2) and hidden (4) flags in words ("soft and smooth"),
 * "" for a plain edge. */
export function edgeFlagWords(flags: number | undefined): string {
  const w = [flags && flags & 1 ? "soft" : "", flags && flags & 2 ? "smooth" : "", flags && flags & 4 ? "hidden" : ""].filter(Boolean);
  return w.length <= 1 ? (w[0] ?? "") : `${w.slice(0, -1).join(", ")} and ${w[w.length - 1]}`;
}
