// The cabinet generator sample (plugins/cabinet-generator/main.js) run as a plugin, with the
// "sawhorse" module faked here and its batches applied to the real kernel (WASM): the panel,
// what it keeps in storage, its messages, the parts it makes, the model hash, the cut list
// and the undo step. The panel in the app is driven in e2e/plugins-samples.spec.ts.
// The kernel tests need `npm run wasm`; skipped without the kernel unless REQUIRE_KERNEL is set.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import { Batch } from "../../packages/plugin-api/src/runtime.ts";
import { decodeValue, Encoded } from "../../packages/plugin-api/src/wire.ts";
import type { CutList, DocState, WasmDoc } from "../kernel/types";
import { formatLength, type UnitSystem } from "../lib/units";
import { boundsOf, cutListOf, nodesOf } from "./model";

const MM = 0.001;
const IN = 0.0254;

type Values = Record<string, string | number | boolean | null>;
type Field = { type: string; id?: string; label?: string; value?: unknown; disabled?: boolean; text?: string; tone?: string };
type PanelSpec = { id: string; body: Field[]; onChange(id: string, v: Values): unknown; onAction(id: string, v: Values): unknown };
type Item = { kind: string; id: number };
type Doc = WasmDoc & { apply_named_batch(json: string): string };

/** One plugin run: the app's side of every call main.js makes. */
class Host {
  system: UnitSystem;
  app = new Map<string, unknown>();
  doc = new Map<string, unknown>();
  selection: Item[] = [];
  notes: { text: string; kind: string }[] = [];
  spec: PanelSpec | null = null;
  /** The panel as the app would show it: the body with every update applied. */
  fields = new Map<string, Field>();
  handlers = new Map<string, (() => unknown)[]>();
  kernel: Doc | null;
  /** Without a kernel: the batches it was asked to apply. */
  sent: { name: string; ops: unknown[] }[] = [];

  constructor(system: UnitSystem, kernel: Doc | null = null) {
    this.system = system;
    this.kernel = kernel;
  }

  state(): DocState | null {
    return this.kernel ? (JSON.parse(this.kernel.state_json()) as DocState) : null;
  }

  api() {
    const host = this;
    const store = (m: Map<string, unknown>) => ({
      get: async (k: string) => (m.has(k) ? structuredClone(m.get(k)) : null),
      set: async (k: string, v: unknown) => void m.set(k, JSON.parse(JSON.stringify(v))),
    });
    return {
      units: { system: () => host.system, format: (m: number) => formatLength(m, host.system) },
      storage: { app: store(host.app), document: store(host.doc) },
      events: { on: (name: string, fn: () => unknown) => void host.handlers.set(name, [...(host.handlers.get(name) ?? []), fn]) },
      ui: {
        registerPanel: async (spec: PanelSpec) => {
          host.spec = spec;
          for (const f of spec.body) if (f.id) host.fields.set(f.id, { ...f });
          return {
            update: async (patch: { fields?: Record<string, Partial<Field>>; values?: Values }) => {
              for (const [id, p] of Object.entries(patch.fields ?? {})) host.fields.set(id, { ...host.fields.get(id)!, ...p });
              for (const [id, v] of Object.entries(patch.values ?? {})) host.fields.set(id, { ...host.fields.get(id)!, value: v });
            },
          };
        },
        registerCommand: async () => {},
        notify: async (text: string, o?: { kind?: string }) => void host.notes.push({ text, kind: o?.kind ?? "info" }),
      },
      model: {
        selection: async () => host.selection,
        select: async (items: Item[]) => void (host.selection = items),
        nodes: async () => nodesOf(host.state()),
        bounds: async () => boundsOf(null, host.state()),
        batch: (name: string) =>
          new Batch(name, async (n, ops) => {
            const plain = ops.map((o) => (o instanceof Encoded ? decodeValue(o.bytes) : o));
            host.sent.push({ name: n, ops: plain });
            if (!host.kernel) return plain.map(() => ({ kind: "imported", nodes: [1000 + host.sent.length] }));
            return JSON.parse(host.kernel.apply_named_batch(JSON.stringify({ name: n, ops: plain }))).outcomes;
          }),
      },
    };
  }

