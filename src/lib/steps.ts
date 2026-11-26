// A document state as undo and redo find it again, and a tool's last step while the model is
// still at it.
import type { DocState } from "../kernel/types";

/** Lane 43 (#7): a document state as undo and redo find it again: its undo depth and model
 * hash (as the named plugin steps are known). Nothing for no document. */
export function stepKey(st: DocState | null | undefined): string {
  // `stats?`: the tools' tests give a host a partial state (sketches only).
  return st ? `${st.undo_depth}:${st.stats?.hash}` : "";
}

/** #84: the step key and the hash of the op log, which every op the kernel logs changes.
 * The step key alone misses a step once the undo history is full (its depth stops growing)
 * when the step leaves the model as it was (activating a scene, which only moves the camera,
 * a rename to the same name): the log hash still moves on. Nothing for no document. */
export function logKey(st: DocState | null | undefined): string {
  return st ? `${stepKey(st)}:${st.log_hash}` : "";
}

/**
 * #84: a tool that changes its last step by a value typed after it (Scale, Move, Rotate,
 * Offset) does so with an Undo, then the step again. That Undo takes whatever step is last,
 * so the tool keeps its step with the `logKey` read right after it (`key`), and redoes it
 * only while the model is still at that key: `last` then, else null (another step came after:
 * a Delete, an import, a plugin's batch, another tool's step, a pane's edit).
 */
export function stillLast<T extends { key: string }>(last: T | null, st: DocState | null): T | null {
  return last && last.key === logKey(st) ? last : null;
}
