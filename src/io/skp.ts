// SketchUp (.skp) import through the npm `openskp` package (MIT), in the webview (ADR-0002).
//
// openskp parses the file (SketchUp 2013-2020 "MFC" containers and 2021+ "VFF" ones) into
// definitions, instances, faces, edges, layers, materials and scenes. This module maps
// that model onto ONE kernel `import` op, so the kernel builds every group and component
// placement through its own validated path: closed shells become solids, everything else a
// visible non-solid mesh with a note. What is and is not carried over is listed in
// docs/skp-compat.md; keep the two in step.
//
// Units: SketchUp stores inches, Z up. The kernel is metres, Z up. Nothing else changes.
import { parseSkp, type Definition, type Edge, type Face, type Instance, type SkpModel } from "openskp";
import type { ImportItem, KernelOp, MaterialDef, SceneDef, V3 } from "../kernel/types";

export const M_PER_IN = 0.0254;

/** The tag given to instances SketchUp had hidden (it comes in hidden). */
export const HIDDEN_TAG = "Hidden in SketchUp";

/** SketchUp's default layer: geometry on it is untagged. */
const DEFAULT_LAYERS = new Set(["", "Layer0", "Untagged"]);

/** A 3x4 SketchUp placement: row-major 3x3 rotation/scale, then translation (inches). */
type Placement = number[];

const IDENTITY: Placement = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0];

/** parent * child (apply child, then parent). */
export function compose(a: Placement, b: Placement): Placement {
  const r = (m: Placement, i: number, j: number) => m[i * 3 + j];
  const out = new Array(12).fill(0);
  for (let i = 0; i < 3; i++)
    for (let j = 0; j < 3; j++) out[i * 3 + j] = r(a, i, 0) * r(b, 0, j) + r(a, i, 1) * r(b, 1, j) + r(a, i, 2) * r(b, 2, j);
  for (let i = 0; i < 3; i++) out[9 + i] = r(a, i, 0) * b[9] + r(a, i, 1) * b[10] + r(a, i, 2) * b[11] + a[9 + i];
  return out;
}

export function apply(m: Placement, p: V3): V3 {
  return [
    m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[9],
    m[3] * p[0] + m[4] * p[1] + m[5] * p[2] + m[10],
    m[6] * p[0] + m[7] * p[1] + m[8] * p[2] + m[11],
  ];
}

function det3(m: Placement): number {
  return m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6]) + m[2] * (m[3] * m[7] - m[4] * m[6]);
}

function placement(inst: Instance): Placement {
  const m = inst.matrix;
  if (m && m.length >= 12) return m.slice(0, 12);
  if (m && m.length === 16)
    // Column-major 4x4: rows of the 3x3 and the translation column.
    return [m[0], m[4], m[8], m[1], m[5], m[9], m[2], m[6], m[10], m[12], m[13], m[14]];
  return IDENTITY;
}

/** Newell normal of a ring. */
function newell(pts: V3[]): V3 {
  const n: V3 = [0, 0, 0];
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % pts.length];
    n[0] += (a[1] - b[1]) * (a[2] + b[2]);
    n[1] += (a[2] - b[2]) * (a[0] + b[0]);
    n[2] += (a[0] - b[0]) * (a[1] + b[1]);
  }
  return n;
}

export interface SkpSummary {
  version: string;
  definitions: number;
  placements: number;
  faces: number;
  looseEdges: number;
  materials: number;
  textures: number;
  layers: number;
  scenes: number;
}

export interface SkpImport {
  op: KernelOp;
  /** What was assumed, flattened or left out, in plain words. */
  notes: string[];
  summary: SkpSummary;
}

interface Ctx {
  model: SkpModel;
  items: ImportItem[];
  notes: Set<string>;
  usedMaterials: Set<string>;
  usedTags: Set<string>;
  hiddenTags: Set<string>;
  summary: SkpSummary;
  groupNames: Map<string, number>;
  skipped: Map<string, number>;
}

function materialName(ctx: Ctx, id: number | null | undefined): string | null {
  if (id === null || id === undefined) return null;
  const m = ctx.model.materialsById.get(id);
  return m ? m.name : null;
}

function bump(map: Map<string, number>, key: string, n = 1) {
  map.set(key, (map.get(key) ?? 0) + n);
}

