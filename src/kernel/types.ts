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
  | { op: "delete_object"; object: number }
  | { op: "create_group"; name: string; objects: number[] }
  | { op: "delete_group"; group: number }
  | { op: "set_tag"; object: number; tag: string | null }
  | { op: "set_tag_visible"; tag: string; visible: boolean }
  | { op: "import"; items?: ImportItem[]; strokes?: ImportStroke[]; materials?: MaterialDef[]; scenes?: SceneDef[]; hidden_tags?: string[] }
  | { op: "define_material"; name: string; color: [number, number, number, number]; texture?: string | null }
  | { op: "set_material"; object: number; material: string | null }
  | { op: "rename_object"; object: number; name: string | null }
  | { op: "delete_mesh"; mesh: number }
  | { op: "undo" }
  | { op: "redo" };

/** One imported polygon soup (crates/kernel/src/meshbuild.rs `ImportItem`): metres, Z up. */
export interface ImportItem {
  name: string;
  group?: string | null;
  tag?: string | null;
  material?: string | null;
  positions: [number, number, number][];
  /** Faces as rings of indices: the outer ring, then holes. */
  faces: number[][][];
  lines?: [number, number][];
  /** Merge adjacent coplanar faces (triangle soups); false keeps drawn dividing edges. */
  merge_coplanar?: boolean;
}

/** A stroke for a sketch (DXF lines, SVG paths): world points on one plane. */
export interface ImportStroke {
  points: [number, number, number][];
  closed?: boolean;
}

export interface MaterialDef {
  name: string;
  /** sRGB + alpha, 0..255. */
  color: [number, number, number, number];
  texture?: string | null;
}

/** A saved view (SketchUp scene): camera in metres, Z up. */
export interface SceneDef {
  name: string;
  eye: V3;
  target: V3;
  up: V3;
  fov?: number;
  parallel?: boolean;
  hidden_tags?: string[];
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
  | { kind: "imported"; objects: number[]; meshes: number[]; groups: number[]; sketches: number[]; notes: string[] }
  | { kind: "material_set" }
  | { kind: "renamed" }
  | { kind: "mesh_deleted" }
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
  /** Every sketch with its plane frame (world <-> sketch coordinates). */
  sketches: SketchInfo[];
  groups: { id: number; name: string; objects: number[] }[];
  /** Guide lines (dir set) and guide points (dir null). */
  guides: { id: number; point: V3; dir: V3 | null }[];
  dimensions: { id: number; a: V3; b: V3; offset: V3; length: number }[];
  tags: Record<string, boolean>;
  /** Name, tag and material of every object (same order as `objects`). */
  object_info: { id: number; name: string | null; tag: string | null; material: string | null }[];
  /** Imported meshes that are not valid solids, and why. */
  meshes: { id: number; name: string; note: string; faces: number; lines: number; tag: string | null; visible: boolean }[];
  materials: { name: string; color: [number, number, number, number]; texture: string | null }[];
  scenes: SceneDef[];
}

export type V3 = [number, number, number];

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
  kind: "face" | "region" | "mesh";
  owner: number;
  index: number;
  origin: [number, number, number];
  normal: [number, number, number];
  u: [number, number, number];
  v: [number, number, number];
  /** The owner's material colour, sRGB + alpha. */
  color?: [number, number, number, number];
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
  edgeOwners: { kind: "object" | "sketch" | "mesh"; id: number }[];
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
  export_stl(object: number): Uint8Array;
  export_obj(object: number): string;
  export_file(format: string, options: string): Uint8Array;
  import_file(name: string, bytes: Uint8Array, options: string): string;
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
  | { kind: "pick"; query: PickQuery }
  | { kind: "infer"; query: InferQuery }
  | { kind: "outline"; part: "face" | "region"; owner: number; index: number }
  | { kind: "export"; format: "stl" | "obj"; object: number }
  | { kind: "exportFile"; format: ExportFormat; options: ExportOptions }
  | { kind: "importFile"; name: string; bytes: Uint8Array; options: ImportOptions }
  | { kind: "importSkp"; name: string; bytes: Uint8Array };
export type WorkerRequest = RequestBody & { id: number };

/** Formats the kernel writes (crates/wasm-api `DocCore::export`). */
export type ExportFormat = "stl" | "3mf" | "glb" | "obj" | "mtl" | "dxf" | "dxf-outline" | "svg-view" | "svg-outline";

/** The camera of an SVG view drawing (crates/export svg::ViewSpec). */
export interface ViewSpec {
  eye: V3;
  target: V3;
  up: V3;
  ortho: boolean;
  fov: number;
  half_height: number;
  width: number;
  height: number;
}

/** Units for files that do not say (STL, OBJ, a DXF without $INSUNITS). */
export type ImportUnit = "mm" | "cm" | "m" | "in" | "ft";

export interface ImportOptions {
  units?: ImportUnit;
  name?: string;
}

export interface ExportOptions {
  scope?: { objects: number[]; meshes: number[] };
  view?: ViewSpec;
  outline?: { kind: "face" | "region"; owner: number; index: number };
  flavour?: "lwpolyline" | "r12";
  mtl_file?: string;
}

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
      file?: Uint8Array | string;
      /** Reader notes from an import (units assumed, entities skipped). */
      notes?: string[];
    }
  | { id: number; ok: false; error: string; missing?: boolean };
