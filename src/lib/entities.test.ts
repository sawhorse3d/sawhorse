import { describe, expect, it } from "vitest";
import type { DocState, Entity, MeshData, SectionView, V3 } from "../kernel/types";
import { rng } from "../io/corpus/models";
import { near } from "./curves";
import { applySelection, boxSelect, entityKey, eraseOp, eraseOps, heldByModel, highlightFor, modeFor, objectEdges, partsOf, selectionLookup, toSketch, fromSketch } from "./entities";
import { meshIndex, ownerEdgeCount, ownerSegments } from "./meshIndex";

// Two parts: face 2 of solid 1 (a unit square at z = 1, two triangles) and region 4 of the
// ground sketch (a unit square at x = 3); one object edge and one sketch edge.
function mesh(): MeshData {
  const sq = (x0: number, z: number) => [x0, 0, z, x0 + 1, 0, z, x0 + 1, 1, z, x0, 1, z];
  return {
    positions: new Float32Array([...sq(0, 1), ...sq(3, 0)]),
    indices: new Uint32Array([0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]),
    faceIds: new Uint32Array([0, 0, 1, 1]),
    edges: new Float32Array([0, 0, 1, 1, 0, 1, 3, 0, 0, 4, 0, 0]),
    edgeOwner: new Uint32Array([0, 1]),
    edgeOwners: [
      { kind: "object", id: 1 },
      { kind: "sketch", id: 1 },
    ],
    parts: [
      { kind: "face", owner: 1, index: 2, origin: [0, 0, 1], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] },
      { kind: "region", owner: 1, index: 4, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] },
    ],
  };
}

const face: Entity = { kind: "face", object: 1, face: 2 };
const region: Entity = { kind: "region", sketch: 1, region: 4 };
const sketchEdge: Entity = { kind: "edge", owner: { kind: "sketch", id: 1 }, a: [3, 0, 0], b: [4, 0, 0], drawn: true };
const objEdge: Entity = { kind: "edge", owner: { kind: "object", id: 1 }, a: [0, 0, 1], b: [1, 0, 1], drawn: false };

const state = {
  sketches: [{ id: 1, origin: [0, 0, 0], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }],
  groups: [],
} as unknown as DocState;

describe("entity identity", () => {
  it("keys edges regardless of direction and floating noise", () => {
    const rev: Entity = { ...objEdge, a: objEdge.b, b: [0 + 1e-12, 0, 1] } as Entity;
    expect(entityKey(rev)).toBe(entityKey(objEdge));
    expect(entityKey(face)).not.toBe(entityKey(region));
  });
});

describe("selection modes", () => {
  it("click replaces, Shift adds, Ctrl toggles", () => {
    expect(modeFor({ shift: false, ctrl: false })).toBe("replace");
    expect(modeFor({ shift: true, ctrl: false })).toBe("add");
    expect(modeFor({ shift: false, ctrl: true })).toBe("toggle");
    let sel = applySelection([], [face], "replace");
    sel = applySelection(sel, [region], "add");
    expect(sel).toEqual([face, region]);
    sel = applySelection(sel, [face], "add");
    expect(sel).toHaveLength(2);
    sel = applySelection(sel, [face, objEdge], "toggle");
    expect(sel).toEqual([region, objEdge]);
    expect(applySelection(sel, [], "replace")).toEqual([]);
  });
});

