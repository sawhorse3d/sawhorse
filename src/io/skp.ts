// SketchUp (.skp) import through the npm `openskp` package (MIT), in the webview (ADR-0002).
//
// openskp parses the file (SketchUp 2013-2020 "MFC" containers and 2021+ "VFF" ones) into
// definitions, instances, faces, edges, layers, materials and scenes. This module maps
// that model onto ONE kernel `import` op, so the kernel builds everything through its own
// validated path: closed shells become solids, everything else a visible non-solid mesh
// with a note. What is and is not carried over is listed in docs/skp-compat.md; keep the
// two in step.
//
// Structure (lane 5, lane 15): a SketchUp definition placed more than once becomes a
// component definition, its placements instances of it (an edit inside one changes them
// all). A definition placed once is a component too when someone gave it a name (a "Leg"),
// and a group when its name is SketchUp's own ("Group#3", "Component#7") or empty. Nested
// placements nest the same way. Hidden placements come in hidden; a placement's layer
// becomes its tag.
//
// Edges (lane 15): SketchUp's soft, smooth and hidden flags come across per edge (the
// item's `edge_flags`), so a circle pulled into a cylinder is round, not 24 lines.
//
// An empty file (nothing drawn, nothing placed: a blank template) is not an import: the
// result says `empty`, with the file's units and style, and the app opens it as an empty
// model in those units.
//
// Units: SketchUp stores inches, Z up. The kernel is metres, Z up. Nothing else changes.
//
// Materials (lane 10): every face keeps its own material (a face without one takes the
// placement's, as SketchUp draws it); a material's texture image comes in (PNG or JPEG, as
// an `add_texture` op batched before the import) with its size in inches; a face's texture
// position (SketchUp's `uvTransform`, or the default projection) comes in as the affine
// mapping it gives in world metres, which the kernel fits to a scale, a rotation and an
// offset (skew and projected textures are dropped, with a note).
//
// Lane 16, limits: a SketchUp 2021+ file is a zip, and openskp's own guard reads only
// declared sizes per entry; before it parses, the archive's directory is read here with
// nothing inflated, and a file declaring more than SKP_LIMITS.inflated bytes or entries
// in all is refused. While mapping, groups (which, unlike components, are expanded once
// per placement) and their geometry are counted against the import limits, and a texture
// image too large for the kernel (bytes, pixels, or past the 512th) is left out with a note
// instead of refusing the whole import.
import { unzipSync } from "fflate";
import { parseSkp, type Definition, type Edge, type Face, type Instance, type SkpModel } from "openskp";
import { imageSize } from "../lib/imageSize";
import type { FaceStyle, ImportDefinition, ImportItem, ImportNode, ImportSection, KernelOp, MaterialDef, OpOutcome, SceneDef, V3, Xform } from "../kernel/types";
import type { UnitSystem } from "../lib/units";

export const M_PER_IN = 0.0254;

/** What one .skp may expand to (lane 16): bytes and entries its archive declares, and the
 * groups, instances, points and faces mapping it may produce (crates/kernel/src/limits.rs
 * keeps the kernel's own, which checks the op again). */
export const SKP_LIMITS = {
  inflated: 512 * 1024 * 1024,
  entries: 5000,
  nodes: 100_000,
  points: 3_000_000,
  faces: 1_500_000,
  textureBytes: 16 * 1024 * 1024,
  texturePixels: 4096 * 4096,
  textures: 512,
};

/** A file that expands past SKP_LIMITS. */
export class SkpTooLarge extends Error {
  constructor(message: string) {
    super(message);
    this.name = "SkpTooLarge";
  }
}

/**
 * Lane 16: a zip-format .skp's directory, read with nothing inflated (fflate's filter sees
 * every entry's declared size and is told to take none): refused past SKP_LIMITS.
 */
