import { describe, expect, it } from "vitest";
import type { DocState, Entity, GraphNode, MeshData, SceneGraph } from "../kernel/types";
import { highlightFor, eraseOp, partsOf } from "./entities";
import { contextTrail, describeNode, expandNodes, objectEditable, placementOf, reachableContext, scopeEntity, sketchEditable, solidsIn } from "./scope";
import { objectsOp, subjectOf } from "../tools/xform";
import { moveOp } from "../tools/move";

const I = { linear: [[1, 0, 0], [0, 1, 0], [0, 0, 1]], translation: [0, 0, 0] } as GraphNode["xform"];
const node = (id: number, kind: "group" | "instance", parent: number | null, label: string, definition: number | null = null): GraphNode => ({
  id,
  kind,
  name: kind === "group" ? label : null,
  label,
  definition,
  parent,
  tag: null,
  hidden: false,
  locked: false,
  visible: true,
  xform: I,
  in_component: false,
});

// The bookshelf: group Carcass (1) holds Side instances 2 and 3 (definition 1); Shelf
// instances 4, 5 at the top level (definition 2). Solids: 10 in 2, 11 in 3, 12 in 4,
// 13 in 5, 14 loose. Sketch 1 at the top level, sketch 2 in the carcass.
function graph(context: number | null = null): SceneGraph {
  const path = context === null ? [] : context === 2 ? [1, 2] : [context];
  return {
    context,
    path,
    nodes: [node(1, "group", null, "Carcass"), node(2, "instance", 1, "Side", 1), node(3, "instance", 1, "Side", 1), node(4, "instance", null, "Shelf", 2), node(5, "instance", null, "Shelf", 2)],
    objects: [
      { id: 10, parent: 2, hidden: false, locked: false, visible: true, in_component: true },
      { id: 11, parent: 3, hidden: false, locked: false, visible: true, in_component: true },
      { id: 12, parent: 4, hidden: false, locked: false, visible: true, in_component: true },
      { id: 13, parent: 5, hidden: false, locked: false, visible: true, in_component: true },
      { id: 14, parent: null, hidden: false, locked: true, visible: true, in_component: false },
    ],
    meshes: [],
    sketches: [
      [1, null],
      [2, 1],
    ],
    definitions: [
      { id: 1, name: "Side", instances: 2, solids: 1, min: [0, 0, 0], max: [0.019, 0.286, 1.83] },
      { id: 2, name: "Shelf", instances: 2, solids: 1, min: [0, 0, 0], max: [0.85, 0.286, 0.019] },
    ],
    tag_colors: {},
  };
}

const state = (context: number | null = null) => ({ graph: graph(context), solids: [] }) as unknown as DocState;
const face = (object: number): Entity => ({ kind: "face", object, face: 2 });

