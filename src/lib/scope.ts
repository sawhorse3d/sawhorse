// The editing context (lane 5): what a click means depends on which group or component is
// open. Pure functions over the kernel's scene graph (DocState.graph), unit-tested in
// scope.test.ts.
//
// - Clicking anything inside a group or component that is not open selects that group or
//   component as one unit ("node"): the outermost one below the open context.
// - Only what is directly inside the open context can be edited. Everything else stays
//   visible (faded) and inferable, but not editable.
import type { DocState, Entity, GraphLeaf, GraphNode, SceneGraph } from "../kernel/types";

export const EMPTY_GRAPH: SceneGraph = { context: null, path: [], nodes: [], objects: [], meshes: [], sketches: [], definitions: [], tag_colors: {} };

export function graphOf(state: DocState | null | undefined): SceneGraph {
  return state?.graph ?? EMPTY_GRAPH;
}

/** Lookups over one scene graph, built once per graph (lane 16): at the model's 100,000
 * solids, a search of the lists for every solid made selecting everything take minutes. */
interface GraphIndex {
  nodes: readonly GraphNode[];
  objects: readonly GraphLeaf[];
  meshes: readonly GraphLeaf[];
  counts: [number, number, number];
  node: Map<number, GraphNode>;
  object: Map<number, GraphLeaf>;
  /** What each node (or the model, null) holds directly, in the graph's order. */
  kidNodes: Map<number | null, number[]>;
  kidObjects: Map<number | null, number[]>;
  kidMeshes: Map<number | null, number[]>;
  objectOrder: Map<number, number>;
  meshOrder: Map<number, number>;
}

const indexes = new WeakMap<SceneGraph, GraphIndex>();

function push<K>(m: Map<K, number[]>, k: K, v: number) {
  const at = m.get(k);
  if (at) at.push(v);
  else m.set(k, [v]);
}

export function graphIndex(g: SceneGraph): GraphIndex {
  const had = indexes.get(g);
  if (had && had.nodes === g.nodes && had.objects === g.objects && had.meshes === g.meshes && had.counts[0] === g.nodes.length && had.counts[1] === g.objects.length && had.counts[2] === g.meshes.length) return had;
  const ix: GraphIndex = {
    nodes: g.nodes,
    objects: g.objects,
    meshes: g.meshes,
    counts: [g.nodes.length, g.objects.length, g.meshes.length],
    node: new Map(),
    object: new Map(),
    kidNodes: new Map(),
    kidObjects: new Map(),
    kidMeshes: new Map(),
    objectOrder: new Map(),
    meshOrder: new Map(),
  };
  for (const n of g.nodes) {
    if (!ix.node.has(n.id)) ix.node.set(n.id, n);
    push(ix.kidNodes, n.parent, n.id);
  }
  g.objects.forEach((o, i) => {
    if (!ix.object.has(o.id)) ix.object.set(o.id, o);
    ix.objectOrder.set(o.id, i);
    push(ix.kidObjects, o.parent, o.id);
  });
  g.meshes.forEach((m, i) => {
    ix.meshOrder.set(m.id, i);
    push(ix.kidMeshes, m.parent, m.id);
  });
  indexes.set(g, ix);
  return ix;
}

export function nodeById(g: SceneGraph, id: number): GraphNode | undefined {
  return graphIndex(g).node.get(id);
}

/** Every solid and imported mesh inside any of `nodes`, at any depth (lane 16: one walk
 * down from them, not a climb up from every solid in the model). */
export function contentsOf(g: SceneGraph, nodes: Iterable<number>): { objects: Set<number>; meshes: Set<number> } {
  const ix = graphIndex(g);
  const objects = new Set<number>();
  const meshes = new Set<number>();
  const seen = new Set<number>();
  const stack = [...nodes];
  while (stack.length) {
    const n = stack.pop()!;
    if (seen.has(n)) continue;
    seen.add(n);
    for (const o of ix.kidObjects.get(n) ?? []) objects.add(o);
    for (const m of ix.kidMeshes.get(n) ?? []) meshes.add(m);
    for (const k of ix.kidNodes.get(n) ?? []) stack.push(k);
  }
  return { objects, meshes };
}

/** The chain of nodes from the outermost down to `parent` itself (empty for the model). */
export function chain(g: SceneGraph, parent: number | null): number[] {
  const out: number[] = [];
  let at = parent;
  while (at !== null && at !== undefined && !out.includes(at)) {
    out.push(at);
    at = nodeById(g, at)?.parent ?? null;
  }
  return out.reverse();
}

export function objectParent(g: SceneGraph, object: number): number | null {
  return graphIndex(g).object.get(object)?.parent ?? null;
}

export function sketchParent(g: SceneGraph, sketch: number): number | null {
  return g.sketches.find(([s]) => s === sketch)?.[1] ?? null;
}

/** Where something with this parent sits relative to the open context. */
export type Placement =
  /** Directly in the open context: editable as it is. */
  | { kind: "here" }
  /** Inside a closed group or component below the context: that node is the unit. */
  | { kind: "inside"; node: number }
  /** Outside the open context (the context is not one of its ancestors). */
  | { kind: "outside" };

export function placementOf(g: SceneGraph, parent: number | null): Placement {
  const ctx = g.path;
  const a = chain(g, parent);
  if (a.length < ctx.length || ctx.some((n, i) => a[i] !== n)) return { kind: "outside" };
  if (a.length === ctx.length) return { kind: "here" };
  return { kind: "inside", node: a[ctx.length] };
}

/** The solid an entity belongs to, if any. */
function ownerObject(e: Entity): number | null {
  if (e.kind === "face" || e.kind === "object") return e.object;
  if ((e.kind === "edge" || e.kind === "vertex") && e.owner.kind === "object") return e.owner.id;
  return null;
}

