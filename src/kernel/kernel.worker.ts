/// <reference lib="webworker" />
// The kernel runs in a Web Worker (ADR-0001): geometry work never blocks the UI
// thread, and the same WASM runs in the browser build and the Tauri webview.
//
// The worker owns ONE persistent kernel document for the session (model + op log +
// undo ring). Every edit is an op applied transactionally; after each request the
// worker answers with the outcome and the fresh scene (view mesh + document state).
//
// Lane 16: every answer also brings the op log entries the app does not hold yet
// (`logSync`), so the app can rebuild the document in a fresh worker; and a failure of the
// kernel itself (a WebAssembly trap: a Rust panic or running out of memory, which leaves
// this worker's kernel unusable) is answered with `fatal`, so the app replaces the worker
// instead of showing every later request refused.
import type { SheetRow } from "./sheetTypes";
import type { DocState, Inference, KernelOp, LibraryEntry, MeshData, OpOutcome, Picked, SceneData, SceneGraph, SectionView, TextureBlob, WasmDoc, WasmModule, WorkerRequest, WorkerResponse } from "./types";

// import.meta.glob tolerates a missing package: the browser build still works
// (with a "kernel not built" HUD) on a machine that has not run `npm run wasm`.
const loaders = import.meta.glob<WasmModule>("../wasm/pkg/sawhorse_wasm.js");
const WASM_ENTRY = "../wasm/pkg/sawhorse_wasm.js";

class KernelMissing extends Error {}

let ready: Promise<WasmModule> | null = null;
/** The kernel's WebAssembly memory (lane 16b: tests read its size, which only grows). */
let wasmMemory: WebAssembly.Memory | null = null;
let doc: WasmDoc | null = null;

function load(): Promise<WasmModule> {
  if (!ready) {
    ready = (async () => {
      const loader = loaders[WASM_ENTRY];
      if (!loader) throw new KernelMissing("Kernel WASM is not built. Run: npm run wasm");
      const mod = await loader();
      const exports = (await mod.default()) as { memory?: WebAssembly.Memory } | undefined;
      wasmMemory = exports?.memory ?? null;
      return mod;
    })();
  }
  return ready;
}

function document(mod: WasmModule): WasmDoc {
  if (!doc) doc = mod.document_new();
  return doc;
}

/** Texture digests the UI already has: each image crosses to the UI thread once (lane 10). */
const sentTextures = new Set<string>();

/** Images the state lists that the UI has not been sent yet. */
function newTextures(d: WasmDoc, state: DocState): TextureBlob[] {
  const out: TextureBlob[] = [];
  for (const t of state.textures ?? []) {
    if (sentTextures.has(t.digest)) continue;
    try {
      out.push({ path: t.path, digest: t.digest, mime: t.mime, bytes: d.texture(t.path) });
      sentTextures.add(t.digest);
    } catch {
      // Not readable now: listed again next time.
    }
  }
  return out;
}

function scene(d: WasmDoc): SceneData {
  const m = d.get_scene_mesh();
  try {
    const mesh: MeshData = {
      positions: m.positions(),
      indices: m.indices(),
      faceIds: m.face_ids(),
      edges: m.edges(),
      edgeOwner: m.edge_owner(),
      edgeOwners: JSON.parse(m.edge_owners_json()),
      parts: JSON.parse(m.parts_json()),
      uvs: m.uvs(),
      edgeNormals: m.edge_normals(),
      edgeFlags: m.edge_flags(),
      normals: m.normals(),
    };
    const state = JSON.parse(d.state_json()) as DocState;
    // Section cuts (lane 12): only for a model that has a section plane.
    const sections = state.graph?.sections?.length ? (JSON.parse(d.section_view()) as SectionView) : undefined;
    // Drawing sheets (lane 12), with every edit (plain data, small).
    const sheets = JSON.parse(d.sheets()) as SheetRow[];
    return { mesh, state, textures: newTextures(d, state), sections, sheets };
  } finally {
    m.free();
  }
}

const ctx = self as unknown as DedicatedWorkerGlobalScope;
/** The document's log compactions when the app's copy of the log was last brought up to
 * date (lane 16). */
let logEpoch = 0;

/** A failure of the kernel itself, not a refusal: after a trap the WebAssembly instance (and
 * the document it holds, borrowed when it failed) cannot be used again. */
function fatal(err: unknown): boolean {
  if (typeof WebAssembly !== "undefined" && err instanceof WebAssembly.RuntimeError) return true;
  const m = err instanceof Error ? err.message : String(err);
  return /recursive use of an object|unreachable|memory access out of bounds|out of memory|RuntimeError/i.test(m);
}