export function checkSkpArchive(bytes: Uint8Array): void {
  if (bytes.length < 4 || bytes[0] !== 0x50 || bytes[1] !== 0x4b) return;
  let entries = 0;
  let total = 0;
  try {
    unzipSync(bytes, {
      filter: (f) => {
        entries++;
        total += f.originalSize;
        if (entries > SKP_LIMITS.entries) throw new SkpTooLarge(`the SketchUp file holds more than ${SKP_LIMITS.entries} entries`);
        if (total > SKP_LIMITS.inflated) throw new SkpTooLarge(`the SketchUp file unpacks to more than ${SKP_LIMITS.inflated / 1024 / 1024} MB`);
        return false;
      },
    });
  } catch (e) {
    if (e instanceof SkpTooLarge) throw e;
    // Not a zip after all (openskp says why), or a broken one: openskp decides.
  }
}

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
  /** Definitions that came in as components (placed more than once). */
  components: number;
  /** Placements that came in as groups. */
  groups: number;
  faces: number;
  looseEdges: number;
  /** Lane 15: edges that came in soft, smooth or hidden. */
  softEdges: number;
  materials: number;
  textures: number;
  layers: number;
  scenes: number;
  /** Section planes imported (lane 12). */
  sections: number;
}

export interface SkpImport {
  /** `null` for an empty file (see `empty`). */
  op: KernelOp | null;
  /** What was assumed, flattened or left out, in plain words. */
  notes: string[];
  summary: SkpSummary;
  /** Lane 15: the file has nothing in it (a blank template): open it as an empty model
   * with its units and style instead of importing nothing. */
  empty?: SkpEmpty;
}

/** What an empty file still says (lane 15). */
export interface SkpEmpty {
  /** The file's unit system as Sawhorse names it, when the file states one. */
  units: UnitSystem | null;
  /** SketchUp's word for it ("Millimeter"), as the file states it. */
  unitsWord: string | null;
  /** The first style the file holds, by name, and the face mode it maps to (null: ours). */
  style: { name: string; face: FaceStyle | null } | null;
}

/** SketchUp's unit word (VFF `meta.dat`) as a Sawhorse unit system. */
export function unitsOf(word: string | null | undefined): UnitSystem | null {
  const w = (word ?? "").trim().toLowerCase();
  if (!w) return null;
  if (w.startsWith("milli")) return "mm";
  if (w.startsWith("centi")) return "cm";
  if (w.startsWith("met")) return "m";
  if (w.startsWith("feet") || w.startsWith("foot")) return "ft-in";
  if (w.startsWith("inch")) return "frac-in";
  return null;
}

/** A SketchUp style name as one of Sawhorse's face modes (null: no close match; ours stays). */
export function faceOfStyle(name: string): FaceStyle | null {
  const n = name.toLowerCase();
  if (/x-?ray/.test(n)) return "x_ray";
  if (/wire ?frame/.test(n)) return "wireframe";
  if (/hidden ?line/.test(n)) return "hidden_line";
  if (/mono ?chrome/.test(n)) return "monochrome";
  if (/shaded/.test(n) && !/texture/.test(n)) return "shaded";
  return null;
}

interface Ctx {
  model: SkpModel;
  items: ImportItem[];
  notes: Set<string>;
  usedMaterials: Set<string>;
  /** Material name -> its texture image as it comes into the model (lane 10). */
  textures: Map<string, { path: string; mime: string; bytes: Uint8Array }>;
  usedTags: Set<string>;
  hiddenTags: Set<string>;
  summary: SkpSummary;
  skipped: Map<string, number>;
  /** How many times each definition is placed, anywhere in the file. */
  placed: Map<number, number>;
  /** Component definitions for the kernel, children before parents. */
  defs: ImportDefinition[];
  /** (definition id, inherited material) -> index into `defs`. */
  defIndex: Map<string, number>;
  /** Lane 16: what mapping has produced so far, held to SKP_LIMITS. */
  spent: { nodes: number; points: number; faces: number };
}

