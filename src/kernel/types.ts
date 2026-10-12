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
  | { op: "push_pull_new_face"; object: number; face: number; distance: number }
  | { op: "split_face"; object: number; face: number; path: [number, number, number][] }
  | { op: "imprint"; object: number; face: number; points: [number, number, number][] }
  | {
      op: "draw";
      target: { on: "sketch"; sketch: number } | { on: "plane"; origin: [number, number, number]; normal: [number, number, number] } | { on: "face"; object: number; face: number };
      points: [number, number, number][];
      closed: boolean;
    }
  | { op: "erase_edge"; object: number; a: [number, number, number]; b: [number, number, number] }
  | { op: "erase_face"; object: number; face: number }
  | { op: "erase_region"; sketch: number; region: number }
  | { op: "batch"; ops: KernelOp[] }
  | { op: "add_guide"; point: [number, number, number]; dir: [number, number, number] | null }
  | { op: "erase_guide"; guide: number }
  | { op: "delete_guides" }
  | { op: "add_dimension"; a: [number, number, number]; b: [number, number, number]; offset: [number, number, number] }
  | { op: "erase_dimension"; dimension: number }
  | { op: "transform"; objects: number[]; xform: Xform }
  | { op: "copy"; objects: number[]; xform: Xform }
  | { op: "array"; objects: number[]; xform: Xform; count: number }
  | { op: "transform_geometry"; object: number; points: V3[]; xform: Xform; autofold: boolean }
  | { op: "offset"; source: OffsetSource; distance: number }
  | { op: "follow_me"; profile: Profile; path: V3[]; closed: boolean }
  | { op: "delete_object"; object: number }
  | { op: "create_group"; name: string; objects: number[] }
  | { op: "delete_group"; group: number }
  | { op: "set_tag"; object: number; tag: string | null }
  | { op: "set_tag_visible"; tag: string; visible: boolean }
  | { op: "undo" }
  | { op: "redo" };

/** What Offset acts on (crates/kernel/src/offset.rs `OffsetSource`). */
export type OffsetSource =
  | { of: "face"; object: number; face: number }
  | { of: "region"; sketch: number; region: number }
  | { of: "edges"; points: V3[]; closed: boolean; normal: V3; object: number | null };

/** What Follow Me sweeps (crates/kernel/src/sweep.rs `Profile`). */
export type Profile = { of: "face"; object: number; face: number } | { of: "region"; sketch: number; region: number };

/** What an offset would draw (Doc.offset_preview). */
export interface OffsetPreview {
  loops: V3[][];
  closed: boolean;
}

/** An affine map (crates/kernel/src/transform.rs `Xform`): p -> linear * p + translation, row-major. */
export interface Xform {
  linear: [V3, V3, V3];
  translation: V3;
}

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
  | { kind: "faces_merged"; face: number }
  | { kind: "guide_added"; guide: number }
  | { kind: "dimension_added"; dimension: number }
  | { kind: "annotation_erased" }
  | { kind: "drawn"; sketch: number | null; regions: number[]; faces: number[]; inside: number | null }
  | { kind: "batch"; outcomes: OpOutcome[] }
  | { kind: "group_created"; group: number }
  | { kind: "group_deleted" }
  | { kind: "tag_set" }
  | { kind: "transformed"; objects: number[] }
  | { kind: "copied"; objects: number[] }
  | { kind: "geometry_moved"; faces: number[] }
  | { kind: "severed"; objects: number[] }
  | { kind: "swept"; objects: number[]; cut: boolean }
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
  /** Per solid, in creation order: display number ("solid 3"), bounds (m) and volume (m^3). */
  solids: SolidInfo[];
  /** Every sketch with its plane frame (world <-> sketch coordinates). */
  sketches: SketchInfo[];
  groups: { id: number; name: string; objects: number[] }[];
  /** Guide lines (dir set) and guide points (dir null). */
  guides: { id: number; point: V3; dir: V3 | null }[];
  dimensions: { id: number; a: V3; b: V3; offset: V3; length: number }[];
  tags: Record<string, boolean>;
}

export type V3 = [number, number, number];

export interface SolidInfo {
  /** Public id (what ops use). */
  id: number;
  /** Display number: what people see ("solid 3"); never reused. */
  number: number;
  min: V3;
  max: V3;
  volume: number;
}

export interface SketchInfo {
  id: number;
  origin: V3;
  normal: V3;
  u: V3;
  v: V3;
}

