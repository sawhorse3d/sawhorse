// Promise wrapper around the kernel worker.
import type { MeshData, RequestBody, SpikeRecipe, WorkerRequest, WorkerResponse } from "./types";

export class KernelUnavailableError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "KernelUnavailableError";
  }
}

type Pending = { resolve: (r: WorkerResponse) => void; reject: (e: Error) => void };

export class KernelClient {
  private worker: Worker;
  private nextId = 1;
  private pending = new Map<number, Pending>();

  constructor() {
    this.worker = new Worker(new URL("./kernel.worker.ts", import.meta.url), { type: "module", name: "sawhorse-kernel" });
    this.worker.onmessage = (e: MessageEvent<WorkerResponse>) => {
      const p = this.pending.get(e.data.id);
      if (!p) return;
      this.pending.delete(e.data.id);
      p.resolve(e.data);
    };
    this.worker.onerror = (e) => {
      const err = new Error(`kernel worker crashed: ${e.message}`);
      for (const p of this.pending.values()) p.reject(err);
      this.pending.clear();
    };
  }

  private call(req: RequestBody): Promise<WorkerResponse> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      const msg: WorkerRequest = { ...req, id };
      this.worker.postMessage(msg);
    });
  }

  async version(): Promise<string> {
    const r = await this.call({ kind: "version" });
    if (!r.ok) throw r.missing ? new KernelUnavailableError(r.error) : new Error(r.error);
    return r.version ?? "unknown";
  }

  /** Build the spike scene: rectangle on the ground plane, extruded by recipe.distance. */
  async build(recipe: SpikeRecipe): Promise<MeshData> {
    const r = await this.call({ kind: "build", recipe });
    if (!r.ok) throw r.missing ? new KernelUnavailableError(r.error) : new Error(r.error);
    if (!r.mesh) throw new Error("kernel returned no mesh");
    return r.mesh;
  }

  dispose(): void {
    this.worker.terminate();
    this.pending.clear();
  }
}

export function createKernel(): KernelClient | null {
  if (typeof Worker === "undefined") return null;
  return new KernelClient();
}
