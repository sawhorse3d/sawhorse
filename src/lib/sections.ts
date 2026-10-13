// Section planes on the UI side (lane 12, ADR-0010): where a plane is drawn (its rectangle
// and the arrows that show the side it keeps), what a click on it hits, which planes a scene
// saves, the names the status line uses, and how the viewport clips the edges a plane cuts
// away. Pure: no DOM, no three.js, unit-tested in sections.test.ts.
import type { DocState, GraphSection, MeshData, SceneGraph, SectionView, V3 } from "../kernel/types";

const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const mul = (a: V3, k: number): V3 => [a[0] * k, a[1] * k, a[2] * k];
const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

/** The planes in the graph (empty for a model without any). */
export function sectionsOf(graph: SceneGraph | null | undefined): GraphSection[] {
  return graph?.sections ?? [];
}

export function sectionById(graph: SceneGraph | null | undefined, id: number): GraphSection | undefined {
  return sectionsOf(graph).find((s) => s.id === id);
}

/** A point of the plane's frame: origin + u x + v y. */
export function onPlane(s: Pick<GraphSection, "origin" | "u" | "v">, x: number, y: number): V3 {
  return add(add(s.origin, mul(s.u, x)), mul(s.v, y));
}

/** The four corners of the rectangle a plane is drawn as (counter-clockwise seen from the
 * tip of its normal). */
export function sectionCorners(s: Pick<GraphSection, "origin" | "u" | "v" | "extent">): [V3, V3, V3, V3] {
  const [x0, y0, x1, y1] = s.extent;
  return [onPlane(s, x0, y0), onPlane(s, x1, y0), onPlane(s, x1, y1), onPlane(s, x0, y1)];
}

/** How long the arrows are: a fifth of the rectangle's shorter side, at most 6 in. */
export function arrowLength(s: Pick<GraphSection, "extent">): number {
  const [x0, y0, x1, y1] = s.extent;
  return Math.min(0.1524, 0.2 * Math.min(x1 - x0, y1 - y0));
}

/** Arrows at the four corners pointing at the side kept (SketchUp draws its arrows the same
 * way), plus the centre arrow that is the plane's reverse control: segment pairs. */
export function sectionArrows(s: Pick<GraphSection, "origin" | "u" | "v" | "extent" | "normal">): { corners: V3[]; centre: V3[] } {
  const len = arrowLength(s);
  const head = len * 0.35;
  const n = s.normal;
  const arrow = (from: V3, side: V3): V3[] => {
    const tip = add(from, mul(n, len));
    const back = add(tip, mul(n, -head));
    return [from, tip, tip, add(back, mul(side, head * 0.6)), tip, add(back, mul(side, -head * 0.6))];
  };
  const corners = sectionCorners(s).flatMap((c) => arrow(c, s.u));
  const [x0, y0, x1, y1] = s.extent;
  const c = onPlane(s, (x0 + x1) / 2, (y0 + y1) / 2);
  return { corners, centre: [...arrow(c, s.u), ...arrow(c, s.v)] };
}

/** Where a ray meets a plane's rectangle: the ray parameter and whether it is on the
 * reverse control (the centre arrow, within `controlRadius` metres of the centre), or null. */
export function raySection(ray: { origin: V3; dir: V3 }, s: Pick<GraphSection, "origin" | "u" | "v" | "extent" | "normal">, controlRadius = 0): { t: number; control: boolean } | null {
  const denom = dot(ray.dir, s.normal);
  const [x0, y0, x1, y1] = s.extent;
  const centre = onPlane(s, (x0 + x1) / 2, (y0 + y1) / 2);
  // The centre arrow first: a short segment along the normal, hit within the radius.
  if (controlRadius > 0) {
    const len = arrowLength(s);
    const hit = closestOnSegment(ray, centre, add(centre, mul(s.normal, len)));
    if (hit && hit.distance <= controlRadius) return { t: hit.t, control: true };
  }
  if (Math.abs(denom) < 1e-12) return null;
  const t = dot(sub(s.origin, ray.origin), s.normal) / denom;
  if (!(t > 0)) return null;
  const p = add(ray.origin, mul(ray.dir, t));
  const d = sub(p, s.origin);
  const x = dot(d, s.u);
  const y = dot(d, s.v);
  if (x < x0 || x > x1 || y < y0 || y > y1) return null;
  return { t, control: false };
}