describe("highlights", () => {
  it("selected faces and regions tint their parts; edges and hover are drawn on top", () => {
    const h = highlightFor([face, objEdge], region, mesh());
    expect(h.selectedParts).toEqual([0]);
    expect(h.hoverParts).toEqual([1]);
    expect(h.selectedEdges).toEqual([[objEdge.kind === "edge" ? objEdge.a : [0, 0, 0], [1, 0, 1]]]);
    // Lane 16b: a whole solid's edges are named by their owner (the viewport draws them).
    const m = mesh();
    const o = highlightFor([{ kind: "object", object: 1 }], null, m);
    expect(o.selectedParts).toEqual([0]);
    expect(o.selectedEdges).toEqual([]);
    expect(o.selectedEdgeOwners).toEqual([0]);
    expect(Array.from(ownerSegments(m, o.selectedEdgeOwners!))).toEqual([0, 0, 1, 1, 0, 1]);
    // Hovering what is already selected adds nothing; hovering the region adds it.
    expect(highlightFor([face], face, m).hoverParts).toEqual([]);
    expect(highlightFor([face], region, m).hoverParts).toEqual([1]);
  });

  it("a mesh's lookups: each solid's parts and edge segments, built once per mesh", () => {
    const m = mesh();
    const ix = meshIndex(m);
    expect(meshIndex(m)).toBe(ix);
    expect(ix.objects).toEqual([1]);
    expect(ix.objectParts.get(1)).toEqual([0]);
    expect(ix.regionParts.get(1)).toEqual([1]);
    expect(ix.objectOwner.get(1)).toBe(0);
    expect(Array.from(ix.ownerStart)).toEqual([0, 1, 2]);
    expect(Array.from(ix.ownerEdges)).toEqual([0, 1]);
    expect(ownerEdgeCount(m, [0, 1])).toBe(2);
    // Segments appended after explicit ones, six floats each.
    expect(Array.from(ownerSegments(m, [1], new Float32Array([9, 9, 9, 8, 8, 8])))).toEqual([9, 9, 9, 8, 8, 8, 3, 0, 0, 4, 0, 0]);
    expect(objectEdges(1, m)).toEqual([
      [
        [0, 0, 1],
        [1, 0, 1],
      ],
    ]);
    expect(objectEdges(7, m)).toEqual([]);
    expect(partsOf(face, m)).toEqual([0]);
    expect(partsOf({ kind: "face", object: 1, face: 3 }, m)).toEqual([]);
    expect(partsOf(region, m)).toEqual([1]);
    expect(partsOf({ kind: "object", object: 1 }, m)).toEqual([0]);
  });

  it("selection membership: solids and groups by number, the rest by key, once per selection", () => {
    const sel: Entity[] = [{ kind: "object", object: 4 }, { kind: "node", node: 9 }, face];
    const l = selectionLookup(sel);
    expect(selectionLookup(sel)).toBe(l);
    expect(l.has({ kind: "object", object: 4 })).toBe(true);
    expect(l.has({ kind: "object", object: 9 })).toBe(false);
    expect(l.has({ kind: "node", node: 9 })).toBe(true);
    expect(l.has({ kind: "face", object: 1, face: 2 })).toBe(true);
    expect(l.has(region)).toBe(false);
  });
});

describe("erasing", () => {
  it("maps every entity to its kernel op", () => {
    expect(eraseOp(face, state)).toEqual({ op: "erase_face", object: 1, face: 2 });
    expect(eraseOp(region, state)).toEqual({ op: "erase_region", sketch: 1, region: 4 });
    expect(eraseOp(objEdge, state)).toEqual({ op: "erase_edge", object: 1, a: [0, 0, 1], b: [1, 0, 1] });
    expect(eraseOp(sketchEdge, state)).toEqual({ op: "erase_segment", sketch: 1, a: [3, 0], b: [4, 0] });
    expect(eraseOp({ kind: "object", object: 1 }, state)).toEqual({ op: "delete_object", object: 1 });
    expect(typeof eraseOp({ kind: "vertex", owner: { kind: "object", id: 1 }, at: [0, 0, 0] }, state)).toBe("string");
  });

  it("deletes whole objects first and drops their own parts from the batch", () => {
    const { ops, skipped } = eraseOps([objEdge, face, region, { kind: "object", object: 1 }], state);
    expect(ops.map((o) => o.op)).toEqual(["delete_object", "erase_region"]);
    expect(skipped).toEqual([]);
  });

  it("converts world points to a sketch frame and back", () => {
    const s = { id: 2, origin: [1, 2, 3] as [number, number, number], normal: [1, 0, 0] as [number, number, number], u: [0, 1, 0] as [number, number, number], v: [0, 0, 1] as [number, number, number] };
    expect(toSketch(s, [1, 5, 7])).toEqual([3, 4]);
    expect(fromSketch(s, 3, 4)).toEqual([1, 5, 7]);
  });
});

