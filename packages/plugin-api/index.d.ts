// Types for the Sawhorse plugin API, version 1: the "sawhorse" module a plugin imports.
//
//   // @ts-check
//   import { model, ui, units } from "sawhorse";
//
// Lengths are metres, Z is up, angles are degrees. Every call names the permission it needs
// (plugin.json "permissions"); the app refuses a call the plugin was not granted, with an
// error whose `code` is "permission". Calls that talk to the app return promises; units and
// geometry are local and synchronous. The authoring guide is docs/PLUGINS.md.
//
// Include this file in your editor or type checker (tsconfig "include", or a
// `/// <reference path=".../index.d.ts" />` line) to check a plain JavaScript plugin.

declare module "sawhorse" {
  /** [x, y, z] in metres (or a direction). */
  export type Vec3 = [number, number, number];

  /** An affine map: p -> linear * p + translation; `linear` holds the 3 x 3 part row by row. */
  export interface Transform {
    linear: [Vec3, Vec3, Vec3];
    translation: Vec3;
  }

  export interface Bounds {
    min: Vec3;
    max: Vec3;
  }

  export interface Plane {
    origin: Vec3;
    /** Unit normal. */
    normal: Vec3;
  }

  export type Permission = "model.read" | "model.write" | "ui" | "storage" | "file" | "net";

  /** The API version this module speaks. */
  export const apiVersion: 1;

  /** This plugin, as its manifest declares it. */
  export const plugin: { readonly id: string; readonly name: string; readonly version: string; readonly permissions: readonly Permission[] };

  /** A line in the plugin's developer console (Plugins pane). `console.log` goes there too. No permission. */
  export function log(...args: unknown[]): void;

  // ---- the model -------------------------------------------------------------------------

  /** A solid (a closed, watertight B-rep). */
  export interface Solid {
    id: number;
    /** What people see: "Solid 3" is number 3; never reused. */
    number: number;
    /** Its own name, if any. */
    name: string | null;
    /** What the Outliner shows: the name, else "Solid <number>". */
    label: string;
    tag: string | null;
    material: string | null;
    /** The group or component instance it is in (null: the model). */
    parent: number | null;
    min: Vec3;
    max: Vec3;
    /** Cubic metres. */
    volume: number;
    /** Drawn (not hidden by itself, its tag or a parent). */
    visible: boolean;
    locked: boolean;
    /** Edits may change it now (it is directly in the open group or component, and not locked). */
    editable: boolean;
    /** Part of a component definition (its geometry is shared by every instance). */
    inComponent: boolean;
  }

  /** A group or a component instance. */
  export interface Node {
    id: number;
    kind: "group" | "instance";
    name: string | null;
    /** What the Outliner shows: the name, else "Group" or the component's name. */
    label: string;
    /** The component definition (instances). */
    definition: number | null;
    parent: number | null;
    tag: string | null;
    hidden: boolean;
    locked: boolean;
    visible: boolean;
    transform: Transform;
    inComponent: boolean;
  }

  /** A component definition: the shared parts of every instance. */
  export interface Definition {
    id: number;
    name: string;
    instances: number;
    solids: number;
    /** Its size in its own axes. */
    min: Vec3;
    max: Vec3;
  }

  /** The model's tree, as the Outliner shows it. */
  export interface TreeNode {
    kind: "model" | "group" | "instance" | "solid" | "mesh";
    /** The node, solid or mesh id (0 for the model). */
    id: number;
    label: string;
    /** The group or instance open for editing. */
    open?: boolean;
    children: TreeNode[];
  }

  export interface Stats {
    solids: number;
    groups: number;
    /** Component definitions. */
    components: number;
    instances: number;
    faces: number;
    edges: number;
    vertices: number;
    sketches: number;
    /** Imported meshes that are not solids. */
    meshes: number;
    /** Cubic metres, every solid. */
    volume: number;
    /** Every solid is closed. */
    watertight: boolean;
  }