/**
 * What a pick means in the open context: the entity itself when it is directly in it; the
 * group or component around it when that is closed; null when it is outside the context
 * (or locked, for editing). Guides and dimensions belong to the whole model.
 */
export function scopeEntity(e: Entity | null, state: DocState | null | undefined): Entity | null {
  if (!e) return null;
  const g = graphOf(state);
  if (e.kind === "node") {
    const p = placementOf(g, nodeById(g, e.node)?.parent ?? null);
    return p.kind === "here" ? e : p.kind === "inside" ? { kind: "node", node: p.node } : null;
  }
  if (e.kind === "section") {
    // A section plane belongs to its group or component like a solid does (lane 12).
    const s = g.sections?.find((x) => x.id === e.section);
    if (!s) return null;
    const p = placementOf(g, s.parent);
    return p.kind === "here" ? e : p.kind === "inside" ? { kind: "node", node: p.node } : null;
  }
  const o = ownerObject(e);
  if (o !== null) {
    const p = placementOf(g, objectParent(g, o));
    return p.kind === "here" ? e : p.kind === "inside" ? { kind: "node", node: p.node } : null;
  }
  const sketch = e.kind === "region" ? e.sketch : (e.kind === "edge" || e.kind === "vertex") && e.owner.kind === "sketch" ? e.owner.id : null;
  if (sketch !== null) return placementOf(g, sketchParent(g, sketch)).kind === "here" ? e : null;
  return e;
}

/** The node (or the model, null) to open so that `e` becomes reachable: the deepest node
 * that is an ancestor of both the context and `e`. */
export function reachableContext(e: Entity, state: DocState | null | undefined): number | null {
  const g = graphOf(state);
  const o = ownerObject(e);
  const parent =
    e.kind === "node"
      ? (nodeById(g, e.node)?.parent ?? null)
      : o !== null
        ? objectParent(g, o)
        : e.kind === "region"
          ? sketchParent(g, e.sketch)
          : e.kind === "section"
            ? (g.sections?.find((x) => x.id === e.section)?.parent ?? null)
            : null;
  const a = chain(g, parent);
  let k = 0;
  while (k < g.path.length && k < a.length && g.path[k] === a[k]) k++;
  return k === 0 ? null : g.path[k - 1];
}

/** Every solid inside a node, at any depth. */
export function solidsIn(g: SceneGraph, node: number): number[] {
  const ix = graphIndex(g);
  return [...contentsOf(g, [node]).objects].sort((a, b) => ix.objectOrder.get(a)! - ix.objectOrder.get(b)!);
}

/** Every imported mesh inside a node, at any depth. */
export function meshesIn(g: SceneGraph, node: number): number[] {
  const ix = graphIndex(g);
  return [...contentsOf(g, [node]).meshes].sort((a, b) => ix.meshOrder.get(a)! - ix.meshOrder.get(b)!);
}

/** A solid can be edited: directly in the open context and not locked (a solid the graph
 * does not list is left to the kernel to judge). */
export function objectEditable(state: DocState | null | undefined, object: number): boolean {
  const g = graphOf(state);
  const o = graphIndex(g).object.get(object);
  return !o || (o.parent === g.context && !o.locked);
}

/** A sketch can be drawn in: it belongs to the open context. */
export function sketchEditable(state: DocState | null | undefined, sketch: number): boolean {
  const g = graphOf(state);
  if (!g.sketches.some(([s]) => s === sketch)) return true;
  return sketchParent(g, sketch) === g.context;
}

/** A name as the status line quotes it: "'Shelf'". Names follow their noun in quotes, so a
 * default name never doubles it ("scene 'Scene 1'", not "scene Scene 1"). */
export function quoted(name: string): string {
  return `'${name}'`;
}

/** "component 'Shelf' (2 of 4)", "group 'Lower'", "a group". */
export function describeNode(g: SceneGraph, id: number): string {
  const n = nodeById(g, id);
  if (!n) return "a group";
  if (n.kind === "group") return n.name ? `group ${quoted(n.name)}` : "a group";
  const d = g.definitions.find((x) => x.id === n.definition);
  const siblings = g.nodes.filter((x) => x.definition === n.definition).map((x) => x.id);
  const k = siblings.indexOf(id) + 1;
  return `component ${quoted(n.label)}${d && d.instances > 1 ? ` (${k} of ${d.instances})` : ""}`;
}

/** Drawn now: a node, or the solid a face, edge or point belongs to, is not hidden (itself, by
 * a tag or by a hidden group around it). What is not drawn must not stay selected: Delete
 * would erase what nobody can see. */
export function entityShown(e: Entity, state: DocState | null | undefined): boolean {
  const g = graphOf(state);
  if (e.kind === "node") return graphIndex(g).node.get(e.node)?.visible ?? false;
  if (e.kind === "section") return g.sections?.find((s) => s.id === e.section)?.visible ?? false;
  const object = e.kind === "object" || e.kind === "face" ? e.object : (e.kind === "edge" || e.kind === "vertex") && e.owner.kind === "object" ? e.owner.id : null;
  if (object !== null) return graphIndex(g).object.get(object)?.visible ?? true;
  return true;
}

/** "Model › Lower › Shelf": where editing happens. */
export function contextTrail(g: SceneGraph): string {
  return ["Model", ...g.path.map((id) => nodeById(g, id)?.label ?? "Group")].join(" › ");
}

/** Everything a node selection covers, for highlights and exports. */
export function expandNodes(es: Entity[], g: SceneGraph): Entity[] {
  return es.flatMap((e) => (e.kind === "node" ? solidsIn(g, e.node).map((o) => ({ kind: "object" as const, object: o })) : [e]));
}
