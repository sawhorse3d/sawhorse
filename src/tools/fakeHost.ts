// A ToolHost for unit tests: records ops, status lines and values; no kernel, no DOM.
import type { EditResult } from "../kernel/client";
import type { DocState, Entity, KernelOp, MeshData, OpOutcome, SceneData, V3 } from "../kernel/types";
import type { Target } from "../lib/edit";
import { formatLength, unitInfo, type UnitSystem } from "../lib/units";
import type { Snap, ToolEvent, ToolHost } from "./tool";

export interface FakeHost extends ToolHost {
  ops: KernelOp[];
  statuses: { text: string | null; error: boolean }[];
  values: string[];
  sel: Entity[];
  hovered: Entity | null;
  /** Outcome the next apply returns (null: refused). */
  outcome: OpOutcome | null;
  _target: Target | null;
  _units: UnitSystem;
  _mesh: MeshData | null;
  _state: DocState | null;
}

export function fakeHost(over: Partial<FakeHost> = {}): FakeHost {
  const h: FakeHost = {
    ops: [],
    statuses: [],
    values: [],
    sel: [],
    hovered: null,
    outcome: { kind: "face_moved" },
    _target: null,
    _units: "frac-in",
    _mesh: null,
    _state: null,
    units: () => h._units,
    fmt: (m) => formatLength(m, h._units),
    unitLabel: () => unitInfo(h._units).short,
    scene: () => (h._mesh && h._state ? ({ mesh: h._mesh, state: h._state } as SceneData) : null),
    mesh: () => h._mesh,
    state: () => h._state,
    apply: async (op, describe) => {
      h.ops.push(op);
      if (!h.outcome) {
        h.statuses.push({ text: "refused", error: true });
        return null;
      }
      const r = { outcome: h.outcome, scene: { mesh: h._mesh, state: h._state } } as unknown as EditResult;
      if (describe) h.statuses.push({ text: describe(r), error: false });
      return r;
    },
    status: (text, error = false) => h.statuses.push({ text, error }),
    showValue: (t) => h.values.push(t),
    selection: () => h.sel,
    setSelection: (es) => {
      h.sel = es;
    },
    hover: (e) => {
      h.hovered = e;
    },
    changed: () => {},
    context: () => null,
    enterGroup: () => {},
    toScreen: (p: V3) => [p[0] * 100, -p[1] * 100],
    target: () => h._target,
    setTarget: (t) => {
      h._target = t;
    },
    ...over,
  };
  return h;
}

/** A pointer event at a world point, snapped as `kind`. */
export function ev(type: ToolEvent["type"], point: V3, opts: Partial<ToolEvent> & { snapKind?: Snap["kind"] } = {}): ToolEvent {
  const { snapKind, ...rest } = opts;
  const kind = snapKind ?? "on-face";
  return {
    type,
    x: point[0] * 100,
    y: -point[1] * 100,
    shift: false,
    ctrl: false,
    alt: false,
    ray: { origin: [point[0], point[1], 10], dir: [0, 0, -1] },
    picked: null,
    snap: { point, kind, tip: kind, surface: null, entity: null },
    ...rest,
  };
}