  export interface DocumentInfo {
    /** The file name, or null for a model not yet saved. */
    name: string | null;
    /** The display units ("frac-in", "in", "ft-in", "mm", "cm", "m"). */
    units: string;
    canUndo: boolean;
    canRedo: boolean;
    /** A hash of the model: equal hashes, equal models. */
    hash: string;
    /** The open group or component (null: the model). */
    context: number | null;
  }

  /** One face of a solid, by persistent id (ids survive pushes, splits and moves). */
  export interface Face {
    id: number;
    normal: Vec3;
    origin: Vec3;
    centroid: Vec3;
    /** Square metres. */
    area: number;
  }

  /** Something selected. */
  export type SelectionItem =
    | { kind: "solid"; id: number }
    | { kind: "group" | "instance"; id: number }
    | { kind: "face"; solid: number; face: number }
    | { kind: "edge"; owner: { kind: "solid" | "sketch"; id: number }; a: Vec3; b: Vec3 }
    | { kind: "vertex"; owner: { kind: "solid" | "sketch"; id: number }; at: Vec3 }
    | { kind: "region"; sketch: number; region: number }
    | { kind: "guide"; id: number }
    | { kind: "dimension"; id: number };

  /** What `model.select` takes: solids, groups and instances (and faces). */
  export type SelectTarget = { kind: "solid"; id: number } | { kind: "group" | "instance" | "node"; id: number } | { kind: "face"; solid: number; face: number };

  export interface Material {
    name: string;
    /** sRGB 0..255 and alpha. */
    color: [number, number, number, number];
    texture: string | null;
    [more: string]: unknown;
  }

  export interface CutRow {
    name: string;
    qty: number;
    /** Finished sizes (exact, metres): thickness <= width <= length. */
    thickness: number;
    width: number;
    length: number;
    material: string | null;
    grain: "length" | "width" | "none";
    /** Sheet goods (else solid lumber). */
    sheet: boolean;
    /** Board feet, quantity included (lumber rows). */
    boardFeet: number;
    /** Square metres, quantity included (sheet rows). */
    area: number;
    /** Not a plain box (its sizes are the box it comes out of). */
    shaped: boolean;
    handed: number;
    hidden: number;
  }

  export interface CutList {
    rows: CutRow[];
    totals: {
      parts: number;
      pieces: number;
      boardFeet: number;
      lumber: { material: string | null; boardFeet: number }[];
      sheets: { material: string | null; thickness: number; area: number; sheets: number }[];
      sheetCount: number;
    };
    hash: string;
  }

  /** A value an earlier op of the same batch produces, filled in when the batch applies. */
  export interface Placeholder {
    readonly $ref: number;
    readonly path: string;
  }
  export type IdLike = number | Placeholder;
  export type Item = { object: IdLike } | { node: IdLike };

  /** A handle on one op of a batch: what it will make. */
  export interface Ref {
    readonly index: number;
    readonly kind: string;
    /** The value at `path` in this op's outcome ("object", "regions.0", "groups.0"). */
    get(path: string): Placeholder;
    /** The solid it makes (extrude, box). */
    readonly object: Placeholder;
    /** The group or instance it makes (group, makeComponent, component). */
    readonly node: Placeholder;
    /** The sketch it makes or draws into. */
    readonly sketch: Placeholder;
    /** The face a closed stroke drawn on a face makes (draw), or the imprinted face (imprint). */
    readonly face: Placeholder;
    /** Region `i` a sketch edit leaves (add_rectangle, draw ...). */
    region(i?: number): Placeholder;
  }

  export interface BoxSpec {
    /** The corner with the smallest x, y and z. */
    min: Vec3;
    /** Width (x), depth (y), height (z), all positive. */
    size: Vec3;
    name?: string;
    /** A material already in the model, or defined earlier in the batch. */
    material?: string;
    tag?: string;
  }