/** Count what mapping produced; refuse past the limits. */
function spend(ctx: Ctx, nodes: number, points: number, faces: number): void {
  const s = ctx.spent;
  s.nodes += nodes;
  s.points += points;
  s.faces += faces;
  if (s.nodes > SKP_LIMITS.nodes) throw new SkpTooLarge(`the SketchUp file expands to more than ${SKP_LIMITS.nodes} groups and copies (groups placed inside groups placed many times?)`);
  if (s.points > SKP_LIMITS.points) throw new SkpTooLarge(`the SketchUp file expands to more than ${SKP_LIMITS.points} points`);
  if (s.faces > SKP_LIMITS.faces) throw new SkpTooLarge(`the SketchUp file expands to more than ${SKP_LIMITS.faces} faces`);
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
  const faceMaterials: (string | null)[] = [];
  const faceUvs: ([number[], number[]] | null)[] = [];
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
    const matId = f.materialId ?? opts.inherited ?? null;
    const mat = materialName(ctx, matId);
    if (mat) {
      bump(paint, mat);
      ctx.usedMaterials.add(mat);
    }
    faceMaterials.push(mat);
    faceUvs.push(mat ? faceUv(ctx, f, matId, outerPts, m) : null);
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
  // Lane 15: soft (1), smooth (2) and hidden (4) edges, on points already in the item.
  const edgeFlags: [number, number, number][] = [];
  for (const e of def.edges) {
    const f = (e.soft ? 1 : 0) | (e.smooth ? 2 : 0) | (e.hidden ? 4 : 0);
    const a = index.get(e.v1Id);
    const b = index.get(e.v2Id);
    if (f && a !== undefined && b !== undefined && a !== b) edgeFlags.push([a, b, f]);
  }
  ctx.summary.softEdges += edgeFlags.length;
  let material: string | null = null;
  if (paint.size) {
    // The solid's own material is the one on most faces; the others are face paint.
    material = [...paint.entries()].sort((x, y) => y[1] - x[1] || (x[0] < y[0] ? -1 : 1))[0][0];
    ctx.usedMaterials.add(material);
  }
  const perFace = faceMaterials.some((x) => x !== material) || faceUvs.some((x) => x !== null);
  if (opts.tag) ctx.usedTags.add(opts.tag);
  spend(ctx, 0, positions.length, faces.length + lines.length);
  return {
    name,
    group: opts.group ?? null,
    tag: opts.tag ?? null,
    material,
    positions,
    faces,
    lines,
    merge_coplanar: false,
    ...(perFace ? { face_materials: faceMaterials, face_uvs: faceUvs } : {}),
    ...(edgeFlags.length ? { edge_flags: edgeFlags } : {}),
  };
}

/** Inverse of a row-major 3x3, or null when it is singular. */
function inv3(a: number[]): number[] | null {
  const [m0, m1, m2, m3, m4, m5, m6, m7, m8] = a;
  const c0 = m4 * m8 - m5 * m7;
  const c1 = m5 * m6 - m3 * m8;
  const c2 = m3 * m7 - m4 * m6;
  const det = m0 * c0 + m1 * c1 + m2 * c2;
  if (!Number.isFinite(det) || Math.abs(det) < 1e-12) return null;
  const k = 1 / det;
  return [c0 * k, (m2 * m7 - m1 * m8) * k, (m1 * m5 - m2 * m4) * k, c1 * k, (m0 * m8 - m2 * m6) * k, (m2 * m3 - m0 * m5) * k, c2 * k, (m1 * m6 - m0 * m7) * k, (m0 * m4 - m1 * m3) * k];
}

const dot3 = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross3 = (a: V3, b: V3): V3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const unit = (a: V3): V3 => {
  const l = Math.hypot(a[0], a[1], a[2]) || 1;
  return [a[0] / l, a[1] / l, a[2] / l];
};

/**
 * A face's texture mapping as world-metre affine rows (u, v in repeats), from SketchUp's
 * face basis (xr = Z x n, yr = n x xr; X and +-Y for a level face), the face's uvTransform
 * (texture space to face plane, row-major) or the default projection, and the material's
 * tile size in inches. Null for an untextured material, or a projective (4-pin) mapping.
 */
