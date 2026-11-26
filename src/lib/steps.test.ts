// #84: a tool changes its last step by typing (Scale, Move, Rotate, Offset) only while the
// model is still at that step: its step key, read right after it, is the model's key now.
import { describe, expect, it } from "vitest";
import type { DocState } from "../kernel/types";
import { stepKey, stillLast } from "./steps";

const at = (depth: number, hash: string) => ({ undo_depth: depth, stats: { hash } }) as unknown as DocState;

describe("a document state's step key", () => {
  it("is its undo depth and model hash, and nothing for no document", () => {
    expect(stepKey(at(3, "abc"))).toBe("3:abc");
    expect(stepKey(null)).toBe("");
    expect(stepKey(undefined)).toBe("");
  });

  it("tells apart the same depth with another model, and the same model at another depth", () => {
    expect(stepKey(at(3, "abc"))).not.toBe(stepKey(at(3, "abd")));
    expect(stepKey(at(3, "abc"))).not.toBe(stepKey(at(4, "abc")));
  });
});

describe("the tool's last step, while the model is still at it", () => {
  it("is kept at its own key, and forgotten at any other", () => {
    const last = { key: stepKey(at(3, "abc")), what: "the move" };
    expect(stillLast(last, at(3, "abc"))).toBe(last);
    expect(stillLast(last, at(4, "xyz"))).toBeNull();
    expect(stillLast(last, at(3, "xyz"))).toBeNull();
    expect(stillLast(last, at(4, "abc"))).toBeNull();
    expect(stillLast(last, null)).toBeNull();
    expect(stillLast(null, at(3, "abc"))).toBeNull();
  });

  it("every run of up to four later steps (another edit, Undo of it, Redo of it) keeps it only where the model is back at its key", () => {
    // The tool's step takes the model to depth 1, hash "own". Another edit pushes a step;
    // Undo takes the top step off; Redo puts the last undone one back. (The tools also forget
    // the step on Undo and Redo through modelChanged; this holds the key alone.)
    type Ev = "edit" | "undo" | "redo";
    const evs: Ev[] = ["edit", "undo", "redo"];
    let runs = 0;
    let kept = 0;
    const walk = (seq: Ev[]) => {
      // The undo stack as hashes, from the tool's step; and what Redo would put back.
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
      const now = at(stack.length, stack.at(-1) ?? "empty");
      const last = { key: stepKey(at(1, "own")) };
      const ok = stillLast(last, now) !== null;
      expect(ok, seq.join(", ")).toBe(stack.length === 1 && stack[0] === "own");
      runs++;
      if (ok) kept++;
      if (seq.length < 4) for (const e of evs) walk([...seq, e]);
    };
    walk([]);
    // 1 + 3 + 9 + 27 + 81 runs.
    expect(runs).toBe(121);
    expect(kept).toBeGreaterThan(1);
  });
});
