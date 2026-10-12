import { describe, expect, it } from "vitest";
import type { DocState, MaterialInfo, MeshData, MeshPart, PaintTarget, SceneGraph, UvXform } from "../kernel/types";
import { fakeHost, ev } from "./fakeHost";
import { PaintTool, dragUv, textureFrame, type PaintContext } from "./paint";

const OAK: MaterialInfo = { name: "White oak", color: [205, 178, 136, 255], texture: "assets/lib-white-oak.png", held: true, size: [0.6, 0.3], rotation: 0, offset: [0, 0] };

function part(owner: number, index: number, extra: Partial<MeshPart> = {}): MeshPart {
  return { kind: "face", owner, index, origin: [0, 0, 1], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0], ...extra };
}

function mesh(parts: MeshPart[]): MeshData {
  return { positions: new Float32Array(), indices: new Uint32Array(), faceIds: new Uint32Array(), edges: new Float32Array(), edgeOwner: new Uint32Array(), edgeOwners: [], parts };
}

/** Solid 1 at the top level, solid 2 inside group 5. */
function state(context: number | null = null): DocState {
  const graph: SceneGraph = {
    context,
    path: context === null ? [] : [context],
    nodes: [{ id: 5, kind: "group", name: "Carcass", label: "Carcass", definition: null, parent: null, tag: null, hidden: false, locked: false, visible: true, xform: { linear: [[1, 0, 0], [0, 1, 0], [0, 0, 1]], translation: [0, 0, 0] }, in_component: false }],
    objects: [
      { id: 1, parent: null, hidden: false, locked: false, visible: true, in_component: false },
      { id: 2, parent: 5, hidden: false, locked: false, visible: true, in_component: false },
    ],
    meshes: [],
    sketches: [],
    definitions: [],
    tag_colors: {},
  };
  return { graph, materials: [OAK] } as unknown as DocState;
}

function setup(active: MaterialInfo | null = OAK) {
  const painted: { target: PaintTarget; what: string }[] = [];
  const previews: ({ parts: number[]; material: MaterialInfo } | null)[] = [];
  const uvs: { part: number; n: number }[] = [];
  const sampled: [number, number][] = [];
  const ctx: PaintContext = {
    active: () => active,
    paint: async (target, what) => {
      painted.push({ target, what });
      return true;
    },
    sample: async (o, f) => {
      sampled.push([o, f]);
      return "White oak";
    },
    preview: (p) => previews.push(p),
    faceUvs: async () => new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]),
    setPartUvs: (p, u) => uvs.push({ part: p, n: u.length }),
    materialOf: (p) => (p.material === OAK.name ? OAK : null),
  };
  const h = fakeHost({
    _mesh: mesh([part(1, 2, { material: OAK.name }), part(1, 3), part(2, 2), part(2, 3, { own: true })]),
    _state: state(),
  });
  const tool = new PaintTool(h, ctx);
  return { h, tool, painted, previews, uvs, sampled };
}

const onFace = (object: number, face: number, opts: Parameters<typeof ev>[2] = {}) =>
  ev("click", [0.5, 0.5, 1], { picked: { entity: { kind: "face", object, face }, point: [0.5, 0.5, 1], surface: null }, ...opts });

