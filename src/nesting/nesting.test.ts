// Nesting round trips through the real WASM kernel (lane 9): the golden bookshelves (the
// cut list's, nested on the built-in stock: oak on 4/4 boards, the back on a 1/4 in sheet;
// and the plywood carcass on a 5 x 10 sheet with kerf 3/32 in, trim and maxrects) are
// replayed, nested, and every export is read back with every part in it where the kernel
// put it; the DXF also goes back through Sawhorse's own DXF importer, where every part
// outline becomes a sketch region. Skips (with a message) when the WASM kernel is not
// built, unless REQUIRE_KERNEL=1.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type { CutList, Nesting, WasmDoc } from "../kernel/types";
import { checkDxf, checkPdf, checkSvg } from "./checks";
import { nestingDxf } from "./dxf";
import { glueNote } from "./format";

const pkg = path.resolve(__dirname, "../wasm/pkg");
const built = existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));
if (!built && process.env.REQUIRE_KERNEL === "1") throw new Error("REQUIRE_KERNEL=1 but src/wasm/pkg is missing: run npm run wasm");

interface Mod {
  initSync(o: { module: Uint8Array }): void;
  document_new(): WasmDoc & { state_json(): string };
  document_replay(ops: string): WasmDoc & { state_json(): string };
}

async function kernel(): Promise<Mod> {
  const mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as Mod;
  mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  return mod;
}

const golden = (name: string) => JSON.parse(readFileSync(path.resolve(__dirname, `../../crates/kernel/tests/golden/${name}.json`), "utf8")) as { ops: unknown[]; nesting: string };

describe.skipIf(!built)("nesting through the WASM kernel", () => {
  for (const name of ["cutlist", "nesting"]) {
    it(`the ${name} golden nests as native did, and every export holds every part`, async () => {
      const mod = await kernel();
      const g = golden(name);
      const d = mod.document_replay(JSON.stringify(g.ops));
      const n = JSON.parse(d.nest("{}")) as Nesting;
      expect(n.hash).toBe(g.nesting);
      expect(n.totals.parts_in).toBe(n.totals.placed + n.totals.unplaced);
      expect(n.pieces.length).toBeGreaterThan(0);
      checkSvg(n, name);
      checkDxf(n);
      checkPdf(n, name);
      // The DXF read back by Sawhorse's own importer: every part outline is a region.
      const fresh = mod.document_new();
      const out = JSON.parse(fresh.import_file(`${name} nesting.dxf`, new TextEncoder().encode(nestingDxf(n)), JSON.stringify({ units: "mm" }))) as { outcome: { kind: string; sketches: number[] }; notes: string[] };
      expect(out.outcome.kind).toBe("imported");
      expect(out.outcome.sketches.length).toBeGreaterThan(0);
      // The tags are TEXT, which the importer skips and says so.
      expect(out.notes.join(" ")).toMatch(/TEXT/);
      const st = JSON.parse(fresh.state_json()) as { stats: { regions: number } };
      const placed = n.pieces.reduce((s, p) => s + p.placements.length, 0);
      expect(st.stats.regions).toBeGreaterThanOrEqual(placed);
    });
  }

  it("#56: each glue-up's row is the cut list's row for that part, at every rounding and with Visible only", async () => {
    const mod = await kernel();
    const d = mod.document_replay(JSON.stringify(golden("cutlist-mixed").ops));
    for (const visible_only of [false, true]) {
      const n = JSON.parse(d.nest(JSON.stringify({ visible_only }))) as Nesting;
      expect(n.glue_ups?.length).toBeGreaterThan(0);
      for (const rounding of ["sixteenth", "thirty_second", "half_mm"]) {
        const list = JSON.parse(d.cut_list(JSON.stringify({ rounding, visible_only }))) as CutList;
        const glued = new Set(n.glue_ups!.map((g) => g.row));
        list.rows.forEach((row, i) => {
          const note = glueNote(row, i, n);
          if (!glued.has(i)) return expect(note, row.name).toBeNull();
          const gs = n.glue_ups!.filter((g) => g.row === i);
          expect(gs.every((g) => g.name === row.name)).toBe(true);
          expect(note).toBe(`glued from ${gs[0].widths.length} (${gs[0].stock})`);
        });
      }
      // The 16 in top beside narrow oak (#56 points 1 to 3): one joint, from the 12 in board.
      const list = JSON.parse(d.cut_list("{}")) as CutList;
      const top = n.glue_ups!.find((g) => list.rows[g.row].size[1] > 15 * 0.0254)!;
      expect(glueNote(list.rows[top.row], top.row, n)).toBe("glued from 2 (4/4 x 12 x 96)");
    }
  });

  it("the default bookshelf: oak on five 4/4 x 12 boards, the back on one 1/4 in sheet", async () => {
    const mod = await kernel();
    const d = mod.document_replay(JSON.stringify(golden("cutlist").ops));
    const n = JSON.parse(d.nest("{}")) as Nesting;
    // The golden adds a mirrored side (three sides) and grain across on the top, which no
    // 12 in board takes (36 in across): reported, not dropped.
    expect(n.totals.sheets).toBe(1);
    expect(n.unplaced.map((u) => [u.name, u.reason])).toEqual([["Top", "grain"]]);
    expect(n.pieces.filter((p) => p.kind === "board").every((p) => p.stock === "4/4 x 12 x 96")).toBe(true);
    const maxrects = JSON.parse(d.nest(JSON.stringify({ algorithm: "maxrects" }))) as Nesting;
    expect(maxrects.algorithm).toBe("maxrects");
    expect(maxrects.totals.placed).toBe(n.totals.placed);
  });
});