  /** The panel's values, as the app reports them with every change. */
  values(): Values {
    const out: Values = {};
    for (const [id, f] of this.fields) if (f.type === "number" || f.type === "checkbox") out[id] = f.value as number | boolean;
    return out;
  }

  /** A field typed (or ticked) and committed, as the panel reports it. */
  async set(id: string, value: number | boolean) {
    this.fields.set(id, { ...this.fields.get(id)!, value });
    await this.spec!.onChange(id, this.values());
  }

  async generate() {
    await this.spec!.onAction("generate", this.values());
  }

  async emit(name: string) {
    for (const fn of this.handlers.get(name) ?? []) await fn();
  }

  status(): string {
    return String(this.fields.get("status")?.text ?? "");
  }
}

const main = path.resolve(__dirname, "../../plugins/cabinet-generator/main.js");

/**
 * Start the plugin in `host`, as enabling it does: the "sawhorse" module (the real shim,
 * aliased for tests in vite.config.ts) hands out the API the sandbox leaves under its symbol.
 */
async function start(host: Host): Promise<Host> {
  (globalThis as Record<symbol, unknown>)[Symbol.for("sawhorse.plugin-api")] = host.api();
  vi.resetModules();
  await import(/* @vite-ignore */ main);
  return host;
}

const pkg = path.resolve(__dirname, "../wasm/pkg");
const built = existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));
let mod: { initSync(o: { module: Buffer }): void; document_new(): Doc } | null = null;
async function newDoc(): Promise<Doc> {
  if (!mod) {
    mod = await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"));
    mod!.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  }
  return mod!.document_new();
}

/** The cut list's rows by name: quantity and T x W x L in metres. */
function cut(host: Host) {
  const c = cutListOf(JSON.parse(host.kernel!.cut_list("{}")) as CutList);
  return Object.fromEntries(c.rows.map((r) => [r.name, { qty: r.qty, t: r.thickness, w: r.width, l: r.length, sheet: r.sheet }]));
}

/** Every solid's box, by its instance's name (Left side, Shelf 1, Back ...). */
function boxes(host: Host) {
  const st = host.state()!;
  const out: Record<string, { min: number[]; max: number[] }> = {};
  for (const n of nodesOf(st)) {
    if (n.kind !== "instance" || !n.name) continue;
    const b = boundsOf([{ kind: "node", node: n.id } as never], st);
    if (b) out[n.name] = b;
  }
  return out;
}

const run = built || process.env.REQUIRE_KERNEL ? describe : describe.skip;

/** A box as [min, max] rounded to the micrometre, to compare with sizes typed in mm or inches. */
const um = (b: { min: number[]; max: number[] }) => [b.min, b.max].map((p) => p.map((v) => Math.round(v * 1e6) / 1e6));

/**
 * What the generator's defaults made on 9c6ea80 (0.1.0-alpha.6 and lane 46), before the back
 * had a field, measured with this file's harness on that commit's main.js: the document
 * hash after one Generate. The defaults must still make exactly these models.
 */
const BEFORE = {
  metric: "cffd0b6d53517ef0",
  inch: "98188732947e50fe",
  // 30 x 72 x 12 in, three shelves (the user guide's and e2e's cabinet).
  inch30x72x12: "8ab29ce2ee74ec38",
  // The metric defaults with Back panel off.
  metricNoBack: "f5c3c1af7ddcfd7b",
  // Started in an inch document, the units then set to millimetres: inch sizes, a 6 mm back.
  inchThenMm: "7a0ffd6b2936473c",
};

