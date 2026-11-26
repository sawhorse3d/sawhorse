// #84: a tool changes its last step by typing (Scale, Move, Rotate, Offset) only while the
// model is still at that step: its log key, read right after it, is the model's key now.
import { describe, expect, it } from "vitest";
import type { DocState } from "../kernel/types";
import { logKey, stepKey, stillLast } from "./steps";

const at = (depth: number, hash: string, log = "log") => ({ undo_depth: depth, stats: { hash }, log_hash: log }) as unknown as DocState;

describe("a document state's step key and log key", () => {
  it("the step key is its undo depth and model hash, and nothing for no document", () => {
    expect(stepKey(at(3, "abc"))).toBe("3:abc");
    expect(stepKey(null)).toBe("");
    expect(stepKey(undefined)).toBe("");
  });

  it("the log key adds the op log's hash, and is nothing for no document", () => {
    expect(logKey(at(3, "abc", "f00"))).toBe("3:abc:f00");
    expect(logKey(null)).toBe("");
  });

  it("each part tells two states apart", () => {
    expect(logKey(at(3, "abc", "1"))).not.toBe(logKey(at(3, "abd", "1")));
    expect(logKey(at(3, "abc", "1"))).not.toBe(logKey(at(4, "abc", "1")));
    expect(logKey(at(3, "abc", "1"))).not.toBe(logKey(at(3, "abc", "2")));
  });
});

describe("the tool's last step, while the model is still at it", () => {
  it("is kept at its own key, and forgotten at any other", () => {
    const last = { key: logKey(at(3, "abc", "1")), what: "the move" };
    expect(stillLast(last, at(3, "abc", "1"))).toBe(last);
    expect(stillLast(last, at(4, "xyz", "2"))).toBeNull();
    expect(stillLast(last, at(3, "xyz", "2"))).toBeNull();
    expect(stillLast(last, at(4, "abc", "2"))).toBeNull();
    expect(stillLast(last, null)).toBeNull();
    expect(stillLast(null, at(3, "abc", "1"))).toBeNull();
  });

  it("with the undo history full, a step that leaves the model as it was still ends it", () => {
    // At the cap the undo depth stays put, and a scene that only moves the camera, or a rename
    // to the same name, leaves the model hash: only the op log's hash moves on.
    const last = { key: logKey(at(200, "abc", "log after the scale")) };
    expect(stillLast(last, at(200, "abc", "log after the scene"))).toBeNull();
    // The step key alone would have kept it.
    expect(stepKey(at(200, "abc", "log after the scene"))).toBe(stepKey(at(200, "abc", "log after the scale")));
  });

  it("every run of up to four later ops (another edit, Undo, Redo) ends it, and only the empty run keeps it", () => {
    // The tool's step leaves the model at depth 1, hash "own", log "own". Every op the kernel
    // logs moves the log on, so even an edit undone, or an Undo redone, leaves another log.
    type Ev = "edit" | "undo" | "redo";
    const evs: Ev[] = ["edit", "undo", "redo"];
    const last = { key: logKey(at(1, "own", "own")) };
    let runs = 0;
    let kept = 0;
    const walk = (seq: Ev[]) => {
      const stack = ["own"];
      const undone: string[] = [];
      let n = 0;
      for (const e of seq) {
        if (e === "edit") {
          stack.push(`edit ${++n}`);
          undone.length = 0;
        } else if (e === "undo" && stack.length) undone.push(stack.pop()!);
        else if (e === "redo" && undone.length) stack.push(undone.pop()!);
      }
      const log = seq.length ? `own, ${seq.join(", ")}` : "own";
      const ok = stillLast(last, at(stack.length, stack.at(-1) ?? "empty", log)) !== null;
      expect(ok, seq.join(", ") || "(none)").toBe(seq.length === 0);
      runs++;
      if (ok) kept++;
      if (seq.length < 4) for (const e of evs) walk([...seq, e]);
    };
    walk([]);
    // Every case: 1 + 3 + 9 + 27 + 81 runs.
    expect(runs).toBe(121);
    expect(kept).toBe(1);
  });
});
