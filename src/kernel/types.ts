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
  | ({ op: "define_material" } & MaterialDef)
  // Lane 10: materials, paint, texture positioning, styles and shadows (crates/kernel/src/looks.rs).
  | { op: "rename_material"; name: string; to: string }
  | { op: "delete_material"; name: string }
  /** A texture image (PNG or JPEG) under assets/<name>.png|.jpg; `data` is base64. */
  | { op: "add_texture"; path: string; mime: string; data: string }
  | { op: "paint"; target: PaintTarget; material: string | null }
  | { op: "set_face_uv"; object: number; face: number; uv: UvXform | null }
  | { op: "set_style"; style: Style | null }
  | { op: "set_shadows"; shadows: Shadows | null }
  | { op: "set_material"; object: number; material: string | null }
  | { op: "rename_object"; object: number; name: string | null }
  | { op: "delete_mesh"; mesh: number }
  /** Cut list (lane 7): which way a part's grain runs ("length" clears the override). */
  | { op: "set_grain"; part: PartRef; grain: Grain }
  /** Cut list: the stock a part is cut from (null: its material's name) and sheet or lumber (null: decide). */
  | { op: "set_stock"; part: PartRef; stock: string | null; sheet: boolean | null }
  /** Cut list (lane 15): count a part, or drop it from the totals and the nesting (still listed). */
  | { op: "set_include"; part: PartRef; include: boolean }
  /** Lane 15: each edge-connected piece of loose non-solid faces into a group of its own (none named: every mesh in the open context). */
  | { op: "group_loose"; meshes: number[] }
  /** Lane 15: soft (1), smooth (2), hidden (4) on edges named by owner and end points. */
  | { op: "set_edge_flags"; edges: EdgeAt[]; set: number; clear: number }
  /** Lane 15: take the paint off what the items hold (and off solids named), so what holds them shows. */
  | { op: "clear_paint"; items: Item[] }
  /** Lane 15: soften by angle (degrees) every edge of these solids and meshes. */
  | { op: "soften_edges"; objects: number[]; meshes?: number[]; angle: number; soft: boolean; smooth: boolean }
  /** Nesting (lane 9): add a stock, or replace the stock named `replace` (a rename carries the defaults). */
  | { op: "define_stock"; stock: StockDef; replace?: string | null }
  | { op: "delete_stock"; name: string }
  /** Which stock a material's parts come from (null: chosen automatically). */
  | { op: "set_material_stock"; material: string; stock: string | null }
  | { op: "set_nest_settings"; settings: NestSettings }
  | { op: "reset_stock_library" }
  // Lane 12: section planes (crates/kernel/src/section.rs). `normal` points at the side kept.
  | { op: "add_section"; origin: V3; normal: V3; name?: string | null; active?: boolean; hidden?: boolean }
  | { op: "place_section"; section: number; origin: V3; normal: V3 }
  | { op: "flip_section"; section: number }
  | { op: "set_section_active"; section: number; active: boolean }
  | { op: "rename_section"; section: number; name: string }
  | { op: "set_section_hidden"; section: number; hidden: boolean }
  | { op: "delete_section"; section: number }
  | { op: "move_sections"; sections: number[]; xform: Xform }
  | { op: "copy_sections"; sections: number[]; xform: Xform; count?: number }
  /** Create Group from Slice: the cut's outline as a sketch in a new group. */
  | { op: "slice_to_group"; section: number; name?: string | null }
  // Lane 12: drawing sheets (src/kernel/sheetTypes.ts).
  | import("./sheetTypes").SheetOp
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
/** Lane 15: one edge of a solid or a mesh, by its end points (world metres). */
export interface EdgeAt {
  object?: number;
  mesh?: number;
  a: V3;
  b: V3;
}

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
  /** Lane 10: each face's material (null: the item's). */
  face_materials?: (string | null)[];
  /** Lane 10: each face's texture mapping, two affine rows in world metres (u, v in repeats). */
  face_uvs?: ([number[], number[]] | null)[];
  /** Lane 15: soft (1), smooth (2) and hidden (4) edges as [index, index, flags]. */
  edge_flags?: [number, number, number][];
}