describe("cabinet generator: the back thickness field", () => {
  it("sits right after Stock, in the document's units, starting at 6 mm or 1/4 in", async () => {
    for (const [system, start6] of [
      ["mm", 6 * MM],
      ["cm", 6 * MM],
      ["frac-in", 0.25 * IN],
      ["ft-in", 0.25 * IN],
    ] as const) {
      const h = await start(new Host(system));
      const ids = h.spec!.body.map((f) => f.id);
      expect(ids.slice(0, 6)).toEqual(["width", "height", "depth", "thickness", "backThickness", "shelves"]);
      expect(h.fields.get("backThickness")).toMatchObject({ type: "number", label: "Back thickness", unit: "length", min: 0, value: start6, disabled: false });
    }
  });

  it("is greyed out while Back panel is off, and the back is then left out of every check", async () => {
    const h = await start(new Host("mm"));
    await h.set("back", false);
    expect(h.fields.get("backThickness")!.disabled).toBe(true);
    expect(h.status()).toBe("Adds a cabinet: 600 x 720 x 560, 1 shelf, 100 toe kick.");
    // A back of no thickness does not matter without a back.
    await h.set("backThickness", 0);
    expect(h.status()).toBe("Adds a cabinet: 600 x 720 x 560, 1 shelf, 100 toe kick.");
    expect(h.fields.get("generate")!.disabled).toBe(false);
    await h.set("back", true);
    expect(h.fields.get("backThickness")!.disabled).toBe(false);
    expect(h.status()).toBe("The back needs a thickness.");
  });

  it("says the back's thickness in the summary line", async () => {
    const mm = await start(new Host("mm"));
    expect(mm.status()).toBe("Adds a cabinet: 600 x 720 x 560, 1 shelf, 6 back, 100 toe kick.");
    await mm.set("backThickness", 18 * MM);
    expect(mm.status()).toBe("Adds a cabinet: 600 x 720 x 560, 1 shelf, 18 back, 100 toe kick.");
    const inch = await start(new Host("frac-in"));
    expect(inch.status()).toBe("Adds a cabinet: 24 x 34 1/2 x 24, 1 shelf, 1/4 back, 4 toe kick.");
  });

  it("refuses a back of no thickness, or one that leaves no depth, in the generator's words", async () => {
    const h = await start(new Host("mm"));
    for (const [value, text] of [
      [0, "The back needs a thickness."],
      [-5 * MM, "The back needs a thickness."],
      [Number.NaN, "The back needs a thickness."],
      // As thick as the cabinet is deep, and a hair under the depth less the stock.
      [560 * MM, "The depth must be more than the stock and the back together (578)."],
      [542 * MM, "The depth must be more than the stock and the back together (560)."],
    ] as const) {
      await h.set("backThickness", value);
      expect(h.status()).toBe(text);
      expect(h.fields.get("status")!.tone).toBe("warning");
      expect(h.fields.get("generate")!.disabled).toBe(true);
      // Generate (the command or the shortcut) says so and makes nothing.
      await h.generate();
      expect(h.notes.at(-1)).toEqual({ text, kind: "warning" });
      expect(h.sent).toHaveLength(0);
    }
    // Just under that, it is a cabinet with very shallow shelves.
    await h.set("backThickness", 541 * MM);
    expect(h.status()).toBe("Adds a cabinet: 600 x 720 x 560, 1 shelf, 541 back, 100 toe kick.");
    // Without a back the depth only has to be more than the stock.
    await h.set("back", false);
    await h.set("depth", 10 * MM);
    expect(h.status()).toBe("The depth must be more than the stock (18).");
  });

  it("keeps a typed thickness for next time; sizes kept before the field existed start at the default", async () => {
    // What the plugin kept in alpha.6: the sizes without a back thickness.
    const old = { width: 0.762, height: 1.8288, depth: 0.3048, thickness: 0.01905, shelves: 3, back: true, toeKick: 0.1016 };
    for (const [system, start6] of [
      ["mm", 6 * MM],
      ["frac-in", 0.25 * IN],
    ] as const) {
      const h = new Host(system);
      h.app.set("sizes", old);
      await start(h);
      expect(h.fields.get("backThickness")!.value).toBe(start6);
      expect(h.fields.get("width")!.value).toBe(0.762);
      expect(h.status()).not.toMatch(/NaN|undefined/);
      // Changing another size keeps the back as it was: it is not written down as typed.
      await h.set("shelves", 2);
      expect(h.app.get("sizes")).toEqual({ ...old, shelves: 2 });
      // Typed, it is kept, and the next start shows it.
      await h.set("backThickness", 18 * MM);
      expect(h.app.get("sizes")).toEqual({ ...old, shelves: 2, backThickness: 18 * MM });
      const again = new Host(system);
      again.app = h.app;
      await start(again);
      expect(again.fields.get("backThickness")!.value).toBe(18 * MM);
    }
    // A kept value that is not a number (a hand-edited store) is the default, not NaN.
    for (const bad of ["18", null, true, { mm: 18 }]) {
      const h = new Host("mm");
      h.app.set("sizes", { ...old, backThickness: bad });
      await start(h);
      expect(h.fields.get("backThickness")!.value).toBe(6 * MM);
      expect(h.status()).toBe("Adds a cabinet: 762 x 1828.8 x 304.8, 3 shelves, 6 back, 101.6 toe kick.");
    }
  });

  it("a back nobody typed follows the document's units; a typed one stays the length it was", async () => {
    const h = await start(new Host("frac-in"));
    expect(h.fields.get("backThickness")!.value).toBe(0.25 * IN);
    h.system = "mm";
    await h.emit("unitsChanged");
    expect(h.fields.get("backThickness")!.value).toBe(6 * MM);
    await h.set("backThickness", 0.5 * IN);
    h.system = "frac-in";
    await h.emit("unitsChanged");
    expect(h.fields.get("backThickness")!.value).toBe(0.5 * IN);
    expect(h.status()).toContain(", 1/2 back,");
  });
});

