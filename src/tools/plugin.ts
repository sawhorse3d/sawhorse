// A plugin's tool in the tool system (lane 11). It is a Tool like the built-in ones: the
// ToolDriver feeds it pointer events that already carry the pick and the inference, the
// Value Control Box feeds it typed values, Esc cancels. It forwards them to the plugin's
// worker and draws what the plugin last asked for (overlay lines, labels, the status line,
// the VCB value), checked here so a plugin can only draw with the app's own colours.
//
// Pointer moves are coalesced: while the plugin is still handling one move, only the latest
// next one is kept. Presses, clicks, drags and values go in order.
import type { Data } from "../../packages/plugin-api/src/rpc.ts";
import type { PluginToolId } from "../lib/tools";
import type { PluginManager } from "../plugins/manager";
import { snapOfEvent } from "../plugins/model";
import { ownMarkerSnap } from "../plugins/toolEvent";
import { checkToolView, type PluginToolView } from "../plugins/toolView";
import { EMPTY_VIEW, Tool, markerOf, type InferContext, type Snap, type ToolEvent, type ToolHost, type ToolView } from "./tool";

export { checkToolView, type PluginToolView } from "../plugins/toolView";

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
      if (key !== this.id || !this.active) return;
      // Checked once by the manager when it arrived (lane 16).
      this.view_ = this.manager.toolView(this.id);
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
    this.view_ = this.manager.toolView(this.id);
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
    // Lane 17: the manager takes the model out for a plugin without model.read (toolEvent.ts);
    // the pointer is all it builds here then.
    if (!this.manager.toolSeesModel(this.id)) return { type: e.type, x: e.x, y: e.y, shift: e.shift, ctrl: e.ctrl, alt: e.alt, ray: { origin: e.ray.origin, dir: e.ray.dir }, snap: null, picked: null } as unknown as Data;
    const pick = snapOfEvent(e.snap, e.picked, this.host.state());
    return { type: e.type, x: e.x, y: e.y, shift: e.shift, ctrl: e.ctrl, alt: e.alt, ray: { origin: e.ray.origin, dir: e.ray.dir }, snap: pick.snap, picked: pick.picked } as unknown as Data;
  }

  onEvent(e: ToolEvent): void {
    // The marker shows what the plugin gets: the inference engine's snap, or for a plugin
    // without model.read the point of its own constraints (lane 17).
    this.snap = this.manager.toolSeesModel(this.id) ? e.snap : ownMarkerSnap(e.ray, this.view_.inference);
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
