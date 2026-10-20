// Shared pieces of the transform tools (Move, Rotate, Scale): affine maps in the kernel's
// plain-data form, what a gesture acts on (whole solids, part of one, sketch geometry), and
// the edges a preview draws. Pure: no DOM, no three.js, unit-tested in move.test.ts and
// friends.
//
// Sketch geometry (lane 22, ADR-0012): points, edges and regions of sketches that have not
// been pushed into a solid move with the solids they are selected with. Each sketch they
// come from is one `transform_sketch` op; a selection of solids and sketch geometry is one
// batch (one undo step). Part of a shape (a corner, an edge of a rectangle) moves within
// its plane, the move projected into it; a whole shape may leave its plane.
//
// Curves (lane 23): a selected curve moves as one (its points all move, the kernel keeps a
// circle a circle under a similarity and makes a copy a new curve); a curve's point selected
// on its own is refused before the gesture starts, with the kernel's own words.
import type { DocState, Entity, KernelOp, MeshData, OpOutcome, V3, Xform } from "../kernel/types";
import { curveSegments, onCurve } from "../lib/curves";
import { objectEdges, objectOf, sketchEdges } from "../lib/entities";
import { faceOf, regionName, solidName } from "../lib/names";
import { describeNode, graphOf, scopeEntity, solidsIn } from "../lib/scope";
import { sectionById, sectionCorners } from "../lib/sections";
import type { OutlineTarget } from "../lib/edit";

/** The message the kernel refuses a curve's point moved on its own with (lane 23). */
export const CURVE_POINT_REFUSAL = "That point is on a curve: moving it alone would bend the curve. Select the whole curve, or use Edit > Explode Curve first.";
import { add, cross, dot, len, mul, sub } from "./draw";

export const IDENTITY: Xform = {
  linear: [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
  ],
  translation: [0, 0, 0],
};

export function translation(d: V3): Xform {
  return { ...IDENTITY, translation: [d[0], d[1], d[2]] };
}

/** Turn `angle` radians about the line through `c` along unit `axis` (right-handed). */
export function rotation(c: V3, axis: V3, angle: number): Xform {
  const n = len(axis);
  const [x, y, z] = n > 0 ? mul(axis, 1 / n) : [0, 0, 1];
  const s = Math.sin(angle);
  const k = Math.cos(angle);
  const t = 1 - k;
  const linear: [V3, V3, V3] = [
    [t * x * x + k, t * x * y - s * z, t * x * z + s * y],
    [t * x * y + s * z, t * y * y + k, t * y * z - s * x],
    [t * x * z - s * y, t * y * z + s * x, t * z * z + k],
  ];
  // p -> R (p - c) + c
  const rc = mulVec(linear, c);
  return { linear, translation: [c[0] - rc[0], c[1] - rc[1], c[2] - rc[2]] };
}

/** Scale by `f` along the world axes about `c` (negative factors mirror). */
export function scaling(c: V3, f: V3): Xform {
  return {
    linear: [
      [f[0], 0, 0],
      [0, f[1], 0],
      [0, 0, f[2]],
    ],
    translation: [c[0] * (1 - f[0]), c[1] * (1 - f[1]), c[2] * (1 - f[2])],
  };
}

export function mulVec(m: [V3, V3, V3], v: V3): V3 {
  return [dot(m[0], v), dot(m[1], v), dot(m[2], v)];
}

export function applyX(x: Xform, p: V3): V3 {
  return add(mulVec(x.linear, p), x.translation);
}

/**
 * Sketch geometry a gesture moves (lane 22): the selected points, edges and whole regions
 * of one sketch, its plane, and every point that moves (the points, the edges' ends, the
 * regions' ring corners). `rings` are the regions' rings (world points).
 */
export interface SketchPart {
  sketch: number;
  points: V3[];
  edges: [V3, V3][];
  regions: number[];
  /** Lane 23: whole curves, by id (their points are in `moving`). */
  curves?: number[];
  rings: V3[][];
  origin: V3;
  normal: V3;
  moving: V3[];
}

