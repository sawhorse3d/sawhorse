import { describe, expect, it } from "vitest";
import { autosaveEnabled, clearOffered, describeAutosave, loadOffered, logIsWorth, MAX_AUTOSAVE_BYTES, saveAutosave, takeAutosave, worthRestoring, type Autosave } from "./autosave";

const meta = (over: Partial<Autosave> = {}): Autosave => ({ savedAt: 0, solids: 2, steps: 2, bytes: 100, worth: true, state: "ok", ...over });

describe("dev autosave", () => {
  it("offers back only real work", () => {
    expect(worthRestoring(null)).toBe(false);
    expect(worthRestoring(meta({ worth: false }))).toBe(false);
    expect(worthRestoring(meta())).toBe(true);
    // Lane 16: worked out from the log's texts, never by parsing the log.
    expect(logIsWorth(['{"op":"new_sketch","origin":[0,0,0],"normal":[0,0,1]}', '{"op":"undo"}'])).toBe(false);
    expect(logIsWorth(['{"op":"new_sketch","origin":[0,0,0],"normal":[0,0,1]}', '{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":1,"h":1}'])).toBe(true);
  });

  it("is on unless the URL turns it off", () => {
    expect(autosaveEnabled("")).toBe(true);
    expect(autosaveEnabled("?autosave=0")).toBe(false);
    expect(autosaveEnabled("?theme=dark&autosave=1")).toBe(true);
  });

  it("says what would come back, and why it cannot", () => {
    expect(describeAutosave(meta(), 0)).toBe("2 solids, 2 steps, saved just now");
    expect(describeAutosave(meta({ solids: 1 }), 5 * 60000)).toBe("1 solid, 2 steps, saved 5 minutes ago");
    expect(describeAutosave(meta({ state: "too_large", bytes: 100 * 1024 * 1024 }), 0)).toContain("100 MB of history: too large to restore here");
    expect(describeAutosave(meta({ state: "unreadable" }), 0)).toBe("the record kept in this browser cannot be read");
  });

  it("never keeps a log over the limit, and fails quietly without IndexedDB (jsdom)", async () => {
    expect(await saveAutosave("x".repeat(MAX_AUTOSAVE_BYTES + 1), { savedAt: 1, solids: 0, steps: 1, worth: true })).toBe(false);
    await saveAutosave("[]", { savedAt: 1, solids: 0, steps: 0, worth: false });
    expect(await takeAutosave()).toBeNull();
    expect(await loadOffered()).toBeNull();
    await clearOffered();
  });
});