describe("the editing context", () => {
  it("places things relative to the open context", () => {
    expect(placementOf(graph(), null)).toEqual({ kind: "here" });
    expect(placementOf(graph(), 2)).toEqual({ kind: "inside", node: 1 });
    expect(placementOf(graph(1), 2)).toEqual({ kind: "inside", node: 2 });
    expect(placementOf(graph(1), null)).toEqual({ kind: "outside" });
    expect(placementOf(graph(2), 2)).toEqual({ kind: "here" });
  });

  it("a click inside a closed group selects the outermost closed node", () => {
    expect(scopeEntity(face(10), state())).toEqual({ kind: "node", node: 1 });
    expect(scopeEntity(face(12), state())).toEqual({ kind: "node", node: 4 });
    expect(scopeEntity(face(14), state())).toEqual(face(14));
    // Inside the carcass: each side is its own unit; the shelves are outside.
    expect(scopeEntity(face(10), state(1))).toEqual({ kind: "node", node: 2 });
    expect(scopeEntity(face(12), state(1))).toBeNull();
    // Inside one side: its face itself.
    expect(scopeEntity(face(10), state(2))).toEqual(face(10));
    expect(scopeEntity(face(11), state(2))).toBeNull(); // the other side is outside it
    // Sketch regions belong to their context; guides to the whole model.
    expect(scopeEntity({ kind: "region", sketch: 1, region: 1 }, state(1))).toBeNull();
    expect(scopeEntity({ kind: "region", sketch: 2, region: 1 }, state(1))).toEqual({ kind: "region", sketch: 2, region: 1 });
    expect(scopeEntity({ kind: "guide", guide: 1 }, state(2))).toEqual({ kind: "guide", guide: 1 });
  });

  it("finds where to close to so something outside becomes reachable", () => {
    expect(reachableContext(face(12), state(2))).toBeNull();
    expect(reachableContext(face(11), state(2))).toBe(1);
    expect(reachableContext(face(10), state(2))).toBe(2);
  });

  it("knows what can be edited", () => {
    expect(objectEditable(state(), 14)).toBe(false); // locked
    expect(objectEditable(state(), 10)).toBe(false);
    expect(objectEditable(state(2), 10)).toBe(true);
    expect(objectEditable(state(), 99)).toBe(true); // not listed: the kernel judges
    expect(sketchEditable(state(1), 2)).toBe(true);
    expect(sketchEditable(state(1), 1)).toBe(false);
  });

  it("names nodes and the trail", () => {
    const g = graph(2);
    expect(describeNode(g, 1)).toBe("group 'Carcass'");
    expect(describeNode(g, 3)).toBe("component 'Side' (2 of 2)");
    expect(contextTrail(g)).toBe("Model › Carcass › Side");
    expect(contextTrail(graph())).toBe("Model");
    expect(solidsIn(g, 1)).toEqual([10, 11]);
    expect(expandNodes([{ kind: "node", node: 1 }, face(14)], g)).toEqual([
      { kind: "object", object: 10 },
      { kind: "object", object: 11 },
      face(14),
    ]);
  });
});

describe("nodes as entities", () => {
  const mesh = {
    parts: [10, 10, 11, 12, 14].map((owner, i) => ({ kind: "face", owner, index: i + 1 })),
    edges: new Float32Array(),
    edgeOwner: new Uint32Array(),
    edgeOwners: [],
  } as unknown as MeshData;

  it("cover every face of every solid inside, and fade what is outside the open one", () => {
    expect(partsOf({ kind: "node", node: 1 }, mesh, graph())).toEqual([0, 1, 2]);
    const h = highlightFor([{ kind: "node", node: 4 }], null, mesh, graph(1));
    expect(h.selectedParts).toEqual([3]);
    expect(h.fadedParts).toEqual([3, 4]);
    expect(highlightFor([], null, mesh, graph()).fadedParts).toEqual([]);
    expect(eraseOp({ kind: "node", node: 4 }, null)).toEqual({ op: "delete_node", node: 4 });
  });

  it("move, copy and arrays carry nodes", () => {
    const s = subjectOf([{ kind: "node", node: 4 }], null, new Map(), state());
    expect(s).toMatchObject({ kind: "objects", objects: [], nodes: [4], covered: [12] });
    if (typeof s === "string" || s.kind !== "objects") throw new Error(String(s));
    const x = { linear: I.linear, translation: [0, 0, 0.6] } as GraphNode["xform"];
    expect(objectsOp(s, x, true, 3)).toEqual({ op: "array", objects: [], nodes: [4], xform: x, count: 3 });
    expect(moveOp(s, [0, 0, 0.6], false)).toMatchObject({ op: "transform", nodes: [4] });
    // With nothing selected, the press picks up the closed unit under the cursor.
    expect(subjectOf([], face(10), new Map(), state())).toMatchObject({ kind: "objects", nodes: [1], covered: [10, 11] });
    expect(subjectOf([], face(12), new Map(), state(1))).toMatch(/outside/);
  });
});
