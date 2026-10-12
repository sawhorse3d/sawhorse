// @vitest-environment node
// .skp import: files written by openskp's MIT writer from our procedural corpus models (no
// SketchUp- or OpenSKP-authored fixtures), parsed by openskp, mapped onto one kernel import
// op, and applied to the real WASM kernel. Skips the kernel half when it is not built.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type { SkpModel } from "openskp";
import { FAMILIES, corpus, rng } from "./corpus/models";
import { apply, compose, isComponent, modelToOp, placementXform, skpToOp } from "./skp";
import type { ImportNode, WasmDoc } from "../kernel/types";

const pkg = path.resolve(__dirname, "../wasm/pkg");
const built = existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));

async function kernel() {
  const mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as {
    initSync(o: { module: Uint8Array }): void;
    document_new(): WasmDoc;
  };
  mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  return mod;
}

const bytesOf = (family: string, seed = 1) => FAMILIES[family](rng(seed)).toBytes();

describe("skp mapping", () => {
  it("placements compose like SketchUp's (row-major 3x3, then translation)", () => {
    const turn = [0, -1, 0, 1, 0, 0, 0, 0, 1, 10, 0, 0]; // 90 degrees about Z, then +10 x
    expect(apply(turn, [1, 0, 0])).toEqual([10, 1, 0]);
    const both = compose(turn, [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 5, 0]);
    expect(apply(both, [0, 0, 0])).toEqual([5, 0, 0]);
  });

  it("a table: the top a group, the legs instances of one component, tags, materials, inches to metres", () => {
    const r = skpToOp(bytesOf("table"), "Table.skp");
    if (r.op.op !== "import") throw new Error("not an import op");
    expect(r.op.items ?? []).toEqual([]); // nothing loose at the top level
    const nodes = r.op.nodes ?? [];
    expect(nodes.map((n) => n.kind)).toEqual(["group", "instance", "instance", "instance", "instance"]);
    const defs = r.op.definitions ?? [];
    expect(defs.map((d) => d.name)).toEqual(["Leg"]);
    expect(nodes.slice(1).every((n) => n.kind === "instance" && n.definition === 0)).toBe(true);
    expect(nodes[1].tag).toBe("Legs");
    const top = nodes[0];
    if (top.kind !== "group") throw new Error();
    expect(top.name).toBe("Top");
    expect(top.items[0].merge_coplanar).toBe(false);
    // Inches -> metres: the top (world coordinates) is 0.6 to 0.85 m up; a leg is in its
    // definition's own frame, placed by its instance.
    const z = top.items[0].positions.map((p) => p[2]);
    expect(Math.max(...z)).toBeGreaterThan(0.6);
    expect(Math.max(...z)).toBeLessThan(0.85);
    const leg = defs[0].items[0].positions;
    expect(Math.min(...leg.map((p) => p[0]))).toBe(0);
    const legs = nodes.slice(1).map((n) => (n.kind === "instance" ? n.xform.translation : null));
    expect(new Set(legs.map((t) => JSON.stringify(t))).size).toBe(4);
    expect((r.op.materials ?? []).length).toBeGreaterThan(0);
    expect(r.summary).toMatchObject({ placements: 5, components: 1, groups: 1 });
    expect(r.notes.some((n) => n.includes("1 component placed more than once came in as components"))).toBe(true);
  });

  it("placements become kernel transforms (3x3 unitless, translation in metres); groups vs components", () => {
    expect(placementXform([0, -1, 0, 1, 0, 0, 0, 0, 1, 10, 0, 20])).toEqual({
      linear: [
        [0, -1, 0],
        [1, 0, 0],
        [0, 0, 1],
      ],
      translation: [0.254, 0, 0.508],
    });
    const def = { name: "Leg" } as never;
    expect(isComponent(def, 4)).toBe(true);
    expect(isComponent(def, 1)).toBe(false);
    expect(isComponent({ name: "Group#7" } as never, 3)).toBe(false);
  });

  it("nested and mirrored components, loose geometry", () => {
    const n = skpToOp(bytesOf("nested"), "Chest.skp");
    if (n.op.op !== "import") throw new Error();
    // Drawer is a component placed several times (one mirrored); its knob, placed once in
    // the drawer's definition, is a group inside that definition.
    const defs = n.op.definitions ?? [];
    expect(defs.map((d) => d.name)).toEqual(["Drawer"]);
    expect(defs[0].children?.map((c) => [c.kind, c.name])).toEqual([["group", "Knob"]]);
    const drawers = (n.op.nodes ?? []).filter((x) => x.kind === "instance");
    expect(drawers.length).toBeGreaterThan(1);
    expect(drawers.some((x) => x.kind === "instance" && x.xform.linear[0][0] === -1)).toBe(true);
    const l = skpToOp(bytesOf("loose"), "Yard.skp");
    if (l.op.op !== "import") throw new Error();
    const root = (l.op.items ?? []).find((i) => i.name === "Yard")!;
    expect(root.faces.length).toBe(1);
    expect(root.lines!.length).toBeGreaterThan(2);
  });

  it("scenes and hidden layers from the parsed model", () => {
    const model = {
      version: "{25.0.0}",
      definitions: new Map(),
      root: { id: 0, guid: "", name: "", vertices: [], edges: [], faces: [], instances: [], sectionPlanes: [], texts: [{ text: "x", hidden: false }], dimensions: [], constructionLines: [], constructionPoints: [], isImage: false, alwaysFacesCamera: false, shadowsFaceSun: false },
      layers: [
        { name: "Layer0", color: { r: 0, g: 0, b: 0 }, hidden: false },
        { name: "Furniture", color: { r: 0, g: 0, b: 0 }, hidden: true },
      ],
      pages: [
        { name: "Front", eye: [0, -100, 50], target: [0, 0, 20], up: [0, 0, 1], fov: 35, parallel: false, orthoHeight: 0, hiddenLayers: ["Furniture"] },
        { name: "Broken", eye: null, target: null, up: null, fov: 35, parallel: false, orthoHeight: 0, hiddenLayers: [] },
      ],
      dimensions: [],
      materials: [],
      materialsById: new Map(),
      styles: [],
      units: "Millimeter",
    } as unknown as SkpModel;
    const r = modelToOp(model, "Room.skp");
    if (r.op.op !== "import") throw new Error();
    expect(r.op.scenes).toEqual([{ name: "Front", eye: [0, -2.54, 1.27], target: [0, 0, 0.508], up: [0, 0, 1], fov: 35, parallel: false, hidden_tags: ["Furniture"] }]);
    expect(r.op.hidden_tags).toEqual(["Furniture"]);
    expect(r.notes).toEqual(expect.arrayContaining([expect.stringContaining("1 scene(s) without a camera"), "not imported: 1 texts", expect.stringContaining("Millimeter")]));
  });

  it("section planes come in switched off: the model's after the import, a group's in the group (lane 12)", () => {
    const empty = { id: 0, guid: "", name: "", vertices: [], edges: [], faces: [], instances: [], sectionPlanes: [], texts: [], dimensions: [], constructionLines: [], constructionPoints: [], isImage: false, alwaysFacesCamera: false, shadowsFaceSun: false };
    const model = {
      version: "{25.0.0}",
      definitions: new Map([[1, { ...empty, id: 1, name: "Group#1", sectionPlanes: [{ plane: [0, 0, 2, -20], name: "", label: "B", hidden: true }] }]]),
      root: { ...empty, sectionPlanes: [{ plane: [1, 0, 0, -12], name: "Side cut", label: "A", hidden: false }], instances: [{ refIdx: 1, matrix: [1, 0, 0, 0, 1, 0, 0, 0, 1, 100, 0, 0], name: "", layer: "Layer0", hidden: false }] },
      layers: [],
      pages: [],
      dimensions: [],
      materials: [],
      materialsById: new Map(),
      styles: [],
      units: null,
    } as unknown as SkpModel;
    const r = modelToOp(model, "Cut.skp");
    if (r.op.op !== "batch") throw new Error(`a batch, not ${r.op.op}`);
    const [imp, plane] = r.op.ops;
    expect(plane).toEqual({ op: "add_section", origin: [12 * 0.0254, 0, 0], normal: [-1, 0, 0], name: "Side cut", active: false });
    if (imp.op !== "import") throw new Error();
    // The group's plane z = 10 in, placed 100 in along red, named by its label, hidden.
    const g = (imp.nodes ?? [])[0];
    expect(g.sections).toEqual([{ origin: [100 * 0.0254, 0, 10 * 0.0254], normal: [0, 0, -1], name: "B", hidden: true }]);
    expect(r.summary.sections).toBe(2);
    expect(r.notes).toEqual(expect.arrayContaining([expect.stringContaining("2 section planes came in switched off")]));
  });

  it("refuses a file that is not a .skp with a plain message", () => {
    expect(() => skpToOp(new Uint8Array([1, 2, 3, 4]), "x.skp")).toThrow(/could not read the SketchUp file/);
  });
});

