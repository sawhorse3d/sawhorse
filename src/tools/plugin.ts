// A plugin's tool in the tool system (lane 11). It is a Tool like the built-in ones: the
// ToolDriver feeds it pointer events that already carry the pick and the inference, the
// Value Control Box feeds it typed values, Esc cancels. It forwards them to the plugin's
// worker and draws what the plugin last asked for (overlay lines, labels, the status line,
// the VCB value), checked here so a plugin can only draw with the app's own colours.
//
// Pointer moves are coalesced: while the plugin is still handling one move, only the latest
// next one is kept. Presses, clicks, drags and values go in order.
import type { Data } from "../../packages/plugin-api/src/rpc.ts";
import type { V3 } from "../kernel/types";
import { INFERENCE_STYLES, type InferenceKind } from "../lib/inferenceStyle";
import type { PluginToolId } from "../lib/tools";
import type { PluginManager } from "../plugins/manager";
import { snapOfEvent } from "../plugins/model";
import type { OverlayItem } from "../viewport/engine";
import { EMPTY_VIEW, Tool, markerOf, type InferContext, type Snap, type ToolEvent, type ToolHost, type ToolView } from "./tool";

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
    for (const o of raw.overlay.slice(0, 200)) {
      if (!isObj(o) || !Array.isArray(o.points)) continue;
      const kind = o.kind === "strip" || o.kind === "loop" ? o.kind : "segments";
      const points = o.points.slice(0, 5000).filter(isV3);
      if (points.length < 2) continue;
      const c = typeof o.color === "string" ? o.color : "accent";
      const color = COLORS[c] ?? (c in INFERENCE_STYLES ? INFERENCE_STYLES[c as InferenceKind].token : "--accent");
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
    if (isObj(inf.lock) && isV3(inf.lock.dir) && typeof inf.lock.kind === "string" && inf.lock.kind in INFERENCE_STYLES) v.inference.lock = { dir: inf.lock.dir, kind: inf.lock.kind as InferenceKind };
    if (typeof inf.grid === "boolean") v.inference.grid = inf.grid;
  }
  return v;
}

export class PluginTool extends Tool {
  readonly id: PluginToolId;
  private manager: PluginManager;
  private snap: Snap | null = null;
  private view_: PluginToolView = checkToolView(null);
  private moving = false;
  private nextMove: ToolEvent | null = null;
  private chain: Promise<unknown> = Promise.resolve();
  private active = false;
  private off: () => void;

  constructor(host: ToolHost, manager: PluginManager, id: PluginToolId) {
    super(host);
    this.id = id;
    this.manager = manager;
    this.off = manager.onToolView((key) => {
      if (key !== this.id) return;
      this.view_ = checkToolView(this.manager.tools.get(this.id)?.view);
      if (!this.active) return;
      if (this.view_.status !== undefined) this.host.status(this.view_.status);
      if (this.view_.value !== undefined) this.host.showValue(this.view_.value);
      this.host.changed();
    });
  }

  dispose(): void {
    this.off();
  }

  private def() {
    return this.manager.tools.get(this.id)?.def;
  }

  /** In order with the other calls; failures go to the plugin's console, not the app. */
  private send(fn: "tool.activate" | "tool.deactivate" | "tool.event" | "tool.value" | "tool.cancel", arg?: Data): Promise<unknown> {
    this.chain = this.chain.then(() => this.manager.toolCall(this.id, fn, arg).catch(() => null));
    return this.chain;
  }

  activate(): void {
    this.active = true;
    void this.send("tool.activate");
  }

  deactivate(): void {
    this.active = false;
    this.snap = null;
    this.nextMove = null;
    void this.send("tool.deactivate");
  }

  reset(): void {
    this.view_ = { ...this.view_, overlay: [], labels: [], busy: false };
  }

  busy(): boolean {
    return this.view_.busy;
  }

  cancel(): boolean {
    if (!this.busy()) return false;
    this.view_ = { ...this.view_, busy: false };
    void this.send("tool.cancel");
    this.host.changed();
    return true;
  }

  valueLabel(): string | null {
    return this.def()?.vcb ?? null;
  }

  variantHint(): string | null {
    const d = this.def();
    return d?.hint ? `${d.title}: ${d.hint}` : null;
  }

  inferContext(): InferContext {
    return this.view_.inference;
  }

  private payload(e: ToolEvent): Data {
    const pick = snapOfEvent(e.snap, e.picked, this.host.state());
    return { type: e.type, x: e.x, y: e.y, shift: e.shift, ctrl: e.ctrl, alt: e.alt, ray: { origin: e.ray.origin, dir: e.ray.dir }, snap: pick.snap, picked: pick.picked } as unknown as Data;
  }

  onEvent(e: ToolEvent): void {
    this.snap = e.snap;
    this.host.changed();
    if (e.type === "move" || e.type === "drag") {
      if (this.moving) {
        this.nextMove = e;
        return;
      }
      this.moving = true;
      const go = (ev: ToolEvent): Promise<unknown> =>
        this.send("tool.event", this.payload(ev)).then((): Promise<unknown> | null => {
          const n = this.nextMove;
          this.nextMove = null;
          if (n && this.active) return go(n);
          this.moving = false;
          return null;
        });
      void go(e);
      return;
    }
    this.nextMove = null;
    void this.send("tool.event", this.payload(e));
  }

  onValue(text: string): void {
    void this.send("tool.value", text);
  }

  view(): ToolView {
    if (!this.active) return EMPTY_VIEW;
    return { overlay: this.view_.overlay, marker: markerOf(this.snap), labels: this.view_.labels, box: null };
  }
}