describe("Paint tool", () => {
  it("paints a face in the open context, a closed group's default, and a whole solid with Ctrl", async () => {
    const { tool, painted } = setup();
    tool.onEvent(onFace(1, 2));
    expect(painted.at(-1)?.target).toEqual({ on: "faces", object: 1, faces: [2] });
    tool.onEvent(onFace(2, 2));
    expect(painted.at(-1)?.target).toEqual({ on: "items", items: [{ node: 5 }] });
    expect(painted.at(-1)?.what).toBe("group 'Carcass'");
    tool.onEvent(onFace(1, 3, { ctrl: true }));
    expect(painted.at(-1)?.target).toEqual({ on: "items", items: [{ object: 1 }] });
  });

  it("paints every selected face when the click lands on one of them", () => {
    const { h, tool, painted } = setup();
    h.sel = [
      { kind: "face", object: 1, face: 2 },
      { kind: "face", object: 1, face: 3 },
    ];
    tool.onEvent(onFace(1, 3));
    expect(painted.at(-1)?.target).toEqual({ on: "faces", object: 1, faces: [2, 3] });
    expect(painted.at(-1)?.what).toBe("2 faces");
  });

  it("a click on a selected item paints the whole selection", () => {
    const { h, tool, painted } = setup();
    h.sel = [{ kind: "node", node: 5 }, { kind: "object", object: 1 }];
    tool.onEvent(onFace(2, 2));
    expect(painted.at(-1)?.target).toEqual({ on: "items", items: [{ node: 5 }, { object: 1 }] });
    expect(painted.at(-1)?.what).toBe("2 selected items");
  });

  it("previews on hover: the face, or the group's faces that inherit", () => {
    const { tool, previews } = setup();
    tool.onEvent({ ...onFace(1, 3), type: "move" });
    expect(previews.at(-1)).toEqual({ parts: [1], material: OAK });
    // A face that already shows the material needs no preview.
    tool.onEvent({ ...onFace(1, 2), type: "move" });
    expect(previews.at(-1)).toBeNull();
    tool.onEvent({ ...onFace(2, 2), type: "move" });
    // Face 3 of solid 2 has paint of its own: it keeps it, so it is not previewed.
    expect(previews.at(-1)?.parts).toEqual([2]);
    tool.onEvent(ev("leave", [0, 0, 0]));
    expect(previews.at(-1)).toBeNull();
  });

  it("samples with Alt and asks for a material when none is active", () => {
    const { tool, sampled, painted } = setup();
    tool.onEvent(onFace(2, 3, { alt: true }));
    expect(sampled).toEqual([[2, 3]]);
    expect(painted).toHaveLength(0);
    const none = setup(null);
    none.tool.onEvent(onFace(1, 2));
    expect(none.painted).toHaveLength(0);
    expect(none.h.statuses.at(-1)?.error).toBe(true);
  });

  it("positions a texture with handles and applies it once when let go", async () => {
    const { h, tool, uvs } = setup();
    expect(await tool.startPositioning(1, 2)).toBe(true);
    const hs = tool.handleScreen()!;
    // Drag the move handle: a quarter along x.
    const at = (x: number, y: number) => ({ origin: [x, y, 10] as [number, number, number], dir: [0, 0, -1] as [number, number, number] });
    tool.onEvent({ ...ev("drag", [0.75, 0.5, 1]), start: hs.move, ray: at(0.75, 0.5) });
    expect(tool.positioningFace()?.uv.offset?.[0]).toBeCloseTo(0.25);
    tool.onEvent({ ...ev("dragend", [0.75, 0.5, 1]), start: hs.move, ray: at(0.75, 0.5) });
    await Promise.resolve();
    expect(h.ops.at(-1)).toMatchObject({ op: "set_face_uv", object: 1, face: 2 });
    expect(uvs.length).toBeGreaterThan(0);
    // Esc ends positioning.
    expect(tool.cancel()).toBe(true);
    expect(tool.positioningFace()).toBeNull();
  });

  it("refuses to position a face without a texture", async () => {
    const { tool } = setup();
    expect(await tool.startPositioning(1, 3)).toBe(false);
  });
});

describe("texture positioning maths", () => {
  const mat = { rotation: 0, offset: [0, 0] as [number, number] };
  const uvAt = (uv: UvXform, q: [number, number]) => {
    const th = ((uv.rotation ?? 0) * Math.PI) / 180;
    const o = uv.offset ?? [0, 0];
    const s = uv.scale ?? [1, 1];
    const x = q[0] - o[0];
    const y = q[1] - o[1];
    return [(Math.cos(th) * x + Math.sin(th) * y) / s[0], (-Math.sin(th) * x + Math.cos(th) * y) / s[1]];
  };
  it("turning and scaling keep the texture fixed under the centre", () => {
    const start: UvXform = { offset: [0.1, -0.2], rotation: 10, scale: [1, 1] };
    const c: [number, number] = [0.5, 0.25];
    const turned = dragUv("turn", start, mat, c, [0.9, 0.25], [0.5, 0.65]);
    expect(turned.rotation).toBeCloseTo(100);
    expect(uvAt(turned, c)[0]).toBeCloseTo(uvAt(start, c)[0]);
    expect(uvAt(turned, c)[1]).toBeCloseTo(uvAt(start, c)[1]);
    const scaled = dragUv("scale", start, mat, c, [0.6, 0.25], [0.7, 0.25]);
    expect(scaled.scale).toEqual([2, 2]);
    expect(uvAt(scaled, c)[0]).toBeCloseTo(uvAt(start, c)[0]);
    const moved = dragUv("move", start, mat, c, [0, 0], [0.3, 0.1]);
    expect(moved.offset?.[0]).toBeCloseTo(0.4);
    expect(moved.offset?.[1]).toBeCloseTo(-0.1);
  });

  it("the texture frame turns to the longer side, like the kernel's", () => {
    const p = part(1, 1);
    expect(textureFrame(p, [[0, 0, 1], [2, 0, 1], [2, 1, 1], [0, 1, 1]]).u).toEqual([1, 0, 0]);
    expect(textureFrame(p, [[0, 0, 1], [1, 0, 1], [1, 3, 1], [0, 3, 1]]).u).toEqual([0, 1, 0]);
  });
});