/** A component definition in an import: geometry in its own coordinates, plus nested groups
 * and instances (crates/kernel/src/model.rs `ImportDefinition`). */
export interface ImportDefinition {
  name: string;
  items: ImportItem[];
  children?: ImportNode[];
  /** Section planes drawn in it (its own coordinates); each placed copy gets them. */
  sections?: ImportSection[];
}

/** A group (geometry in the enclosing frame) or an instance of an import definition (index,
 * placed by `xform` in the enclosing frame). */
/** A section plane in an import (lane 12): it comes in inactive. */
export interface ImportSection {
  origin: V3;
  /** The side kept. */
  normal: V3;
  name?: string | null;
  hidden?: boolean;
}

export type ImportNode = { name?: string | null; tag?: string | null; hidden?: boolean; sections?: ImportSection[] } & (
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
  /** A texture the model holds (assets/...), or the file name of one it does not. */
  texture?: string | null;
  /** Texture width and height, metres (absent: one foot across at the image's aspect). */
  size?: [number, number] | null;
  /** Texture rotation, degrees. */
  rotation?: number;
  /** Texture offset along the face, metres. */
  offset?: [number, number];
  /** What it is as stock: the cut list reads the species from it. */
  stock?: StockLink | null;
}

/** Lumber or sheet goods, and the species or product in words (crates/kernel/src/looks.rs). */
export interface StockLink {
  kind: "lumber" | "sheet";
  species?: string;
}

/** How a texture sits on one face, on top of its material's own placement. */
export interface UvXform {
  /** Multiplies the material's size (absent: [1, 1]). */
  scale?: [number, number];
  /** Degrees, counter-clockwise seen from the face's front. */
  rotation?: number;
  /** Metres, in the face's texture frame. */
  offset?: [number, number];
}

/** What a paint op paints. */
export type PaintTarget = { on: "faces"; object: number; faces: number[] } | { on: "items"; items: Item[] };

export type FaceStyle = "textured" | "shaded" | "monochrome" | "hidden_line" | "wireframe" | "x_ray";

export interface EdgeStyle {
  /** Edges drawn at all (absent: true). */
  edges?: boolean;
  profiles?: boolean;
  extensions?: boolean;
  endpoints?: boolean;
  /** Lane 15: soft and hidden edges drawn dashed. */
  soft?: boolean;
}

/** A drawing style (crates/kernel/src/looks.rs `Style`). Colours are sRGB [r, g, b]. */
export interface Style {
  preset?: string | null;
  face?: FaceStyle;
  edges?: EdgeStyle;
  /** Absent: the theme's background. */
  background?: [number, number, number] | null;
  sky?: [number, number, number] | null;
  ground?: [number, number, number] | null;
  /** Active section planes cut the model (lane 12; absent: true). */
  section_cuts?: boolean;
  /** Section planes drawn (absent: true). */
  section_planes?: boolean;
  /** Cut faces filled (absent: true). */
  section_fill?: boolean;
  section_fill_color?: [number, number, number] | null;
  section_line_color?: [number, number, number] | null;
  /** Screen pixels, 1..12 (absent: 3). */
  section_line_width?: number | null;
}

/** The sun and its shadows (crates/kernel/src/looks.rs `Shadows`): local solar time, north +Y. */
export interface Shadows {
  on: boolean;
  month: number;
  day: number;
  /** Minutes after midnight. */
  minutes: number;
  latitude: number;
  /** Brightness of sunlight and of shade, 0..100. */
  light: number;
  dark: number;
  ground: boolean;
  faces: boolean;
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
  /** The style and shadows saved with the scene (lane 10); activating it brings them back. */
  style?: Style | null;
  shadows?: Shadows | null;
  /** The section planes active and hidden when it was saved (lane 12). */
  sections?: { active?: number[]; hidden?: number[] } | null;
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
  | { kind: "texture_added"; path: string }
  | { kind: "style_set" }
  | { kind: "renamed" }
  | { kind: "mesh_deleted" }
  | { kind: "part_set" }
  | { kind: "loose_grouped"; groups: number[] }
  | { kind: "edges_set"; changed: number }
  | { kind: "paint_cleared"; cleared_faces: number; cleared_items: number }
  | { kind: "stock_set" }
  | { kind: "restored"; objects: number; meshes: number; sketches: number }
  | { kind: "section_added"; section: number }
  | { kind: "section_set" }
  | { kind: "section_deleted" }
  | { kind: "sections_moved"; sections: number[] }
  | { kind: "sections_copied"; sections: number[] }
  | { kind: "sliced"; group: number; sketch: number }
  | { kind: "sheet_added"; sheet: number }
  | { kind: "sheet_item_added"; sheet: number; item: number }
  | { kind: "sheet_set" }
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
  materials: MaterialInfo[];
  scenes: SceneDef[];
  /** Texture images the model holds (lane 10). */
  textures?: TextureInfo[];
  /** The drawing style and the sun (null: the defaults). */
  style?: Style | null;
  shadows?: Shadows | null;
}

