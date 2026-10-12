// The model API over the real kernel (WASM, as the worker runs it): a plugin's batch with
// placeholders applies as ONE op and one undo step, lands on the same hashes as the
// built-in tools (the spike_box golden), and a refused batch leaves the document exactly
// as it was. Needs `npm run wasm`; skipped without the kernel unless REQUIRE_KERNEL is set.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { Batch, componentOp } from "../../packages/plugin-api/src/runtime.ts";
import type { CutList, DocState, WasmDoc } from "../kernel/types";
import { cutListOf, solidsOf, statsOf, treeOf } from "./model";

const pkg = path.resolve(__dirname, "../wasm/pkg");
const built = existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));
const golden = (name: string) => JSON.parse(readFileSync(path.resolve(__dirname, `../../crates/kernel/tests/golden/${name}.json`), "utf8"));

type Mod = { initSync(o: { module: Buffer }): void; document_new(): WasmDoc & { apply_named_batch(json: string): string } };
async function kernel(): Promise<Mod> {
  const mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as Mod;
  mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  return mod;
}

/** Build ops with the plugin-side Batch (as a plugin would) and hand them to the kernel. */
async function opsOf(build: (b: Batch) => void): Promise<{ name: string; ops: unknown[] }> {
  let sent: { name: string; ops: unknown[] } | null = null;
  const b = new Batch("Test", async (name, ops) => {
    sent = { name, ops };
    return [];
  });
  build(b);
  await b.commit();
  return sent!;
}

const run = built || process.env.REQUIRE_KERNEL ? describe : describe.skip;

run("plugin batches through the WASM kernel", () => {
  it("a batch that draws and pulls a box hashes as the built-in box (spike_box golden)", async () => {
    const mod = await kernel();
    const d = mod.document_new();
    const req = await opsOf((b) => {
      const rect = b.op({ op: "add_rectangle", sketch: 1, x: 0, y: 0, w: 0.6096, h: 0.4064 });
      b.op({ op: "extrude", sketch: 1, region: rect.region(0), distance: 0.3048 });
    });
    const out = JSON.parse(d.apply_named_batch(JSON.stringify(req)));
    expect(out.kind).toBe("batch");
    expect(out.outcomes[1]).toMatchObject({ kind: "object_created" });
    const g = golden("spike_box");
    expect(d.document_hash()).toBe(g.hash);
    expect(JSON.parse(d.cut_list("{}")).hash).toBe(g.cut_list);
    const st = JSON.parse(d.state_json()) as DocState;
    expect(st.undo_depth).toBe(1);
    // The log holds one plain batch (it replays anywhere).
    const log = JSON.parse(d.op_log_json());
    expect(log[log.length - 1].op).toBe("batch");
    expect(JSON.stringify(log)).not.toContain("$ref");
    d.undo();
    expect(JSON.parse(d.state_json()).stats.objects).toBe(0);
  });

  it("a refused batch leaves the document untouched and names the op", async () => {
    const mod = await kernel();
    const d = mod.document_new();
    d.apply_op(JSON.stringify({ op: "add_rectangle", sketch: 1, x: 0, y: 0, w: 0.3, h: 0.3 }));
    d.apply_op(JSON.stringify({ op: "extrude", sketch: 1, region: 1, distance: 0.3 }));
    const before = { hash: d.document_hash(), log: d.op_log_json(), state: d.state_json() };
    const req = await opsOf((b) => {
      const box = b.box({ min: [1, 0, 0], size: [0.1, 0.1, 0.1], name: "Ok" });
      b.rename({ object: box.object }, "Renamed");
      b.op({ op: "push_pull", object: 4242, face: 1, distance: 0.1 });
    });
    expect(() => d.apply_named_batch(JSON.stringify(req))).toThrow(/op 2 \(push_pull\): no object #4242/);
    expect({ hash: d.document_hash(), log: d.op_log_json(), state: d.state_json() }).toEqual(before);
  });

  it("a component of named parts reads straight into the Outliner and the cut list", async () => {
    const mod = await kernel();
    const d = mod.document_new();
    const t = 0.75 * 0.0254;
    const op = componentOp({
      name: "Cabinet",
      instanceName: "Base cabinet",
      parts: [
        { name: "Side", boxes: [{ min: [0, 0, 0], size: [t, 0.3, 0.9] }], copies: [{ at: [0, 0, 0], name: "Left side" }, { at: [0.6 - t, 0, 0], name: "Right side" }] },
        { name: "Shelf", boxes: [{ min: [0, 0, 0], size: [0.6 - 2 * t, 0.3, t] }], copies: [1, 2, 3].map((k) => ({ at: [t, 0, 0.2 * k] as [number, number, number], name: `Shelf ${k}` })) },
      ],
    });
    d.apply_named_batch(JSON.stringify({ name: "Generate cabinet", ops: [op] }));
    const st = JSON.parse(d.state_json()) as DocState;
    const tree = treeOf(st);
    const cab = tree.children[0];
    expect(cab).toMatchObject({ kind: "instance", label: "Base cabinet" });
    expect(cab.children.map((c) => c.label)).toEqual(["Left side", "Right side", "Shelf 1", "Shelf 2", "Shelf 3"]);
    expect(statsOf(st)).toMatchObject({ solids: 5, components: 3, instances: 6, groups: 0 });
    expect(solidsOf(st).every((s) => s.inComponent && !s.editable)).toBe(true);
    const cut = cutListOf(JSON.parse(d.cut_list("{}")) as CutList);
    expect(cut.rows.map((r) => [r.name, r.qty]).sort()).toEqual([
      ["Shelf", 3],
      ["Side", 2],
    ]);
    expect(cut.rows.find((r) => r.name === "Side")!.length).toBeCloseTo(0.9, 9);
  });
});