/** What an edge, vertex or guide belongs to (crates/inference scene::Owner). */
export interface Owner {
  kind: "object" | "sketch" | "guide";
  id: number;
}

/** Something the user can point at (crates/inference pick::Entity), plus whole objects. */
export type Entity =
  | { kind: "vertex"; owner: Owner; at: V3 }
  | { kind: "edge"; owner: Owner; a: V3; b: V3; drawn: boolean }
  | { kind: "face"; object: number; face: number }
  | { kind: "region"; sketch: number; region: number }
  | { kind: "guide"; guide: number }
  | { kind: "dimension"; dimension: number }
  | { kind: "object"; object: number };

/** A plane frame (u x v = normal). */
export interface FrameData {
  origin: V3;
  normal: V3;
  u: V3;
  v: V3;
}

export interface SurfaceHit {
  at: { kind: "face"; object: number; face: number } | { kind: "region"; sketch: number; region: number };
  frame: FrameData;
  point: V3;
  t: number;
}

/** Result of a pick (crates/inference pick::Picked). */
export interface Picked {
  entity: Entity;
  point: V3;
  surface: SurfaceHit | null;
}

/** World size of a screen pixel (crates/inference geom::PixelModel). */
export interface PixelModel {
  ortho: boolean;
  scale: number;
  eye: V3;
  forward: V3;
}

/** An inference line to draw (crates/inference infer::InfLine). */
export interface InfLine {
  from: V3;
  to: V3;
  kind: import("../lib/inferenceStyle").InferenceKind;
}

/** Where the cursor snaps (crates/inference infer::Inference). */
export interface Inference {
  kind: import("../lib/inferenceStyle").InferenceKind;
  point: V3;
  tip: string;
  entity: Entity | null;
  surface: SurfaceHit | null;
  lines: InfLine[];
  /** The nearest edge under the cursor (kept as a parallel reference). */
  edge: [V3, V3] | null;
  picked: Picked | null;
  /** For a snap on a solid's edge or corner: the face beside it facing the eye most. */
  adjacent: SurfaceHit | null;
}

/** An inference request (crates/inference infer::InferQuery). */
export interface InferQuery {
  origin: V3;
  dir: V3;
  px: PixelModel;
  start?: V3 | null;
  plane?: { origin: V3; normal: V3 } | null;
  lock?: { dir: V3; kind: import("../lib/inferenceStyle").InferenceKind } | null;
  refs?: [V3, V3][];
  anchors?: V3[];
  grid?: boolean;
  grid_step?: [number, number];
  point_px?: number;
  line_px?: number;
}

export interface PickQuery {
  origin: V3;
  dir: V3;
  px: PixelModel;
  point_px?: number;
  edge_px?: number;
}

/** A pickable part: an object's face (index = persistent face id) or a sketch region (index = region id). */
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
  /** One per edge segment: index into `edgeOwners`. */
  edgeOwner: Uint32Array;
  edgeOwners: { kind: "object" | "sketch"; id: number }[];
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
  edge_owner(): Uint32Array;
  edge_owners_json(): string;
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
  pick(query: string): string;
  infer(query: string): string;
  faces_json(object: number): string;
  outline(kind: string, owner: number, index: number): string;
  offset_preview(query: string): string;
  export_stl(object: number): Uint8Array;
  export_obj(object: number): string;
  free(): void;
}

export interface WasmModule {
  default: (input?: unknown) => Promise<unknown>;
  document_new(): WasmDoc;
  /** A document rebuilt by replaying an op log (JSON). Throws on a refused entry. */
  document_replay(opsJson: string): WasmDoc;
  kernel_version(): string;
}

export type RequestBody =
  | { kind: "version" }
  | { kind: "init" }
  | { kind: "replay"; ops: KernelOp[] }
  | { kind: "scene" }
  | { kind: "apply"; op: KernelOp }
  | { kind: "undo" }
  | { kind: "redo" }
  | { kind: "log" }
  | { kind: "pick"; query: PickQuery }
  | { kind: "infer"; query: InferQuery }
  | { kind: "outline"; part: "face" | "region"; owner: number; index: number }
  | { kind: "offsetPreview"; source: OffsetSource; distance: number }
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
      picked?: Picked | null;
      inference?: Inference | null;
      rings?: V3[][];
      preview?: OffsetPreview;
      file?: Uint8Array | string;
    }
  | { id: number; ok: false; error: string; missing?: boolean };
