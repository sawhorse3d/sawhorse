// Promise wrapper around the kernel worker, which holds the persistent document.
//
// Lane 16, containment: the client keeps the document's op log (each answer brings the
// entries it does not have, `logSync`), so when the worker fails it can start a fresh one
// and rebuild the document from that log, undo history included. It does so when the kernel
// itself fails (a trap: a Rust panic or out of memory, answered as `fatal`), when the worker
// dies (a JavaScript error escaping it), when a request runs past its deadline (a hung
// import), and when the person cancels a long request. The request that failed is refused
// (KernelRestarted: it changed nothing), requests waiting behind it run on the new worker,
// and `onRestart` tells the app (which says so).
import type { FaceInfo, CutList, CutOptions, Nesting, NestOptions, ExportFormat, ExportOptions, ImportOptions, Inference, InferQuery, KernelOp, LibraryEntry, OffsetPreview, OffsetSource, OpOutcome, Picked, PickQuery, RequestBody, SceneData, SceneGraph, UvXform, V3, WorkerRequest, WorkerResponse } from "./types";

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

/** The kernel was restarted while a request was running: the request changed nothing. */
export class KernelRestarted extends Error {
  constructor(message: string) {
    super(message);
    this.name = "KernelRestarted";
  }
}

type Pending = { resolve: (r: WorkerResponse) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> | null; kind: string };

/** How long a request may run before the worker is taken for hung and replaced (ms). */
export const DEADLINES: Record<string, number> = { importFile: 180_000, importSkp: 180_000, openShz: 180_000, replay: 300_000, default: 120_000 };

/** Why the kernel was restarted, for the app. */
export interface RestartInfo {
  /** What happened, in plain words. */
  reason: string;
  /** The rebuilt document's scene, or null when it could not be rebuilt (a fresh one is shown). */
  scene: SceneData | null;
  /** The rebuild's own error, when it failed. */
  error?: string;
}

export interface EditResult {
  outcome: OpOutcome;
  scene: SceneData;
}

export class KernelClient {
  private worker: Worker;
  private nextId = 1;
  private pending = new Map<number, Pending>();
  /** The document's op log, one JSON text per op, kept in step with the worker's. */
  private mirror: string[] = [];
  /** Set while a fresh worker is being started and the document rebuilt in it. */
  private restarting: Promise<void> | null = null;
  /** Restarts so far (tests read it). */
  restarts = 0;
  /** Told after every restart (the app shows the rebuilt scene and says what happened). */
  onRestart: ((info: RestartInfo) => void) | null = null;
  /** Restart times in the last half minute, and why the kernel was given up on. */
  private recent: number[] = [];
  private dead: string | null = null;
  /** Tests: the next import fails inside the kernel the way a panic would. */
  private failNext: string | null = null;
  private makeWorker: () => Worker;

  constructor(makeWorker: () => Worker = () => new Worker(new URL("./kernel.worker.ts", import.meta.url), { type: "module", name: "sawhorse-kernel" })) {
    this.makeWorker = makeWorker;
    this.worker = this.spawn();
  }

  private spawn(): Worker {
    const w = this.makeWorker();
    w.onmessage = (e: MessageEvent<WorkerResponse>) => {
      if (w !== this.worker) return;
      const p = this.pending.get(e.data.id);
      if (!p) return;
      this.pending.delete(e.data.id);
      if (p.timer !== null) clearTimeout(p.timer);
      p.resolve(e.data);
    };
    // A JavaScript error escaping the worker (or the worker failing to load): it is replaced.
    w.onerror = (e) => {
      e.preventDefault?.();
      if (w === this.worker) void this.restart(`the kernel worker failed (${e.message || "no message"})`);
    };
    return w;
  }

  /** The op log the client holds (JSON texts), for autosave and restarts. */
  logTexts(): readonly string[] {
    return this.mirror;
  }

  /** The op log as one JSON array text. */
  logJson(): string {
    return `[${this.mirror.join(",")}]`;
  }

  /** Tests: make the next import fail inside the kernel ("panic"). */
  failNextImport(how = "panic"): void {
    this.failNext = how;
  }

  /** Stop what the kernel is doing (a long import or open): the worker is replaced and the
   * document rebuilt from its log; the running request is refused. */
  cancel(reason = "cancelled"): Promise<void> {
    return this.restart(reason);
  }