/** A material as the Materials pane shows it (crates/wasm-api `MaterialInfo`). */
export interface MaterialInfo {
  name: string;
  color: [number, number, number, number];
  texture: string | null;
  /** The texture is an image the model holds (not just a file name). */
  held?: boolean;
  /** Texture size as drawn, metres. */
  size?: [number, number];
  size_set?: boolean;
  rotation?: number;
  offset?: [number, number];
  stock?: StockLink | null;
  /** Faces showing it. */
  used?: number;
}

export interface TextureInfo {
  path: string;
  mime: string;
  width: number;
  height: number;
  /** Content digest: the worker sends each image's bytes once, by it. */
  digest: string;
}

/** A material of the built-in library (crates/export/src/library.rs `LibraryEntry`). */
export interface LibraryEntry {
  name: string;
  category: "Wood" | "Sheet goods" | "Colours" | "Glass and metal";
  color: [number, number, number, number];
  texture: string | null;
  size: [number, number] | null;
  stock: StockLink | null;
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
  /** Part of a component definition (inside an instance). */
  in_component: boolean;
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
  /** Section planes (lane 12), in id order. */
  sections?: GraphSection[];
}

/** A section plane (crates/kernel/src/nodes.rs `GraphSection`). */
export interface GraphSection {
  id: number;
  name: string;
  parent: number | null;
  active: boolean;
  hidden: boolean;
  /** Drawn: not hidden and its context shown. */
  visible: boolean;
  origin: V3;
  /** The side kept. */
  normal: V3;
  u: V3;
  v: V3;
  /** The rectangle it is drawn as, in its frame: [x0, y0, x1, y1], metres. */
  extent: [number, number, number, number];
}

