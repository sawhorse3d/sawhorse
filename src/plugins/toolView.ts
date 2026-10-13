// What a plugin's tool asks to show (ui.toolView), checked: only the app's own colours, finite
// points, bounded sizes (lane 11). Lane 16: also a budget for the whole view (50,000 points),
// and the manager checks a view once, when it arrives for the active tool, instead of every
// tool instance checking every view.
import type { V3 } from "../kernel/types";
import { INFERENCE_STYLES, type InferenceKind } from "../lib/inferenceStyle";
import type { InferContext } from "../tools/tool";
import type { OverlayItem } from "../viewport/engine";

/** Most points a tool view draws in all (its polylines together). */
export const TOOL_VIEW_POINTS = 50_000;

const COLORS: Record<string, string> = { accent: "--accent", preview: "--sel", selection: "--sel", warning: "--warn", ok: "--ok", red: "--axis-x", green: "--axis-y", blue: "--axis-z", muted: "--muted" };

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isV3 = (v: unknown): v is V3 => Array.isArray(v) && v.length === 3 && v.every((x) => typeof x === "number" && Number.isFinite(x));

export interface PluginToolView {
  overlay: OverlayItem[];
  labels: { at: V3; text: string }[];
  status: string | null | undefined;
  value: string | undefined;
  busy: boolean;
  inference: InferContext;
}

/** A plugin's ToolView, checked: known colours, finite points, bounded sizes. */
export function checkToolView(raw: unknown): PluginToolView {
  const v: PluginToolView = { overlay: [], labels: [], status: undefined, value: undefined, busy: false, inference: {} };
  if (!isObj(raw)) return v;
  if (Array.isArray(raw.overlay))
    for (let i = 0, budget = TOOL_VIEW_POINTS; i < Math.min(raw.overlay.length, 200) && budget > 0; i++) {
      const o: unknown = raw.overlay[i];
      if (!isObj(o) || !Array.isArray(o.points)) continue;
      const kind = o.kind === "strip" || o.kind === "loop" ? o.kind : "segments";
      const points = o.points.slice(0, Math.min(5000, budget)).filter(isV3);
      budget -= points.length;
      if (points.length < 2) continue;
      const c = typeof o.color === "string" ? o.color : "accent";
      // Lane 17: own keys only ("constructor" is not a colour).
      const color = Object.hasOwn(COLORS, c) ? COLORS[c] : Object.hasOwn(INFERENCE_STYLES, c) ? INFERENCE_STYLES[c as InferenceKind].token : "--accent";
      const width = typeof o.width === "number" && Number.isFinite(o.width) ? Math.min(Math.max(o.width, 0.5), 6) : 1.75;
      v.overlay.push({ kind, points, color, width, dashed: !!o.dashed });
    }
  if (Array.isArray(raw.labels))
    for (const l of raw.labels.slice(0, 100)) if (isObj(l) && isV3(l.at) && typeof l.text === "string") v.labels.push({ at: l.at, text: l.text.slice(0, 80) });
  if (raw.status === null || typeof raw.status === "string") v.status = raw.status === null ? null : raw.status.slice(0, 300);
  if (typeof raw.value === "string") v.value = raw.value.slice(0, 80);
  v.busy = !!raw.busy;
  const inf = raw.inference;
  if (isObj(inf)) {
    if (isV3(inf.start)) v.inference.start = inf.start;
    if (isObj(inf.plane) && isV3(inf.plane.origin) && isV3(inf.plane.normal)) v.inference.plane = { origin: inf.plane.origin, normal: inf.plane.normal };
    if (isObj(inf.lock) && isV3(inf.lock.dir) && typeof inf.lock.kind === "string" && Object.hasOwn(INFERENCE_STYLES, inf.lock.kind)) v.inference.lock = { dir: inf.lock.dir, kind: inf.lock.kind as InferenceKind };
    if (typeof inf.grid === "boolean") v.inference.grid = inf.grid;
  }
  return v;
}

