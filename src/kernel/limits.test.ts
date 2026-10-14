// Lane 16b: one set of numbers for the model's count limits, in the kernel, the app and the
// docs. The kernel's (crates/kernel/src/limits.rs, sheets.rs, looks.rs) are read from the
// source, and docs/LIMITS.md's "The model" table from the page.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { checkCounts, countsOf, isLimitRefusal, MODEL_LIMITS, ModelLimitError } from "./limits";
import type { DocState } from "./types";

const rust = (file: string, name: string): number => {
  const src = readFileSync(`crates/kernel/src/${file}`, "utf8");
  const m = new RegExp(`pub const ${name}: usize = ([0-9_]+);`).exec(src);
  if (!m) throw new Error(`${name} not found in ${file}`);
  return Number(m[1].replace(/_/g, ""));
};

/** The "The model" table of docs/LIMITS.md: first column -> second column. */
function modelTable(): Map<string, string> {
  const md = readFileSync("docs/LIMITS.md", "utf8");
  const at = md.indexOf("### The model");
  const end = md.indexOf("\n### ", at + 5);
  const rows = new Map<string, string>();
  for (const line of md.slice(at, end).split("\n")) {
    const cells = line.split("|").map((c) => c.trim());
    if (cells.length > 3 && cells[1] && !/^-+$/.test(cells[1]) && cells[1] !== "What") rows.set(cells[1], cells[2]);
  }
  return rows;
}

const num = (s: string | undefined) => Number((/[0-9][0-9,]*/.exec(s ?? "")?.[0] ?? "NaN").replace(/,/g, ""));

describe("the model's count limits (lane 16b)", () => {
  it("are the kernel's numbers", () => {
    expect(MODEL_LIMITS.guides).toBe(rust("limits.rs", "MAX_GUIDES"));
    expect(MODEL_LIMITS.dimensions).toBe(rust("limits.rs", "MAX_DIMENSIONS"));
    expect(MODEL_LIMITS.tags).toBe(rust("limits.rs", "MAX_TAGS"));
    expect(MODEL_LIMITS.materials).toBe(rust("limits.rs", "MAX_MATERIALS"));
    expect(MODEL_LIMITS.scenes).toBe(rust("limits.rs", "MAX_SCENES"));
    expect(MODEL_LIMITS.sections).toBe(rust("limits.rs", "MAX_SECTIONS"));
    expect(MODEL_LIMITS.sheets).toBe(rust("sheets.rs", "MAX_SHEETS"));
    expect(MODEL_LIMITS.sheetItems).toBe(rust("sheets.rs", "MAX_ITEMS"));
    expect(MODEL_LIMITS.textures).toBe(rust("looks.rs", "MAX_TEXTURES"));
  });

  it("are the numbers docs/LIMITS.md states", () => {
    const t = modelTable();
    expect(num(t.get("Guides"))).toBe(MODEL_LIMITS.guides);
    expect(num(t.get("Dimensions"))).toBe(MODEL_LIMITS.dimensions);
    expect(num(t.get("Tags"))).toBe(MODEL_LIMITS.tags);
    expect(num(t.get("Materials"))).toBe(MODEL_LIMITS.materials);
    expect(t.get("Materials")).toContain(`${MODEL_LIMITS.textures}`);
    expect(num(t.get("Scenes"))).toBe(MODEL_LIMITS.scenes);
    expect(num(t.get("Section planes"))).toBe(MODEL_LIMITS.sections);
    expect(t.get("Drawing sheets")).toBe(`${MODEL_LIMITS.sheets} sheets, ${MODEL_LIMITS.sheetItems.toLocaleString("en-US")} items per sheet`);
    expect(num(t.get("Texture images"))).toBe(MODEL_LIMITS.textures);
  });

  it("refuse a count past its limit in the kernel's words", () => {
    expect(() => checkCounts({ guides: MODEL_LIMITS.guides })).not.toThrow();
    let e: unknown;
    try {
      checkCounts({ guides: 1, materials: MODEL_LIMITS.materials + 1 });
    } catch (x) {
      e = x;
    }
    expect(e).toBeInstanceOf(ModelLimitError);
    expect((e as Error).message).toBe("the model would hold 4097 materials; at most 4096");
    expect(isLimitRefusal((e as Error).message)).toBe(true);
    const state = { guides: [{}, {}], dimensions: [], tags: { a: true, b: false }, materials: [{ name: "Oak" }], scenes: [], graph: { sections: [{}] } } as unknown as DocState;
    expect(countsOf(state)).toMatchObject({ guides: 2, dimensions: 0, tags: 2, materials: 1, scenes: 0, sections: 1, sheets: 0 });
  });
});