/** What the viewport needs for section cuts (crates/wasm-api drawings.rs `SectionView`). */
export interface SectionView {
  cuts: { section: number; positions: number[]; indices: number[]; outline: number[]; area: number }[];
  /** Planes as [nx, ny, nz, c] keeping n . p + c >= 0, and what each set clips. */
  clips: { planes: [number, number, number, number][]; objects: number[]; meshes: number[]; sketches: number[] }[];
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
  | { kind: "edge"; owner: Owner; a: V3; b: V3; drawn: boolean; /** Lane 15: soft 1, smooth 2, hidden 4. */ flags?: number }
  | { kind: "face"; object: number; face: number }
  | { kind: "region"; sketch: number; region: number }
  | { kind: "guide"; guide: number }
  | { kind: "dimension"; dimension: number }
  | { kind: "object"; object: number }
  /** A group or component instance, picked or selected as one (lane 5). */
  | { kind: "node"; node: number }
  /** A section plane (lane 12). */
  | { kind: "section"; section: number };

/** A plane frame (u x v = normal). */
export interface FrameData {
  origin: V3;
  normal: V3;
  u: V3;
  v: V3;
}

/** A surface hit as tools see it: a face or a sketch region. (The kernel also answers with the
 * cap a section plane cuts through a solid, lane 12: the tool driver moves those to
 * `Snap.cap` and leaves `surface` empty, so no tool draws on or pushes a cap.) */
export interface SurfaceHit {
  at: { kind: "face"; object: number; face: number } | { kind: "region"; sketch: number; region: number };
  frame: FrameData;
  point: V3;
  t: number;
}

/** The cap a section plane cuts through a solid, under the cursor (lane 12). */
export interface CapHit {
  object: number;
  section: number;
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
  plane?: { origin: V3; normal: V3   /** Lane 15: the pick may be a soft or hidden edge (the Eraser with Ctrl+Shift). */
  soft?: boolean;
} | null;
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
  /** The colour of the material the face shows (its own, its solid's or its container's), sRGB + alpha. */
  color?: [number, number, number, number];
  /** The name of that material (lane 10). */
  material?: string;
  /** The face has a material of its own (not inherited). */
  own?: boolean;
  /** How a texture sits on the face, when positioned. */
  uv?: UvXform;
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
  /** Texture coordinates, two per render vertex (lane 10; zero on untextured faces). */
  uvs?: Float32Array;
  /** The two face normals at each edge, six per edge segment (zero where there is none). */
  edgeNormals?: Float32Array;
  /** Lane 15: soft (1), smooth (2) and hidden (4) flags, one per edge segment. */
  edgeFlags?: Uint8Array;
  /** Lane 15: shading normals, three per render vertex, only when some edge is smooth. */
  normals?: Float32Array;
}

/** A texture image the UI has not seen yet (sent once per digest, transferred). */
export interface TextureBlob {
  path: string;
  digest: string;
  mime: string;
  bytes: Uint8Array;
}

export interface SceneData {
  mesh: MeshData;
  state: DocState;
  /** Texture images new to the UI since the last scene (lane 10). */
  textures?: TextureBlob[];
  /** Section cuts to draw (lane 12; absent when the model has no section plane). */
  sections?: SectionView;
  /** The drawing sheets, in order (lane 12). */
  sheets?: import("./sheetTypes").SheetRow[];
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
  uvs(): Float32Array;
  edge_normals(): Float32Array;
  edge_flags(): Uint8Array;
  normals(): Float32Array;
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
  /** The cut list (lane 7) for CutOptions JSON, as JSON (crates/kernel/src/cutlist.rs). */
  cut_list(options: string): string;
  /** The nesting (lane 9) for NestOptions JSON, as JSON (crates/kernel/src/nesting.rs). */
  nest(options: string): string;
  import_file(name: string, bytes: Uint8Array, options: string): string;
  /** Lane 11: a named batch whose ops may hold placeholders ({"$ref": i, "path": "..."}), applied as one op (crates/kernel/src/refbatch.rs). */
  apply_named_batch(json: string): string;
  /** Lane 16: `pluginData` is written as plugin-data.json when not empty. */
  save_shz(displayUnits: string, thumbnail: Uint8Array, pluginData?: string): Uint8Array;
  open_shz(bytes: Uint8Array): string;
  /** Lane 16: entries in the op log, and the log from `from` on, one JSON text per op. */
  op_log_len(): number;
  /** Lane 16: how many times the op log was compacted. */
  op_log_epoch(): number;
  op_log_tail(from: number): string[];
  /** Lane 16, tests only: fail mid-edit the way a kernel bug would (a panic: a trap). */
  fail_for_test(how: string): string;
  /** The material a face shows (the Paint tool's sample), as JSON. */
  sample(object: number, face: number): string;
  /** The bytes of a texture image the model holds. */
  texture(path: string): Uint8Array;
  /** Texture coordinates a face would get with a positioning (live preview). */
  face_uvs(object: number, face: number, uvJson: string): Float32Array;
  /** What the viewport draws for section cuts (lane 12), as JSON. */
  section_view(): string;
  /** The drawing sheets in order (lane 12), as JSON. */
  sheets(): string;
  /** A sheet laid out on paper (lane 12), as JSON. */
  sheet_render(sheet: number): string;
  /** Where a cursor snaps on a sheet viewport's drawing (lane 12), as JSON. */
  sheet_snap(sheet: number, item: number, x: number, y: number, radius: number): string;
  /** How a viewport item would draw: the factor that fits it, its drawing's size (lane 12). */
  viewport_fit(item_json: string): string;
  /** A sheet as DXF in its own units (lane 12). */
  sheet_dxf(sheet: number): string;
  free(): void;
}

export interface WasmModule {
  default: (input?: unknown) => Promise<unknown>;
  document_new(): WasmDoc;
  /** A document rebuilt by replaying an op log (JSON). Throws on a refused entry. */
  document_replay(opsJson: string): WasmDoc;
  kernel_version(): string;
  /** The built-in material library, as JSON. */
  material_library(): string;
  /** The ops that put a library material into a model (texture + definition), as JSON. */
  material_library_ops(name: string, asName: string): string;
  material_library_texture(name: string): Uint8Array;
  /** A small swatch of a library material's texture (96 x 48 PNG). */
  material_library_swatch(name: string): Uint8Array;
}

export type RequestBody =
  | { kind: "version" }
  /** Lane 16b: the kernel's WebAssembly memory in bytes (it only grows: what a request allocated at its peak). */
  | { kind: "memory" }
  | { kind: "init" }
  /** An op log to rebuild from: the ops, or (lane 16) their JSON as one text. */
  | { kind: "replay"; ops?: KernelOp[]; json?: string }
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
  | { kind: "importFile"; name: string; bytes: Uint8Array; options: ImportOptions; failForTest?: string }
  | { kind: "importSkp"; name: string; bytes: Uint8Array; failForTest?: string }
  | { kind: "saveShz"; displayUnits: string; thumbnail: Uint8Array; pluginData: string }
  | { kind: "openShz"; bytes: Uint8Array }
  | { kind: "cutList"; options: CutOptions }
  | { kind: "nest"; options: NestOptions }
  | { kind: "sample"; object: number; face: number }
  | { kind: "faceUvs"; object: number; face: number; uv: UvXform }
  | { kind: "library" }
  | { kind: "libraryOps"; name: string; asName: string }
  | { kind: "libraryTexture"; name: string }
  | { kind: "texture"; path: string }
  /** Lane 11 (plugins): a solid's faces by persistent id; a named batch with placeholders, one undo step. */
  | { kind: "faces"; object: number }
  | { kind: "namedBatch"; name: string; ops: unknown[] }
  // Lane 12: drawing sheets (read-only).
  | { kind: "sheetRender"; sheet: number }
  | { kind: "sheetSnap"; sheet: number; item: number; x: number; y: number; radius: number }
  | { kind: "viewportFit"; item: import("./sheetTypes").ItemKind }
  | { kind: "sheetDxf"; sheet: number };
/** `known`: how much of the op log the app holds (lane 16): the answer brings the rest. */
export type WorkerRequest = RequestBody & { id: number; known?: number };

/** Formats the kernel writes (crates/wasm-api `DocCore::export`). */
export type ExportFormat = "stl" | "3mf" | "glb" | "obj" | "mtl" | "dxf" | "dxf-outline" | "svg-view" | "svg-outline" | "svg-section" | "dxf-section";

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
  /** The section plane a slice export draws (lane 12). */
  section?: number;
}

