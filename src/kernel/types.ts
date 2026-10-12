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
  | { op: "transform"; objects: number[]; xform: Xform; nodes?: number[] }
  | { op: "copy"; objects: number[]; xform: Xform; nodes?: number[] }
  | { op: "array"; objects: number[]; xform: Xform; count: number; nodes?: number[] }
  | { op: "transform_geometry"; object: number; points: V3[]; xform: Xform; autofold: boolean }
  | { op: "offset"; source: OffsetSource; distance: number }
  | { op: "follow_me"; profile: Profile; path: V3[]; closed: boolean }
  | { op: "delete_object"; object: number }
  | { op: "create_group"; name: string; objects: number[] }
  | { op: "delete_group"; group: number }
  | { op: "set_tag"; object: number; tag: string | null }
  | { op: "set_tag_visible"; tag: string; visible: boolean }
  // Lane 5: groups, components, the editing context, tags, scenes.
  | { op: "set_context"; node: number | null }
  | { op: "leave_context" }
  | { op: "make_group"; items: Item[]; name?: string | null }
  | { op: "make_component"; items: Item[]; name: string }
  | { op: "make_unique"; node: number }
  | { op: "explode"; node: number }
  | { op: "delete_node"; node: number }
  | { op: "rename"; item: Item; name: string | null }
  | { op: "rename_definition"; definition: number; name: string }
  | { op: "set_hidden"; items: Item[]; hidden: boolean }
  | { op: "set_locked"; items: Item[]; locked: boolean }
  | { op: "tag_items"; items: Item[]; tag: string | null }
  | { op: "create_tag"; name: string; color?: [number, number, number] | null }
  | { op: "rename_tag"; tag: string; name: string }
  | { op: "set_tag_color"; tag: string; color: [number, number, number] | null }
  | { op: "delete_tag"; tag: string }
  | { op: "add_scene"; scene: SceneDef }
  | { op: "update_scene"; name: string; scene: SceneDef }
  | { op: "delete_scene"; name: string }
  | { op: "activate_scene"; name: string }
  | { op: "import"; items?: ImportItem[]; strokes?: ImportStroke[]; materials?: MaterialDef[]; scenes?: SceneDef[]; hidden_tags?: string[]; definitions?: ImportDefinition[]; nodes?: ImportNode[] }
  | { op: "define_material"; name: string; color: [number, number, number, number]; texture?: string | null }
  | { op: "set_material"; object: number; material: string | null }
  | { op: "rename_object"; object: number; name: string | null }
  | { op: "delete_mesh"; mesh: number }
  | { op: "undo" }
  | { op: "redo" };

/** A solid, or a group or component instance, as ops name them (crates/kernel/src/nodes.rs `Item`). */
export type Item = { object: number } | { node: number };

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

/** A component definition in an import: geometry in its own coordinates, plus nested groups
 * and instances (crates/kernel/src/model.rs `ImportDefinition`). */
export interface ImportDefinition {
  name: string;
  items: ImportItem[];
  children?: ImportNode[];
}

/** A group (geometry in the enclosing frame) or an instance of an import definition (index,
 * placed by `xform` in the enclosing frame). */
export type ImportNode = { name?: string | null; tag?: string | null; hidden?: boolean } & (
  | { kind: "group"; items: ImportItem[]; children?: ImportNode[] }
  | { kind: "instance"; definition: number; xform: Xform }
);

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
  /** The group or instance open when the scene was saved (null: the model). */
  context?: number | null;
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
  | { kind: "context_changed"; context: number | null }
  | { kind: "component_created"; instance: number; definition: number }
  | { kind: "made_unique"; definition: number }
  | { kind: "exploded"; objects: number[]; nodes: number[] }
  | { kind: "node_deleted" }
  | { kind: "visibility_set" }
  | { kind: "lock_set" }
  | { kind: "scene_set" }
  | { kind: "scene_activated"; context: number | null }
  | { kind: "tag_set" }
  | { kind: "transformed"; objects: number[]; nodes?: number[] }
  | { kind: "copied"; objects: number[]; nodes?: number[] }
  | { kind: "geometry_moved"; faces: number[] }
  | { kind: "severed"; objects: number[] }
  | { kind: "swept"; objects: number[]; cut: boolean }
  | { kind: "imported"; objects: number[]; meshes: number[]; groups: number[]; sketches: number[]; notes: string[] }
  | { kind: "material_set" }
  | { kind: "renamed" }
  | { kind: "mesh_deleted" }
  | { kind: "restored"; objects: number; meshes: number; sketches: number }
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
  /** Groups (not instances): name and the solids directly in each. */
  groups: { id: number; name: string; objects: number[] }[];
  /** The model's tree: groups, component instances and definitions, the open context. */
  graph: SceneGraph;
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