  export interface PartSpec {
    /** The part's component name: what the cut list calls it. */
    name: string;
    /** Its geometry in its own axes (several boxes make one part, like a glue-up). */
    boxes: { min: Vec3; size: Vec3 }[];
    material?: string;
    /** Where its copies go in the assembly: positions or transforms, each with an optional name. */
    copies: { at: Vec3 | Transform; name?: string }[];
  }

  export interface ComponentSpec {
    /** The assembly's component name. */
    name: string;
    parts: PartSpec[];
    /** Where the assembly goes (the origin when absent). */
    at?: Vec3 | Transform;
    /** A name for the placed instance (the Outliner shows it). */
    instanceName?: string;
    tag?: string;
  }

  export interface BatchResult {
    name: string;
    /** One outcome per op (the kernel's outcome objects, e.g. { kind: "object_created", object: 7 }). */
    outcomes: unknown[];
    /** What a placeholder stands for now that the batch is applied. */
    value(p: Placeholder): unknown;
  }

  /**
   * A transactional batch. Collect ops, then `commit()`: the app applies them through the
   * kernel as ONE undo step named `name`; the kernel validates the result like any edit, and
   * if any op is refused nothing changes and `commit` rejects with the reason.
   */
  export interface Batch {
    readonly name: string;
    readonly length: number;
    /** Any kernel op (docs/PLUGINS.md, "Ops"); ids may be placeholders from earlier ops. */
    op(op: { op: string; [field: string]: unknown }): Ref;
    /** A rectangular solid. `ref.object` is the new solid. */
    box(spec: BoxSpec): Ref;
    /** A component built from parts, each part a component of its own (so the cut list lists them). `ref.node` is the placed instance. */
    component(spec: ComponentSpec): Ref;
    group(items: Item[], name?: string): Ref;
    makeComponent(items: Item[], name: string): Ref;
    rename(item: Item, name: string | null): Ref;
    delete(item: Item): Ref;
    transform(items: Item[], xform: Transform): Ref;
    setMaterial(object: IdLike, material: string | null): Ref;
    defineMaterial(name: string, color: [number, number, number, number]): Ref;
    setTag(items: Item[], tag: string | null): Ref;
    /** Needs model.write. */
    commit(): Promise<BatchResult>;
  }

  export const model: {
    /** model.read */
    document(): Promise<DocumentInfo>;
    /** model.read */
    stats(): Promise<Stats>;
    /** model.read. Every solid, in creation order. */
    solids(): Promise<Solid[]>;
    /** model.read */
    solid(id: number): Promise<Solid | null>;
    /** model.read. The faces of a solid with their persistent ids. */
    faces(solid: number): Promise<Face[]>;
    /** model.read. Every group and component instance. */
    nodes(): Promise<Node[]>;
    /** model.read */
    definitions(): Promise<Definition[]>;
    /** model.read */
    tree(): Promise<TreeNode>;
    /** model.read */
    selection(): Promise<SelectionItem[]>;
    /** model.read. Replace the selection (what is not in the open context selects what holds it). */
    select(items: SelectTarget[]): Promise<void>;
    /** model.read. The box round these items (solids, groups, instances), or round everything; null when empty. */
    bounds(items?: SelectTarget[]): Promise<Bounds | null>;
    /** model.read. The materials the model defines (more fields when the app has them). */
    materials(): Promise<Material[]>;
    /** model.read. The cut list, as the Cut list pane works it out. */
    cutList(options?: { rounding?: "sixteenth" | "thirty_second" | "half_mm"; visibleOnly?: boolean }): Promise<CutList>;
    /** Start a batch of changes named `name` (the undo step's name). Committing needs model.write. */
    batch(name: string): Batch;
  };

  // ---- the window ------------------------------------------------------------------------

