import { describe, expect, it } from "vitest";
import type { DocState, Entity, MeshData } from "../kernel/types";
import { applySelection, boxSelect, entityKey, eraseOp, eraseOps, highlightFor, modeFor, objectEdges, partsOf, selectionLookup, toSketch, fromSketch } from "./entities";
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