function faceUv(ctx: Ctx, f: Face, matId: number | null, outer: V3[], m: Placement): [number[], number[]] | null {
  if (matId === null) return null;
  const mat = ctx.model.materialsById.get(matId);
  const tex = mat?.texture;
  if (!tex || !texturePath(ctx, mat!.name)) return null;
  if (f.uvProjected) {
    ctx.notes.add("projected textures (terrain drapes) came in at their default position");
    return null;
  }
  const n = unit(newell(outer));
  if (!Number.isFinite(n[0])) return null;
  const vertical = Math.abs(n[2]) > 1 - 1e-9;
  const xr: V3 = vertical ? [1, 0, 0] : unit(cross3([0, 0, 1], n));
  const yr: V3 = vertical ? [0, Math.sign(n[2]) || 1, 0] : cross3(n, xr);
  const tw = tex.width > 1e-9 ? tex.width : 1;
  const th = tex.height > 1e-9 ? tex.height : 1;
  const t = f.uvTransform && f.uvTransform.length >= 9 ? inv3(f.uvTransform.slice(0, 9)) : null;
  if (f.uvTransform && !t) return null;
  if (t && (Math.abs(t[2]) > 1e-9 || Math.abs(t[5]) > 1e-9)) {
    ctx.notes.add("distorted (four-pin) textures came in without their distortion");
  }
  // uv of a definition point (inches), affine part of SketchUp's mapping.
  const uvOf = (p: V3): [number, number] => {
    const x = dot3(p, xr);
    const y = dot3(p, yr);
    if (!t) return [x / tw, y / th];
    const u = x * t[0] + y * t[3] + t[6];
    const v = x * t[1] + y * t[4] + t[7];
    const q = x * t[2] + y * t[5] + t[8];
    const w = Math.abs(q) > 1e-12 ? q : 1;
    return [u / w / tw, v / w / th];
  };
  // The affine map in world metres, from four points: the face's first corner and steps
  // along xr, yr and the normal (which changes nothing).
  const p0 = outer[0];
  const pts: V3[] = [p0, [p0[0] + xr[0], p0[1] + xr[1], p0[2] + xr[2]], [p0[0] + yr[0], p0[1] + yr[1], p0[2] + yr[2]], [p0[0] + n[0], p0[1] + n[1], p0[2] + n[2]]];
  const world = pts.map((p) => apply(m, p).map((c) => c * M_PER_IN) as V3);
  const uvs = pts.map(uvOf);
  const d = [1, 2, 3].map((i) => [world[i][0] - world[0][0], world[i][1] - world[0][1], world[i][2] - world[0][2]]);
  const dm = inv3([...d[0], ...d[1], ...d[2]]);
  if (!dm) return null;
  const row = (k: 0 | 1): number[] => {
    const r = [1, 2, 3].map((i) => uvs[i][k] - uvs[0][k]);
    // g . d_i = r_i  =>  g = D^-1 r (D has the d_i as rows).
    const g = [0, 1, 2].map((j) => dm[j * 3] * r[0] + dm[j * 3 + 1] * r[1] + dm[j * 3 + 2] * r[2]);
    return [...g, uvs[0][k] - (g[0] * world[0][0] + g[1] * world[0][1] + g[2] * world[0][2])];
  };
  return [row(0), row(1)];
}

/** The model path of a material's texture image, when it can come in (PNG or JPEG bytes). */
function texturePath(ctx: Ctx, materialName: string): string | null {
  return ctx.textures.get(materialName)?.path ?? null;
}

/** Image type from the first bytes. */
function sniff(d: Uint8Array): "image/png" | "image/jpeg" | null {
  if (d.length > 8 && d[0] === 0x89 && d[1] === 0x50 && d[2] === 0x4e && d[3] === 0x47) return "image/png";
  if (d.length > 3 && d[0] === 0xff && d[1] === 0xd8) return "image/jpeg";
  return null;
}

function base64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/** A SketchUp placement (inches) as a kernel transform (metres): the 3x3 is unitless, the
 * translation scales. Definition geometry is converted to metres in its own frame. */
export function placementXform(m: Placement): Xform {
  return {
    linear: [
      [m[0], m[1], m[2]],
      [m[3], m[4], m[5]],
      [m[6], m[7], m[8]],
    ],
    translation: [m[9] * M_PER_IN, m[10] * M_PER_IN, m[11] * M_PER_IN],
  };
}