run("cabinet generator through the kernel", () => {
  it("the defaults make exactly the cabinets they made before the field", async () => {
    const metric = await start(new Host("mm", await newDoc()));
    await metric.generate();
    expect(metric.kernel!.document_hash()).toBe(BEFORE.metric);
    expect(metric.notes.at(-1)!.text).toBe("Made a cabinet: Side x 2, Top, Bottom, Shelf, Back, Toe kick. The cut list lists every part.");
    const inch = await start(new Host("frac-in", await newDoc()));
    await inch.generate();
    expect(inch.kernel!.document_hash()).toBe(BEFORE.inch);
    const big = await start(new Host("frac-in", await newDoc()));
    for (const [id, v] of [["width", 30 * IN], ["height", 72 * IN], ["depth", 12 * IN], ["shelves", 3]] as const) await big.set(id, v);
    await big.generate();
    expect(big.kernel!.document_hash()).toBe(BEFORE.inch30x72x12);
    const noBack = await start(new Host("mm", await newDoc()));
    await noBack.set("back", false);
    await noBack.generate();
    expect(noBack.kernel!.document_hash()).toBe(BEFORE.metricNoBack);
    const switched = await start(new Host("frac-in", await newDoc()));
    switched.system = "mm";
    await switched.emit("unitsChanged");
    await switched.generate();
    expect(switched.kernel!.document_hash()).toBe(BEFORE.inchThenMm);
    // Sizes kept by alpha.6 (no back thickness) make the cabinet they made then.
    const kept = new Host("mm", await newDoc());
    kept.app.set("sizes", { width: 600 * MM, height: 720 * MM, depth: 560 * MM, thickness: 18 * MM, shelves: 1, back: true, toeKick: 100 * MM });
    await start(kept);
    await kept.generate();
    expect(kept.kernel!.document_hash()).toBe(BEFORE.metric);
  });

  it("a 16 mm and an 18 mm back sit between the sides at the rear, and the top, bottom and shelf stop at them", async () => {
    for (const b of [16, 18]) {
      const h = await start(new Host("mm", await newDoc()));
      await h.set("backThickness", b * MM);
      await h.generate();
      const at = boxes(h);
      // Between the sides (18 mm stock, 600 wide), from the toe kick (100) to the top (720),
      // its back face flush with the sides' (560 deep).
      expect(um(at["Back"])).toEqual([
        [0.018, (560 - b) * MM, 0.1],
        [0.582, 0.56, 0.72],
      ]);
      for (const name of ["Top", "Bottom", "Shelf 1"]) {
        expect(at[name].min[1]).toBeCloseTo(0, 9);
        expect(at[name].max[1]).toBeCloseTo((560 - b) * MM, 9);
      }
      for (const name of ["Left side", "Right side"]) expect(um(at[name]).map((p) => p[1])).toEqual([0, 0.56]);
      expect(um(at["Cabinet"])).toEqual([
        [0, 0, 0],
        [0.6, 0.56, 0.72],
      ]);
      // The cut list: a Back as thick as typed, 564 x 620, solid stock like the sides, which
      // is how a part over 1/4 in with no material reads; the rest as before, less deep.
      const rows = cut(h);
      expect(rows["Back"]).toMatchObject({ qty: 1, sheet: false });
      expect([rows["Back"].t, rows["Back"].w, rows["Back"].l].map((v) => Math.round(v * 1e6) / 1e3)).toEqual([b, 564, 620]);
      expect(rows["Side"]).toMatchObject({ qty: 2, sheet: false });
      expect(rows["Side"].w).toBeCloseTo(0.56, 9);
      for (const name of ["Top", "Bottom", "Shelf"]) expect([rows[name].w, rows[name].l].map((v) => Math.round(v * 1e6) / 1e3)).toEqual([560 - b, 564]);
      expect(h.notes.at(-1)!.text).toBe("Made a cabinet: Side x 2, Top, Bottom, Shelf, Back, Toe kick. The cut list lists every part.");
      // Only which cabinet it made is kept in the document, as before.
      expect([...h.doc.keys()]).toEqual(["cabinet"]);
    }
  });

  it("a 1/2 in back in an inch document: Back 1/2 x 28 1/2 x 68, the shelves 11 1/2 deep", async () => {
    const h = await start(new Host("frac-in", await newDoc()));
    for (const [id, v] of [["width", 30 * IN], ["height", 72 * IN], ["depth", 12 * IN], ["shelves", 3], ["backThickness", 0.5 * IN]] as const) await h.set(id, v);
    expect(h.status()).toBe("Adds a cabinet: 30 x 72 x 12, 3 shelves, 1/2 back, 4 toe kick.");
    await h.generate();
    const rows = cut(h);
    const inches = (r: { t: number; w: number; l: number }) => [r.t, r.w, r.l].map((v) => Math.round((v / IN) * 1e6) / 1e6);
    expect(inches(rows["Back"])).toEqual([0.5, 28.5, 68]);
    expect(rows["Back"].sheet).toBe(false);
    expect(inches(rows["Side"])).toEqual([0.75, 12, 72]);
    for (const name of ["Top", "Bottom", "Shelf"]) expect(inches(rows[name])).toEqual([0.75, 11.5, 28.5]);
    expect(rows["Shelf"].qty).toBe(3);
    expect(inches(rows["Toe kick"])).toEqual([0.75, 4, 28.5]);
    expect(um(boxes(h)["Back"])[0][1]).toBeCloseTo(11.5 * IN, 6);
  });

  it("changing only the back's thickness and generating again replaces the cabinet, as one undo step", async () => {
    const h = await start(new Host("mm", await newDoc()));
    await h.generate();
    const first = h.kernel!.document_hash();
    const depth = (JSON.parse(h.kernel!.state_json()) as DocState).undo_depth;
    const id = h.doc.get("cabinet");
    // The new cabinet is selected, so Generate updates it.
    expect(h.fields.get("generate")!.label).toBe("Update selected cabinet");
    await h.set("backThickness", 18 * MM);
    expect(h.status()).toBe("Replaces the selected cabinet with 600 x 720 x 560, 1 shelf, 18 back, 100 toe kick.");
    await h.generate();
    expect(h.sent.at(-1)!.name).toBe("Update cabinet");
    expect(h.notes.at(-1)!.text).toMatch(/^Updated the cabinet: /);
    const st = JSON.parse(h.kernel!.state_json()) as DocState;
    expect(st.undo_depth).toBe(depth + 1);
    // One cabinet, in the same place, with the thicker back.
    expect(nodesOf(st).filter((n) => n.name === "Cabinet")).toHaveLength(1);
    expect(st.solids).toHaveLength(7);
    expect(um(boxes(h)["Cabinet"])).toEqual([
      [0, 0, 0],
      [0.6, 0.56, 0.72],
    ]);
    expect(cut(h)["Back"].t).toBeCloseTo(0.018, 9);
    expect(h.doc.get("cabinet")).not.toBe(id);
    // One Ctrl+Z brings back the 6 mm one.
    h.kernel!.undo();
    expect(h.kernel!.document_hash()).toBe(first);
  });
});