/** What a transform gesture acts on. */
export type Subject =
  /**
   * Whole solids, groups and component instances (`nodes`, lane 5). `covered`: every solid
   * that moves (the solids, and everything inside the nodes), for previews and bounds.
   * `names`: how the nodes are called in the status line. `sketches` (lane 22): sketch
   * geometry moving with them (`objects` empty when only sketch geometry moves).
   */
  | {
      kind: "objects";
      objects: number[];
      nodes?: number[];
      covered?: number[];
      names?: string[];
      /** Section planes moving along (lane 12), and their rectangles for the preview. */
      sections?: number[];
      sectionEdges?: [V3, V3][];
      sketches?: SketchPart[];
    }
  /** Part of one solid: its points (a face's corners, an edge's ends, a vertex). */
  | { kind: "geometry"; object: number; points: V3[]; what: string; sketches?: SketchPart[] };

/** An entity that is sketch geometry: a region, or a sketch's edge, point or curve. */
export function isSketchEntity(e: Entity): boolean {
  return e.kind === "region" || ((e.kind === "edge" || e.kind === "vertex" || e.kind === "curve") && e.owner.kind === "sketch");
}

function sketchOf(e: Entity): number {
  return e.kind === "region" ? e.sketch : (e as { owner: { id: number } }).owner.id;
}

const near = (p: V3, q: V3) => Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]) <= 1e-9;

/** "region 2", "a sketch edge", "3 sketch points": what of one sketch moves. */
function sketchPartNames(p: SketchPart): string[] {
  const out = p.regions.map((r) => regionName(p.sketch, r));
  const nc = p.curves?.length ?? 0;
  if (nc) out.push(nc === 1 ? "a curve" : `${nc} curves`);
  if (p.edges.length) out.push(p.edges.length === 1 ? "a sketch edge" : `${p.edges.length} sketch edges`);
  if (p.points.length) out.push(p.points.length === 1 ? "a sketch point" : `${p.points.length} sketch points`);
  return out;
}

