// Axis locks for tools that draw from a start point (Line, Rect, Tape, Push drags...):
//   Right arrow  lock to the red axis (X)        Left arrow  green (Y)
//   Up arrow     blue (Z)                         Down arrow  the current parallel /
//                                                             perpendicular direction
//   the same arrow again unlocks; holding Shift locks whatever line inference is showing
//   (axis, parallel, perpendicular) until Shift is released.
import type { V3 } from "../kernel/types";
import type { InferenceKind } from "../lib/inferenceStyle";
import type { Snap } from "./tool";

export interface Lock {
  dir: V3;
  kind: InferenceKind;
}

const AXIS_KEYS: Record<string, Lock> = {
  ArrowRight: { dir: [1, 0, 0], kind: "axis-x" },
  ArrowLeft: { dir: [0, 1, 0], kind: "axis-y" },
  ArrowUp: { dir: [0, 0, 1], kind: "axis-z" },
};

const LINE_KINDS = new Set<InferenceKind>(["axis-x", "axis-y", "axis-z", "parallel", "perpendicular"]);

export class LockState {
  lock: Lock | null = null;
  /** The lock came from holding Shift (released with it). */
  private fromShift = false;

  clear(): void {
    this.lock = null;
    this.fromShift = false;
  }

  /** The direction of a line inference from `start` in `snap`, if it shows one. */
  static lineOf(snap: Snap | null, start: V3 | null): Lock | null {
    if (!snap || !start) return null;
    const l = snap.lines.find((x) => LINE_KINDS.has(x.kind));
    if (!l) return null;
    const d: V3 = [l.to[0] - l.from[0], l.to[1] - l.from[1], l.to[2] - l.from[2]];
    const n = Math.hypot(...d);
    if (n < 1e-12) return null;
    return { dir: [d[0] / n, d[1] / n, d[2] / n], kind: l.kind };
  }

  /** Handle an arrow key; returns true when it changed the lock. */
  onKey(key: string, snap: Snap | null, start: V3 | null): boolean {
    if (!start) return false;
    const axis = AXIS_KEYS[key];
    if (axis) {
      this.lock = this.lock && this.lock.kind === axis.kind ? null : axis;
      this.fromShift = false;
      return true;
    }
    if (key === "ArrowDown") {
      const cur = LockState.lineOf(snap, start);
      if (this.lock && (!cur || cur.kind === this.lock.kind)) this.lock = null;
      else if (cur) this.lock = cur;
      this.fromShift = false;
      return true;
    }
    return false;
  }

  onShift(down: boolean, snap: Snap | null, start: V3 | null): boolean {
    if (down) {
      if (this.lock) return false;
      const cur = LockState.lineOf(snap, start);
      if (!cur) return false;
      this.lock = cur;
      this.fromShift = true;
      return true;
    }
    if (this.fromShift) {
      this.clear();
      return true;
    }
    return false;
  }
}