  /** One element of a panel. Every element with an `id` reports its value to the handlers. */
  export type Field =
    | { type: "text"; id: string; label: string; value?: string; placeholder?: string; disabled?: boolean }
    /** A number; with unit "length" it is metres, shown and typed in the document's units. */
    | { type: "number"; id: string; label: string; value?: number; unit?: "length" | "none" | "count" | "angle"; min?: number; max?: number; step?: number; disabled?: boolean }
    | { type: "select"; id: string; label: string; value?: string; options: { value: string; label: string }[]; disabled?: boolean }
    | { type: "checkbox"; id: string; label: string; value?: boolean; disabled?: boolean }
    | { type: "button"; id: string; label: string; primary?: boolean; disabled?: boolean }
    | { type: "table"; id?: string; columns: string[]; rows: (string | number)[][]; align?: ("left" | "right")[] }
    | { type: "note"; id?: string; text: string; tone?: "info" | "warning" | "error" }
    /** A picture from a base64 data: URL: png, jpeg, gif or webp (lane 16: not svg), at most
     * 256 KB and 2048 x 2048 pixels; 4 images and 512 KB per panel. */
    | { type: "image"; id?: string; src: string; alt: string; width?: number };

  export type Values = Record<string, string | number | boolean | null>;

  export interface PanelSpec {
    /** Declared in plugin.json contributes.panels. */
    id: string;
    title?: string;
    body: Field[];
    /** A button was pressed. */
    onAction?(id: string, values: Values): unknown;
    /** A field's value changed (number and text fields: on Enter or when they lose focus). */
    onChange?(id: string, values: Values): unknown;
  }

  export interface Panel {
    readonly id: string;
    /** Change fields (by id: any of their properties) and values. */
    update(patch: { fields?: Record<string, Partial<Field>>; values?: Values; title?: string }): Promise<void>;
    /** Replace the whole body. */
    setBody(body: Field[]): Promise<void>;
  }

  /**
   * What a tool event carries. With `model.read`: the same picture a built-in tool sees (the
   * inference engine's snap and what is under the cursor). With `ui` alone the model is left
   * out: `picked` is always null, and `snap` is null unless the tool has set inference
   * constraints of its own (`ToolView.inference`), when it is the point those give for the
   * eye ray (on its locked line, or on its plane), worked out from the ray and the tool's own
   * values only (`surface` and `entity` null, `kind` the lock's kind or "free").
   */
  export interface ToolEvent {
    type: "move" | "down" | "click" | "dblclick" | "drag" | "dragend" | "leave";
    /** Canvas pixels. */
    x: number;
    y: number;
    shift: boolean;
    ctrl: boolean;
    alt: boolean;
    /** The eye ray through the cursor. */
    ray: { origin: Vec3; dir: Vec3 };
    /** Where the cursor snaps (the inference engine's answer), with its kind ("endpoint", "on-face" ...). Without model.read: null, or the point of the tool's own constraints. */
    snap: { point: Vec3; kind: string; tip: string; surface: SurfaceHit | null; entity: SelectionItem | null } | null;
    /** What is directly under the cursor. Always null without model.read. */
    picked: { entity: SelectionItem | null; point: Vec3; surface: SurfaceHit | null } | null;
  }

  /** A face (or sketch region) under the cursor with its plane frame. */
  export interface SurfaceHit {
    at: { kind: "face"; solid: number; face: number } | { kind: "region"; sketch: number; region: number };
    frame: { origin: Vec3; normal: Vec3; u: Vec3; v: Vec3 };
    point: Vec3;
  }

  /** What a tool draws and says. Colours are the app's: "accent", "preview", "warning", "red", "green", "blue", or an inference kind. */
  export interface ToolView {
    /** Lines in the viewport (world points). */
    overlay?: { kind: "segments" | "strip" | "loop"; points: Vec3[]; color?: string; width?: number; dashed?: boolean }[];
    labels?: { at: Vec3; text: string }[];
    /** The status line. */
    status?: string | null;
    /** Shown in the Value Control Box. */
    value?: string;
    /** A gesture is going on (Esc cancels it instead of clearing the selection). */
    busy?: boolean;
    /** Inference constraints for the next pointer events, like a built-in tool's. */
    inference?: { start?: Vec3 | null; plane?: Plane | null; lock?: { dir: Vec3; kind: string } | null; grid?: boolean } | null;
  }

