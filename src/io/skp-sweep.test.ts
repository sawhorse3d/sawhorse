// @vitest-environment node
// Lane 25: every .skp under a private directory through the import path in Node (openskp's
// parse, src/io/skp.ts's mapping, the real WASM kernel), one row per file: whether it opens
// or is refused (with the exact message), what came in (solids, non-solid meshes, curves,
// scenes) and the import notes. It is the quick half of e2e/private-fixtures.spec.ts's sweep
// (that one drives the app): fast enough to run before and after an openskp bump and diff.
//
//   SAWHORSE_PRIVATE_FIXTURES=<dir> [SKP_SWEEP_OUT=<file.json>] npx vitest run src/io/skp-sweep.test.ts
//
// It skips when SAWHORSE_PRIVATE_FIXTURES is unset or missing, so CI never depends on it. The
// files are never copied anywhere; only their relative names and the numbers are written.
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { skpToOp } from "./skp";
import type { WasmDoc } from "../kernel/types";

const dir = process.env.SAWHORSE_PRIVATE_FIXTURES;
const pkg = path.resolve(__dirname, "../wasm/pkg");
const ready = !!dir && existsSync(dir) && existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));

function skpFiles(root: string): string[] {
  const out: string[] = [];
  const walk = (rel: string) => {
    for (const e of readdirSync(path.join(root, rel), { withFileTypes: true })) {
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) walk(r);
      else if (/\.skp$/i.test(e.name)) out.push(r);
    }
  };
  walk("");
  return out.sort();
}

export interface SweepRow {
  file: string;
  opens: boolean;
  refusal?: string;
  version?: string;
  solids?: number;
  meshes?: number;
  curves?: number;
  curveKinds?: Record<string, number>;
  scenes?: number;
  empty?: boolean;
  notes?: string[];
}

describe.skipIf(!ready)("private .skp sweep (Node)", () => {
  it("every file opens or is refused in words, and the rows are written", async () => {
    const mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as {
      initSync(o: { module: Uint8Array }): void;
      document_new(): WasmDoc;
    };
    mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
    const rows: SweepRow[] = [];
    for (const file of skpFiles(dir!)) {
      const bytes = new Uint8Array(readFileSync(path.join(dir!, file)));
      let r: ReturnType<typeof skpToOp>;
      try {
        r = skpToOp(bytes, path.basename(file));
      } catch (e) {
        rows.push({ file, opens: false, refusal: e instanceof Error ? e.message : String(e) });
        continue;
      }
      if (!r.op) {
        rows.push({ file, opens: true, version: r.summary.version, empty: true, solids: 0, meshes: 0, curves: 0, scenes: 0, notes: r.notes });
        continue;
      }
      const d = mod.document_new();
      const out = JSON.parse(d.apply_op(JSON.stringify(r.op)));
      if (out.error || out.kind === "error") {
        rows.push({ file, opens: false, version: r.summary.version, refusal: `kernel: ${out.error ?? JSON.stringify(out)}` });
        d.free();
        continue;
      }
      const st = JSON.parse(d.state_json());
      const m = d.get_scene_mesh();
      const curves = JSON.parse(m.curves_json()) as { kind: string }[];
      m.free();
      const kinds: Record<string, number> = {};
      for (const c of curves) kinds[c.kind] = (kinds[c.kind] ?? 0) + 1;
      const imported = out.kind === "batch" ? out.outcomes.find((o: { kind: string }) => o.kind === "imported") : out;
      rows.push({
        file,
        opens: true,
        version: r.summary.version,
        solids: st.stats.objects,
        meshes: st.meshes.length,
        curves: curves.length,
        curveKinds: kinds,
        scenes: st.scenes.length,
        notes: [...r.notes, ...((imported?.notes as string[] | undefined) ?? [])],
      });
      d.free();
    }
    for (const row of rows) console.log(`[skp-sweep] ${JSON.stringify(row)}`);
    if (process.env.SKP_SWEEP_OUT) writeFileSync(process.env.SKP_SWEEP_OUT, JSON.stringify(rows, null, 2) + "\n");
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) if (!row.opens) expect(row.refusal, row.file).toMatch(/\S/);
  }, 600_000);
});
