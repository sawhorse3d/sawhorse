import { describe, expect, it } from "vitest";
import type { GraphSection, MeshData, SceneGraph, SectionView, V3 } from "../kernel/types";
import { activeSectionHere, clipSegment, clippedEdges, pickSection, raySection, sceneSectionsOf, sectionArrows, sectionCorners } from "./sections";
import { EMPTY_GRAPH } from "./scope";
import { objectsOp, subjectOf, translation } from "../tools/xform";
import type { DocState } from "../kernel/types";

/** A plane at x = 0.5 keeping -X, drawn 1 m square around (0.5, 0.5, 0.5). */
function plane(over: Partial<GraphSection> = {}): GraphSection {
  return {
    id: 1,
    name: "Section 1",
    parent: null,
    active: true,
    hidden: false,
    visible: true,
    origin: [0.5, 0, 0],
    normal: [-1, 0, 0],
    u: [0, -1, 0],
    v: [0, 0, 1],
    extent: [-1, 0, 0, 1],
    ...over,
  };
}

const near = (a: V3, b: V3) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]) < 1e-12;

describe("section planes in the viewport", () => {
  it("draws the plane's rectangle and arrows pointing at what stays", () => {
    const s = plane();
    const c = sectionCorners(s);
    expect(c.every((p) => p[0] === 0.5)).toBe(true);
    expect(near(c[0], [0.5, 1, 0])).toBe(true);
    const { corners, centre } = sectionArrows(s);
    // Four arrows of three strokes each, every stroke pointing towards -X.
    expect(corners.length).toBe(4 * 6);
    for (let i = 0; i < corners.length; i += 6) expect(corners[i + 1][0]).toBeLessThan(corners[i][0]);
    expect(centre.length).toBe(12);
  });

  it("is picked where the ray meets its rectangle, the centre arrow as its reverse control", () => {
    const s = plane();
    const hit = raySection({ origin: [5, 0.5, 0.5], dir: [-1, 0, 0] }, s);
    expect(hit).toEqual({ t: 4.5, control: false });
    expect(raySection({ origin: [5, 3, 0.5], dir: [-1, 0, 0] }, s)).toBeNull();
    // Looking down at the centre arrow: the reverse control.
    const ctl = raySection({ origin: [0.45, 0.5, 9], dir: [0, 0, -1] }, s, 0.06);
    expect(ctl?.control).toBe(true);
    const best = pickSection({ origin: [5, 0.5, 0.25], dir: [-1, 0, 0] }, [plane({ id: 1 }), plane({ id: 2, origin: [2, 0, 0] })], true, () => 0.001);
    expect(best?.section).toBe(2);
    expect(pickSection({ origin: [5, 0.5, 0.25], dir: [-1, 0, 0] }, [plane()], false, () => 0.001)).toBeNull();
    expect(pickSection({ origin: [5, 0.5, 0.25], dir: [-1, 0, 0] }, [plane({ visible: false })], true, () => 0.001)).toBeNull();
  });

  it("clips edges to the side kept", () => {
    const keepBelowHalf: [number, number, number, number][] = [[-1, 0, 0, 0.5]];
    expect(clipSegment([0, 0, 0], [1, 0, 0], keepBelowHalf)).toEqual([
      [0, 0, 0],
      [0.5, 0, 0],
    ]);
    expect(clipSegment([0.6, 0, 0], [1, 0, 0], keepBelowHalf)).toBeNull();
    const mesh = {
      edges: new Float32Array([0, 0, 0, 1, 0, 0, 0.6, 0, 0, 0.9, 0, 0, 0, 0, 0, 0, 1, 0]),
      edgeOwner: new Uint32Array([0, 0, 1]),
      edgeOwners: [
        { kind: "object", id: 1 },
        { kind: "sketch", id: 1 },
      ],
      edgeNormals: new Float32Array(18),
    } as unknown as MeshData;
    const view: SectionView = { cuts: [], clips: [{ planes: keepBelowHalf, objects: [1], meshes: [], sketches: [] }] };
    const c = clippedEdges(mesh, view);
    // The first edge is halved, the second cut away, the sketch's edge left alone.
    expect(c.owner).toEqual(new Uint32Array([0, 1]));
    expect([...c.edges]).toEqual([0, 0, 0, 0.5, 0, 0, 0, 0, 0, 0, 1, 0]);
    expect(c.normals?.length).toBe(12);
    // Nothing to clip: the mesh's own arrays.
    expect(clippedEdges(mesh, null).edges).toBe(mesh.edges);
  });

  it("remembers the active and hidden planes for a scene, and finds the active one here", () => {
    const g: SceneGraph = {
      ...EMPTY_GRAPH,
      context: 7,
      path: [7],
      nodes: [{ id: 7, kind: "group", name: "G", label: "G", definition: null, parent: null, tag: null, hidden: false, locked: false, visible: true, xform: { linear: [[1, 0, 0], [0, 1, 0], [0, 0, 1]], translation: [0, 0, 0] }, in_component: false }],
      sections: [plane({ id: 1 }), plane({ id: 2, active: false, hidden: true, parent: 7 })],
    };
    expect(sceneSectionsOf(g)).toEqual({ active: [1], hidden: [2] });
    // Inside group 7 there is no active plane: the model's one applies.
    expect(activeSectionHere(g)?.id).toBe(1);
    expect(activeSectionHere(EMPTY_GRAPH)).toBeUndefined();
  });
});

describe("moving section planes with Move and Rotate", () => {
  const state = { graph: { ...EMPTY_GRAPH, sections: [plane({ id: 3 })] } } as unknown as DocState;
  it("moves a plane alone, or with solids in one batch, and copies it inactive", () => {
    const only = subjectOf([{ kind: "section", section: 3 }], null, new Map(), state);
    if (typeof only === "string" || only.kind !== "objects") throw new Error(String(only));
    expect(only.sectionEdges?.length).toBe(4);
    const x = translation([1, 0, 0]);
    expect(objectsOp(only, x, false)).toEqual({ op: "move_sections", sections: [3], xform: x });
    expect(objectsOp(only, x, true, 3)).toEqual({ op: "copy_sections", sections: [3], xform: x, count: 3 });
    const both = subjectOf([{ kind: "object", object: 1 }, { kind: "section", section: 3 }], null, new Map(), state);
    if (typeof both === "string" || both.kind !== "objects") throw new Error(String(both));
    expect(objectsOp(both, x, false)).toEqual({
      op: "batch",
      ops: [
        { op: "transform", objects: [1], xform: x },
        { op: "move_sections", sections: [3], xform: x },
      ],
    });
    // Parts of a solid do not move with a plane.
    expect(typeof subjectOf([{ kind: "face", object: 1, face: 2 }, { kind: "section", section: 3 }], null, new Map(), state)).toBe("string");
  });
});