describe.skipIf(!built)("skp files through the kernel", () => {
  it("every corpus family imports; solids where SketchUp had solids, a noted mesh where not", async () => {
    const mod = await kernel();
    const seen: Record<string, { objects: number; meshes: number; notes: string[] }> = {};
    for (const spec of corpus(20)) {
      const r = skpToOp(spec.build(), `${spec.name}.skp`);
      const d = mod.document_new();
      const out = JSON.parse(d.apply_op(JSON.stringify(r.op)));
      expect(out.kind, spec.name).toBe("imported");
      const st = JSON.parse(d.state_json());
      expect(st.stats.watertight, spec.name).toBe(true);
      seen[spec.name] = { objects: out.objects.length, meshes: out.meshes.length, notes: out.notes };
      d.free();
    }
    for (const [name, s] of Object.entries(seen)) {
      const family = name.slice(4);
      if (family === "open") {
        expect(s.objects, name).toBe(0);
        expect(s.meshes, name).toBe(1);
        expect(s.notes.join(" "), name).toMatch(/Tray: kept as a mesh, not a solid: 4 open edges/);
      } else if (family === "loose") {
        expect(s.objects, name).toBe(1);
        expect(s.meshes, name).toBe(1);
      } else {
        expect(s.meshes, `${name}: ${s.notes.join("; ")}`).toBe(0);
        expect(s.objects, name).toBeGreaterThan(0);
      }
      if (family === "reversed") expect(s.notes.join(" ")).toMatch(/turned 1 reversed face/);
    }
    expect(seen["002-table"].objects).toBe(5);
    expect(seen["004-house"].objects).toBe(5);
  });

  it("the window hole makes the front wall a genus-1 solid", async () => {
    const mod = await kernel();
    // The first house whose front wall has a window (a face with a hole loop).
    const isWall = (n: ImportNode) => n.kind === "group" && n.name === "Front wall" && n.items.some((i) => i.faces.some((f) => f.length > 1));
    const r = corpus(200)
      .filter((s) => s.family === "house")
      .map((s) => skpToOp(s.build(), `${s.name}.skp`))
      .find((x) => x.op.op === "import" && (x.op.nodes ?? []).some(isWall))!;
    if (r.op.op !== "import") throw new Error();
    const wall = { ...r.op, items: [], nodes: (r.op.nodes ?? []).filter(isWall) };
    const d = mod.document_new();
    d.apply_op(JSON.stringify(wall));
    const st = JSON.parse(d.state_json());
    expect(st.stats.objects).toBe(1);
    expect(st.stats.genus).toBe(1);
    // The wall's layer is its group's tag.
    expect(st.graph.nodes[0]).toMatchObject({ kind: "group", label: "Front wall", tag: "Walls" });
    d.free();
  });

  it("a hidden placement comes in hidden; components come in as one definition with instances", async () => {
    const { create } = await import("openskp");
    const { box } = await import("./corpus/models");
    const b = create();
    const leg = b.addComponentDefinition("Leg", (def) => box(def, 0, 0, 0, 2, 2, 10));
    b.addInstance(leg, { translation: [0, 0, 0] });
    b.addInstance(leg, { translation: [10, 0, 0], hidden: true });
    const r = skpToOp(b.toBytes(), "legs.skp");
    if (r.op.op !== "import") throw new Error();
    expect(r.op.hidden_tags).toEqual([]);
    expect((r.op.nodes ?? []).map((n) => n.hidden)).toEqual([false, true]);
    const mod = await kernel();
    const d = mod.document_new();
    d.apply_op(JSON.stringify(r.op));
    const st = JSON.parse(d.state_json());
    expect(st.stats.objects).toBe(2);
    expect(st.graph.definitions).toMatchObject([{ name: "Leg", instances: 2, solids: 1 }]);
    expect(st.graph.nodes.map((n: { kind: string; hidden: boolean; visible: boolean }) => [n.kind, n.hidden, n.visible])).toEqual([
      ["instance", false, true],
      ["instance", true, false],
    ]);
    // Edit one leg's definition through the kernel: both legs change.
    const solid = st.graph.objects.find((o: { parent: number }) => o.parent === st.graph.nodes[0].id).id;
    d.apply_op(JSON.stringify({ op: "set_context", node: st.graph.nodes[0].id }));
    d.apply_op(JSON.stringify({ op: "push_pull", object: solid, face: JSON.parse(d.faces_json(solid)).find((f: { normal: number[] }) => f.normal[2] > 0.9).id, distance: 0.0254 }));
    const after = JSON.parse(d.state_json());
    for (const s of after.solids) expect(s.max[2]).toBeCloseTo(11 * 0.0254, 9);
    d.free();
  });

  it("faces keep their own materials; texture images, sizes and positions come in (lane 10)", async () => {
    const { create } = await import("openskp");
    const { deflateSync } = await import("node:zlib");
    // A 4 x 2 RGB PNG written here (no fixture).
    const crcTable = Array.from({ length: 256 }, (_, n) => {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      return c >>> 0;
    });
    const crc = (b: Uint8Array) => {
      let c = 0xffffffff;
      for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8);
      return (c ^ 0xffffffff) >>> 0;
    };
    const chunk = (type: string, data: Uint8Array) => {
      const out = new Uint8Array(12 + data.length);
      const dv = new DataView(out.buffer);
      dv.setUint32(0, data.length);
      out.set(new TextEncoder().encode(type), 4);
      out.set(data, 8);
      dv.setUint32(8 + data.length, crc(out.subarray(4, 8 + data.length)));
      return out;
    };
    const ihdr = new Uint8Array(13);
    new DataView(ihdr.buffer).setUint32(0, 4);
    new DataView(ihdr.buffer).setUint32(4, 2);
    ihdr.set([8, 2, 0, 0, 0], 8);
    const raw = new Uint8Array(2 * (1 + 12)).map((_, i) => (i % 13 === 0 ? 0 : 120 + (i % 7) * 10));
    const parts = [new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", ihdr), chunk("IDAT", new Uint8Array(deflateSync(raw))), chunk("IEND", new Uint8Array())];
    const png = new Uint8Array(parts.reduce((n, x) => n + x.length, 0));
    let at = 0;
    for (const x of parts) {
      png.set(x, at);
      at += x.length;
    }
    const b = create();
    const oak = b.addTextureMaterial("Oak", png, "oak.png", 6, 12);
    const red = b.addMaterial("Red", [180, 30, 20]);
    // A 10 x 4 x 2 in box: the top red, every other face oak.
    b.addGroup((g) => {
      const P = (x: number, y: number, z: number): [number, number, number] => [x, y, z];
      g.addFace([P(0, 0, 0), P(0, 4, 0), P(10, 4, 0), P(10, 0, 0)], { material: oak });
      g.addFace([P(0, 0, 2), P(10, 0, 2), P(10, 4, 2), P(0, 4, 2)], { material: red });
      g.addFace([P(0, 0, 0), P(10, 0, 0), P(10, 0, 2), P(0, 0, 2)], { material: oak });
      g.addFace([P(10, 0, 0), P(10, 4, 0), P(10, 4, 2), P(10, 0, 2)], { material: oak });
      g.addFace([P(10, 4, 0), P(0, 4, 0), P(0, 4, 2), P(10, 4, 2)], { material: oak });
      g.addFace([P(0, 4, 0), P(0, 0, 0), P(0, 0, 2), P(0, 4, 2)], { material: oak });
    }, { name: "Board" });
    const r = skpToOp(b.toBytes(), "board.skp");
    if (r.op.op !== "batch") throw new Error(`expected the image batched first, got ${r.op.op}`);
    const [tex, imp] = r.op.ops;
    expect(tex).toMatchObject({ op: "add_texture", path: "assets/skp-oak.png", mime: "image/png" });
    if (imp.op !== "import") throw new Error();
    const oakDef = (imp.materials ?? []).find((m) => m.name === "Oak")!;
    expect(oakDef.texture).toBe("assets/skp-oak.png");
    expect(oakDef.size?.[0]).toBeCloseTo(12 * 0.0254, 9);
    expect(oakDef.size?.[1]).toBeCloseTo(6 * 0.0254, 9);
    expect(r.notes.join(" ")).toMatch(/1 texture image imported/);
    const mod = await kernel();
    const d = mod.document_new();
    d.apply_op(JSON.stringify(r.op));
    const st = JSON.parse(d.state_json());
    expect(st.stats.objects).toBe(1);
    const id = st.objects[0];
    const shows = [1, 2, 3, 4, 5, 6].map((f) => JSON.parse(d.sample(id, f)));
    expect(shows.filter((m) => m === "Red")).toHaveLength(1);
    expect(shows.filter((m) => m === "Oak")).toHaveLength(5);
    expect(st.textures.map((t: { path: string }) => t.path)).toEqual(["assets/skp-oak.png"]);
    d.free();
  });
});