  /**
   * Replace the worker: refuse what was running, start a fresh worker, rebuild the document
   * from the log (or start a new one when there is no log), and tell the app.
   */
  private restart(reason: string): Promise<void> {
    if (this.restarting) return this.restarting;
    if (this.dead) return Promise.resolve();
    this.restarts++;
    // A worker that keeps failing (its script will not load, every rebuild traps) is not
    // restarted forever: after three restarts in half a minute the kernel is unavailable.
    const now = Date.now();
    this.recent = this.recent.filter((t) => now - t < 30_000);
    this.recent.push(now);
    if (this.recent.length > 3) {
      this.dead = `The kernel failed ${this.recent.length} times in half a minute (${reason}); reload the page.`;
      const gone = new KernelUnavailableError(this.dead);
      for (const p of this.pending.values()) {
        if (p.timer !== null) clearTimeout(p.timer);
        p.reject(gone);
      }
      this.pending.clear();
      this.worker.terminate();
      this.onRestart?.({ reason, scene: null, error: this.dead });
      return Promise.resolve();
    }
    const old = this.worker;
    old.terminate();
    const err = new KernelRestarted(`The kernel stopped (${reason}) and was restarted from your last step; this did not change the model.`);
    for (const p of this.pending.values()) {
      if (p.timer !== null) clearTimeout(p.timer);
      p.reject(err);
    }
    this.pending.clear();
    this.worker = this.spawn();
    const log = this.logJson();
    const rebuild = async (): Promise<RestartInfo> => {
      try {
        const r = this.mirror.length ? await this.send({ kind: "replay", json: log }) : await this.send({ kind: "init" });
        return { reason, scene: r.scene ?? null };
      } catch (e) {
        // The log does not rebuild (it should: every entry was applied before): a new document.
        const error = e instanceof Error ? e.message : String(e);
        try {
          const r = await this.send({ kind: "init" });
          return { reason, scene: r.scene ?? null, error };
        } catch {
          return { reason, scene: null, error };
        }
      }
    };
    this.restarting = rebuild().then((info) => {
      this.restarting = null;
      this.onRestart?.(info);
    });
    return this.restarting;
  }

  /** Post one request to the current worker, with its deadline; no waiting on a restart. */
  private send(req: RequestBody): Promise<WorkerResponse & { ok: true }> {
    const id = this.nextId++;
    return new Promise<WorkerResponse>((resolve, reject) => {
      const ms = DEADLINES[req.kind] ?? DEADLINES.default;
      const timer = setTimeout(() => {
        if (!this.pending.has(id)) return;
        void this.restart(`${req.kind} ran for more than ${Math.round(ms / 1000)} s`);
      }, ms);
      this.pending.set(id, { resolve, reject, timer, kind: req.kind });
      const msg: WorkerRequest = { ...req, id, known: this.mirror.length };
      // Bytes handed to the kernel are transferred, not copied (lane 16); the caller's
      // array is detached afterwards.
      const transfer: Transferable[] = [];
      if ((req.kind === "importFile" || req.kind === "importSkp" || req.kind === "openShz") && req.bytes.byteOffset === 0 && req.bytes.byteLength === req.bytes.buffer.byteLength && req.bytes.buffer instanceof ArrayBuffer) transfer.push(req.bytes.buffer);
      this.worker.postMessage(msg, transfer);
    }).then((r) => {
      if (r.ok && r.logSync) {
        this.mirror.length = Math.min(this.mirror.length, r.logSync.from);
        for (const t of r.logSync.tail) this.mirror.push(t);
      }
      if (!r.ok) {
        if (r.fatal) {
          void this.restart(r.error);
          throw new KernelRestarted(`The kernel stopped (${r.error}) and was restarted from your last step; this did not change the model.`);
        }
        throw r.missing ? new KernelUnavailableError(r.error) : new KernelRefusal(r.error);
      }
      return r;
    });
  }

