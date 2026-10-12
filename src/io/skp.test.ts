// @vitest-environment node
// .skp import: files written by openskp's MIT writer from our procedural corpus models (no
// SketchUp- or OpenSKP-authored fixtures), parsed by openskp, mapped onto one kernel import
// op, and applied to the real WASM kernel. Skips the kernel half when it is not built.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type { SkpModel } from "openskp";
import { FAMILIES, corpus, rng } from "./corpus/models";
import { HIDDEN_TAG, apply, compose, modelToOp, skpToOp } from "./skp";
import type { WasmDoc } from "../kernel/types";

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

  it("a table: groups, components as separate placements, tags, materials, inches to metres", () => {
    const r = skpToOp(bytesOf("table"), "Table.skp");
    if (r.op.op !== "import") throw new Error("not an import op");
    const items = r.op.items ?? [];
    expect(items.map((i) => i.name)).toEqual(["Top", "Leg", "Leg", "Leg", "Leg"]);
    expect(items.map((i) => i.group)).toEqual(["Top", "Leg", "Leg #2", "Leg #3", "Leg #4"]);
    expect(items[1].tag).toBe("Legs");
    expect(items.every((i) => i.merge_coplanar === false)).toBe(true);
    expect((r.op.materials ?? []).length).toBe(new Set(items.map((i) => i.material)).size);
    // Inches -> metres: every coordinate is a multiple of 0.0254 / 1000 at most.
    const top = items[0].positions;
    expect(Math.max(...top.map((p) => p[2]))).toBeGreaterThan(0.6);
    expect(Math.max(...top.map((p) => p[2]))).toBeLessThan(0.85);
    expect(r.summary.placements).toBe(5);
    expect(r.notes.some((n) => n.includes("separate copies"))).toBe(true);
  });

  it("hidden instances, nested and mirrored components, loose geometry", () => {
    const n = skpToOp(bytesOf("nested"), "Chest.skp");
    if (n.op.op !== "import") throw new Error();
    const names = (n.op.items ?? []).map((i) => i.name);
    expect(names.filter((x) => x === "Knob").length).toBe(names.filter((x) => x === "Drawer").length);
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
    const r = corpus(200)
      .filter((s) => s.family === "house")
      .map((s) => skpToOp(s.build(), `${s.name}.skp`))
      .find((x) => x.op.op === "import" && (x.op.items ?? []).some((i) => i.name === "Front wall" && i.faces.some((f) => f.length > 1)))!;
    if (r.op.op !== "import") throw new Error();
    const wall = { ...r.op, items: (r.op.items ?? []).filter((i) => i.name === "Front wall") };
    const d = mod.document_new();
    d.apply_op(JSON.stringify(wall));
    const st = JSON.parse(d.state_json());
    expect(st.stats.objects).toBe(1);
    expect(st.stats.genus).toBe(1);
    expect(st.object_info[0].tag).toBe("Walls");
    d.free();
  });

  it("a hidden placement comes in on a hidden tag", async () => {
    const { create } = await import("openskp");
    const { box } = await import("./corpus/models");
    const b = create();
    const leg = b.addComponentDefinition("Leg", (def) => box(def, 0, 0, 0, 2, 2, 10));
    b.addInstance(leg, { translation: [0, 0, 0] });
    b.addInstance(leg, { translation: [10, 0, 0], hidden: true });
    const r = skpToOp(b.toBytes(), "legs.skp");
    if (r.op.op !== "import") throw new Error();
    expect(r.op.hidden_tags).toEqual([HIDDEN_TAG]);
    const mod = await kernel();
    const d = mod.document_new();
    d.apply_op(JSON.stringify(r.op));
    const st = JSON.parse(d.state_json());
    expect(st.stats.objects).toBe(2);
    expect(st.tags[HIDDEN_TAG]).toBe(false);
    d.free();
  });
});
