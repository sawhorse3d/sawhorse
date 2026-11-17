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
import { builtInStockText, glueNote, JOINT_LOSS_DEFAULT, KERF_DEFAULT } from "./format";

const IN = 0.0254;
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
    // The golden: its glued 16 in top, at every rounding.
    check(mod.document_replay(JSON.stringify(golden("cutlist-mixed").ops)), false);
    // A 3 in rail drawn first and hidden, then a 15 in panel: with Visible only the rail's row
    // drops out and the panel's moves up one, and the glue-up must follow it.
    const d = mod.document_new();
    const board = (w: number, y: number) => {
      const drawn = JSON.parse(d.apply_op(JSON.stringify({ op: "draw", target: { on: "plane", origin: [0, 0, 0], normal: [0, 0, 1] }, points: [[0, y * IN, 0], [40 * IN, y * IN, 0], [40 * IN, (y + w) * IN, 0], [0, (y + w) * IN, 0]], closed: true })));
      return JSON.parse(d.apply_op(JSON.stringify({ op: "extrude", sketch: drawn.sketch, region: drawn.inside, distance: 0.75 * IN }))).object as number;
    };
    const rail = board(3, 0);
    board(15, 10);
    d.apply_op(JSON.stringify({ op: "set_hidden", items: [{ object: rail }], hidden: true }));
    const rows = (visible_only: boolean) => (JSON.parse(d.nest(JSON.stringify({ visible_only }))) as Nesting).glue_ups!.map((g) => g.row);
    expect([rows(false), rows(true)]).toEqual([[1], [0]]);
    check(d, true);
  });

  /** Every glue-up's row is the cut list's row for its part at every rounding, Visible only
   * on and off, and every other row has no note. */
  function check(d: WasmDoc, hidden: boolean) {
    for (const visible_only of [false, true]) {
      const n = JSON.parse(d.nest(JSON.stringify({ visible_only }))) as Nesting;
      expect(n.glue_ups?.length).toBeGreaterThan(0);
      if (hidden) expect((JSON.parse(d.cut_list(JSON.stringify({ visible_only }))) as CutList).rows.length).toBe(visible_only ? 1 : 2);
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
      // The golden's 16 in top beside narrow oak (#56 points 1 to 3): one joint, from the 12 in
      // board; the lone 15 in panel: one joint, from 4/4 x 8 (library order on the tie).
      const list = JSON.parse(d.cut_list(JSON.stringify({ visible_only }))) as CutList;
      const wide = n.glue_ups!.find((g) => list.rows[g.row].size[1] > 14.9 * IN)!;
      expect(glueNote(list.rows[wide.row], wide.row, n)).toBe(hidden ? "glued from 2 (4/4 x 8 x 96)" : "glued from 2 (4/4 x 12 x 96)");
    }
  }

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

  it("#58: Built-in stock puts back what its message says: the kernel's own defaults", async () => {
    const mod = await kernel();
    const d = mod.document_replay(JSON.stringify(golden("cutlist").ops));
    d.apply_op(JSON.stringify({ op: "set_nest_settings", settings: { kerf: 0.002, trim: 0.005, algorithm: "maxrects", jointLoss: 0.001, minStrip: 0.03 } }));
    expect((JSON.parse(d.nest("{}")) as Nesting).settings.kerf).toBe(0.002);
    d.apply_op(JSON.stringify({ op: "reset_stock_library" }));
    const s = (JSON.parse(d.nest("{}")) as Nesting).settings;
    expect([s.kerf, s.trim, s.algorithm, s.jointLoss ?? JOINT_LOSS_DEFAULT, s.minStrip]).toEqual([KERF_DEFAULT, 0, "guillotine", JOINT_LOSS_DEFAULT, undefined]);
    expect(builtInStockText("sixteenth")).toBe("kerf 1/8 in, no trim, joint loss 1/8 in, minimum strip auto, table saw");
  });
});