/** The closest approach of a ray to a segment: ray parameter and distance (metres). */
export function closestOnSegment(ray: { origin: V3; dir: V3 }, a: V3, b: V3): { t: number; distance: number } | null {
  const d = sub(b, a);
  const w = sub(ray.origin, a);
  const aa = dot(ray.dir, ray.dir);
  const bb = dot(ray.dir, d);
  const cc = dot(d, d);
  const dd = dot(ray.dir, w);
  const ee = dot(d, w);
  const den = aa * cc - bb * bb;
  let s = 0;
  let t: number;
  if (den > 1e-18) {
    s = Math.min(1, Math.max(0, (aa * ee - bb * dd) / den));
  }
  t = (bb * s - dd) / aa;
  if (t < 0) {
    t = 0;
    s = cc > 0 ? Math.min(1, Math.max(0, ee / cc)) : 0;
  }
  const p = add(ray.origin, mul(ray.dir, t));
  const q = add(a, mul(d, s));
  const e = sub(p, q);
  return { t, distance: Math.sqrt(dot(e, e)) };
}

/** The nearest drawn plane a ray hits, or null. `metresPerPixel` sizes the reverse control. */
export function pickSection(ray: { origin: V3; dir: V3 }, sections: GraphSection[], shown: boolean, metresPerPixel: (p: V3) => number): { section: number; t: number; control: boolean } | null {
  if (!shown) return null;
  let best: { section: number; t: number; control: boolean } | null = null;
  for (const s of sections) {
    if (!s.visible) continue;
    const [x0, y0, x1, y1] = s.extent;
    const r = raySection(ray, s, 6 * metresPerPixel(onPlane(s, (x0 + x1) / 2, (y0 + y1) / 2)));
    if (r && (!best || r.t < best.t || (r.control && !best.control && Math.abs(r.t - best.t) < 1e-9))) best = { section: s.id, ...r };
  }
  return best;
}

/** What a scene saved now remembers about the planes. */
export function sceneSectionsOf(graph: SceneGraph | null | undefined): { active: number[]; hidden: number[] } {
  const ss = sectionsOf(graph);
  return { active: ss.filter((s) => s.active).map((s) => s.id), hidden: ss.filter((s) => s.hidden).map((s) => s.id) };
}

/** "section plane 'Side cut' (active)". */
export function describeSection(graph: SceneGraph | null | undefined, id: number): string {
  const s = sectionById(graph, id);
  if (!s) return "a section plane";
  return `section plane '${s.name}'${s.active ? " (active)" : ""}`;
}

/** What to say when exporting a plane's slice was refused (`error`: the kernel's words). A
 * plane that cuts nothing (laid on a face, keeping the side the whole solid is on; or past
 * every solid) leaves nothing to export: say why and what to do, never just nothing. */
export function sliceExportRefused(error: string, name: string): string {
  if (/nothing to export/i.test(error)) {
    return `Nothing to export: ${name} cuts no solid. A plane laid on a face keeps the side it faces and cuts nothing until you move it into the solid (or Reverse it).`;
  }
  return `Export refused: ${error}`;
}

/** The planes directly in the open context (the ones tools may move, flip or delete). */
export function contextSections(state: DocState | null | undefined): GraphSection[] {
  const g = state?.graph;
  return sectionsOf(g).filter((s) => s.parent === (g?.context ?? null));
}

/** The active plane of the open context, or of the nearest context above it: what File >
 * Export > Section slice exports when no plane is selected. */
export function activeSectionHere(graph: SceneGraph | null | undefined): GraphSection | undefined {
  const ss = sectionsOf(graph);
  let at: number | null = graph?.context ?? null;
  const seen = new Set<number>();
  for (;;) {
    const s = ss.find((x) => x.active && x.parent === at);
    if (s) return s;
    if (at === null || seen.has(at)) return undefined;
    seen.add(at);
    at = graph?.nodes.find((n) => n.id === at)?.parent ?? null;
  }
}

/** A plane's direction away from the eye: the side kept when a plane is placed on a surface
 * (what the eye looks into stays; nothing is cut until the plane moves in). */
export function awayFromEye(normal: V3, viewDir: V3): V3 {
  return dot(normal, viewDir) >= 0 ? normal : reversed(normal);
}

/** `n` turned round (no negative zeros: the op log stays tidy). */
export function reversed(n: V3): V3 {
  return n.map((x) => (x === 0 ? 0 : -x)) as V3;
}