/** SketchUp names its groups' hidden definitions "Group#1", "Group#2"... */
const GROUP_DEF = /^Group#\d+$/;
/** ...and a component nobody named "Component#1", "Component#2"... */
const DEFAULT_DEF = /^(Group|Component)#\d+$/;

/** A name someone gave the definition (not empty, not SketchUp's own numbering). */
export function userNamed(name: string): boolean {
  const n = name.trim();
  return n !== "" && !DEFAULT_DEF.test(n);
}

/** A definition comes in as a component when it is placed more than once (and is not one of
 * SketchUp's group definitions), or placed once with a name someone gave it (lane 15: a
 * "Leg" drawn once is still the Leg component); otherwise its placement is a group. */
export function isComponent(def: Definition, placed: number): boolean {
  const name = def.name.trim();
  if (placed > 1) return !GROUP_DEF.test(name);
  return placed === 1 && userNamed(name);
}

/** True when some face of the definition takes its material from the placement. */
function inherits(def: Definition): boolean {
  return def.faces.some((f) => f.materialId === null || f.materialId === undefined);
}

function countPlacements(model: SkpModel): Map<number, number> {
  const placed = new Map<number, number>();
  const seen = new Set<number>();
  const visit = (def: Definition, depth: number) => {
    if (depth > 64) return;
    for (const inst of def.instances) {
      placed.set(inst.refIdx, (placed.get(inst.refIdx) ?? 0) + 1);
      const child = model.definitions.get(inst.refIdx);
      // A definition's own placements count once, however often it is placed.
      if (child && !seen.has(child.id)) {
        seen.add(child.id);
        visit(child, depth + 1);
      }
    }
  };
  visit(model.root, 0);
  return placed;
}

function layerTag(layer: string | undefined | null): string | null {
  const l = (layer ?? "").trim();
  return DEFAULT_LAYERS.has(l) ? null : l;
}

/**
 * A SketchUp section plane (lane 12) in the frame `m` places it in (inches to metres). The
 * file stores the plane as a, b, c, d (a x + b y + c z + d = 0, inches); the side kept is
 * taken to be the one opposite (a, b, c), the way a plane laid on a face keeps what is behind
 * it (if a file's cut comes in the wrong way round, Reverse fixes it). Which plane was
 * active is not read (openskp does not expose it), so every plane comes in inactive.
 */
export function sectionOf(plane: { plane: number[]; name?: string; label?: string; hidden?: boolean }, m: Placement): ImportSection | null {
  const [a, b, c, d] = plane.plane ?? [];
  const n2 = a * a + b * b + c * c;
  if (![a, b, c, d].every(Number.isFinite) || !(n2 > 1e-18)) return null;
  // A point on the plane, and the kept direction, in the definition's frame (inches).
  const p: V3 = [(-d * a) / n2, (-d * b) / n2, (-d * c) / n2];
  const keep: V3 = [-a, -b, -c];
  // The point placed; the normal by the inverse transpose of the placement's 3x3.
  const q = apply(m, p);
  const inv = inv3([m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8]]);
  if (!inv) return null;
  const n: V3 = [inv[0] * keep[0] + inv[3] * keep[1] + inv[6] * keep[2], inv[1] * keep[0] + inv[4] * keep[1] + inv[7] * keep[2], inv[2] * keep[0] + inv[5] * keep[1] + inv[8] * keep[2]];
  const len = Math.hypot(n[0], n[1], n[2]);
  if (!(len > 1e-18)) return null;
  const name = (plane.name || plane.label || "").trim();
  // No negative zeros (the op log stays tidy).
  const z = (x: number) => (x === 0 ? 0 : x);
  return {
    origin: [z(q[0] * M_PER_IN), z(q[1] * M_PER_IN), z(q[2] * M_PER_IN)],
    normal: [z(n[0] / len), z(n[1] / len), z(n[2] / len)],
    name: name ? name.slice(0, 64) : null,
    hidden: !!plane.hidden,
  };
}