describe("what the model still holds (#110)", () => {
  const held = { ...state, guides: [{ id: 5, point: [0, 0, 0], dir: [1, 0, 0] }], dimensions: [{ id: 6, a: [0, 0, 0], b: [1, 0, 0], offset: [0, 1, 0], length: 1 }], graph: { nodes: [{ id: 9 }], objects: [], meshes: [], sections: [{ id: 3 }], path: [] } } as unknown as DocState;
  const f32 = (x: number) => Math.fround(x);

  it("keeps an edge its owner still has, either way round and through single precision", () => {
    expect(heldByModel(objEdge, mesh(), held)).toBe(true);
    expect(heldByModel(sketchEdge, mesh(), held)).toBe(true);
    expect(heldByModel({ ...sketchEdge, a: [4, 0, 0], b: [3, 0, 0] } as Entity, mesh(), held)).toBe(true);
    const m = { ...mesh(), edges: new Float32Array([0, 0, 1, 1, 0, 1, 0.3048, 0.1, 0, 0.9144, 0.1, 0]) };
    const exact: Entity = { ...sketchEdge, a: [0.3048, 0.1, 0], b: [0.9144, 0.1, 0] } as Entity;
    expect([f32(0.3048), f32(0.9144)]).not.toEqual([0.3048, 0.9144]);
    expect(heldByModel(exact, m, held)).toBe(true);
    // 5 km up the blue axis the allowance is 2 mm: an end 1.5 mm off is the same end.
    const tall = { ...mesh(), edges: new Float32Array([0, 0, 0, 0, 0, 5000]), edgeOwner: new Uint32Array([0]), edgeOwners: [{ kind: "sketch", id: 1 }] } as MeshData;
    for (const [a, b] of [[[0, 0, 0], [0, 0, 5000.0015]], [[0, 0, 5000.0015], [0, 0, 0]]] as [V3, V3][]) expect(heldByModel({ ...sketchEdge, a, b } as Entity, tall, held)).toBe(true);
    for (const [a, b] of [[[0, 0, 0], [0, 0, 5000.003]], [[0, 0, 5000.003], [0, 0, 0]]] as [V3, V3][]) expect(heldByModel({ ...sketchEdge, a, b } as Entity, tall, held)).toBe(false);
    expect(heldByModel({ kind: "vertex", owner: { kind: "sketch", id: 1 }, at: [0, 0, 5000.0015] }, tall, held)).toBe(true);
  });

  it("drops an edge its owner no longer has: gone, moved, one end moved, or another owner's", () => {
    const empty = { ...mesh(), edges: new Float32Array(0), edgeOwner: new Uint32Array(0), edgeOwners: [] };
    expect(heldByModel(sketchEdge, empty, held)).toBe(false);
    expect(heldByModel(objEdge, empty, held)).toBe(false);
    expect(heldByModel({ ...sketchEdge, a: [3, 1, 0], b: [4, 1, 0] } as Entity, mesh(), held)).toBe(false);
    expect(heldByModel({ ...sketchEdge, b: [5, 0, 0] } as Entity, mesh(), held)).toBe(false);
    expect(heldByModel({ ...sketchEdge, owner: { kind: "sketch", id: 2 } } as Entity, mesh(), held)).toBe(false);
    expect(heldByModel({ ...sketchEdge, owner: { kind: "object", id: 1 } } as Entity, mesh(), held)).toBe(false);
    expect(heldByModel({ ...objEdge, owner: { kind: "sketch", id: 1 } } as Entity, mesh(), held)).toBe(false);
    // A solid that is gone, with an edge where a sketch still has one.
    expect(heldByModel({ ...sketchEdge, owner: { kind: "object", id: 2 } } as Entity, mesh(), held)).toBe(false);
  });

  it("finds ends as a scan of every segment with `near` would, from 1 cm to 5 km out", () => {
    // Seeded meshes of one sketch's segments at random sizes, single precision as the view
    // mesh is. Each segment asked for at its exact ends, and at ends moved by up to twice the
    // allowance, across the grid's cell boundaries: the answer must be the plain scan's.
    const r = rng(2026_11_27);
    const counts = { kept: 0, dropped: 0, cases: 0 };
    for (let m = 0; m < 60; m++) {
      const scale = 10 ** (-2 + 6 * r());
      const pt = (): V3 => [scale * (r() - 0.5), scale * (r() - 0.5), scale * (r() - 0.5)];
      // The first from the origin, as a sketch through it: the far ends set the allowance.
      const segs: [V3, V3][] = Array.from({ length: 40 }, (_, i) => [i === 0 ? [0, 0, 0] : pt(), pt()]);
      const edges = new Float32Array(segs.flatMap(([a, b]) => [...a, ...b]));
      const sm = { ...mesh(), edges, edgeOwner: new Uint32Array(segs.length), edgeOwners: [{ kind: "sketch", id: 1 }] } as MeshData;
      const f = (i: number, o: number): V3 => [edges[6 * i + o], edges[6 * i + o + 1], edges[6 * i + o + 2]];
      const scan = (a: V3, b: V3) => segs.some((_, i) => (near(a, f(i, 0)) && near(b, f(i, 3))) || (near(a, f(i, 3)) && near(b, f(i, 0))));
      const nudge = (p: V3): V3 => {
        const tol = 2 * (2e-6 + 4e-7 * Math.max(...p.map(Math.abs)));
        return p.map((x) => x + tol * (2 * r() - 1)) as V3;
      };
      for (const [a, b] of segs) {
        for (const [qa, qb] of [[a, b], [b, a], [nudge(a), nudge(b)], [nudge(b), nudge(a)]] as [V3, V3][]) {
          const want = scan(qa, qb);
          const e: Entity = { kind: "edge", owner: { kind: "sketch", id: 1 }, a: qa, b: qb, drawn: true };
          expect(heldByModel(e, sm, held), `${JSON.stringify([qa, qb])} at scale ${scale}`).toBe(want);
          expect(heldByModel({ kind: "vertex", owner: { kind: "sketch", id: 1 }, at: qa }, sm, held)).toBe(segs.some((_, i) => near(qa, f(i, 0)) || near(qa, f(i, 3))));
          counts[want ? "kept" : "dropped"]++;
          counts.cases++;
        }
      }
    }
    // Every exact end is kept, and the moved ones land on both sides of the allowance.
    expect(counts.cases).toBe(60 * 40 * 4);
    expect(counts.kept).toBeGreaterThan(60 * 40 * 2);
    expect(counts.dropped).toBeGreaterThan(500);
  });

  it("keeps a point at an end of one of its owner's edges, and drops it elsewhere", () => {
    expect(heldByModel({ kind: "vertex", owner: { kind: "sketch", id: 1 }, at: [4, 0, 0] }, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "vertex", owner: { kind: "object", id: 1 }, at: [0, 0, 1] }, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "vertex", owner: { kind: "sketch", id: 1 }, at: [3.5, 0, 0] }, mesh(), held)).toBe(false);
    expect(heldByModel({ kind: "vertex", owner: { kind: "object", id: 1 }, at: [4, 0, 0] }, mesh(), held)).toBe(false);
  });

  it("keeps a curve while the mesh still draws it", () => {
    const curve: Entity = { kind: "curve", owner: { kind: "sketch", id: 1 }, curve: 1 };
    const withCurve = { ...mesh(), edgeCurve: new Uint32Array([0, 1]), curves: [{ owner: { kind: "sketch", id: 1 }, curve: 1, kind: "polyline", closed: false, segments: 1, centre: [0, 0, 0], normal: [0, 0, 1] }] } as unknown as MeshData;
    expect(heldByModel(curve, withCurve, held)).toBe(true);
    expect(heldByModel(curve, mesh(), held)).toBe(false);
    expect(heldByModel({ ...curve, curve: 2 } as Entity, withCurve, held)).toBe(false);
  });

  it("keeps guides, dimensions, sections and nodes the state names, and points or edges of a guide while it stays", () => {
    expect(heldByModel({ kind: "guide", guide: 5 }, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "guide", guide: 4 }, mesh(), held)).toBe(false);
    expect(heldByModel({ kind: "dimension", dimension: 6 }, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "dimension", dimension: 7 }, mesh(), held)).toBe(false);
    expect(heldByModel({ kind: "section", section: 3 }, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "section", section: 2 }, mesh(), held)).toBe(false);
    expect(heldByModel({ kind: "node", node: 9 }, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "node", node: 8 }, mesh(), held)).toBe(false);
    expect(heldByModel({ kind: "vertex", owner: { kind: "guide", id: 5 }, at: [7, 0, 0] }, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "vertex", owner: { kind: "guide", id: 4 }, at: [7, 0, 0] }, mesh(), held)).toBe(false);
    expect(heldByModel({ kind: "edge", owner: { kind: "guide", id: 5 }, a: [0, 0, 0], b: [1, 0, 0], drawn: true }, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "edge", owner: { kind: "guide", id: 4 }, a: [0, 0, 0], b: [1, 0, 0], drawn: true }, mesh(), held)).toBe(false);
  });

  it("keeps faces, regions and solids by their parts, as before", () => {
    expect(heldByModel(face, mesh(), held)).toBe(true);
    expect(heldByModel(region, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "object", object: 1 }, mesh(), held)).toBe(true);
    expect(heldByModel({ kind: "face", object: 1, face: 3 }, mesh(), held)).toBe(false);
    expect(heldByModel({ kind: "region", sketch: 1, region: 5 }, mesh(), held)).toBe(false);
    expect(heldByModel({ kind: "object", object: 2 }, mesh(), held)).toBe(false);
  });

  it("asks each owner of one mesh in turn from its own ends, as the app does", () => {
    // One mesh, one grid per owner: the solid's edge is not found among the sketch's ends.
    const m = mesh();
    expect(heldByModel(sketchEdge, m, held)).toBe(true);
    expect(heldByModel(objEdge, m, held)).toBe(true);
    expect(heldByModel({ ...objEdge, a: [3, 0, 0], b: [4, 0, 0] } as Entity, m, held)).toBe(false);
    expect(heldByModel({ ...sketchEdge, a: [0, 0, 1], b: [1, 0, 1] } as Entity, m, held)).toBe(false);
    expect(heldByModel({ kind: "vertex", owner: { kind: "object", id: 1 }, at: [3, 0, 0] }, m, held)).toBe(false);
  });

  it("under a section plane keeps an edge or point as the view cuts it, and nothing that is gone", () => {
    // A pick under an active plane is cut short at it, or on the cut's outline: neither is a
    // segment of the view mesh end to end. Solid 1's edge (0, 0, 1)-(1, 0, 1) is cut by the
    // plane x = 0.5 (kept: x <= 0.5); sketch 1's edge (3, 0, 0)-(4, 0, 0) by x = 3.5.
    const cutAt = (x: number) => [-1, 0, 0, x] as [number, number, number, number];
    const sections = (outline: number[] = [], objects = [1]): SectionView => ({
      cuts: [{ section: 3, positions: [], indices: [], outline, area: 0 }],
      clips: [
        { planes: [cutAt(0.5)], objects, meshes: [], sketches: [] },
        { planes: [cutAt(3.5)], objects: [], meshes: [], sketches: [1] },
      ],
    });
    const solids = { ...held, solids: [{ id: 1, number: 1, min: [0, 0, 0], max: [1, 1, 1], volume: 1 }] } as unknown as DocState;
    const short: Entity = { kind: "edge", owner: { kind: "object", id: 1 }, a: [0, 0, 1], b: [0.5, 0, 1], drawn: false };
    const sketchShort: Entity = { kind: "edge", owner: { kind: "sketch", id: 1 }, a: [3, 0, 0], b: [3.5, 0, 0], drawn: true };
    const point: Entity = { kind: "vertex", owner: { kind: "sketch", id: 1 }, at: [3.5, 0, 0] };
    expect(heldByModel(short, mesh(), solids)).toBe(false);
    expect(heldByModel(short, mesh(), solids, sections())).toBe(true);
    expect(heldByModel({ ...short, a: [0.5, 0, 1], b: [0, 0, 1] } as Entity, mesh(), solids, sections())).toBe(true);
    expect(heldByModel(sketchShort, mesh(), solids, sections())).toBe(true);
    expect(heldByModel(point, mesh(), solids, sections())).toBe(true);
    // Gone, though a plane is active and the owner still drawn: a piece of an edge whose ends
    // are neither its segment's ends nor on the plane (what a line splitting it left, undone),
    // off every segment, past a segment's end, along another owner's segment.
    expect(heldByModel({ ...short, b: [0.3, 0, 1] } as Entity, mesh(), solids, sections())).toBe(false);
    expect(heldByModel({ ...sketchShort, a: [3.25, 0, 0], b: [3.75, 0, 0] } as Entity, mesh(), solids, sections())).toBe(false);
    expect(heldByModel({ ...point, at: [3.25, 0, 0] } as Entity, mesh(), solids, sections())).toBe(false);
    expect(heldByModel({ ...short, a: [0, 1, 1], b: [0.5, 1, 1] } as Entity, mesh(), solids, sections())).toBe(false);
    expect(heldByModel({ ...short, a: [0.5, 0, 1], b: [1.5, 0, 1] } as Entity, mesh(), solids, sections())).toBe(false);
    expect(heldByModel({ ...sketchShort, a: [0, 0, 1], b: [0.5, 0, 1] } as Entity, mesh(), solids, sections())).toBe(false);
    // A plane lying along the edge (one placed on a face) does not cut it short: a piece of
    // that edge, or a point along it, is gone, not cut.
    const inFace: SectionView = { cuts: [], clips: [{ planes: [[0, 0, -1, 1]], objects: [1], meshes: [], sketches: [] }] };
    expect(heldByModel(short, mesh(), solids, inFace)).toBe(false);
    expect(heldByModel({ kind: "vertex", owner: { kind: "object", id: 1 }, at: [0.5, 0, 1] }, mesh(), solids, inFace)).toBe(false);
    expect(heldByModel(objEdge, mesh(), solids, inFace)).toBe(true);
    // Nor does one the edge lies wholly on the kept side of.
    expect(heldByModel(short, mesh(), solids, { cuts: [], clips: [{ planes: [cutAt(2)], objects: [1], meshes: [], sketches: [] }] })).toBe(false);
    // On the cut's outline (on the plane, inside the solid): kept while the plane cuts it.
    const rim: Entity = { kind: "edge", owner: { kind: "object", id: 1 }, a: [0.5, 0.2, 0.5], b: [0.5, 0.8, 0.5], drawn: false };
    const corner: Entity = { kind: "vertex", owner: { kind: "object", id: 1 }, at: [0.5, 0.5, 0.5] };
    const outline = [0.5, 0, 0.5, 0.5, 1, 0.5];
    expect(heldByModel(rim, mesh(), solids, sections(outline))).toBe(true);
    expect(heldByModel(corner, mesh(), solids, sections(outline))).toBe(true);
    expect(heldByModel(rim, mesh(), solids, sections())).toBe(false);
    expect(heldByModel(corner, mesh(), solids, sections())).toBe(false);
    // Another solid's cut line, outside this one: not this solid's.
    const away: Entity = { kind: "edge", owner: { kind: "object", id: 1 }, a: [0.5, 5.2, 0.5], b: [0.5, 5.8, 0.5], drawn: false };
    expect(heldByModel(away, mesh(), solids, sections([...outline, 0.5, 5, 0.5, 0.5, 6, 0.5]))).toBe(false);
    // A sketch has no cut face: an outline never keeps a sketch's edge.
    const sketchRim: Entity = { kind: "edge", owner: { kind: "sketch", id: 1 }, a: [3.5, 0.2, 0], b: [3.5, 0.8, 0], drawn: true };
    expect(heldByModel(sketchRim, mesh(), solids, sections([3.5, 0, 0, 3.5, 1, 0]))).toBe(false);
    // Only what the planes clip: a solid they do not clip keeps the plain rule.
    expect(heldByModel(short, mesh(), solids, sections(outline, [2]))).toBe(false);
    expect(heldByModel(rim, mesh(), solids, sections(outline, [2]))).toBe(false);
    // And only while the owner is drawn at all.
    const empty = { ...mesh(), edges: new Float32Array(0), edgeOwner: new Uint32Array(0), edgeOwners: [] };
    expect(heldByModel(short, empty, solids, sections(outline))).toBe(false);
  });

  it("holds nothing with no mesh", () => {
    expect(heldByModel(objEdge, null, held)).toBe(false);
  });
});

