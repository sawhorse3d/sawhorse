import { describe, expect, it } from "vitest";
import type { KernelOp } from "../kernel/types";
import { autosaveEnabled, describeAutosave, loadAutosave, saveAutosave, worthRestoring } from "./autosave";

const ground: KernelOp = { op: "new_sketch", origin: [0, 0, 0], normal: [0, 0, 1] };
const rect: KernelOp = { op: "add_rectangle", sketch: 1, x: 0, y: 0, w: 1, h: 1 };

describe("dev autosave", () => {
  it("offers back only real work", () => {
    expect(worthRestoring(null)).toBe(false);
    expect(worthRestoring({ log: [ground], savedAt: 0, solids: 0 })).toBe(false);
    expect(worthRestoring({ log: [ground, { op: "undo" }], savedAt: 0, solids: 0 })).toBe(false);
    expect(worthRestoring({ log: [ground, rect], savedAt: 0, solids: 0 })).toBe(true);
  });

  it("is on unless the URL turns it off", () => {
    expect(autosaveEnabled("")).toBe(true);
    expect(autosaveEnabled("?autosave=0")).toBe(false);
    expect(autosaveEnabled("?theme=dark&autosave=1")).toBe(true);
  });

  it("says what would come back", () => {
    const a = { log: [ground, rect], savedAt: 0, solids: 2 };
    expect(describeAutosave(a, 0)).toBe("2 solids, 2 steps, saved just now");
    expect(describeAutosave({ ...a, solids: 1 }, 5 * 60000)).toBe("1 solid, 2 steps, saved 5 minutes ago");
  });

  it("fails quietly without IndexedDB (jsdom)", async () => {
    await saveAutosave({ log: [ground, rect], savedAt: 1, solids: 0 });
    expect(await loadAutosave()).toBeNull();
  });
});
