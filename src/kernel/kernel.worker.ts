/// <reference lib="webworker" />
// The kernel runs in a Web Worker (ADR-0001): geometry work never blocks the UI
// thread, and the same WASM runs in the browser build and the Tauri webview.
//
// The worker owns ONE persistent kernel document for the session (model + op log +
// undo ring). Every edit is an op applied transactionally; after each request the
// worker answers with the outcome and the fresh scene (view mesh + document state).
import type { SheetRow } from "./sheetTypes";
import type { DocState, Inference, KernelOp, LibraryEntry, MeshData, OpOutcome, Picked, SceneData, SceneGraph, SectionView, TextureBlob, WasmDoc, WasmModule, WorkerRequest, WorkerResponse } from "./types";

// import.meta.glob tolerates a missing package: the browser build still works
// (with a "kernel not built" HUD) on a machine that has not run `npm run wasm`.
const loaders = import.meta.glob<WasmModule>("../wasm/pkg/sawhorse_wasm.js");
const WASM_ENTRY = "../wasm/pkg/sawhorse_wasm.js";

class KernelMissing extends Error {}

let ready: Promise<WasmModule> | null = null;
let doc: WasmDoc | null = null;

function load(): Promise<WasmModule> {
  if (!ready) {
    ready = (async () => {
      const loader = loaders[WASM_ENTRY];
      if (!loader) throw new KernelMissing("Kernel WASM is not built. Run: npm run wasm");
      const mod = await loader();
      await mod.default();
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

ctx.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const req = event.data;
  let res: WorkerResponse;
  try {
    const mod = await load();
    switch (req.kind) {
      case "version":
        res = { id: req.id, ok: true, version: mod.kernel_version() };
        break;
      case "init": {
        doc?.free();
        doc = mod.document_new();
        // A fresh UI (or a test resetting it) has no images yet.
        sentTextures.clear();
        res = { id: req.id, ok: true, scene: scene(doc) };
        break;
      }
      case "replay": {
        // Build first, swap after: a refused log leaves the current document alone.
        const next = mod.document_replay(JSON.stringify(req.ops));
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
        const r = JSON.parse(d.import_file(req.name, req.bytes, JSON.stringify(req.options ?? {}))) as { outcome: OpOutcome; notes: string[] };
        res = { id: req.id, ok: true, outcome: r.outcome, notes: r.notes, scene: scene(d) };
        break;
      }
      case "importSkp": {
        // openskp (MIT) parses SketchUp files here, off the UI thread, loaded on first use.
        const { skpToOp } = await import("../io/skp");
        const r = skpToOp(req.bytes, req.name);
        const d = document(mod);
        const applied = JSON.parse(d.apply_op(JSON.stringify(r.op))) as OpOutcome;
        // Texture images go in first, batched with the import: report the import itself.
        const outcome = applied.kind === "batch" ? applied.outcomes[applied.outcomes.length - 1] : applied;
        res = { id: req.id, ok: true, outcome, notes: r.notes, scene: scene(d) };
        break;
      }
      case "saveShz": {
        const file = document(mod).save_shz(req.displayUnits, req.thumbnail);
        res = { id: req.id, ok: true, file };
        break;
      }
      case "openShz": {
        const d = document(mod);
        const info = JSON.parse(d.open_shz(req.bytes)) as { notes: string[]; display_units: string | null; generator: string };
        const st = JSON.parse(d.state_json()) as { objects: number[]; meshes: unknown[]; sketches: unknown[] };
        const outcome: OpOutcome = { kind: "restored", objects: st.objects.length, meshes: st.meshes.length, sketches: st.sketches.length };
        res = { id: req.id, ok: true, outcome, notes: info.notes, opened: { display_units: info.display_units, generator: info.generator }, scene: scene(d) };
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
  } catch (err) {
    res = { id: req.id, ok: false, error: err instanceof Error ? err.message : String(err), missing: err instanceof KernelMissing };
  }
  const transfer: Transferable[] = [];
  if (res.ok && res.scene) {
    const m = res.scene.mesh;
    transfer.push(m.positions.buffer, m.indices.buffer, m.faceIds.buffer, m.edges.buffer, m.edgeOwner.buffer);
    if (m.uvs) transfer.push(m.uvs.buffer);
    if (m.edgeNormals) transfer.push(m.edgeNormals.buffer);
    for (const t of res.scene.textures ?? []) transfer.push(t.bytes.buffer);
  }
  if (res.ok && res.file instanceof Uint8Array) transfer.push(res.file.buffer);
  if (res.ok && res.uvs) transfer.push(res.uvs.buffer);
  ctx.postMessage(res, transfer);
};
