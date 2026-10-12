// @vitest-environment node
// Every exporter's output read back by an independent reader, through the real WASM kernel:
// DXF by the `dxf-parser` library, GLB by three.js's GLTFLoader, 3MF unzipped by fflate and
// its model XML checked, STL by hand. Then the kernel's own importers re-import STL, 3MF and
// GLB and must get the same solids back. Skips (with a message) when the WASM kernel is not
// built, unless REQUIRE_KERNEL=1.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import DxfParser from "dxf-parser";
import { unzipSync, strFromU8 } from "fflate";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import * as THREE from "three";
import type { WasmDoc } from "../kernel/types";

const pkg = path.resolve(__dirname, "../wasm/pkg");
const built = existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));
if (!built && process.env.REQUIRE_KERNEL === "1") throw new Error("REQUIRE_KERNEL=1 but src/wasm/pkg is missing: run npm run wasm");

interface Mod {
  initSync(o: { module: Uint8Array }): void;
  document_new(): WasmDoc & { import_file(name: string, bytes: Uint8Array, options: string): string };
}

async function kernel(): Promise<Mod> {
  const mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as Mod;
  mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  return mod;
}

/** Two painted, named boxes: 100 x 200 x 50 mm and 40 x 40 x 30 mm. */
function twoBoxes(mod: Mod) {
  const d = mod.document_new();
  d.apply_op(JSON.stringify({ op: "add_rectangle", sketch: 1, x: 0, y: 0, w: 0.1, h: 0.2 }));
  d.apply_op(JSON.stringify({ op: "extrude", sketch: 1, region: 1, distance: 0.05 }));
  // The second rectangle's region id comes from the outcome of drawing it.
  const drawn = JSON.parse(d.apply_op(JSON.stringify({ op: "add_rectangle", sketch: 1, x: 0.3, y: 0, w: 0.04, h: 0.04 })));
  d.apply_op(JSON.stringify({ op: "extrude", sketch: 1, region: drawn.regions[0], distance: 0.03 }));
  for (const o of [
    { op: "define_material", name: "Oak", color: [200, 160, 100, 255] },
    { op: "set_material", object: 1, material: "Oak" },
    { op: "rename_object", object: 1, name: "Side panel" },
    { op: "rename_object", object: 2, name: "Block" },
  ])
    d.apply_op(JSON.stringify(o));
  return d;
}

const text = (b: Uint8Array) => new TextDecoder().decode(b);

