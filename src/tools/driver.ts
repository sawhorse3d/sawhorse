// The event pump between the viewport and the active tool. Pointer moves are coalesced
// (only the latest is resolved while a query is in flight); presses, clicks and drags are
// queued in order and each is resolved at its own position, so a fast click lands where it
// was made. "Resolved" means: the eye ray, what is under the cursor (a pick) and where the
// cursor snaps (an inference), both answered by the kernel worker.
import type { KernelClient } from "../kernel/client";
import type { Picked, V3 } from "../kernel/types";
import type { ViewportPointer } from "../components/Viewport";
import type { ViewportEngine } from "../viewport/engine";
import { INFERENCE_STYLES } from "../lib/inferenceStyle";
import type { InferContext, Snap, Tool, ToolEvent, ToolEventType } from "./tool";
import { DRAG_PX } from "./select";

export interface Resolver {
  /** Resolve a canvas position to a pick and a snap for the active tool's context. */
  (x: number, y: number, ctx: InferContext, mods: { shift: boolean; ctrl: boolean; alt: boolean }): Promise<{
    ray: { origin: V3; dir: V3 };
    picked: Picked | null;
    snap: Snap | null;
  } | null>;
}

/** A snap from a plain pick (what the pointer is over, no inference engine). */
export function snapFromPick(picked: Picked | null, ray: { origin: V3; dir: V3 }): Snap | null {
  if (picked) {
    const e = picked.entity;
    const kind = e.kind === "vertex" ? "endpoint" : e.kind === "edge" ? "on-edge" : "on-face";
    return { point: picked.point, kind, tip: INFERENCE_STYLES[kind].label, surface: picked.surface, entity: e };
  }
  // The ground plane.
  const dz = ray.dir[2];
  if (Math.abs(dz) < 1e-12) return null;
  const t = -ray.origin[2] / dz;
  if (!(t > 0)) return null;
  return { point: [ray.origin[0] + ray.dir[0] * t, ray.origin[1] + ray.dir[1] * t, 0], kind: "free", tip: "", surface: null, entity: null };
}

/** The resolver backed by the kernel worker's pick (step 4) and inference (step 5). */
export function kernelResolver(kernel: () => KernelClient | null, engine: () => ViewportEngine | null): Resolver {
  return async (x, y) => {
    const eng = engine();
    const k = kernel();
    if (!eng) return null;
    const ray = eng.ray(x, y);
    const px = eng.pixelScale();
    let picked: Picked | null = null;
    if (k) {
      try {
        picked = await k.pick({ origin: ray.origin, dir: ray.dir, px: { ortho: px.ortho, scale: px.scale, eye: px.eye, forward: px.forward } });
      } catch {
        picked = null;
      }
    }
    return { ray, picked, snap: snapFromPick(picked, ray) };
  };
}

export class ToolDriver {
  private chain: Promise<void> = Promise.resolve();
  private pendingMove: ViewportPointer | null = null;
  private pumping = false;
  private queued = 0;
  private down: { x: number; y: number; dragged: boolean } | null = null;
  private lastMods = { shift: false, ctrl: false, alt: false };

  constructor(
    private resolve: Resolver,
    private tool: () => Tool,
  ) {}

  /** The left button is down and has moved far enough to be a drag. */
  dragging(): boolean {
    return !!this.down?.dragged;
  }

  pointer(p: ViewportPointer): void {
    this.lastMods = { shift: p.shift, ctrl: p.ctrl, alt: p.alt };
    switch (p.type) {
      case "move":
        if (this.down && !this.down.dragged && Math.hypot(p.x - this.down.x, p.y - this.down.y) > DRAG_PX) this.down.dragged = true;
        this.pendingMove = p;
        this.pump();
        return;
      case "down":
        if (p.button === 0) {
          this.down = { x: p.x, y: p.y, dragged: false };
          this.enqueue("down", p);
        }
        return;
      case "up": {
        if (p.button !== 0 || !this.down) return;
        const d = this.down;
        this.down = null;
        this.enqueue(d.dragged ? "dragend" : "click", p, [d.x, d.y]);
        return;
      }
      case "dblclick":
        this.enqueue("dblclick", p);
        return;
      case "leave":
        this.enqueue("leave", p);
        return;
    }
  }

  /** Re-run the last move (e.g. after a key changed the inference context). */
  refresh(): void {
    if (!this.pendingMove && this.last) {
      this.pendingMove = this.last;
      this.pump();
    }
  }

  private last: ViewportPointer | null = null;

  private pump(): void {
    if (this.pumping) return;
    this.pumping = true;
    this.chain = this.chain.then(async () => {
      while (this.pendingMove && this.queued === 0) {
        const p = this.pendingMove;
        this.pendingMove = null;
        this.last = p;
        const type: ToolEventType = this.down?.dragged ? "drag" : "move";
        await this.dispatch(type, p, this.down ? [this.down.x, this.down.y] : undefined);
      }
      this.pumping = false;
      if (this.pendingMove) this.pump();
    });
  }

  private enqueue(type: ToolEventType, p: ViewportPointer, start?: [number, number]): void {
    this.queued++;
    this.chain = this.chain.then(async () => {
      try {
        await this.dispatch(type, p, start);
      } finally {
        this.queued--;
        if (this.queued === 0 && this.pendingMove) this.pump();
      }
    });
  }

  private async dispatch(type: ToolEventType, p: ViewportPointer, start?: [number, number]): Promise<void> {
    const tool = this.tool();
    const r = await this.resolve(p.x, p.y, tool.inferContext(), this.lastMods);
    if (!r) return;
    const e: ToolEvent = { type, x: p.x, y: p.y, start, shift: p.shift, ctrl: p.ctrl, alt: p.alt, ray: r.ray, picked: r.picked, snap: r.snap };
    // The tool may have changed while the query was in flight.
    if (this.tool() === tool) tool.onEvent(e);
  }

  /** Events are queued or being resolved. */
  pending(): boolean {
    return this.pumping || this.queued > 0;
  }

  /** Wait for every queued event. */
  idle(): Promise<void> {
    return this.chain;
  }
}