  export interface Tool {
    readonly id: string;
    setView(view: ToolView): Promise<void>;
  }

  export interface ToolSpec {
    /** Declared in plugin.json contributes.tools (title, hint and the VCB label come from there). */
    id: string;
    onActivate?(tool: Tool): unknown;
    onDeactivate?(tool: Tool): unknown;
    onEvent?(e: ToolEvent, tool: Tool): unknown;
    /** Enter in the Value Control Box. */
    onValue?(text: string, tool: Tool): unknown;
    /** Esc while busy. */
    onCancel?(tool: Tool): unknown;
  }

  export const ui: {
    /** ui. Declared in contributes.commands; shown in the Plugins menu. */
    registerCommand(spec: { id: string; run(): unknown }): Promise<void>;
    /** ui. Declared in contributes.panels; shown in the Plugins pane under the plugin's name. */
    registerPanel(spec: PanelSpec): Promise<Panel>;
    /** ui. Declared in contributes.tools; shown in the rail's plugin group. */
    registerTool(spec: ToolSpec): Promise<Tool>;
    /** ui. Make one of this plugin's tools the active tool. */
    activateTool(id: string): Promise<void>;
    /** ui. A message in the status line. */
    notify(text: string, options?: { kind?: "info" | "warning" | "error" }): Promise<void>;
    /** ui. Ask for a line of text; null when cancelled. */
    prompt(spec: { title: string; label?: string; value?: string; ok?: string }): Promise<string | null>;
    /** ui. Ask yes or no. */
    confirm(spec: { title: string; text: string; ok?: string }): Promise<boolean>;
  };

  /** Lengths in the document's display units (no permission; local). */
  export const units: {
    /** "frac-in", "in", "ft-in", "mm", "cm" or "m". */
    system(): string;
    /** "in", "mm" ... */
    short(): string;
    /** A typed length in metres ("3/4", "19mm", "1'6\""), or null. */
    parse(text: string): number | null;
    read(text: string): { ok: true; metres: number } | { ok: false; error: string };
    /** Metres as the document shows them ("11 1/4", "~ 285.8"). */
    format(metres: number): string;
  };

  /** Pure helpers (no permission; local). */
  export const geometry: {
    vec3: {
      add(a: Vec3, b: Vec3): Vec3;
      sub(a: Vec3, b: Vec3): Vec3;
      scale(a: Vec3, s: number): Vec3;
      dot(a: Vec3, b: Vec3): number;
      cross(a: Vec3, b: Vec3): Vec3;
      length(a: Vec3): number;
      distance(a: Vec3, b: Vec3): number;
      normalize(a: Vec3): Vec3;
      lerp(a: Vec3, b: Vec3, t: number): Vec3;
      equals(a: Vec3, b: Vec3, tol?: number): boolean;
      perpendicular(a: Vec3): Vec3;
    };
    plane: {
      fromPointNormal(origin: Vec3, normal: Vec3): Plane;
      fromPoints(a: Vec3, b: Vec3, c: Vec3): Plane | null;
      distance(pl: Plane, p: Vec3): number;
      project(pl: Plane, p: Vec3): Vec3;
      /** u level when the plane is upright; u x v = normal. */
      frame(pl: Plane): { u: Vec3; v: Vec3 };
      intersectLine(pl: Plane, p: Vec3, dir: Vec3): Vec3 | null;
    };
    bbox: {
      empty(): Bounds;
      isEmpty(b: Bounds): boolean;
      fromPoints(pts: Vec3[]): Bounds;
      addPoint(b: Bounds, p: Vec3): Bounds;
      union(a: Bounds, b: Bounds): Bounds;
      size(b: Bounds): Vec3;
      center(b: Bounds): Vec3;
      contains(b: Bounds, p: Vec3, tol?: number): boolean;
      corners(b: Bounds): Vec3[];
    };
    transform: {
      identity(): Transform;
      translation(t: Vec3): Transform;
      rotation(axis: Vec3, degrees: number, point?: Vec3): Transform;
      scale(factors: number | Vec3, point?: Vec3): Transform;
      /** a after b. */
      compose(a: Transform, b: Transform): Transform;
      apply(m: Transform, p: Vec3): Vec3;
      applyVector(m: Transform, v: Vec3): Vec3;
      determinant(m: Transform): number;
      invert(m: Transform): Transform | null;
    };
  };