  private async call(req: RequestBody): Promise<WorkerResponse & { ok: true }> {
    // A request made while the worker is being replaced waits for the rebuilt document.
    while (this.restarting) await this.restarting;
    if (this.dead) throw new KernelUnavailableError(this.dead);
    if ((req.kind === "importFile" || req.kind === "importSkp") && this.failNext) {
      req = { ...req, failForTest: this.failNext };
      this.failNext = null;
    }
    return this.send(req);
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

  /** Replace the document with one rebuilt from an op log (autosave restore, undo history
   * included): the ops, or their JSON as one text (lane 16: the app never parses it). */
  async replay(ops: KernelOp[] | string): Promise<SceneData> {
    const r = await this.call(typeof ops === "string" ? { kind: "replay", json: ops } : { kind: "replay", ops });
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

  /** What an offset would draw (read-only). Throws KernelRefusal when it would be refused. */
  async offsetPreview(source: OffsetSource, distance: number): Promise<OffsetPreview> {
    const r = await this.call({ kind: "offsetPreview", source, distance });
    return r.preview ?? { loops: [], closed: true };
  }

  /** Where the cursor snaps for a drawing tool (read-only). */
  async infer(query: InferQuery): Promise<Inference | null> {
    return (await this.call({ kind: "infer", query })).inference ?? null;
  }

  async log(): Promise<KernelOp[]> {
    return (await this.call({ kind: "log" })).log ?? [];
  }

  /** The model's tree on its own (read-only): groups, instances, definitions, the context. */
  async graph(): Promise<SceneGraph> {
    const g = (await this.call({ kind: "graph" })).graph;
    if (!g) throw new Error("kernel returned no scene graph");
    return g;
  }

  async export(format: "stl" | "obj", object: number): Promise<Uint8Array | string> {
    const r = await this.call({ kind: "export", format, object });
    if (r.file === undefined) throw new Error("kernel returned no file");
    return r.file;
  }

  /** The document (or a scope of it) as a file in `format`. Throws KernelRefusal with a plain message when refused. */
  async exportFile(format: ExportFormat, options: ExportOptions = {}): Promise<Uint8Array> {
    const r = await this.call({ kind: "exportFile", format, options });
    if (!(r.file instanceof Uint8Array)) throw new Error("kernel returned no file");
    return r.file;
  }

  /** Import a file (format from its name, else its bytes) as one undoable step. */
  async importFile(name: string, bytes: Uint8Array, options: ImportOptions = {}): Promise<EditResult & { notes: string[] }> {
    const r = await this.call({ kind: "importFile", name, bytes, options });
    return { ...this.edit(r), notes: r.notes ?? [] };
  }

  /** Import a SketchUp file (parsed by openskp in the worker) as one undoable step. An
   * empty file (lane 15) changes nothing and comes back as `{ empty, notes }` instead. */
  async importSkp(name: string, bytes: Uint8Array): Promise<(EditResult & { notes: string[]; empty?: undefined }) | { empty: EmptySkp; notes: string[] }> {
    const r = await this.call({ kind: "importSkp", name, bytes });
    if (r.emptySkp) return { empty: r.emptySkp, notes: r.notes ?? [] };
    return { ...this.edit(r), notes: r.notes ?? [] };
  }

  /** The document as a .shz (zip: scene.json, geometry.glb, thumbnail.png; and the plugins'
   * document stores as plugin-data.json when `pluginData` is given, lane 16). */
  async saveShz(displayUnits: string, thumbnail: Uint8Array, pluginData = ""): Promise<Uint8Array> {
    const r = await this.call({ kind: "saveShz", displayUnits, thumbnail, pluginData });
    if (!(r.file instanceof Uint8Array)) throw new Error("kernel returned no file");
    return r.file;
  }

  /** Replace the document with an opened .shz. A refused file changes nothing. */
  async openShz(bytes: Uint8Array): Promise<EditResult & { notes: string[]; opened: { display_units: string | null; generator: string; plugin_data?: string | null } }> {
    const r = await this.call({ kind: "openShz", bytes });
    return { ...this.edit(r), notes: r.notes ?? [], opened: r.opened ?? { display_units: null, generator: "" } };
  }

  /** The cut list (read-only): parts, quantities, finished sizes, board feet and sheets. */
  async cutList(options: CutOptions): Promise<CutList> {
    const c = (await this.call({ kind: "cutList", options })).cutList;
    if (!c) throw new Error("kernel returned no cut list");
    return c;
  }

  /** The nesting (read-only): the cut list's parts laid out on the document's stock. */
  async nest(options: NestOptions): Promise<Nesting> {
    const n = (await this.call({ kind: "nest", options })).nesting;
    if (!n) throw new Error("kernel returned no nesting");
    return n;
  }

  /** The material face `face` of `object` shows (the Paint tool's Alt+click sample). */
  async sample(object: number, face: number): Promise<string | null> {
    return (await this.call({ kind: "sample", object, face })).material ?? null;
  }

  /** Texture coordinates the face would get with positioning `uv` (live preview; empty: no texture). */
  async faceUvs(object: number, face: number, uv: UvXform): Promise<Float32Array> {
    return (await this.call({ kind: "faceUvs", object, face, uv })).uvs ?? new Float32Array();
  }

  /** The built-in material library (read-only). */
  async library(): Promise<LibraryEntry[]> {
    return (await this.call({ kind: "library" })).library ?? [];
  }

  /** The ops that put library material `name` into the model (its texture and definition), named `asName`. */
  async libraryOps(name: string, asName = ""): Promise<KernelOp[]> {
    return (await this.call({ kind: "libraryOps", name, asName })).ops ?? [];
  }

  /** A library material's swatch (a small PNG of its texture), for the Materials pane. */
  async libraryTexture(name: string): Promise<Uint8Array> {
    const r = await this.call({ kind: "libraryTexture", name });
    if (!(r.file instanceof Uint8Array)) throw new Error("kernel returned no image");
    return r.file;
  }

  /** The bytes of a texture image the model holds. */
  async texture(path: string): Promise<Uint8Array> {
    const r = await this.call({ kind: "texture", path });
    if (!(r.file instanceof Uint8Array)) throw new Error("kernel returned no image");
    return r.file;
  }

  /** A solid's faces by persistent id (read-only; plugins). */
  async faces(object: number): Promise<FaceInfo[]> {
    return (await this.call({ kind: "faces", object })).faces ?? [];
  }

  /** A plugin's named batch: ops with placeholders, applied as ONE op (one undo step). Throws KernelRefusal when any op is refused (nothing changes). */
  async namedBatch(name: string, ops: unknown[]): Promise<EditResult> {
    return this.edit(await this.call({ kind: "namedBatch", name, ops }));
  }

  /** Sheet `sheet` laid out on paper (lane 12, read-only): what the canvas and every sheet
   * export draw. */
  async sheetRender(sheet: number): Promise<import("../sheets/render").SheetRender> {
    const r = (await this.call({ kind: "sheetRender", sheet })).sheetRender;
    if (!r) throw new Error("kernel returned no sheet");
    return r;
  }

  /** Where a cursor at paper (x, y) snaps on viewport `item` of sheet `sheet` (lane 12). */
  async sheetSnap(sheet: number, item: number, x: number, y: number, radius: number): Promise<import("../sheets/render").SheetSnap | null> {
    return (await this.call({ kind: "sheetSnap", sheet, item, x, y, radius })).sheetSnap ?? null;
  }

  /** How viewport `item` (not yet placed) would draw: the factor that fits its drawing in its
   * rectangle and the drawing's size (lane 12). */
  async viewportFit(item: import("./sheetTypes").ItemKind): Promise<import("../sheets/render").ViewportFit> {
    const r = (await this.call({ kind: "viewportFit", item })).viewportFit;
    if (!r) throw new Error("kernel returned no fit");
    return r;
  }

  /** Sheet `sheet` as DXF text, at paper size in the sheet's units (lane 12). */
  async sheetDxf(sheet: number): Promise<string> {
    const r = (await this.call({ kind: "sheetDxf", sheet })).sheetDxf;
    if (r === undefined) throw new Error("kernel returned no DXF");
    return r;
  }

  /** A fresh document (ground sketch only), as an edit result. */
  async newDocument(): Promise<EditResult> {
    const scene = await this.init();
    return { outcome: { kind: "restored", objects: 0, meshes: 0, sketches: scene.state.sketches.length }, scene };
  }

  private edit(r: WorkerResponse & { ok: true }): EditResult {
    if (!r.outcome || !r.scene) throw new Error("kernel returned no result");
    return { outcome: r.outcome, scene: r.scene };
  }

  dispose(): void {
    this.worker.terminate();
    for (const p of this.pending.values()) if (p.timer !== null) clearTimeout(p.timer);
    this.pending.clear();
  }
}

/** What an empty .skp still says (lane 15): its units and its style. */
export type EmptySkp = NonNullable<Extract<WorkerResponse, { ok: true }>["emptySkp"]>;

export function createKernel(): KernelClient | null {
  if (typeof Worker === "undefined") return null;
  return new KernelClient();
}
