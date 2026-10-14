// Lane 16b: lookups over one view mesh, built once per mesh (the kernel sends a new MeshData
// after every edit, so a WeakMap keyed by it never serves a stale answer).
//
// A selection of every one of 100,000 solids asked, for each solid, which parts and which
// edge segments are its own. Answered by a pass over the whole mesh (600,000 parts and
// 1,200,000 segments) per question, that was the window's thread busy for seconds; answered
// from these lists it is the selection's own size and no more.
import type { MeshData } from "../kernel/types";

export interface MeshIndex {
  /** Face parts of each solid, in mesh order. */
  objectParts: Map<number, number[]>;
  /** Parts of each imported mesh. */
  meshParts: Map<number, number[]>;
  /** Region parts of each sketch. */
  regionParts: Map<number, number[]>;
  /** The solids that have faces in the view, each once, in mesh order. */
  objects: number[];
  /** The `edgeOwners` index of each solid that has edges in the view. */
  objectOwner: Map<number, number>;
  /** Edge segments by owner (CSR): owner k's segments are
   * `ownerEdges[ownerStart[k]] .. ownerEdges[ownerStart[k + 1] - 1]`, in mesh order. */
  ownerStart: Uint32Array;
  ownerEdges: Uint32Array;
}

const built = new WeakMap<MeshData, MeshIndex>();

function push(m: Map<number, number[]>, k: number, v: number): void {
  const l = m.get(k);
  if (l) l.push(v);
  else m.set(k, [v]);
}

/** The lookups of `mesh` (built on first use, then kept with it). */
export function meshIndex(mesh: MeshData): MeshIndex {
  const had = built.get(mesh);
  if (had) return had;
  const objectParts = new Map<number, number[]>();
  const meshParts = new Map<number, number[]>();
  const regionParts = new Map<number, number[]>();
  const objects: number[] = [];
  for (let i = 0; i < mesh.parts.length; i++) {
    const p = mesh.parts[i];
    if (p.kind === "face") {
      if (!objectParts.has(p.owner)) objects.push(p.owner);
      push(objectParts, p.owner, i);
    } else if (p.kind === "mesh") push(meshParts, p.owner, i);
    else if (p.kind === "region") push(regionParts, p.owner, i);
  }
  const objectOwner = new Map<number, number>();
  mesh.edgeOwners.forEach((o, k) => {
    if (o && o.kind === "object") objectOwner.set(o.id, k);
  });
  // Counting sort of the segments by owner.
  const owners = mesh.edgeOwners.length;
  const ownerStart = new Uint32Array(owners + 1);
  const eo = mesh.edgeOwner;
  for (let i = 0; i < eo.length; i++) if (eo[i] < owners) ownerStart[eo[i] + 1]++;
  for (let k = 0; k < owners; k++) ownerStart[k + 1] += ownerStart[k];
  const ownerEdges = new Uint32Array(ownerStart[owners]);
  const at = ownerStart.slice(0, owners);
  for (let i = 0; i < eo.length; i++) if (eo[i] < owners) ownerEdges[at[eo[i]]++] = i;
  const ix: MeshIndex = { objectParts, meshParts, regionParts, objects, objectOwner, ownerStart, ownerEdges };
  built.set(mesh, ix);
  return ix;
}

/** How many edge segments `owners` (indices into `edgeOwners`) have together. */
export function ownerEdgeCount(mesh: MeshData, owners: readonly number[]): number {
  const ix = meshIndex(mesh);
  let n = 0;
  for (const k of owners) if (k + 1 < ix.ownerStart.length) n += ix.ownerStart[k + 1] - ix.ownerStart[k];
  return n;
}

/** The segments of `owners` as xyz pairs (six floats each), appended after `extra`. */
export function ownerSegments(mesh: MeshData, owners: readonly number[], extra: Float32Array = new Float32Array(0)): Float32Array {
  const ix = meshIndex(mesh);
  const out = new Float32Array(extra.length + 6 * ownerEdgeCount(mesh, owners));
  out.set(extra);
  let w = extra.length;
  const e = mesh.edges;
  for (const k of owners) {
    if (k + 1 >= ix.ownerStart.length) continue;
    for (let j = ix.ownerStart[k]; j < ix.ownerStart[k + 1]; j++) {
      const s = 6 * ix.ownerEdges[j];
      out.set(e.subarray(s, s + 6), w);
      w += 6;
    }
  }
  return out;
}
