import { afterEach, describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { DocState, MeshData } from "../kernel/types";
import { faceOf, friendlyError, hasRawId, regionName, setNameSource, solidName } from "./names";

const REUSED = 2 ** 32 + 1; // the second object in slot 1

function scene(): { state: DocState; mesh: MeshData } {
  const state = { solids: [{ id: 2, number: 2, min: [0, 0, 0], max: [1, 1, 1], volume: 1 }, { id: REUSED, number: 3, min: [0, 0, 0], max: [1, 1, 1], volume: 1 }] } as unknown as DocState;
  const part = (owner: number, index: number) => ({ kind: "region", owner, index }) as unknown as MeshData["parts"][number];
  const mesh = { parts: [part(1, 7), part(1, REUSED + 4), part(1, 3), part(5, 1)] } as unknown as MeshData;
  return { state, mesh };
}

afterEach(() => setNameSource(null, null));

describe("friendly names", () => {
  it("names solids by display number, never by public id", () => {
    const { state, mesh } = scene();
    setNameSource(state, mesh);
    expect(solidName(REUSED)).toBe("solid 3");
    expect(faceOf(REUSED, 6)).toBe("face 6 of solid 3");
    expect(solidName(99)).toBe("a solid");
    expect(hasRawId(faceOf(REUSED, 6))).toBe(false);
  });

  it("numbers regions by their place in the sketch", () => {
    const { state, mesh } = scene();
    setNameSource(state, mesh);
    expect(regionName(1, 3)).toBe("region 1");
    expect(regionName(1, 7)).toBe("region 2");
    expect(regionName(1, REUSED + 4)).toBe("region 3");
    expect(regionName(2, 3)).toBe("a region");
  });

  it("rewrites kernel refusals", () => {
    const { state, mesh } = scene();
    setNameSource(state, mesh);
    const msgs = [`no object #${REUSED}`, "no face #12 on this object", "region #4294967298 is already a solid", "no sketch #9", "no guide or dimension #3"];
    const out = msgs.map(friendlyError);
    expect(out).toEqual(["that solid no longer exists", "no face 12 on this object", "that region is already a solid", "that sketch no longer exists", "that guide or dimension no longer exists"]);
    expect(friendlyError(`object #${REUSED} moved`)).toBe("solid 3 moved");
    for (const t of out) expect(hasRawId(t)).toBe(false);
    expect(hasRawId(`solid ${REUSED}`)).toBe(true);
  });

  it("reads every kernel refusal that names an id as a sentence (lane 23b)", () => {
    const { state, mesh } = scene();
    setNameSource(state, mesh);
    // Every message the kernel and the wasm bridge format with an id, with ids filled in.
    const root = resolve(__dirname, "../..");
    const files = ["crates/kernel/src/error.rs", "crates/kernel/src/looks.rs", "crates/wasm-api/src/lib.rs", "crates/wasm-api/src/drawings.rs"];
    const templates = files.flatMap((f) => {
      const src = readFileSync(resolve(root, f), "utf8");
      return [...src.matchAll(/(?:#\[error\(|format!\()"([^"]*#\{[^"]*)"/g)].map((m) => m[1]);
    });
    expect(templates.length).toBeGreaterThan(20);
    for (const t of templates) {
      // A small id: face ids are per solid and small; a public id's reuse bits are tested above.
      const msg = t.replace(/\{[^}]*\}/g, "7");
      const out = friendlyError(msg);
      expect(hasRawId(out), `${t} -> ${out}`).toBe(false);
      expect(out, `${t} -> ${out}`).not.toMatch(/that one/);
    }
    expect(friendlyError("no curve #1 there")).toBe("that curve no longer exists");
    expect(friendlyError("no item #2 on sheet #1")).toBe("that item is no longer on that sheet");
    expect(friendlyError(`no face 3 on solid #${REUSED}`)).toBe("no face 3 on solid 3");
  });

  it("never lets a raw id through, whatever the message", () => {
    const { state, mesh } = scene();
    setNameSource(state, mesh);
    const msgs = [
      `Export refused: no object #${REUSED} in the model`,
      "no group or component #4294967299",
      "no component definition #12",
      "no imported mesh #3",
      `Could not open the file: object ${REUSED} is broken`,
    ];
    const out = msgs.map(friendlyError);
    expect(out.slice(1, 4)).toEqual(["that group or component no longer exists", "that component no longer exists", "that imported mesh no longer exists"]);
    for (const t of out) expect(hasRawId(t), t).toBe(false);
  });
});