export type WorkerResponse =
  | {
      id: number;
      ok: true;
      version?: string;
      /** Lane 16b: the kernel's WebAssembly memory, bytes. */
      wasmBytes?: number;
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
      /** What opening a .shz found: the unit system it was saved in, who wrote it, and (lane 16)
       * its plugin-data.json as text (read by the kernel, capped). */
      opened?: { display_units: string | null; generator: string; plugin_data?: string | null };
      /** Lane 16: the op log entries the app does not have yet (from `from` on), one JSON text
       * per op: what it keeps to rebuild the document in a fresh worker. */
      logSync?: { from: number; tail: string[] };
      /** Lane 15: the .skp was empty (nothing drawn): its units and style; nothing changed. */
      emptySkp?: { units: string | null; unitsWord: string | null; style: { name: string; face: FaceStyle | null } | null };
      cutList?: CutList;
      nesting?: Nesting;
      /** A sampled material (null: the default look). */
      material?: string | null;
      uvs?: Float32Array;
      library?: LibraryEntry[];
      ops?: KernelOp[];
      /** A sheet laid out (lane 12). */
      sheetRender?: import("../sheets/render").SheetRender;
      /** A sheet snap (lane 12; null: nothing near). */
      sheetSnap?: import("../sheets/render").SheetSnap | null;
      /** A viewport's fit (lane 12). */
      viewportFit?: import("../sheets/render").ViewportFit;
      /** A sheet as DXF text (lane 12). */
      sheetDxf?: string;
      faces?: FaceInfo[];
    }
  /** `fatal` (lane 16): the kernel itself failed (a trap, out of memory): this worker is done for. */
  | { id: number; ok: false; error: string; missing?: boolean; fatal?: boolean };