/** A definition's (or the model's) section planes, placed by `m`. */
function sectionsIn(ctx: Ctx, def: Definition, m: Placement): ImportSection[] {
  const out: ImportSection[] = [];
  for (const sp of (def.sectionPlanes as { plane: number[]; name?: string; label?: string; hidden?: boolean }[] | undefined) ?? []) {
    const s = sectionOf(sp, m);
    if (s) out.push(s);
    else bump(ctx.skipped, "section planes without a usable position");
  }
  ctx.summary.sections += out.length;
  return out;
}

function countSkipped(ctx: Ctx, def: Definition) {
  for (const k of ["texts", "dimensions", "constructionLines", "constructionPoints"] as const) {
    const n = (def[k] as unknown[] | undefined)?.length ?? 0;
    if (n) bump(ctx.skipped, k, n);
  }
}

/**
 * One placement as a kernel node, placed by `m` (inches) in the enclosing frame (the model,
 * a component definition or a group's enclosing frame): an instance of a component
 * definition, or a group holding the definition's geometry and its nested placements.
 */
function nodeFor(ctx: Ctx, inst: Instance, m: Placement, depth: number, inherited: number | null): ImportNode | null {
  if (depth > 64) {
    ctx.notes.add("components nested deeper than 64 levels were left out (a cycle?)");
    return null;
  }
  const child = ctx.model.definitions.get(inst.refIdx);
  if (!child) {
    bump(ctx.skipped, "instances of missing definitions");
    return null;
  }
  if (child.isImage) {
    bump(ctx.skipped, "images");
    return null;
  }
  ctx.summary.placements++;
  spend(ctx, 1, 0, 0);
  const p = compose(m, placement(inst));
  const material = inst.materialId ?? inherited;
  const tag = layerTag(inst.layer);
  if (tag) ctx.usedTags.add(tag);
  const own = (inst.name ?? "").trim();
  if (isComponent(child, ctx.placed.get(child.id) ?? 0)) {
    const definition = defFor(ctx, child, material, depth + 1);
    if (definition === null) return null;
    return { name: own && own !== child.name ? own : null, tag, hidden: !!inst.hidden, kind: "instance", definition, xform: placementXform(p) };
  }
  ctx.summary.groups++;
  countSkipped(ctx, child);
  const name = own || child.name.trim() || "Group";
  const item = itemOf(ctx, child, p, name, { inherited: material });
  const children = child.instances.map((i) => nodeFor(ctx, i, p, depth + 1, material)).filter((n): n is ImportNode => n !== null);
  // Section planes drawn in the group (lane 12), in the enclosing frame like its geometry.
  const sections = sectionsIn(ctx, child, p);
  return { name: GROUP_DEF.test(name) ? null : name, tag, hidden: !!inst.hidden, kind: "group", items: item ? [item] : [], children, ...(sections.length ? { sections } : {}) };
}

/** The component definition for `def` painted with `material` where its faces inherit
 * (built once, its nested definitions first). Returns its index, null when too deep. */
function defFor(ctx: Ctx, def: Definition, material: number | null, depth: number): number | null {
  const key = `${def.id}|${inherits(def) ? (material ?? "") : ""}`;
  const known = ctx.defIndex.get(key);
  if (known !== undefined) return known;
  if (depth > 64) {
    ctx.notes.add("components nested deeper than 64 levels were left out (a cycle?)");
    return null;
  }
  countSkipped(ctx, def);
  const base = def.name.trim() || "Component";
  // The same component painted differently in different places: one definition per paint.
  const variant = [...ctx.defIndex.keys()].some((k) => k.startsWith(`${def.id}|`));
  const paint = materialName(ctx, material);
  const name = variant ? `${base} (${paint ?? "unpainted"})` : base;
  if (variant) ctx.notes.add(`${base}: placed with different paint, so it came in as one component per paint`);
  const item = itemOf(ctx, def, IDENTITY, base, { inherited: material });
  const children = def.instances.map((i) => nodeFor(ctx, i, IDENTITY, depth + 1, material)).filter((n): n is ImportNode => n !== null);
  // Its section planes in its own coordinates: each copy placed at the top gets them (lane 12).
  const sections = sectionsIn(ctx, def, IDENTITY);
  ctx.defs.push({ name, items: item ? [item] : [], children, ...(sections.length ? { sections } : {}) });
  const index = ctx.defs.length - 1;
  ctx.defIndex.set(key, index);
  ctx.summary.components++;
  return index;
}