describe.skipIf(!built)("exporters, read by independent readers", () => {
  it("DXF: dxf-parser reads the model and both outline flavours", async () => {
    const mod = await kernel();
    const d = twoBoxes(mod);
    const parser = new DxfParser();
    const model = parser.parseSync(text(d.export_file("dxf", "")))!;
    expect(model.header["$INSUNITS"]).toBe(4);
    const kinds = model.entities.map((e) => e.type);
    expect(kinds.filter((k) => k === "3DFACE")).toHaveLength(12);
    expect(kinds.filter((k) => k === "LINE")).toHaveLength(24);
    expect(Object.keys(model.tables.layer.layers).sort()).toEqual(["0", "Block", "Side panel"]);
    const corners = model.entities
      .filter((e) => e.type === "3DFACE")
      .flatMap((e) => (e as unknown as { vertices: { x: number; y: number; z: number }[] }).vertices);
    expect(Math.max(...corners.map((v) => v.y))).toBe(200);
    const outline = JSON.stringify({ outline: { kind: "face", owner: 1, index: 2 } });
    const lw = parser.parseSync(text(d.export_file("dxf-outline", outline)))!;
    expect(lw.entities).toHaveLength(1);
    const poly = lw.entities[0] as unknown as { type: string; shape: boolean; vertices: { x: number; y: number }[] };
    expect(poly.type).toBe("LWPOLYLINE");
    expect(poly.shape).toBe(true);
    expect(poly.vertices.map((v) => [v.x, v.y])).toEqual(
      expect.arrayContaining([
        [0, 0],
        [100, 200],
      ]),
    );
    const r12 = parser.parseSync(text(d.export_file("dxf-outline", JSON.stringify({ outline: { kind: "face", owner: 1, index: 2 }, flavour: "r12" }))))!;
    expect(r12.entities[0].type).toBe("POLYLINE");
    expect((r12.entities[0] as unknown as { vertices: unknown[] }).vertices).toHaveLength(4);
    d.free();
  });

  it("GLB: three.js GLTFLoader loads a named node per solid, Y up, in metres", async () => {
    const mod = await kernel();
    const d = twoBoxes(mod);
    const glb = d.export_file("glb", "");
    const gltf = await new Promise<{ scene: THREE.Group }>((resolve, reject) =>
      new GLTFLoader().parse(glb.slice().buffer, "", resolve as never, reject),
    );
    const names = gltf.scene.children.map((c) => c.name);
    // three.js sanitises node names for animation binding: spaces become underscores.
    expect(names).toEqual(["Side_panel", "Block"]);
    const box = new THREE.Box3().setFromObject(gltf.scene.children[0]);
    expect(box.max.y).toBeCloseTo(0.05, 6); // height is +Y
    expect(box.min.z).toBeCloseTo(-0.2, 6); // depth runs to -Z
    const mesh = gltf.scene.children[0] as THREE.Mesh;
    expect(mesh.geometry.getAttribute("_face_id")).toBeTruthy();
    expect((mesh.material as THREE.MeshStandardMaterial).name).toBe("Oak");
    d.free();
  });

  it("3MF: a zip with the core parts; slicer-ready model XML", async () => {
    const mod = await kernel();
    const d = twoBoxes(mod);
    const files = unzipSync(d.export_file("3mf", ""));
    expect(Object.keys(files).sort()).toEqual(["3D/3dmodel.model", "[Content_Types].xml", "_rels/.rels"]);
    const xml = strFromU8(files["3D/3dmodel.model"]);
    expect(xml).toContain('unit="millimeter"');
    expect(xml.match(/<object /g)).toHaveLength(2);
    expect(xml.match(/<item /g)).toHaveLength(2);
    expect(xml).toContain('name="Side panel"');
    expect(strFromU8(files["_rels/.rels"])).toContain('Target="/3D/3dmodel.model"');
    d.free();
  });

  it("STL: binary layout, millimetres; re-imported by our reader it is the same solids", async () => {
    const mod = await kernel();
    const d = twoBoxes(mod);
    const before = JSON.parse(d.state_json()).stats;
    for (const [format, name] of [
      ["stl", "parts.stl"],
      ["3mf", "parts.3mf"],
      ["glb", "parts.glb"],
      ["obj", "parts.obj"],
      ["dxf", "parts.dxf"],
    ] as const) {
      const bytes = d.export_file(format, "");
      if (format === "stl") {
        const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
        expect(view.getUint32(80, true)).toBe(24);
        expect(bytes.byteLength).toBe(84 + 24 * 50);
      }
      const fresh = mod.document_new();
      const r = JSON.parse(fresh.import_file(name, bytes, ""));
      expect(r.outcome.kind, format).toBe("imported");
      expect(r.outcome.objects, format).toHaveLength(2);
      expect(r.outcome.meshes, format).toHaveLength(0);
      const after = JSON.parse(fresh.state_json()).stats;
      expect(after.faces, format).toBe(before.faces);
      expect(after.vertices, format).toBe(before.vertices);
      expect(Math.abs(after.volume_m3 - before.volume_m3), format).toBeLessThan(1e-9);
      fresh.free();
    }
    d.free();
  });

  it("an open mesh: STL and 3MF refuse it by name; GLB keeps it as not solid", async () => {
    const mod = await kernel();
    const d = mod.document_new();
    d.apply_op(
      JSON.stringify({
        op: "import",
        items: [{ name: "Sheet", positions: [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]], faces: [[[0, 1, 2, 3]]] }],
      }),
    );
    expect(() => d.export_file("stl", "")).toThrow(/"Sheet" is not a solid/);
    expect(() => d.export_file("3mf", "")).toThrow(/open edge/);
    const glb = d.export_file("glb", "");
    expect(text(glb.slice(20, 20 + new DataView(glb.buffer).getUint32(12, true)))).toContain('"solid":false');
    d.free();
  });
});
