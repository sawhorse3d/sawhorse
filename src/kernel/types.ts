// Shapes that cross the kernel boundary (wasm-api -> worker -> UI).
// Lengths are metres, Z-up, exactly as the Rust kernel stores them (ADR-0001).
// Ids are kernel public ids (plain numbers; the ground sketch is 1).

/** One kernel op, in the op-log JSON format (see crates/kernel/src/model.rs `Op`). */
export type KernelOp =
  | { op: "new_sketch"; origin: [number, number, number]; normal: [number, number, number] }
  | { op: "add_rectangle"; sketch: number; x: number; y: number; w: number; h: number }
  | { op: "insert_segment"; sketch: number; a: [number, number]; b: [number, number] }
  | { op: "erase_segment"; sketch: number; a: [number, number]; b: [number, number] }
  | { op: "add_polyline"; sketch: number; points: [number, number][]; closed: boolean }
  | { op: "extrude"; sketch: number; region: number; distance: number }
  | { op: "push_pull"; object: number; face: number; distance: number }
  | { op: "split_face"; object: number; face: number; path: [number, number, number][] }
  | { op: "imprint"; object: number; face: number; points: [number, number, number][] }
  | { op: "delete_object"; object: number }
  | { op: "create_group"; name: string; objects: number[] }
  | { op: "delete_group"; group: number }
  | { op: "set_tag"; object: number; tag: string | null }
  | { op: "set_tag_visible"; tag: string; visible: boolean }
  | { op: "undo" }
  | { op: "redo" };

/** What an op did (crates/kernel/src/model.rs `OpOutcome`). */
export type OpOutcome =
  | { kind: "sketch_created"; sketch: number }
  | { kind: "sketch_edited"; regions: number[] }
  | { kind: "object_created"; object: number }
  | { kind: "face_moved" }
  | { kind: "object_collapsed"; regions: number[] }
  | { kind: "face_split"; faces: number[] }
  | { kind: "imprinted"; face: number }
  | { kind: "object_deleted" }
  | { kind: "group_created"; group: number }
  | { kind: "group_deleted" }
  | { kind: "tag_set" }
  | { kind: "undone" }
  | { kind: "redone" };

export interface KernelStats {
  sketches: number;
  regions: number;
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

/** Document summary after every op. */
export interface DocState {
  stats: KernelStats;
  can_undo: boolean;
  can_redo: boolean;
  undo_depth: number;
  redo_depth: number;
  log_len: number;
  log_hash: string;
  objects: number[];
  tags: Record<string, boolean>;
}

/** A pickable part: an object's face (index = face ordinal) or a sketch region (index = region id). */
export interface MeshPart {
  kind: "face" | "region";
  owner: number;
  index: number;
  origin: [number, number, number];
  normal: [number, number, number];
  u: [number, number, number];
  v: [number, number, number];
}

export interface MeshData {
  /** xyz per render vertex, metres. Vertices are not shared between faces (flat shading). */
  positions: Float32Array;
  /** Three indices per triangle. */
  indices: Uint32Array;
  /** One part index per triangle (into `parts`). */
  faceIds: Uint32Array;
  /** Edge segments as xyz pairs (6 floats per edge), metres. */
  edges: Float32Array;
  parts: MeshPart[];
}

export interface SceneData {
  mesh: MeshData;
  state: DocState;
}

/** The subset of the wasm-bindgen module the worker uses (see crates/wasm-api/src/lib.rs). */
export interface WasmMesh {
  positions(): Float32Array;
  indices(): Uint32Array;
  face_ids(): Uint32Array;
  edges(): Float32Array;
  parts_json(): string;
  free(): void;
}

export interface WasmDoc {
  apply_op(json: string): string;
  undo(): string;
  redo(): string;
  get_scene_mesh(): WasmMesh;
  get_view_mesh(object: number): WasmMesh;
  document_hash(): string;
  log_hash(): string;
  op_log_json(): string;
  state_json(): string;
  export_stl(object: number): Uint8Array;
  export_obj(object: number): string;
  free(): void;
}

export interface WasmModule {
  default: (input?: unknown) => Promise<unknown>;
  document_new(): WasmDoc;
  kernel_version(): string;
}

export type RequestBody =
  | { kind: "version" }
  | { kind: "init" }
  | { kind: "scene" }
  | { kind: "apply"; op: KernelOp }
  | { kind: "undo" }
  | { kind: "redo" }
  | { kind: "log" }
  | { kind: "export"; format: "stl" | "obj"; object: number };
export type WorkerRequest = RequestBody & { id: number };

export type WorkerResponse =
  | {
      id: number;
      ok: true;
      version?: string;
      outcome?: OpOutcome;
      scene?: SceneData;
      log?: KernelOp[];
      file?: Uint8Array | string;
    }
  | { id: number; ok: false; error: string; missing?: boolean };