describe("window and crossing selection", () => {
  // toScreen: x * 100, -y * 100 (z ignored), so the face spans x 0..100, y -100..0.
  const toScreen = (p: [number, number, number]): [number, number] => [p[0] * 100, -p[1] * 100];

  it("a window selects what is wholly inside", () => {
    const got = boxSelect(mesh(), toScreen, { x0: -10, y0: -110, x1: 110, y1: 10 }, false);
    expect(got).toEqual([{ kind: "object", object: 1 }]);
    const both = boxSelect(mesh(), toScreen, { x0: -10, y0: -110, x1: 500, y1: 10 }, false);
    expect(both.map((e) => e.kind).sort()).toEqual(["edge", "object", "region"]);
  });

  it("a crossing box selects what it touches", () => {
    const got = boxSelect(mesh(), toScreen, { x0: 350, y0: -60, x1: 50, y1: -40 }, true);
    expect(got.map((e) => e.kind).sort()).toEqual(["object", "region"]);
    const none = boxSelect(mesh(), toScreen, { x0: 250, y0: -60, x1: 200, y1: -40 }, true);
    expect(none).toEqual([]);
    const edgeOnly = boxSelect(mesh(), toScreen, { x0: 360, y0: 5, x1: 340, y1: -5 }, true);
    expect(edgeOnly.map((e) => e.kind).sort()).toEqual(["edge", "region"]);
  });
});
