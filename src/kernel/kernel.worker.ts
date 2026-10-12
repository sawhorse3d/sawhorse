/// <reference lib="webworker" />
// The kernel runs in a Web Worker (ADR-0001): geometry work never blocks the UI
// thread, and the same WASM runs in the browser build and the Tauri webview.
//
// The worker owns ONE persistent kernel document for the session (model + op log +
// undo ring). Every edit is an op applied transactionally; after each request the
// worker answers with the outcome and the fresh scene (view mesh + document state).
import type { Inference, KernelOp, MeshData, Picked, SceneData, WasmDoc, WasmModule, WorkerRequest, WorkerResponse } from "./types";

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
    };
    return { mesh, state: JSON.parse(d.state_json()) };
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
      case "infer":
        res = { id: req.id, ok: true, inference: JSON.parse(document(mod).infer(JSON.stringify(req.query))) as Inference | null };
        break;
      case "pick":
        res = { id: req.id, ok: true, picked: JSON.parse(document(mod).pick(JSON.stringify(req.query))) as Picked | null };
        break;
      case "log":
        res = { id: req.id, ok: true, log: JSON.parse(document(mod).op_log_json()) as KernelOp[] };
        break;
      case "exportFile": {
        const file = document(mod).export_file(req.format, JSON.stringify(req.options ?? {}));
        res = { id: req.id, ok: true, file };
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
  }
  if (res.ok && res.file instanceof Uint8Array) transfer.push(res.file.buffer);
  ctx.postMessage(res, transfer);
};
