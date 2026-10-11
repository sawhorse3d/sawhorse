// Promise wrapper around the kernel worker, which holds the persistent document.
import type { Inference, InferQuery, KernelOp, OpOutcome, Picked, PickQuery, RequestBody, SceneData, V3, WorkerRequest, WorkerResponse } from "./types";

export class KernelUnavailableError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "KernelUnavailableError";
  }
}

/** A typed kernel refusal: the document is unchanged. */
export class KernelRefusal extends Error {
  constructor(message: string) {
    super(message);
    this.name = "KernelRefusal";
  }
}

type Pending = { resolve: (r: WorkerResponse) => void; reject: (e: Error) => void };

export interface EditResult {
  outcome: OpOutcome;
  scene: SceneData;
}

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

  private async call(req: RequestBody): Promise<WorkerResponse & { ok: true }> {
    const id = this.nextId++;
    const r = await new Promise<WorkerResponse>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      const msg: WorkerRequest = { ...req, id };
      this.worker.postMessage(msg);
    });
    if (!r.ok) throw r.missing ? new KernelUnavailableError(r.error) : new KernelRefusal(r.error);
    return r;
  }

  async version(): Promise<string> {
    return (await this.call({ kind: "version" })).version ?? "unknown";
  }

  /** Start a fresh document (ground sketch only). */
  async init(): Promise<SceneData> {
    const r = await this.call({ kind: "init" });
    if (!r.scene) throw new Error("kernel returned no scene");
    return r.scene;
  }

  /** Apply one op to the persistent document. Throws KernelRefusal when refused. */
  async apply(op: KernelOp): Promise<EditResult> {
    return this.edit(await this.call({ kind: "apply", op }));
  }

  async undo(): Promise<EditResult> {
    return this.edit(await this.call({ kind: "undo" }));
  }

  async redo(): Promise<EditResult> {
    return this.edit(await this.call({ kind: "redo" }));
  }

  /** What is under the cursor (read-only; never changes the document). */
  async pick(query: PickQuery): Promise<Picked | null> {
    return (await this.call({ kind: "pick", query })).picked ?? null;
  }

  /** World rings (outer first) of a face or region: what Push/Pull previews. */
  async outline(part: "face" | "region", owner: number, index: number): Promise<V3[][]> {
    return (await this.call({ kind: "outline", part, owner, index })).rings ?? [];
  }

  /** Where the cursor snaps for a drawing tool (read-only). */
  async infer(query: InferQuery): Promise<Inference | null> {
    return (await this.call({ kind: "infer", query })).inference ?? null;
  }

  async log(): Promise<KernelOp[]> {
    return (await this.call({ kind: "log" })).log ?? [];
  }

  async export(format: "stl" | "obj", object: number): Promise<Uint8Array | string> {
    const r = await this.call({ kind: "export", format, object });
    if (r.file === undefined) throw new Error("kernel returned no file");
    return r.file;
  }

  private edit(r: WorkerResponse & { ok: true }): EditResult {
    if (!r.outcome || !r.scene) throw new Error("kernel returned no result");
    return { outcome: r.outcome, scene: r.scene };
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