ctx.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const req = event.data;
  let res: WorkerResponse;
  // The document was replaced (init, replay, open): the app's copy of the log starts over.
  let replaced = false;
  try {
    const mod = await load();
    switch (req.kind) {
      case "version":
        res = { id: req.id, ok: true, version: mod.kernel_version() };
        break;
      case "memory":
        res = { id: req.id, ok: true, wasmBytes: wasmMemory?.buffer.byteLength ?? 0 };
        break;
      case "init": {
        doc?.free();
        doc = mod.document_new();
        replaced = true;
        // A fresh UI (or a test resetting it) has no images yet.
        sentTextures.clear();
        res = { id: req.id, ok: true, scene: scene(doc) };
        break;
      }
      case "replay": {
        // Build first, swap after: a refused log leaves the current document alone.
        const next = mod.document_replay(req.json ?? JSON.stringify(req.ops ?? []));
        replaced = true;
        doc?.free();
        doc = next;
        res = { id: req.id, ok: true, scene: scene(doc) };
        break;
      }
      case "scene":
        res = { id: req.id, ok: true, scene: scene(document(mod)) };
        break;
      case "apply": {
        const d = document(mod);
        const outcome = JSON.parse(d.apply_op(JSON.stringify(req.op)));
        res = { id: req.id, ok: true, outcome, scene: scene(d) };
        break;
      }
      case "undo":
      case "redo": {
        const d = document(mod);
        const outcome = JSON.parse(req.kind === "undo" ? d.undo() : d.redo());
        res = { id: req.id, ok: true, outcome, scene: scene(d) };
        break;
      }
      case "outline":
        res = { id: req.id, ok: true, rings: JSON.parse(document(mod).outline(req.part, req.owner, req.index)) };
        break;
      case "offsetPreview":
        res = { id: req.id, ok: true, preview: JSON.parse(document(mod).offset_preview(JSON.stringify({ source: req.source, distance: req.distance }))) };
        break;
      case "infer":
        res = { id: req.id, ok: true, inference: JSON.parse(document(mod).infer(JSON.stringify(req.query))) as Inference | null };
        break;
      case "pick":
        res = { id: req.id, ok: true, picked: JSON.parse(document(mod).pick(JSON.stringify(req.query))) as Picked | null };
        break;
      case "log":
        res = { id: req.id, ok: true, log: JSON.parse(document(mod).op_log_json()) as KernelOp[] };
        break;
      case "graph":
        res = { id: req.id, ok: true, graph: JSON.parse(document(mod).scene_graph()) as SceneGraph };
        break;
      case "importFile": {
        const d = document(mod);
        if (req.failForTest) d.fail_for_test(req.failForTest);
        const r = JSON.parse(d.import_file(req.name, req.bytes, JSON.stringify(req.options ?? {}))) as { outcome: OpOutcome; notes: string[] };
        res = { id: req.id, ok: true, outcome: r.outcome, notes: r.notes, scene: scene(d) };
        break;
      }
      case "importSkp": {
        // openskp (MIT) parses SketchUp files here, off the UI thread, loaded on first use.
        const { skpToOp, importOutcomeOf } = await import("../io/skp");
        if (req.failForTest) document(mod).fail_for_test(req.failForTest);
        const r = skpToOp(req.bytes, req.name);
        if (!r.op) {
          // Lane 15: an empty file changes nothing here; the app opens it as an empty model.
          res = { id: req.id, ok: true, notes: r.notes, emptySkp: r.empty ?? { units: null, unitsWord: null, style: null } };
          break;
        }
        const d = document(mod);
        const applied = JSON.parse(d.apply_op(JSON.stringify(r.op))) as OpOutcome;
        // Texture images go in first and section planes after, batched with the import:
        // report the import itself.
        const outcome = importOutcomeOf(applied);
        res = { id: req.id, ok: true, outcome, notes: r.notes, scene: scene(d) };
        break;
      }
      case "saveShz": {
        const file = document(mod).save_shz(req.displayUnits, req.thumbnail, req.pluginData ?? "");
        res = { id: req.id, ok: true, file };
        break;
      }
      case "openShz": {
        const d = document(mod);
        const info = JSON.parse(d.open_shz(req.bytes)) as { notes: string[]; display_units: string | null; generator: string; plugin_data?: string | null };
        replaced = true;
        const st = JSON.parse(d.state_json()) as { objects: number[]; meshes: unknown[]; sketches: unknown[] };
        const outcome: OpOutcome = { kind: "restored", objects: st.objects.length, meshes: st.meshes.length, sketches: st.sketches.length };
        res = { id: req.id, ok: true, outcome, notes: info.notes, opened: { display_units: info.display_units, generator: info.generator, plugin_data: info.plugin_data ?? null }, scene: scene(d) };
        break;
      }
      case "exportFile": {
        const file = document(mod).export_file(req.format, JSON.stringify(req.options ?? {}));
        res = { id: req.id, ok: true, file };
        break;
      }
      case "cutList":
        res = { id: req.id, ok: true, cutList: JSON.parse(document(mod).cut_list(JSON.stringify(req.options))) };
        break;
      case "nest":
        res = { id: req.id, ok: true, nesting: JSON.parse(document(mod).nest(JSON.stringify(req.options))) };
        break;
      case "sample":
        res = { id: req.id, ok: true, material: JSON.parse(document(mod).sample(req.object, req.face)) as string | null };
        break;
      case "faceUvs":
        res = { id: req.id, ok: true, uvs: document(mod).face_uvs(req.object, req.face, JSON.stringify(req.uv)) };
        break;
      case "library":
        res = { id: req.id, ok: true, library: JSON.parse(mod.material_library()) as LibraryEntry[] };
        break;
      case "libraryOps":
        res = { id: req.id, ok: true, ops: JSON.parse(mod.material_library_ops(req.name, req.asName)) as KernelOp[] };
        break;
      case "libraryTexture":
        res = { id: req.id, ok: true, file: mod.material_library_swatch(req.name) };
        break;
      case "texture":
        res = { id: req.id, ok: true, file: document(mod).texture(req.path) };
        break;
      // Lane 12: drawing sheets (read-only).
      case "sheetRender":
        res = { id: req.id, ok: true, sheetRender: JSON.parse(document(mod).sheet_render(req.sheet)) };
        break;
      case "sheetSnap":
        res = { id: req.id, ok: true, sheetSnap: JSON.parse(document(mod).sheet_snap(req.sheet, req.item, req.x, req.y, req.radius)) };
        break;
      case "sheetDxf":
        res = { id: req.id, ok: true, sheetDxf: document(mod).sheet_dxf(req.sheet) };
        break;
      case "viewportFit":
        res = { id: req.id, ok: true, viewportFit: JSON.parse(document(mod).viewport_fit(JSON.stringify(req.item))) };
        break;
      case "faces":
        res = { id: req.id, ok: true, faces: JSON.parse(document(mod).faces_json(req.object)) };
        break;
      case "namedBatch": {
        // A plugin's batch (lane 11): placeholders resolved, applied as one op (one undo step).
        const d = document(mod);
        const outcome = JSON.parse(d.apply_named_batch(JSON.stringify({ name: req.name, ops: req.ops }))) as OpOutcome;
        res = { id: req.id, ok: true, outcome, scene: scene(d) };
        break;
      }
      case "export": {
        const d = document(mod);
        const file = req.format === "stl" ? d.export_stl(req.object) : d.export_obj(req.object);
        res = { id: req.id, ok: true, file };
        break;
      }
    }
    // The log entries the app does not hold yet (lane 16).
    // A compacted log (one restore of the model, past its byte budget) starts over even when
    // its length happens to match what the app holds.
    if (res.ok && req.known !== undefined && doc) {
      const len = doc.op_log_len();
      const epoch = doc.op_log_epoch();
      const compacted = epoch !== logEpoch;
      logEpoch = epoch;
      if (replaced || compacted || len !== req.known) {
        const from = replaced || compacted || len < req.known ? 0 : req.known;
        res.logSync = { from, tail: doc.op_log_tail(from) };
      }
    }
  } catch (err) {
    res = { id: req.id, ok: false, error: err instanceof Error ? err.message : String(err), missing: err instanceof KernelMissing, fatal: fatal(err) };
  }
  const transfer: Transferable[] = [];
  if (res.ok && res.scene) {
    const m = res.scene.mesh;
    transfer.push(m.positions.buffer, m.indices.buffer, m.faceIds.buffer, m.edges.buffer, m.edgeOwner.buffer);
    if (m.uvs) transfer.push(m.uvs.buffer);
    if (m.edgeNormals) transfer.push(m.edgeNormals.buffer);
    if (m.edgeFlags) transfer.push(m.edgeFlags.buffer);
    if (m.normals) transfer.push(m.normals.buffer);
    for (const t of res.scene.textures ?? []) transfer.push(t.bytes.buffer);
  }
  if (res.ok && res.file instanceof Uint8Array) transfer.push(res.file.buffer);
  if (res.ok && res.uvs) transfer.push(res.uvs.buffer);
  ctx.postMessage(res, transfer);
};
