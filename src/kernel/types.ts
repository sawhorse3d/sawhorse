// Shapes that cross the kernel boundary (wasm-api -> worker -> UI).
// Lengths are metres, Z-up, exactly as the Rust kernel stores them (ADR-0001).

/** The end-to-end spike: one rectangle on the ground plane, extruded by a distance. */
export interface SpikeRecipe {
  /** Rectangle size along +X, metres. */
  width: number;
  /** Rectangle size along +Y, metres. */
  depth: number;
  /** Extrusion along +Z (negative extrudes downward), metres. */
  distance: number;
}

export interface KernelStats {
  objects: number;
  faces: number;
  edges: number;
  vertices: number;
  holes: number;
  shells: number;
  watertight: boolean;
  /** V - E + F - H summed over shells. */
  euler: number;
  genus: number;
  volume_m3: number;
  /** Deterministic model hash (golden-hash format), hex. */
  hash: string;
}

export interface MeshData {
  /** xyz per render vertex, metres. Vertices are not shared between faces (flat shading). */
  positions: Float32Array;
  /** Three indices per triangle. */
  indices: Uint32Array;
  /** One kernel face id per triangle. */
  faceIds: Uint32Array;
  /** Edge segments as xyz pairs (6 floats per edge), metres. */
  edges: Float32Array;
  stats: KernelStats;
}

/** The subset of the wasm-bindgen module the worker uses (see crates/wasm-api/src/lib.rs). */
export interface WasmMesh {
  positions(): Float32Array;
  indices(): Uint32Array;
  face_ids(): Uint32Array;
  edges(): Float32Array;
  free(): void;
}

export interface WasmScene {
  add_rectangle(x: number, y: number, w: number, h: number): number;
  extrude(region: number, distance: number): number;
  push_pull(object: number, face: number, distance: number): void;
  get_mesh(): WasmMesh;
  stats_json(): string;
  hash(): string;
  free(): void;
}

export interface WasmModule {
  default: (input?: unknown) => Promise<unknown>;
  new_scene(): WasmScene;
  kernel_version(): string;
}

export type RequestBody = { kind: "build"; recipe: SpikeRecipe } | { kind: "version" };
export type WorkerRequest = RequestBody & { id: number };

export type WorkerResponse =
  | { id: number; ok: true; mesh?: MeshData; version?: string }
  | { id: number; ok: false; error: string; missing?: boolean };
