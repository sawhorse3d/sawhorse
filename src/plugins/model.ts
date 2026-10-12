// The model as plugins read it (packages/plugin-api/index.d.ts): plain, documented shapes
// made from the kernel's document state, so the API stays put when the kernel's own
// structures grow. Pure functions, unit-tested in model.test.ts.
import type { CutList, DocState, Entity, Inference, Picked, SurfaceHit, V3 } from "../kernel/types";
import { graphOf, objectEditable } from "../lib/scope";

type Json = null | boolean | number | string | Json[] | { [k: string]: Json };

export interface PluginSolid {
  id: number;
  number: number;
  name: string | null;
  label: string;
  tag: string | null;
  material: string | null;
  parent: number | null;
  min: V3;
  max: V3;
  volume: number;
  visible: boolean;
  locked: boolean;
  editable: boolean;
  inComponent: boolean;
}

export function solidsOf(state: DocState | null): PluginSolid[] {
  if (!state) return [];
  const g = graphOf(state);
  const info = new Map(state.object_info.map((o) => [o.id, o]));
  const leaf = new Map(g.objects.map((o) => [o.id, o]));
  return state.solids.map((s) => {
    const i = info.get(s.id);
    const l = leaf.get(s.id);
    return {
      id: s.id,
      number: s.number,
      name: i?.name ?? null,
      label: i?.name ?? `Solid ${s.number}`,
      tag: i?.tag ?? null,
      material: i?.material ?? null,
      parent: l?.parent ?? null,
      min: s.min,
      max: s.max,
      volume: s.volume,
      visible: l?.visible ?? true,
      locked: l?.locked ?? false,
      editable: objectEditable(state, s.id),
      inComponent: l?.in_component ?? false,
    };
  });
}

export function nodesOf(state: DocState | null) {
  return graphOf(state).nodes.map((n) => ({
    id: n.id,
    kind: n.kind,
    name: n.name,
    label: n.label,
    definition: n.definition,
    parent: n.parent,
    tag: n.tag,
    hidden: n.hidden,
    locked: n.locked,
    visible: n.visible,
    transform: n.xform,
    inComponent: n.in_component,
  }));
}

export function definitionsOf(state: DocState | null) {
  return graphOf(state).definitions.map((d) => ({ id: d.id, name: d.name, instances: d.instances, solids: d.solids, min: d.min, max: d.max }));
}

export interface TreeNode {
  kind: "model" | "group" | "instance" | "solid" | "mesh";
  id: number;
  label: string;
  open?: boolean;
  children: TreeNode[];
}

/** The Outliner's tree: groups and instances (in id order), then solids, then meshes. */
export function treeOf(state: DocState | null): TreeNode {
  const root: TreeNode = { kind: "model", id: 0, label: "Model", children: [] };
  if (!state) return root;
  const g = graphOf(state);
  const solidLabel = new Map(solidsOf(state).map((s) => [s.id, s.label]));
  const meshName = new Map((state.meshes ?? []).map((m) => [m.id, m.name]));
  const byParent = new Map<number | null, TreeNode[]>();
  const add = (parent: number | null, t: TreeNode) => {
    const list = byParent.get(parent) ?? [];
    list.push(t);
    byParent.set(parent, list);
  };
  const nodes = new Map<number, TreeNode>();
  for (const n of g.nodes) {
    const t: TreeNode = { kind: n.kind, id: n.id, label: n.label, children: [] };
    if (g.path.includes(n.id)) t.open = true;
    nodes.set(n.id, t);
    add(n.parent, t);
  }
  for (const o of g.objects) add(o.parent, { kind: "solid", id: o.id, label: solidLabel.get(o.id) ?? `Solid ${o.id}`, children: [] });
  for (const m of g.meshes) add(m.parent, { kind: "mesh", id: m.id, label: meshName.get(m.id) ?? "Mesh", children: [] });
  root.children = byParent.get(null) ?? [];
  for (const [id, t] of nodes) t.children = byParent.get(id) ?? [];
  return root;
}

export function statsOf(state: DocState | null) {
  if (!state) return { solids: 0, groups: 0, components: 0, instances: 0, faces: 0, edges: 0, vertices: 0, sketches: 0, meshes: 0, volume: 0, watertight: true };
  const g = graphOf(state);
  const s = state.stats;
  return {
    solids: s.objects,
    groups: g.nodes.filter((n) => n.kind === "group").length,
    components: g.definitions.length,
    instances: g.nodes.filter((n) => n.kind === "instance").length,
    faces: s.faces,
    edges: s.edges,
    vertices: s.vertices,
    sketches: s.sketches,
    meshes: state.meshes?.length ?? 0,
    volume: s.volume_m3,
    watertight: s.watertight,
  };
}

export type PluginSelectionItem =
  | { kind: "solid"; id: number }
  | { kind: "group" | "instance"; id: number }
  | { kind: "face"; solid: number; face: number }
  | { kind: "edge"; owner: { kind: "solid" | "sketch"; id: number }; a: V3; b: V3 }
  | { kind: "vertex"; owner: { kind: "solid" | "sketch"; id: number }; at: V3 }
  | { kind: "region"; sketch: number; region: number }
  | { kind: "guide"; id: number }
  | { kind: "dimension"; id: number };