/** One face of a solid (Doc.faces_json): persistent id, plane and size. */
export interface FaceInfo {
  id: number;
  normal: V3;
  origin: V3;
  centroid: V3;
  area: number;
}

// ---- cut list (lane 7; crates/kernel/src/cutlist.rs) --------------------------------------

export type Grain = "length" | "width" | "none";
export type Rounding = "sixteenth" | "thirty_second" | "half_mm";

/** What a grain or stock op names: a component (its definition), a group or a solid. */
export type PartRef = { definition: number } | { node: number } | { object: number };

/** Which part a row is. */
export type PartKey = { definition: number } | { node: number } | { object: number } | { member: { definition: number; slot: number } };

export interface CutOptions {
  rounding: Rounding;
  /** Count only what is drawn. */
  visible_only: boolean;
}

/** A part's minimal box: thickness, width and length directions (world) and sizes (metres). */
export interface PartBox {
  axes: [V3, V3, V3];
  size: V3;
  center: V3;
  shaped: boolean;
}

export interface Occurrence extends PartBox {
  item: Item;
  visible: boolean;
  handed: boolean;
}

export interface CutRow {
  key: PartKey;
  target: PartRef;
  name: string;
  qty: number;
  total: number;
  hidden: number;
  handed: number;
  /** Thickness, width, length (metres, exact). */
  size: V3;
  rounded: V3;
  /** Rounded sizes in rounding steps (sixteenths, thirty-seconds or half millimetres). */
  steps: [number, number, number];
  shaped: boolean;
  scaled: boolean;
  /** Lane 15: not a watertight solid even after the heal: sized from the box around its faces. */
  not_solid: boolean;
  /** Lane 15: counted in the totals and nested (false: dropped by the person, still listed). */
  included: boolean;
  /** Lane 15 fixes: listed but counted nowhere and not nested: a flat face with no
   * thickness, or open faces thicker than any board (the whole of something, not a board).
   * Lane 20: also a whole piece drawn as one block (thicker than any board and over 12 in
   * both ways), a solid thicker than any board more than 30% empty, and lumber wider than
   * 4 ft. */
  uncounted?: Uncounted;
  /** Lane 15: the nominal (rough) thickness board feet are counted on, metres (4/4: 1 in). */
  nominal_thickness?: number;
  /** Lane 15: that board's nominal ("4/4"), when a library board gave it. */
  nominal?: string;
  grain: Grain;
  grain_set: boolean;
  material: string | null;
  stock_set: boolean;
  sheet: boolean;
  sheet_set: boolean;
  board_feet: number;
  area_m2: number;
  occurrences: Occurrence[];
}

/** Lane 15 fixes: why a listed part counts nowhere. */
export type Uncounted = "flat" | "not_a_board" | "whole_piece" | "hollow" | "too_wide";

/** Lane 15 fixes: a group or instance holding faces outside any part. */
export interface LoosePlace {
  node: number | null;
  name: string;
  faces: number;
}

export interface CutTotals {
  parts: number;
  /** Pieces counted (dropped rows left out). */
  pieces: number;
  /** Lane 15: rows dropped from the totals. */
  excluded: number;
  /** Lane 15: rows that are not solids (sized from their bounds). */
  not_solid: number;
  /** Lane 15: faces of non-solid geometry outside any part (never listed). */
  loose_faces: number;
  /** Lane 15 fixes: of those, the faces in the open context (what Make parts from loose faces acts on). */
  loose_here: number;
  /** Lane 15 fixes: where the rest are (a group or instance; `node` null: the top level). */
  loose_places: LoosePlace[];
  /** Lane 15 fixes: rows listed but counted nowhere ({@link CutRow.uncounted}). */
  uncounted: number;
  hidden: number;
  board_feet: number;
  lumber: { material: string | null; board_feet: number }[];
  sheets: { material: string | null; thickness: number; steps: number; area_m2: number; sheets: number; oversize: boolean }[];
  sheet_count: number;
}

