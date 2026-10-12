// Promise wrapper around the kernel worker, which holds the persistent document.
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

  /** Replace the document with one rebuilt from an op log (autosave restore, undo history included). */
  async replay(ops: KernelOp[]): Promise<SceneData> {
    const r = await this.call({ kind: "replay", ops });
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

  /** Import a SketchUp file (parsed by openskp in the worker) as one undoable step. */
  async importSkp(name: string, bytes: Uint8Array): Promise<EditResult & { notes: string[] }> {
    const r = await this.call({ kind: "importSkp", name, bytes });
    return { ...this.edit(r), notes: r.notes ?? [] };
  }

  /** The document as a .shz (zip: scene.json, geometry.glb, thumbnail.png). */
  async saveShz(displayUnits: string, thumbnail: Uint8Array): Promise<Uint8Array> {
    const r = await this.call({ kind: "saveShz", displayUnits, thumbnail });
    if (!(r.file instanceof Uint8Array)) throw new Error("kernel returned no file");
    return r.file;
  }

  /** Replace the document with an opened .shz. A refused file changes nothing. */
  async openShz(bytes: Uint8Array): Promise<EditResult & { notes: string[]; opened: { display_units: string | null; generator: string } }> {
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
    this.pending.clear();
  }
}

export function createKernel(): KernelClient | null {
  if (typeof Worker === "undefined") return null;
  return new KernelClient();
}