  export interface EventPayloads {
    /** model.read. */
    selectionChanged: { selection: SelectionItem[] };
    /** model.read. After every edit, undo and redo (a moment after, coalesced). */
    documentChanged: { hash: string; canUndo: boolean };
    /** model.read. A new model, or a file opened. */
    documentOpened: { name: string | null };
    /** ui. The active tool changed ("select", "pushpull", or "plugin:<plugin id>/<tool id>"). */
    toolChanged: { tool: string };
    /** No permission. The display units changed (sawhorse.units already follows them). */
    unitsChanged: { units: string };
  }

  /** Stops listening when called. `ready` resolves when the app has accepted the
   * subscription and rejects with a PluginApiError ("permission" when plugin.json does not
   * ask for what the event needs, "invalid" for an event that does not exist). */
  export interface Unsubscribe {
    (): void;
    readonly ready: Promise<void>;
  }

  export const events: {
    /** Listen; returns a function that stops listening (with `ready`, see Unsubscribe). */
    on<K extends keyof EventPayloads>(name: K, handler: (payload: EventPayloads[K]) => unknown): Unsubscribe;
  };

  /** What every refused call rejects with (lane 15). Calls never throw for a refusal: they
   * reject. `code`: "permission" (plugin.json does not ask for it), "invalid" (a parameter
   * the app cannot use), "too_large" (the call or its answer is over the message budget,
   * lane 16: 4 MB a message, more for files and the network), "not_found", "busy" (too many
   * calls at once or too fast), "stopped" (the plugin stopped while the call waited),
   * "unknown" (no such call), "send_failed" (the message could not be sent), "refused"
   * (anything else the app said no to). */
  export class PluginApiError extends Error {
    readonly code: string;
  }

  /** A small key-value store of JSON values, for this plugin only. */
  export interface Store {
    get(key: string): Promise<unknown>;
    set(key: string, value: unknown): Promise<void>;
    delete(key: string): Promise<void>;
    keys(): Promise<string[]>;
  }

  /** storage. `app`: kept by this app (per device, 256 KB; 1 MB for all plugins). `document`: saved inside the .shz with the model (64 KB; 1 MB for all plugins). JSON values only. */
  export const storage: { app: Store; document: Store };

  export const file: {
    /** file. Save through the app's save dialog (the user picks the place); null when cancelled. */
    save(spec: { name: string; data: string | Uint8Array; mime?: string; description?: string }): Promise<{ name: string } | null>;
    /** file. Open through the app's open dialog; null when cancelled. */
    open(spec?: { extensions?: string[]; description?: string }): Promise<{ name: string; bytes: Uint8Array } | null>;
  };

  export const net: {
    /** net. A request to a host on the manifest's "net" list (https only; no cookies). Also available as the global `fetch` for plugins with "net". */
    fetch(url: string, init?: { method?: string; headers?: Record<string, string>; body?: string | Uint8Array }): Promise<Response>;
  };

  const sawhorse: {
    apiVersion: typeof apiVersion;
    plugin: typeof plugin;
    log: typeof log;
    model: typeof model;
    ui: typeof ui;
    units: typeof units;
    geometry: typeof geometry;
    events: typeof events;
    storage: typeof storage;
    file: typeof file;
    net: typeof net;
    PluginApiError: typeof PluginApiError;
  };
  export default sawhorse;
}
