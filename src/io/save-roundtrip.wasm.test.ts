// @vitest-environment node
// Lane 28 (#21, #34, #35): Save refused models that had had something deleted ("Could not
// save: the file did not read back as written: the model differs"), because the reader
// numbered ids 1, 2, 3 ... again. Here every corpus .skp (the 203 files `npm run corpus:skp`
// writes, built in memory from the same models, so this needs no files on disk) is imported
// as File > Import imports it, saved as Ctrl+S saves it (the kernel's `save_shz`, which reads
// its own file back and refuses one that differs), and opened in a fresh document, which
// must hash the same. Then the deletions that used to leave holes in the ids: the first
// top-level group or component exploded, the next one deleted, the first loose solid deleted,
// each followed by a save and an open. Needs `npm run wasm`; skipped without the kernel unless
// REQUIRE_KERNEL is set.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { beforeAll, describe, expect, it } from "vitest";
import { corpus, curveModels } from "./corpus/models";
import { skpToOp } from "./skp";
import type { WasmDoc } from "../kernel/types";

const pkg = path.resolve(__dirname, "../wasm/pkg");
const built = existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));
const run = built || process.env.REQUIRE_KERNEL ? describe : describe.skip;

type Mod = { initSync(o: { module: Uint8Array }): void; document_new(): WasmDoc & { open_shz(bytes: Uint8Array): string } };
let mod: Mod;

type Graph = { nodes: { id: number; parent: number | null; kind: string }[]; objects: { id: number; parent: number | null }[] };
const graph = (d: WasmDoc) => JSON.parse(d.scene_graph()) as Graph;

/** Save `d` as Ctrl+S does and open the file in a fresh document: it must be the same model. */
function savesAndOpens(d: WasmDoc, what: string): number {
  const bytes = d.save_shz("frac-in", new Uint8Array(), "");
  const back = mod.document_new();
  back.open_shz(bytes);
  expect(back.document_hash(), `${what}: the opened file`).toBe(d.document_hash());
  expect(graph(back), `${what}: the tree and its ids`).toEqual(graph(d));
  back.free?.();
  return bytes.length;
}

const specs = [...corpus(200), ...curveModels()];

run("every corpus .skp saves and opens as the same model (lane 28)", () => {
  beforeAll(async () => {
    mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as Mod;
    mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  });

  it("covers the whole corpus", () => {
    expect(specs.length).toBe(203);
  });

  it.each(specs.map((s) => [s.name, s] as const))("%s", (name, spec) => {
    const r = skpToOp(spec.build(), `${name}.skp`);
    expect(r.op, `${name}: imports`).toBeTruthy();
    const d = mod.document_new();
    d.apply_op(JSON.stringify(r.op));
    savesAndOpens(d, `${name} as imported`);
    const top = graph(d).nodes.filter((n) => n.parent === null);
    if (top[0]) {
      d.apply_op(JSON.stringify({ op: "explode", node: top[0].id }));
      savesAndOpens(d, `${name} with its first group or component exploded`);
    }
    if (top[1]) {
      d.apply_op(JSON.stringify({ op: "delete_node", node: top[1].id }));
      savesAndOpens(d, `${name} with its second deleted`);
    }
    const loose = graph(d).objects.find((o) => o.parent === null);
    if (loose) {
      d.apply_op(JSON.stringify({ op: "delete_object", object: loose.id }));
      savesAndOpens(d, `${name} with a loose solid deleted`);
    }
    d.free?.();
  });
});
