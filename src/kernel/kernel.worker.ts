/// <reference lib="webworker" />
// The kernel runs in a Web Worker (ADR-0001): geometry work never blocks the UI
// thread, and the same WASM runs in the browser build and the Tauri webview.
import type { MeshData, WasmModule, WorkerRequest, WorkerResponse } from "./types";

// import.meta.glob tolerates a missing package: the browser build still works
// (with a "kernel not built" HUD) on a machine that has not run `npm run wasm`.
const loaders = import.meta.glob<WasmModule>("../wasm/pkg/sawhorse_wasm.js");
const WASM_ENTRY = "../wasm/pkg/sawhorse_wasm.js";

class KernelMissing extends Error {}

let ready: Promise<WasmModule> | null = null;

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

function build(mod: WasmModule, recipe: { width: number; depth: number; distance: number }): MeshData {
  const scene = mod.new_scene();
  try {
    const region = scene.add_rectangle(0, 0, recipe.width, recipe.depth);
    scene.extrude(region, recipe.distance);
    const mesh = scene.get_mesh();
    try {
      return {
        positions: mesh.positions(),
        indices: mesh.indices(),
        faceIds: mesh.face_ids(),
        edges: mesh.edges(),
        stats: JSON.parse(scene.stats_json()),
      };
    } finally {
      mesh.free();
    }
  } finally {
    scene.free();
  }
}

const ctx = self as unknown as DedicatedWorkerGlobalScope;

ctx.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const req = event.data;
  let res: WorkerResponse;
  try {
    const mod = await load();
    if (req.kind === "version") {
      res = { id: req.id, ok: true, version: mod.kernel_version() };
    } else {
      res = { id: req.id, ok: true, mesh: build(mod, req.recipe) };
    }
  } catch (err) {
    res = { id: req.id, ok: false, error: err instanceof Error ? err.message : String(err), missing: err instanceof KernelMissing };
  }
  const transfer: Transferable[] = [];
  if (res.ok && res.mesh) {
    const m = res.mesh;
    transfer.push(m.positions.buffer, m.indices.buffer, m.faceIds.buffer, m.edges.buffer);
  }
  ctx.postMessage(res, transfer);
};