const SKIPPED_LABEL: Record<string, string> = {
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
    textures: new Map(),
    usedTags: new Set(),
    hiddenTags: new Set(),
    skipped: new Map(),
    placed: countPlacements(model),
    defs: [],
    defIndex: new Map(),
    spent: { nodes: 0, points: 0, faces: 0 },
    summary: {
      version: model.version,
      definitions: Math.max(0, model.definitions.size),
      placements: 0,
      components: 0,
      groups: 0,
      faces: 0,
      looseEdges: 0,
      softEdges: 0,
      materials: model.materials.length,
      textures: model.materials.filter((m) => m.texture).length,
      layers: model.layers.length,
      scenes: model.pages.length,
      sections: 0,
    },
  };
  // Texture images: PNG and JPEG come in, one per material, under a path the kernel takes.
  // Lane 16: one the kernel would refuse (over 16 MB, over 4096 x 4096 pixels, past the
  // 512th) is left out with a note, so the rest of the model still comes in.
  const takenPaths = new Set<string>();
  let tooBig = 0;
  for (const m of model.materials) {
    const data = m.texture?.data;
    const mime = data ? sniff(data) : null;
    if (!data || !mime) continue;
    const px = imageSize(data);
    if (data.length > SKP_LIMITS.textureBytes || !px || px.width * px.height > SKP_LIMITS.texturePixels || ctx.textures.size >= SKP_LIMITS.textures) {
      tooBig++;
      continue;
    }
    const slug =
      `skp-${m.name}`
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 48) || "skp-texture";
    let path = `assets/${slug}.${mime === "image/png" ? "png" : "jpg"}`;
    for (let k = 2; takenPaths.has(path); k++) path = `assets/${slug}-${k}.${mime === "image/png" ? "png" : "jpg"}`;
    takenPaths.add(path);
    ctx.textures.set(m.name, { path, mime, bytes: data });
  }
  if (tooBig)
    ctx.notes.add(
      `${tooBig} texture image${tooBig === 1 ? "" : "s"} left out (over ${SKP_LIMITS.textureBytes / 1024 / 1024} MB, over 4096 x 4096 pixels, or past ${SKP_LIMITS.textures}): their materials show their colours`,
    );
  // Loose geometry drawn at the top level of the model.
  const root = itemOf(ctx, model.root, IDENTITY, stem, {});
  if (root) ctx.items.push(root);
  countSkipped(ctx, model.root);
  const nodes = model.root.instances.map((i) => nodeFor(ctx, i, IDENTITY, 0, null)).filter((n): n is ImportNode => n !== null);
  // Section planes drawn at the top of the model (lane 12): placed after the import, in the
  // same undo step, inactive.
  const rootSections = sectionsIn(ctx, model.root, IDENTITY);

  const materials: MaterialDef[] = model.materials
    .filter((m) => ctx.usedMaterials.has(m.name))
    .map((m) => {
      const a = m.color.a !== undefined && m.color.a < 255 ? m.color.a : Math.round(255 * (1 - (m.transparency ?? 0)));
      const held = ctx.textures.get(m.name);
      const def: MaterialDef = { name: m.name, color: [m.color.r, m.color.g, m.color.b, Math.max(0, Math.min(255, a))], texture: held?.path ?? (m.texture?.filename || null) };
      // The texture's tile size, inches to metres.
      if (held && m.texture && m.texture.width > 1e-9 && m.texture.height > 1e-9) def.size = [m.texture.width * M_PER_IN, m.texture.height * M_PER_IN];
      return def;
    });
  const images = [...ctx.textures.entries()].filter(([name]) => ctx.usedMaterials.has(name));
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
  if (images.length) ctx.notes.add(`${images.length} texture image${images.length === 1 ? "" : "s"} imported with ${images.length === 1 ? "its" : "their"} sizes and positions`);
  const kept = ctx.summary.textures - images.length;
  if (kept > 0) ctx.notes.add(`${kept} textured material(s) whose image is not PNG or JPEG (or is unused): colours imported, the texture kept as a file-name reference`);
  if (ctx.summary.components)
    ctx.notes.add(`${ctx.summary.components} component${ctx.summary.components === 1 ? "" : "s"} came in as components (placed more than once, or named in SketchUp): editing one copy changes them all`);
  if (ctx.summary.softEdges) ctx.notes.add(`${ctx.summary.softEdges} soft, smooth or hidden edge${ctx.summary.softEdges === 1 ? "" : "s"} came in as they were`);
  for (const [k, n] of ctx.skipped) ctx.notes.add(`not imported: ${n} ${SKIPPED_LABEL[k] ?? k}`);
  if (model.units) ctx.notes.add(`model units: ${model.units} (geometry is converted from inches)`);
  if (ctx.summary.sections)
    ctx.notes.add(`${ctx.summary.sections} section plane${ctx.summary.sections === 1 ? "" : "s"} came in switched off (the file does not say which one was active): activate one from the Outliner`);
  const hidden_tags = [...ctx.hiddenTags];
  // Lane 15: a file with nothing in it opens as an empty model in its own units and style.
  if (!ctx.items.length && !nodes.length && !scenes.length && !rootSections.length) {
    const styleName = model.styles[0]?.name.trim().replace(/^\[(.*)\]$/, "$1") ?? "";
    const style = styleName ? { name: styleName, face: faceOfStyle(styleName) } : null;
    const empty: SkpEmpty = { units: unitsOf(model.units), unitsWord: model.units ?? null, style };
    const notes = [`${stem} has nothing drawn in it: opened as an empty model`];
    if (empty.units) notes.push(`units: ${model.units}`);
    if (style) notes.push(`style: ${style.name}${style.face ? "" : " (Sawhorse's own look kept)"}`);
    return { op: null, notes: [...notes, ...[...ctx.notes].filter((n) => !n.startsWith("model units"))], summary: ctx.summary, empty };
  }
  const importOp: KernelOp = { op: "import", items: ctx.items, materials, scenes, hidden_tags, definitions: ctx.defs, nodes };
  const planes: KernelOp[] = rootSections.map((s) => ({ op: "add_section", origin: s.origin, normal: s.normal, name: s.name ?? null, active: false, ...(s.hidden ? { hidden: true } : {}) }));
  // The images go in first, the top-level section planes after, all in the same undo step.
  const op: KernelOp =
    images.length || planes.length ? { op: "batch", ops: [...images.map(([, t]): KernelOp => ({ op: "add_texture", path: t.path, mime: t.mime, data: base64(t.bytes) })), importOp, ...planes] } : importOp;
  return {
    op,
    notes: [...ctx.notes],
    summary: ctx.summary,
  };
}