export interface CutList {
  rounding: Rounding;
  visible_only: boolean;
  rows: CutRow[];
  totals: CutTotals;
  /** Width and length of the sheet the rough count assumes (metres). */
  sheet_size: [number, number];
  hash: string;
  /** How the app writes the sizes (set by the app from the display units, never by the
   * kernel; absent: fractions for inch roundings). Display only, like the rounding. */
  style?: "fraction" | "decimal" | "mm";
}

// ---- nesting (lane 9; crates/kernel/src/stock.rs, nesting.rs) -------------------------------

export type StockKind = "sheet" | "board";
export type StockGrain = "length" | "none";
export type NestAlgorithm = "guillotine" | "maxrects";

/** One kind of stock (metres; `length` runs along the grain). */
export interface StockDef {
  name: string;
  kind: StockKind;
  length: number;
  width: number;
  thickness: number;
  /** A board's nominal thickness ("4/4"). */
  nominal?: string | null;
  grain: StockGrain;
}

export interface NestSettings {
  /** Width of every cut (metres). */
  kerf: number;
  /** Taken off each edge of a sheet, each end of a board (metres). */
  trim: number;
  algorithm: NestAlgorithm;
}

export interface StockLibrary {
  stocks: StockDef[];
  /** Material -> stock name. */
  defaults?: Record<string, string>;
  settings: NestSettings;
}

export interface NestOptions {
  visible_only?: boolean;
  algorithm?: NestAlgorithm;
}

export type CutDir = "rip" | "crosscut";

export interface NestCut {
  n: number;
  dir: CutDir;
  /** Where the kerf starts (y for a rip, x for a crosscut), metres from the corner. */
  at: number;
  from: number;
  to: number;
  /** The fence setting: the width (rip) or length (crosscut) left on the near side; the trim for a trim cut. */
  size: number;
  trim: boolean;
}

export interface Placement {
  row: number;
  copy: number;
  tag: string;
  name: string;
  item: Item;
  x: number;
  y: number;
  dx: number;
  dy: number;
  turned: boolean;
  grain: Grain;
  size: V3;
  /** Lane 15: its part is not a solid (the rectangle is the box around its faces). */
  not_solid?: boolean;
}

export interface StockPiece {
  index: number;
  group: number;
  seq: number;
  count: number;
  stock: string;
  kind: StockKind;
  nominal?: string;
  material: string | null;
  length: number;
  width: number;
  thickness: number;
  grain: StockGrain;
  trim: number;
  kerf: number;
  /** x0, y0, x1, y1 inside the trim. */
  usable: [number, number, number, number];
  placements: Placement[];
  cuts: NestCut[];
  guillotine: boolean;
  parts_area: number;
  stock_area: number;
  waste_area: number;
  efficiency: number;
}

export type UnplacedReason = "no_stock" | "thickness" | "too_large" | "grain" | "flat" | "not_a_board" | "whole_piece" | "hollow" | "too_wide";

export interface Unplaced {
  row: number;
  copy: number;
  tag: string;
  name: string;
  item: Item;
  size: V3;
  material: string | null;
  stock: string | null;
  sheet: boolean;
  reason: UnplacedReason;
}

export interface NestGroup {
  stock: string;
  kind: StockKind;
  nominal?: string;
  material: string | null;
  length: number;
  width: number;
  thickness: number;
  count: number;
  parts: number;
  parts_area: number;
  stock_area: number;
  efficiency: number;
  board_feet: number;
}

export interface Nesting {
  algorithm: NestAlgorithm;
  settings: NestSettings;
  visible_only: boolean;
  library: StockLibrary;
  pieces: StockPiece[];
  unplaced: Unplaced[];
  totals: {
    sheets: number;
    boards: number;
    parts_in: number;
    placed: number;
    unplaced: number;
    parts_area: number;
    stock_area: number;
    efficiency: number;
    groups: NestGroup[];
    by_nominal: { nominal: string; count: number; board_feet: number }[];
  };
  hash: string;
}