/** One definition's own faces and loose edges, placed by `m`, as an import item. */
function itemOf(ctx: Ctx, def: Definition, m: Placement, name: string, opts: { group?: string; tag?: string | null; inherited?: number | null }): ImportItem | null {
  const verts = new Map<number, V3>(def.vertices.map((v) => [v.id, [v.x, v.y, v.z]]));
  const edges = new Map<number, Edge>(def.edges.map((e) => [e.id, e]));
  const positions: V3[] = [];
  const index = new Map<number, number>();
  const at = (vid: number): number | null => {
    const p = verts.get(vid);
    if (!p) return null;
    let i = index.get(vid);
    if (i === undefined) {
      const w = apply(m, p);
      positions.push([w[0] * M_PER_IN, w[1] * M_PER_IN, w[2] * M_PER_IN]);
      i = positions.length - 1;
      index.set(vid, i);
    }
    return i;
  };
  const mirrored = det3(m) < 0;
  const faces: number[][][] = [];
  const used = new Set<number>();
  const paint = new Map<string, number>();
  const loopVerts = (loop: Face["loops"][number]): number[] => {
    const out: number[] = [];
    for (const ce of loop) {
      const e = edges.get(ce.edgeId);
      if (!e) continue;
      used.add(e.id);
      const v = ce.orientation === 1 ? e.v1Id : e.v2Id;
      if (out[out.length - 1] !== v) out.push(v);
    }
    if (out.length > 1 && out[0] === out[out.length - 1]) out.pop();
    return out;
  };
  for (const f of def.faces) {
    const rings = f.loops.map(loopVerts).filter((r) => r.length >= 3);
    if (!rings.length || f.loops.length === 0 || loopVerts(f.loops[0]).length < 3) {
      bump(ctx.skipped, "faces without a usable outer loop");
      continue;
    }
    // Wind every face around its stored (front) normal, in the definition's own frame.
    const outerPts = rings[0].map((v) => verts.get(v)!).filter(Boolean);
    const nn = newell(outerPts);
    const agrees = nn[0] * f.normal[0] + nn[1] * f.normal[1] + nn[2] * f.normal[2] >= 0;
    const flip = agrees === mirrored;
    const idx = rings.map((r) => {
      const ring = r.map(at).filter((x): x is number => x !== null);
      return flip ? ring.reverse() : ring;
    });
    if (idx[0].length < 3) continue;
    faces.push(idx);
    const mat = materialName(ctx, f.materialId ?? opts.inherited ?? null);
    if (mat) bump(paint, mat);
    ctx.summary.faces++;
  }
  const lines: [number, number][] = [];
  for (const e of def.edges) {
    if (used.has(e.id) || e.hidden) continue;
    const a = at(e.v1Id);
    const b = at(e.v2Id);
    if (a !== null && b !== null && a !== b) lines.push([a, b]);
  }
  ctx.summary.looseEdges += lines.length;
  if (!faces.length && !lines.length) return null;
  let material: string | null = null;
  if (paint.size) {
    material = [...paint.entries()].sort((x, y) => y[1] - x[1] || (x[0] < y[0] ? -1 : 1))[0][0];
    ctx.usedMaterials.add(material);
    if (paint.size > 1) ctx.notes.add(`${name}: faces painted with ${paint.size} materials; the object takes the most used (${material})`);
  }
  if (opts.tag) ctx.usedTags.add(opts.tag);
  return {
    name,
    group: opts.group ?? null,
    tag: opts.tag ?? null,
    material,
    positions,
    faces,
    lines,
    merge_coplanar: false,
  };
}

function uniqueGroup(ctx: Ctx, name: string): string {
  const n = (ctx.groupNames.get(name) ?? 0) + 1;
  ctx.groupNames.set(name, n);
  return n === 1 ? name : `${name} #${n}`;
}

function layerTag(layer: string | undefined | null): string | null {
  const l = (layer ?? "").trim();
  return DEFAULT_LAYERS.has(l) ? null : l;
}

/** Place `def`'s instances (recursively) under placement `m`. */
function walk(ctx: Ctx, def: Definition, m: Placement, depth: number, inherited: { group?: string; tag: string | null; material: number | null; hidden: boolean }) {
  if (depth > 64) {
    ctx.notes.add("components nested deeper than 64 levels were left out (a cycle?)");
    return;
  }
  for (const inst of def.instances) {
    const child = ctx.model.definitions.get(inst.refIdx);
    if (!child) {
      bump(ctx.skipped, "instances of missing definitions");
      continue;
    }
    if (child.isImage) {
      bump(ctx.skipped, "images");
      continue;
    }
    ctx.summary.placements++;
    const mm = compose(m, placement(inst));
    const name = (inst.name || child.name || "Component").trim() || "Component";
    const group = depth === 0 ? uniqueGroup(ctx, name) : inherited.group;
    const hidden = inherited.hidden || inst.hidden;
    const tag = hidden ? HIDDEN_TAG : (layerTag(inst.layer) ?? inherited.tag);
    if (hidden) ctx.hiddenTags.add(HIDDEN_TAG);
    const material = inst.materialId ?? inherited.material;
    const item = itemOf(ctx, child, mm, name, { group, tag, inherited: material });
    if (item) ctx.items.push(item);
    for (const k of ["sectionPlanes", "texts", "dimensions", "constructionLines", "constructionPoints"] as const) {
      const n = (child[k] as unknown[] | undefined)?.length ?? 0;
      if (n) bump(ctx.skipped, k);
    }
    walk(ctx, child, mm, depth + 1, { group, tag, material, hidden });
  }
}