/** An app entity as a plugin sees it (null for what the API does not name). */
export function itemOfEntity(e: Entity | null | undefined, state: DocState | null): PluginSelectionItem | null {
  if (!e) return null;
  switch (e.kind) {
    case "object":
      return { kind: "solid", id: e.object };
    case "node": {
      const n = graphOf(state).nodes.find((x) => x.id === e.node);
      return { kind: n?.kind === "instance" ? "instance" : "group", id: e.node };
    }
    case "face":
      return { kind: "face", solid: e.object, face: e.face };
    case "edge":
      if (e.owner.kind === "guide") return { kind: "guide", id: e.owner.id };
      return { kind: "edge", owner: { kind: e.owner.kind === "object" ? "solid" : "sketch", id: e.owner.id }, a: e.a, b: e.b };
    case "vertex":
      if (e.owner.kind === "guide") return { kind: "guide", id: e.owner.id };
      return { kind: "vertex", owner: { kind: e.owner.kind === "object" ? "solid" : "sketch", id: e.owner.id }, at: e.at };
    case "region":
      return { kind: "region", sketch: e.sketch, region: e.region };
    case "guide":
      return { kind: "guide", id: e.guide };
    case "dimension":
      return { kind: "dimension", id: e.dimension };
  }
}

export function selectionOf(es: Entity[], state: DocState | null): PluginSelectionItem[] {
  return es.flatMap((e) => {
    const i = itemOfEntity(e, state);
    return i ? [i] : [];
  });
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const idOf = (v: unknown, what: string): number => {
  if (typeof v !== "number" || !Number.isSafeInteger(v) || v < 0) throw new TypeError(`${what} must be a whole number`);
  return v;
};

/** What `model.select` / `model.bounds` take, as app entities. Unknown ids are refused. */
export function entitiesOfTargets(targets: unknown, state: DocState | null): Entity[] {
  if (!Array.isArray(targets)) throw new TypeError("select takes a list of { kind, id } items");
  if (targets.length > 10_000) throw new TypeError("select takes at most 10,000 items");
  const g = graphOf(state);
  const solids = new Set(state?.objects ?? []);
  return targets.map((t, i) => {
    if (!isObj(t)) throw new TypeError(`item ${i} must be an object`);
    if (t.kind === "solid") {
      const id = idOf(t.id, `item ${i}.id`);
      if (!solids.has(id)) throw new TypeError(`there is no solid ${id}`);
      return { kind: "object", object: id };
    }
    if (t.kind === "group" || t.kind === "instance" || t.kind === "node") {
      const id = idOf(t.id, `item ${i}.id`);
      if (!g.nodes.some((n) => n.id === id)) throw new TypeError(`there is no group or instance ${id}`);
      return { kind: "node", node: id };
    }
    if (t.kind === "face") {
      const solid = idOf(t.solid, `item ${i}.solid`);
      if (!solids.has(solid)) throw new TypeError(`there is no solid ${solid}`);
      return { kind: "face", object: solid, face: idOf(t.face, `item ${i}.face`) };
    }
    throw new TypeError(`item ${i}: kind must be "solid", "group", "instance" or "face"`);
  });
}

/** The box round some solids, groups and instances (everything when `es` is null), or null. */
export function boundsOf(es: Entity[] | null, state: DocState | null): { min: V3; max: V3 } | null {
  if (!state) return null;
  const g = graphOf(state);
  let ids: Set<number>;
  if (es === null) ids = new Set(state.objects);
  else {
    ids = new Set();
    const under = (node: number): number[] => {
      const out = g.objects.filter((o) => o.parent === node).map((o) => o.id);
      for (const n of g.nodes) if (n.parent === node) out.push(...under(n.id));
      return out;
    };
    for (const e of es) {
      if (e.kind === "object" || e.kind === "face") ids.add(e.object);
      if (e.kind === "node") under(e.node).forEach((o) => ids.add(o));
    }
  }
  const boxes = state.solids.filter((s) => ids.has(s.id));
  if (!boxes.length) return null;
  const min = [0, 1, 2].map((k) => Math.min(...boxes.map((b) => b.min[k]))) as V3;
  const max = [0, 1, 2].map((k) => Math.max(...boxes.map((b) => b.max[k]))) as V3;
  return { min, max };
}

/** The cut list, plugin-shaped (metres, square metres, board feet). */
export function cutListOf(c: CutList) {
  return {
    rows: c.rows.map((r) => ({
      name: r.name,
      qty: r.qty,
      thickness: r.size[0],
      width: r.size[1],
      length: r.size[2],
      material: r.material,
      grain: r.grain,
      sheet: r.sheet,
      boardFeet: r.board_feet,
      area: r.area_m2,
      shaped: r.shaped,
      handed: r.handed,
      hidden: r.hidden,
    })),
    totals: {
      parts: c.totals.parts,
      pieces: c.totals.pieces,
      boardFeet: c.totals.board_feet,
      lumber: c.totals.lumber.map((l) => ({ material: l.material, boardFeet: l.board_feet })),
      sheets: c.totals.sheets.map((s) => ({ material: s.material, thickness: s.thickness, area: s.area_m2, sheets: s.sheets })),
      sheetCount: c.totals.sheet_count,
    },
    hash: c.hash,
  };
}

function surfaceOf(s: SurfaceHit | null | undefined) {
  if (!s) return null;
  const at = s.at.kind === "face" ? { kind: "face" as const, solid: s.at.object, face: s.at.face } : { kind: "region" as const, sketch: s.at.sketch, region: s.at.region };
  return { at, frame: s.frame, point: s.point };
}

/** A tool event's picture (snap and pick), plugin-shaped. */
export function snapOfEvent(snap: { point: V3; kind: string; tip: string; surface: SurfaceHit | null; entity: Entity | null } | null, picked: Picked | null, state: DocState | null) {
  return {
    snap: snap ? { point: snap.point, kind: snap.kind, tip: snap.tip, surface: surfaceOf(snap.surface), entity: itemOfEntity(snap.entity, state) } : null,
    picked: picked ? { entity: itemOfEntity(picked.entity, state), point: picked.point, surface: surfaceOf(picked.surface) } : null,
  };
}

export type { Inference, Json };