/** Parse a .skp file's bytes and map them onto one kernel import op. */
/** The import's own outcome out of what applying a {@link skpToOp} op returned: the op is a
 * batch when texture images go in before the import or section planes after it, and the
 * report describes the import, not the last plane added (lane 15: a file with a section
 * plane was reported as "Imported x.skp." with nothing counted). */
export function importOutcomeOf(applied: OpOutcome): OpOutcome {
  if (applied.kind !== "batch") return applied;
  return applied.outcomes.find((o) => o.kind === "imported") ?? applied.outcomes[applied.outcomes.length - 1] ?? applied;
}

export function skpToOp(bytes: ArrayBuffer | Uint8Array, fileName: string): SkpImport {
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  // Lane 16: the archive's declared sizes, before openskp inflates anything.
  checkSkpArchive(view);
  // The bytes are this worker's own (transferred, lane 16): no copy unless they are a view
  // of a larger buffer.
  const buf = view.byteOffset === 0 && view.byteLength === view.buffer.byteLength ? view.buffer : view.slice().buffer;
  let model: SkpModel;
  try {
    model = parseSkp(buf as ArrayBuffer);
  } catch (e) {
    throw new Error(`could not read the SketchUp file: ${e instanceof Error ? e.message : String(e)}`);
  }
  return modelToOp(model, fileName);
}