const SKIPPED_LABEL: Record<string, string> = {
  sectionPlanes: "section planes",
  texts: "texts",
  dimensions: "dimensions",
  constructionLines: "guide lines",
  constructionPoints: "guide points",
};

/** Map a parsed SketchUp model onto one kernel import op. */
export function modelToOp(model: SkpModel, fileName: string): SkpImport {
  const stem = fileName.replace(/^.*[\\/]/, "").replace(/\.[^.]*$/, "") || "SketchUp model";
  const ctx: Ctx = {
    model,
    items: [],
    notes: new Set(),
    usedMaterials: new Set(),
    usedTags: new Set(),
    hiddenTags: new Set(),
    groupNames: new Map(),
    skipped: new Map(),
    summary: {
      version: model.version,
      definitions: Math.max(0, model.definitions.size),
      placements: 0,
      faces: 0,
      looseEdges: 0,
      materials: model.materials.length,
      textures: model.materials.filter((m) => m.texture).length,
      layers: model.layers.length,
      scenes: model.pages.length,
    },
  };
  // Loose geometry drawn at the top level of the model.
  const root = itemOf(ctx, model.root, IDENTITY, stem, {});
  if (root) ctx.items.push(root);
  for (const k of ["sectionPlanes", "texts", "dimensions", "constructionLines", "constructionPoints"] as const) {
    const n = (model.root[k] as unknown[] | undefined)?.length ?? 0;
    if (n) bump(ctx.skipped, k, n);
  }
  walk(ctx, model.root, IDENTITY, 0, { tag: null, material: null, hidden: false });

  const materials: MaterialDef[] = model.materials
    .filter((m) => ctx.usedMaterials.has(m.name))
    .map((m) => {
      const a = m.color.a !== undefined && m.color.a < 255 ? m.color.a : Math.round(255 * (1 - (m.transparency ?? 0)));
      return { name: m.name, color: [m.color.r, m.color.g, m.color.b, Math.max(0, Math.min(255, a))], texture: m.texture?.filename || null };
    });
  for (const l of model.layers) if (l.hidden && layerTag(l.name)) ctx.hiddenTags.add(l.name.trim());
  const scenes: SceneDef[] = model.pages
    .filter((p) => p.eye && p.target)
    .map((p) => ({
      name: p.name || "Scene",
      eye: p.eye!.map((x) => x * M_PER_IN) as V3,
      target: p.target!.map((x) => x * M_PER_IN) as V3,
      up: (p.up ?? [0, 0, 1]) as V3,
      fov: p.fov || 35,
      parallel: !!p.parallel,
      hidden_tags: p.hiddenLayers.filter((l) => layerTag(l)),
    }));
  if (model.pages.length > scenes.length) ctx.notes.add(`${model.pages.length - scenes.length} scene(s) without a camera were left out`);
  if (ctx.summary.textures) ctx.notes.add(`${ctx.summary.textures} textured material(s): colours imported, textures kept as file-name references (not drawn yet)`);
  if (ctx.groupNames.size && [...ctx.groupNames.values()].some((n) => n > 1))
    ctx.notes.add("components placed more than once came in as separate copies (one group per placement; editing one does not change the others)");
  for (const [k, n] of ctx.skipped) ctx.notes.add(`not imported: ${n} ${SKIPPED_LABEL[k] ?? k}`);
  if (model.units) ctx.notes.add(`model units: ${model.units} (geometry is converted from inches)`);
  const hidden_tags = [...ctx.hiddenTags];
  return {
    op: { op: "import", items: ctx.items, materials, scenes, hidden_tags },
    notes: [...ctx.notes],
    summary: ctx.summary,
  };
}

/** Parse a .skp file's bytes and map them onto one kernel import op. */
export function skpToOp(bytes: ArrayBuffer | Uint8Array, fileName: string): SkpImport {
  const buf = bytes instanceof Uint8Array ? bytes.slice().buffer : bytes;
  let model: SkpModel;
  try {
    model = parseSkp(buf as ArrayBuffer);
  } catch (e) {
    throw new Error(`could not read the SketchUp file: ${e instanceof Error ? e.message : String(e)}`);
  }
  return modelToOp(model, fileName);
}