/** A group or component instance (crates/kernel/src/nodes.rs `GraphNode`). */
export interface GraphNode {
  id: number;
  kind: "group" | "instance";
  /** Its own name, if any. */
  name: string | null;
  /** What the Outliner shows: the name, else "Group" or the definition's name. */
  label: string;
  /** The definition (instances). */
  definition: number | null;
  parent: number | null;
  tag: string | null;
  hidden: boolean;
  locked: boolean;
  /** Drawn: not hidden, its tag and every ancestor shown. */
  visible: boolean;
  xform: Xform;
  /** Part of a component definition (nested inside an instance). */
  in_component: boolean;
}

/** Where a solid (or imported mesh) sits in the tree. */
export interface GraphLeaf {
  id: number;
  parent: number | null;
  hidden: boolean;
  locked: boolean;
  visible: boolean;
}

export interface GraphDefinition {
  id: number;
  name: string;
  instances: number;
  solids: number;
  /** Size in its own axes (metres): what a cut list measures. */
  min: V3;
  max: V3;
}

/** The model's tree (crates/kernel/src/nodes.rs `Graph`). */
export interface SceneGraph {
  /** The open group or instance (null: the model). */
  context: number | null;
  /** From the outermost open node down to the context. */
  path: number[];
  nodes: GraphNode[];
  objects: GraphLeaf[];
  meshes: GraphLeaf[];
  /** [sketch, parent]. */
  sketches: [number, number | null][];
  definitions: GraphDefinition[];
  /** Tag colours as #rrggbb. */
  tag_colors: Record<string, string>;
}

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
  | { kind: "object"; object: number }
  /** A group or component instance, picked or selected as one (lane 5). */
  | { kind: "node"; node: number };

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
  scene_graph(): string;
  pick(query: string): string;
  infer(query: string): string;
  faces_json(object: number): string;
  outline(kind: string, owner: number, index: number): string;
  offset_preview(query: string): string;
  export_stl(object: number): Uint8Array;
  export_obj(object: number): string;
  export_file(format: string, options: string): Uint8Array;
  import_file(name: string, bytes: Uint8Array, options: string): string;
  save_shz(displayUnits: string, thumbnail: Uint8Array): Uint8Array;
  open_shz(bytes: Uint8Array): string;
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
  | { kind: "graph" }
  | { kind: "pick"; query: PickQuery }
  | { kind: "infer"; query: InferQuery }
  | { kind: "outline"; part: "face" | "region"; owner: number; index: number }
  | { kind: "offsetPreview"; source: OffsetSource; distance: number }
  | { kind: "export"; format: "stl" | "obj"; object: number }
  | { kind: "exportFile"; format: ExportFormat; options: ExportOptions }
  | { kind: "importFile"; name: string; bytes: Uint8Array; options: ImportOptions }
  | { kind: "importSkp"; name: string; bytes: Uint8Array }
  | { kind: "saveShz"; displayUnits: string; thumbnail: Uint8Array }
  | { kind: "openShz"; bytes: Uint8Array };
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
      graph?: SceneGraph;
      picked?: Picked | null;
      inference?: Inference | null;
      rings?: V3[][];
      preview?: OffsetPreview;
      file?: Uint8Array | string;
      /** Reader notes from an import (units assumed, entities skipped). */
      notes?: string[];
      /** What opening a .shz found: the unit system it was saved in, who wrote it. */
      opened?: { display_units: string | null; generator: string };
    }
  | { id: number; ok: false; error: string; missing?: boolean };