/** "solid 3", "solids 2 and 5", "component Shelf (2 of 4)", "face 6 of solid 3", "region 1". */
export function describeSubject(s: Subject): string {
  const sk = (s.sketches ?? []).flatMap(sketchPartNames);
  const names = s.kind === "geometry" ? [s.what, ...sk] : [...s.objects.map(solidName), ...(s.names ?? []), ...sk];
  if (names.length === 1) return names[0];
  if (names.length <= 3) return `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
  return `${names.length} items`;
}

/** Every solid a whole-objects subject moves. */
export function coveredSolids(s: Subject): number[] {
  if (s.kind === "geometry") return [s.object];
  return s.covered ?? s.objects;
}

/** A whole-objects subject from solids, nodes and section planes (with the scene graph for
 * names and the planes' rectangles). */
export function objectsSubject(objects: number[], nodes: number[], state: DocState | null | undefined, sections: number[] = []): Subject {
  const g = graphOf(state);
  const covered = [...new Set([...objects, ...nodes.flatMap((n) => solidsIn(g, n))])];
  const names = [...nodes.map((n) => describeNode(g, n)), ...sections.map((k) => `section plane '${sectionById(g, k)?.name ?? k}'`)];
  const sectionEdges: [V3, V3][] = sections.flatMap((k) => {
    const s = sectionById(g, k);
    if (!s) return [];
    const c = sectionCorners(s);
    return c.map((p, i): [V3, V3] => [p, c[(i + 1) % 4]]);
  });
  return { kind: "objects", objects, nodes, covered, names, ...(sections.length ? { sections, sectionEdges } : {}) };
}

/** One `transform_sketch` op per sketch (lane 22). Copies leave out sketches with only
 * points selected (a point alone copies nothing) unless nothing else would be copied. */
export function sketchOps(parts: SketchPart[], xform: Xform, copy: boolean, count = 1, alone = true): KernelOp[] {
  const useful = copy && !alone ? parts.filter((p) => p.edges.length || p.regions.length) : parts;
  return useful.map(
    (p): KernelOp => ({
      op: "transform_sketch",
      sketch: p.sketch,
      ...(p.points.length ? { points: p.points } : {}),
      ...(p.edges.length ? { edges: p.edges } : {}),
      ...(p.regions.length ? { regions: p.regions } : {}),
      ...(p.curves?.length ? { curves: p.curves } : {}),
      xform,
      ...(copy ? { copy: true } : {}),
      ...(copy && count > 1 ? { count } : {}),
    }),
  );
}

function oneOp(ops: KernelOp[]): KernelOp {
  return ops.length === 1 ? ops[0] : { op: "batch", ops };
}

/** The transform / copy / array op for a whole-objects subject: one op, or a batch (one undo
 * step) when section planes or sketch geometry move along. */
export function objectsOp(s: Extract<Subject, { kind: "objects" }>, xform: Xform, copy: boolean, count = 1): KernelOp {
  const nodes = s.nodes?.length ? { nodes: s.nodes } : {};
  const ops: KernelOp[] = [];
  if (s.objects.length || s.nodes?.length) {
    if (!copy) ops.push({ op: "transform", objects: s.objects, xform, ...nodes });
    else if (count > 1) ops.push({ op: "array", objects: s.objects, xform, count, ...nodes });
    else ops.push({ op: "copy", objects: s.objects, xform, ...nodes });
  }
  if (s.sections?.length) ops.push(copy ? { op: "copy_sections", sections: s.sections, xform, count } : { op: "move_sections", sections: s.sections, xform });
  if (s.sketches?.length) ops.push(...sketchOps(s.sketches, xform, copy, count, ops.length === 0));
  return oneOp(ops);
}

/** The op for moving part of a solid (and any sketch geometry with it). */
export function geometryOp(s: Extract<Subject, { kind: "geometry" }>, xform: Xform, autofold: boolean): KernelOp {
  return oneOp([{ op: "transform_geometry", object: s.object, points: s.points, xform, autofold }, ...sketchOps(s.sketches ?? [], xform, false)]);
}

/** The faces, regions and curves whose outlines a subject needs (corners of faces, rings of
 * regions, a curve's points), keyed as `subjectOf` reads them. `picked` counts when nothing
 * is selected. */
export function outlineTargets(selection: Entity[], picked: Entity | null = null): { key: string; target: OutlineTarget }[] {
  const from = selection.length ? selection : picked ? [picked] : [];
  const out: { key: string; target: OutlineTarget }[] = [];
  for (const e of from) {
    if (e.kind === "face") out.push({ key: `${e.object}:${e.face}`, target: { kind: "face", object: e.object, face: e.face } });
    else if (e.kind === "region") out.push({ key: `r${e.sketch}:${e.region}`, target: { kind: "region", sketch: e.sketch, region: e.region } });
    else if (e.kind === "curve" && e.owner.kind === "sketch") out.push({ key: `c${e.owner.id}:${e.curve}`, target: { kind: "curve", sketch: e.owner.id, curve: e.curve } });
    else if (e.kind === "curve" && e.owner.kind === "object") out.push({ key: `k${e.owner.id}:${e.curve}`, target: { kind: "rim", object: e.owner.id, curve: e.curve } });
  }
  return out;
}

/** The outlines `subjectOf` needs, from the host. */
export async function loadOutlines(outline: (t: OutlineTarget) => Promise<V3[][]>, selection: Entity[], picked: Entity | null = null): Promise<Map<string, V3[][]>> {
  const m = new Map<string, V3[][]>();
  for (const { key, target } of outlineTargets(selection, picked)) m.set(key, await outline(target));
  return m;
}

/** The faces whose outlines a geometry subject needs (to collect their corners). */
export function facesToOutline(selection: Entity[]): { object: number; face: number }[] {
  return selection.filter((e): e is Extract<Entity, { kind: "face" }> => e.kind === "face").map((e) => ({ object: e.object, face: e.face }));
}

/** Sketch geometry grouped by sketch, with each sketch's plane (lane 22). Region rings come
 * from `outlines` (key `r<sketch>:<region>`). */
export function sketchParts(es: Entity[], outlines: Map<string, V3[][]>, state?: DocState | null): SketchPart[] {
  const by = new Map<number, SketchPart>();
  for (const e of es) {
    if (!isSketchEntity(e)) continue;
    const id = sketchOf(e);
    let p = by.get(id);
    if (!p) {
      const info = state?.sketches?.find((s) => s.id === id);
      p = { sketch: id, points: [], edges: [], regions: [], rings: [], origin: info?.origin ?? [0, 0, 0], normal: info?.normal ?? [0, 0, 1], moving: [] };
      by.set(id, p);
    }
    const mv = (q: V3) => {
      if (!p.moving.some((r) => near(r, q))) p.moving.push(q);
    };
    if (e.kind === "vertex") {
      if (!p.points.some((q) => near(q, e.at))) p.points.push(e.at);
      mv(e.at);
    } else if (e.kind === "edge") {
      if (!p.edges.some(([a, b]) => (near(a, e.a) && near(b, e.b)) || (near(a, e.b) && near(b, e.a)))) p.edges.push([e.a, e.b]);
      mv(e.a);
      mv(e.b);
    } else if (e.kind === "region" && !p.regions.includes(e.region)) {
      p.regions.push(e.region);
      for (const ring of outlines.get(`r${e.sketch}:${e.region}`) ?? []) {
        p.rings.push(ring);
        ring.forEach(mv);
      }
    } else if (e.kind === "curve") {
      const curves = (p.curves ??= []);
      if (!curves.includes(e.curve)) {
        curves.push(e.curve);
        for (const ring of outlines.get(`c${e.owner.id}:${e.curve}`) ?? []) ring.forEach(mv);
      }
    }
  }
  return [...by.values()];
}

/**
 * What to transform: the selected solids, groups and components; or the selected faces,
 * edges and points of ONE solid (part of it); and any sketch points, edges and regions
 * selected with them (lane 22). With nothing selected, what is under the cursor (the group
 * or component around it when that is not open; a sketch region, edge or point itself).
 * Face corners and region rings come from `outlines` (see `outlineTargets`). A string says
 * why nothing can move.
 */
export function subjectOf(selection: Entity[], picked: Entity | null, outlines: Map<string, V3[][]>, state?: DocState | null): Subject | string {
  if (selection.length) {
    const sketches = sketchParts(selection, outlines, state);
    const withSketches = <T extends Subject>(s: T): T => (sketches.length ? { ...s, sketches } : s);
    const rest = selection.filter((e) => !isSketchEntity(e));
    if (!rest.length) return { kind: "objects", objects: [], sketches };
    const objects = [...new Set(rest.filter((e) => e.kind === "object").map((e) => (e as { object: number }).object))];
    const nodes = [...new Set(rest.filter((e) => e.kind === "node").map((e) => (e as { node: number }).node))];
    const sections = [...new Set(rest.filter((e) => e.kind === "section").map((e) => (e as { section: number }).section))];
    if (sections.length) {
      if (rest.some((e) => e.kind !== "object" && e.kind !== "node" && e.kind !== "section")) return "Move section planes with whole solids, groups and components (not parts of a solid).";
      return withSketches(objectsSubject(objects, nodes, state, sections));
    }
    if (nodes.length) return withSketches(objectsSubject(objects, nodes, state));
    if (objects.length) return withSketches({ kind: "objects", objects });
    if (rest.some((e) => e.kind === "guide" || e.kind === "dimension" || ((e.kind === "edge" || e.kind === "vertex") && e.owner.kind === "guide"))) return "Guides and dimensions don't move: erase and place them again.";
    const owners = [...new Set(rest.map(objectOf))];
    if (owners.length !== 1 || owners[0] === null) return "Move parts of one solid at a time, or double-click to select whole solids.";
    const object = owners[0];
    const points: V3[] = [];
    const push = (p: V3) => {
      if (!points.some((q) => near(q, p))) points.push(p);
    };
    let faces = 0;
    let what = "";
    for (const e of rest) {
      if (e.kind === "face") {
        faces++;
        what = faceOf(e.object, e.face);
        for (const ring of outlines.get(`${e.object}:${e.face}`) ?? []) ring.forEach(push);
      } else if (e.kind === "edge") {
        push(e.a);
        push(e.b);
        what ||= `an edge of ${solidName(object)}`;
      } else if (e.kind === "vertex") {
        push(e.at);
        what ||= `a point of ${solidName(object)}`;
      } else if (e.kind === "curve") {
        // Lane 23: a solid's rim moves all of its points.
        for (const ring of outlines.get(`k${e.owner.id}:${e.curve}`) ?? []) ring.forEach(push);
        what ||= `a curve of ${solidName(object)}`;
      }
    }
    if (!points.length) return "Nothing to move there.";
    if (rest.length > 1) what = faces > 1 ? `${faces} faces of ${solidName(object)}` : `${rest.length} parts of ${solidName(object)}`;
    return withSketches({ kind: "geometry", object, points, what });
  }
  if (picked && state) {
    const unit = scopeEntity(picked, state);
    if (!unit) return "That is outside the group or component you are editing: press Esc to close it first.";
    if (unit.kind === "node") return objectsSubject([], [unit.node], state);
  }
  if (picked && isSketchEntity(picked)) return { kind: "objects", objects: [], sketches: sketchParts([picked], outlines, state) };
  const o = picked ? objectOf(picked) : null;
  if (o === null) return "Click a solid or a sketch shape, or select what to move first.";
  return { kind: "objects", objects: [o] };
}

/** Edges a preview shows for the subject, before the transform (solids and section planes;
 * sketch geometry previews through `sketchPreviews`). */
export function subjectEdges(s: Subject, mesh: MeshData | null): [V3, V3][] {
  if (s.kind === "objects") return [...coveredSolids(s).flatMap((o) => objectEdges(o, mesh)), ...(s.sectionEdges ?? [])];
  // Part of a solid: every edge of that solid touching a moving point (both ends known).
  const moving = (p: V3) => s.points.some((q) => Math.hypot(q[0] - p[0], q[1] - p[1], q[2] - p[2]) <= 1e-7);
  return objectEdges(s.object, mesh).filter(([a, b]) => moving(a) || moving(b));
}

/** The preview segments: each edge with its moving ends mapped (whole solids: both ends). */
export function previewSegments(s: Subject, edges: [V3, V3][], x: Xform): V3[] {
  const moving = (p: V3) => s.kind === "objects" || s.points.some((q) => Math.hypot(q[0] - p[0], q[1] - p[1], q[2] - p[2]) <= 1e-7);
  const out: V3[] = [];
  for (const [a, b] of edges) {
    out.push(moving(a) ? applyX(x, a) : a, moving(b) ? applyX(x, b) : b);
  }
  return out;
}

/**
 * A sketch part's preview: the sketch's edges touching a moving point (the view mesh is
 * single precision, so points match within a few micrometres), and whether the part is a
 * whole shape that may leave its plane (no point selected on its own, and every edge left
 * behind that touches a moving point meets it on a whole region's ring), as the kernel
 * decides it.
 */
export interface SketchPreview {
  part: SketchPart;
  edges: [V3, V3][];
  liftable: boolean;
}

const meshNear = (p: V3, q: V3) => Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]) <= 2e-6 + 4e-7 * Math.max(Math.abs(q[0]), Math.abs(q[1]), Math.abs(q[2]));

export function sketchPreviews(s: Subject, mesh: MeshData | null): SketchPreview[] {
  return (s.sketches ?? []).map((part) => {
    const all = sketchEdges(part.sketch, mesh);
    const moves = (p: V3) => part.moving.some((q) => meshNear(p, q));
    const ringPts = part.rings.flat();
    const onRing = (p: V3) => ringPts.some((q) => meshNear(p, q));
    const same = ([a, b]: [V3, V3], [c, d]: [V3, V3]) => (meshNear(a, c) && meshNear(b, d)) || (meshNear(a, d) && meshNear(b, c));
    const ringEdges: [V3, V3][] = part.rings.flatMap((r) => r.map((p, i): [V3, V3] => [p, r[(i + 1) % r.length]]));
    // Lane 23: a whole curve's segments are carried edges too.
    const curveEdges: [V3, V3][] = (part.curves ?? []).flatMap((c) => curveSegments(mesh, { kind: "sketch", id: part.sketch }, c));
    const carried = (e: [V3, V3]) => part.edges.some((c) => same(e, c)) || ringEdges.some((c) => same(e, c)) || curveEdges.some((c) => same(e, c));
    const edges = all.filter(([a, b]) => moves(a) || moves(b));
    const ends = [...part.edges.flat(), ...ringPts, ...curveEdges.flat()];
    const lone = part.points.some((p) => !ends.some((q) => meshNear(p, q)));
    const liftable = !lone && (part.edges.length > 0 || part.regions.length > 0 || (part.curves?.length ?? 0) > 0) && edges.every((e) => carried(e) || e.every((p) => !moves(p) || onRing(p)));
    return { part, edges, liftable };
  });
}

/** Lane 23: the refusal for a curve's point moved on its own (a sketch point, or a solid's
 * point on one of its rims, selected alone), or null. */
export function curvePointRefusal(s: Subject, selection: Entity[], picked: Entity | null, mesh: MeshData | null): string | null {
  for (const part of s.sketches ?? []) {
    const owner = { kind: "sketch" as const, id: part.sketch };
    // A point at the end of something else that moves is not on its own.
    const ends = [...part.edges.flat(), ...part.rings.flat(), ...(part.curves ?? []).flatMap((c) => curveSegments(mesh, owner, c).flat())];
    for (const p of part.points) {
      if (!ends.some((q) => meshNear(p, q)) && onCurve(mesh, owner, p)) return CURVE_POINT_REFUSAL;
    }
  }
  if (s.kind === "geometry") {
    const from = selection.length ? selection : picked ? [picked] : [];
    const parts = from.filter((e) => objectOf(e) === s.object);
    if (parts.length && parts.every((e) => e.kind === "vertex") && parts.some((e) => e.kind === "vertex" && onCurve(mesh, { kind: "object", id: s.object }, e.at))) return CURVE_POINT_REFUSAL;
  }
  return null;
}

/** Whether `x` maps the plane through `origin` across `normal` onto itself. */
export function keepsPlane(x: Xform, origin: V3, normal: V3): boolean {
  const n = unit3(normal);
  const helper: V3 = Math.abs(n[2]) < 0.9 ? [0, 0, 1] : [0, 1, 0];
  const u = unit3(cross(helper, n));
  const v = cross(n, u);
  const m = cross(mulVec(x.linear, u), mulVec(x.linear, v));
  const mn = unit3(m);
  const o = applyX(x, origin);
  return Math.abs(dot(mn, n)) >= 1 - 1e-12 && Math.abs(dot(sub(o, origin), n)) <= 1e-6;
}

function unit3(v: V3): V3 {
  const l = len(v);
  return l > 0 ? mul(v, 1 / l) : [0, 0, 1];
}

/** Where a point of a sketch part goes under `x`, as the kernel moves it: projected into
 * its plane unless the part is a whole shape and the map takes the plane elsewhere. */
export function sketchPointMap(part: SketchPart, liftable: boolean, x: Xform): (p: V3) => V3 {
  const lifts = liftable && !keepsPlane(x, part.origin, part.normal);
  const n = unit3(part.normal);
  return (p: V3) => {
    const q = applyX(x, p);
    return lifts ? q : projectToPlane(q, part.origin, n);
  };
}

/** The preview segments of sketch geometry: each edge with its moving ends mapped. */
export function sketchSegments(previews: SketchPreview[], x: Xform, copy = false): V3[] {
  const out: V3[] = [];
  for (const { part, edges, liftable } of previews) {
    const map = sketchPointMap(part, liftable, x);
    const moves = (p: V3) => part.moving.some((q) => meshNear(p, q));
    for (const [a, b] of edges) {
      if (copy) {
        // A copy shows the copied edges only: both ends moving.
        if (moves(a) && moves(b)) out.push(map(a), map(b));
      } else out.push(moves(a) ? map(a) : a, moves(b) ? map(b) : b);
    }
  }
  return out;
}

/** The sketch-move outcomes of an edit, in op order (a batch's included). */
function sketchMoves(o: OpOutcome): Extract<OpOutcome, { kind: "sketch_moved" }>[] {
  if (o.kind === "sketch_moved") return [o];
  if (o.kind === "batch") return o.outcomes.flatMap(sketchMoves);
  return [];
}

/** True when an edit's outcome is (or includes) a copy. */
export function isCopied(o: OpOutcome): boolean {
  if (o.kind === "copied" || o.kind === "sketch_copied" || o.kind === "sections_copied") return true;
  return o.kind === "batch" && o.outcomes.some(isCopied);
}

/**
 * The selection after a move of `subject` by `x` (lane 22): its sketch points and edges
 * where they went, its regions kept (in their plane) or replaced by the regions they became
 * on the plane they were lifted to. Everything else stays selected as it was.
 */
export function movedSelection(sel: Entity[], subject: Subject, previews: SketchPreview[], x: Xform, outcome: OpOutcome): Entity[] {
  const moves = sketchMoves(outcome);
  const out: Entity[] = [];
  const parts = subject.sketches ?? [];
  const byId = new Map(parts.map((p, i) => [p.sketch, { p, i }]));
  const added = new Set<string>();
  for (const e of sel) {
    if (!isSketchEntity(e)) {
      out.push(e);
      continue;
    }
    const hit = byId.get(sketchOf(e));
    if (!hit) {
      out.push(e);
      continue;
    }
    const pv = previews.find((v) => v.part === hit.p);
    const map = sketchPointMap(hit.p, pv?.liftable ?? false, x);
    const now = moves[hit.i];
    const sketch = now?.sketch ?? hit.p.sketch;
    if (e.kind === "vertex") out.push({ ...e, owner: { kind: "sketch", id: sketch }, at: map(e.at) });
    else if (e.kind === "edge") out.push({ ...e, owner: { kind: "sketch", id: sketch }, a: map(e.a), b: map(e.b) });
    else if (e.kind === "curve") {
      // Lane 23: the curve where it went: its id in the plane, its new id where it was lifted.
      const k = (hit.p.curves ?? []).indexOf(e.curve);
      const now2 = now?.curves?.[k];
      if (now2) out.push({ kind: "curve", owner: { kind: "sketch", id: now2[0] }, curve: now2[1] });
      else if (sketch === hit.p.sketch) out.push(e);
    } else if (sketch === hit.p.sketch) out.push(e);
    else if (now && !added.has(`${hit.i}`)) {
      added.add(`${hit.i}`);
      for (const [s, r] of now.regions) out.push({ kind: "region", sketch: s, region: r });
    }
  }
  return out;
}

/** World bounds (min, max) of points. */
export function boundsOf(points: V3[]): [V3, V3] | null {
  if (!points.length) return null;
  const lo: V3 = [Infinity, Infinity, Infinity];
  const hi: V3 = [-Infinity, -Infinity, -Infinity];
  for (const p of points) {
    for (let k = 0; k < 3; k++) {
      lo[k] = Math.min(lo[k], p[k]);
      hi[k] = Math.max(hi[k], p[k]);
    }
  }
  return [lo, hi];
}

/** Signed angle (radians) from `a` to `b` about `n` (all in the plane across `n`). */
export function signedAngle(a: V3, b: V3, n: V3): number {
  return Math.atan2(dot(cross(a, b), n), dot(a, b));
}

/** `p` projected onto the plane through `c` across unit `n`. */
export function projectToPlane(p: V3, c: V3, n: V3): V3 {
  return sub(p, mul(n, dot(sub(p, c), n)));
}