/** Planes in three.js form ([nx, ny, nz, c], keeping n . p + c >= 0) per edge of the view
 * mesh, by the edge's owner (solid, imported mesh or sketch); null for an edge nothing
 * clips. */
export function edgeClipPlanes(mesh: MeshData, view: SectionView | null | undefined): ([number, number, number, number][] | null)[] {
  const out: ([number, number, number, number][] | null)[] = new Array(mesh.edgeOwners.length).fill(null);
  if (!view?.clips.length) return out;
  const byKey = new Map<string, [number, number, number, number][]>();
  for (const c of view.clips) {
    for (const o of c.objects) byKey.set(`object:${o}`, c.planes);
    for (const m of c.meshes) byKey.set(`mesh:${m}`, c.planes);
    for (const s of c.sketches) byKey.set(`sketch:${s}`, c.planes);
  }
  mesh.edgeOwners.forEach((o, i) => {
    out[i] = byKey.get(`${o.kind}:${o.id}`) ?? null;
  });
  return out;
}

/** The planes clipping each mesh part (by part index), null where nothing clips. */
export function partClipPlanes(mesh: MeshData, view: SectionView | null | undefined): ([number, number, number, number][] | null)[] {
  const out: ([number, number, number, number][] | null)[] = new Array(mesh.parts.length).fill(null);
  if (!view?.clips.length) return out;
  const byKey = new Map<string, [number, number, number, number][]>();
  for (const c of view.clips) {
    for (const o of c.objects) byKey.set(`face:${o}`, c.planes);
    for (const m of c.meshes) byKey.set(`mesh:${m}`, c.planes);
    for (const s of c.sketches) byKey.set(`region:${s}`, c.planes);
  }
  mesh.parts.forEach((p, i) => {
    out[i] = byKey.get(`${p.kind}:${p.owner}`) ?? null;
  });
  return out;
}

/** Clip segment a-b to the kept side of every plane; null when nothing is left. */
export function clipSegment(a: V3, b: V3, planes: [number, number, number, number][]): [V3, V3] | null {
  let t0 = 0;
  let t1 = 1;
  for (const [nx, ny, nz, c] of planes) {
    const da = nx * a[0] + ny * a[1] + nz * a[2] + c;
    const db = nx * b[0] + ny * b[1] + nz * b[2] + c;
    if (da < 0 && db < 0) return null;
    if (da < 0 || db < 0) {
      const t = da / (da - db);
      if (da < 0) t0 = Math.max(t0, t);
      else t1 = Math.min(t1, t);
    }
  }
  if (t1 - t0 <= 1e-12) return null;
  const at = (t: number): V3 => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
  return [at(t0), at(t1)];
}

/** The view mesh's edges with what the active planes cut away taken off: positions, the
 * matching edge normals and owners (edges cut away entirely are dropped). */
export function clippedEdges(mesh: MeshData, view: SectionView | null | undefined): { edges: Float32Array; normals: Float32Array | undefined; owner: Uint32Array; flags: Uint8Array | undefined } {
  if (!view?.clips.length) return { edges: mesh.edges, normals: mesh.edgeNormals, owner: mesh.edgeOwner, flags: mesh.edgeFlags };
  const planes = edgeClipPlanes(mesh, view);
  const e = mesh.edges;
  const n = mesh.edgeNormals;
  const edges: number[] = [];
  const normals: number[] = [];
  const owner: number[] = [];
  // Lane 15: each kept segment keeps its soft / smooth / hidden flags.
  const f = mesh.edgeFlags && mesh.edgeFlags.length === mesh.edgeOwner.length ? mesh.edgeFlags : undefined;
  const flags: number[] = [];
  for (let i = 0; i < mesh.edgeOwner.length; i++) {
    const a: V3 = [e[6 * i], e[6 * i + 1], e[6 * i + 2]];
    const b: V3 = [e[6 * i + 3], e[6 * i + 4], e[6 * i + 5]];
    const ps = planes[mesh.edgeOwner[i]];
    const seg = ps ? clipSegment(a, b, ps) : [a, b];
    if (!seg) continue;
    edges.push(...seg[0], ...seg[1]);
    if (n) for (let k = 0; k < 6; k++) normals.push(n[6 * i + k]);
    owner.push(mesh.edgeOwner[i]);
    if (f) flags.push(f[i]);
  }
  return { edges: new Float32Array(edges), normals: n ? new Float32Array(normals) : undefined, owner: new Uint32Array(owner), flags: f ? new Uint8Array(flags) : undefined };
}
