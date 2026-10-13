// The plugin side of the API: the object a plugin gets from `import ... from "sawhorse"`.
// It only shapes calls into messages (rpc.ts) and keeps the plugin's callbacks (commands,
// panel handlers, tool handlers, event listeners) so the host can ask for them to run. It
// enforces nothing: the host checks every request against the plugin's permissions.
//
// The sandbox (src/plugins/sandbox.worker.ts in the app) builds one runtime per worker and
// installs its `api` where the "sawhorse" module (sawhorse.js) picks it up. Unit tests
// build it over a fake transport (test/runtime.test.ts).
import { bbox, plane, transform, vec3, type Bounds, type Transform, type Vec3 } from "./geometry.ts";
import { checkData, EVENT_NAMES, logText, PluginApiError, type Data, type HostMessage, type LogLevel, type PluginInfo, type PluginMessage } from "./rpc.ts";

export { PluginApiError };

/** Lane 15: a call refused before it is sent rejects (never throws) with a typed error. */
const refuse = (e: unknown, code = "invalid"): Promise<never> =>
  Promise.reject(e instanceof PluginApiError ? e : new PluginApiError(e instanceof Error ? e.message : String(e), code));

/** A value produced by an earlier op of the same batch, filled in by the kernel worker. */
export interface Placeholder {
  readonly $ref: number;
  readonly path: string;
}

export type IdLike = number | Placeholder;
export type Item = { object: IdLike } | { node: IdLike };

export interface UnitsImpl {
  parse(text: string, system: string): { ok: true; metres: number } | { ok: false; error: string };
  format(metres: number, system: string): string;
  short(system: string): string;
}

export interface RuntimeOptions {
  send(msg: PluginMessage): void;
  plugin: PluginInfo;
  units: string;
  unitsImpl: UnitsImpl;
}

type Handler = (...args: never[]) => unknown;

/** What path of an op's outcome holds the thing it made, by op kind. */
const MADE: Record<string, Partial<Record<"object" | "node" | "sketch" | "face", string>>> = {
  extrude: { object: "object" },
  import: { object: "objects.0", node: "groups.0", sketch: "sketches.0" },
  new_sketch: { sketch: "sketch" },
  make_group: { node: "group" },
  create_group: { node: "group" },
  make_component: { node: "instance" },
  draw: { sketch: "sketch", face: "inside" },
  imprint: { face: "face" },
  add_guide: {},
};

export class Ref {
  readonly index: number;
  readonly kind: string;
  constructor(index: number, kind: string) {
    this.index = index;
    this.kind = kind;
  }

  /** The value at `path` in this op's outcome ("object", "regions.0", "groups.0"). */
  get(path: string): Placeholder {
    if (typeof path !== "string" || !/^[a-z_]+(\.[a-z_0-9]+)*$|^$/.test(path)) throw new TypeError(`bad outcome path ${JSON.stringify(path)}`);
    return Object.freeze({ $ref: this.index, path });
  }

  private made(what: "object" | "node" | "sketch" | "face"): Placeholder {
    const path = MADE[this.kind]?.[what];
    if (!path) throw new TypeError(`a ${this.kind} op makes no ${what}; use ref.get("<outcome path>")`);
    return this.get(path);
  }

  /** The solid this op made (extrude, box, import). */
  get object(): Placeholder {
    return this.made("object");
  }
  /** The group or component instance this op made (group, component, makeComponent, import). */
  get node(): Placeholder {
    return this.made("node");
  }
  /** The sketch this op made or drew into. */
  get sketch(): Placeholder {
    return this.made("sketch");
  }
  /** The face a closed stroke drawn on a face made (draw), or the imprinted face (imprint). */
  get face(): Placeholder {
    return this.made("face");
  }
  /** Region `i` of a sketch edit's outcome (add_rectangle, insert_segment, draw ...). */
  region(i = 0): Placeholder {
    return this.get(`regions.${i}`);
  }
}

/** Look `path` up in an outcome ("regions.0" -> outcome.regions[0]). */
export function outcomeValue(outcome: unknown, path: string): unknown {
  let at: unknown = outcome;
  if (path === "") return at;
  for (const part of path.split(".")) {
    if (Array.isArray(at) && /^\d+$/.test(part)) at = at[Number(part)];
    else if (at && typeof at === "object" && !Array.isArray(at)) at = (at as Record<string, unknown>)[part];
    else return undefined;
  }
  return at;
}

export interface BoxSpec {
  /** The corner with the smallest x, y and z (metres). */
  min: Vec3;
  /** Width (x), depth (y), height (z), all positive (metres). */
  size: Vec3;
  name?: string;
  material?: string;
  tag?: string;
}

export interface PartSpec {
  /** The component's name (what the cut list calls the part). */
  name: string;
  /** Its geometry in its own axes: boxes (several make one part, like a glue-up). */
  boxes: { min: Vec3; size: Vec3 }[];
  material?: string;
  /** Where its copies go inside the assembly; each a position or a transform, with an optional name. */
  copies: { at: Vec3 | Transform; name?: string }[];
}

export interface ComponentSpec {
  /** The assembly's name. */
  name: string;
  parts: PartSpec[];
  /** Where the assembly goes (a position or a transform); the origin when absent. */
  at?: Vec3 | Transform;
  /** A name for the placed instance (the Outliner shows it). */
  instanceName?: string;
  tag?: string;
}

const isTransform = (x: unknown): x is Transform => typeof x === "object" && x !== null && "linear" in x;
const xformOf = (at: Vec3 | Transform | undefined): Transform => (at === undefined ? transform.identity() : isTransform(at) ? at : transform.translation(at));

/** A rectangular box as an import item (outward faces). */
export function boxItem(name: string, min: Vec3, size: Vec3, extra: { material?: string; tag?: string } = {}): Record<string, unknown> {
  if (!size.every((s) => Number.isFinite(s) && s > 0)) throw new RangeError(`box "${name}": every size must be a positive number of metres (got ${JSON.stringify(size)})`);
  if (!min.every(Number.isFinite)) throw new RangeError(`box "${name}": min must be three finite numbers`);
  const positions: Vec3[] = [];
  for (let k = 0; k < 8; k++) positions.push([min[0] + (k & 1 ? size[0] : 0), min[1] + (k & 2 ? size[1] : 0), min[2] + (k & 4 ? size[2] : 0)]);
  const faces = [
    [[0, 2, 3, 1]],
    [[4, 5, 7, 6]],
    [[0, 1, 5, 4]],
    [[2, 6, 7, 3]],
    [[0, 4, 6, 2]],
    [[1, 3, 7, 5]],
  ];
  const item: Record<string, unknown> = { name, positions, faces };
  if (extra.material) item.material = extra.material;
  if (extra.tag) item.tag = extra.tag;
  return item;
}

/** The import op for a component (definitions for each part, one for the assembly, one placed instance). */
export function componentOp(spec: ComponentSpec): Record<string, unknown> {
  if (!spec.parts.length) throw new RangeError(`component "${spec.name}" needs at least one part`);
  const definitions: Record<string, unknown>[] = spec.parts.map((p) => ({
    name: p.name,
    items: p.boxes.map((b, i) => boxItem(p.boxes.length > 1 ? `${p.name} ${i + 1}` : p.name, b.min, b.size, { material: p.material })),
  }));
  const children: Record<string, unknown>[] = [];
  spec.parts.forEach((p, i) => {
    for (const c of p.copies) {
      const child: Record<string, unknown> = { kind: "instance", definition: i, xform: xformOf(c.at) };
      if (c.name) child.name = c.name;
      children.push(child);
    }
  });
  definitions.push({ name: spec.name, items: [], children });
  const node: Record<string, unknown> = { kind: "instance", definition: definitions.length - 1, xform: xformOf(spec.at) };
  if (spec.instanceName) node.name = spec.instanceName;
  if (spec.tag) node.tag = spec.tag;
  return { op: "import", definitions, nodes: [node] };
}

export interface BatchResult {
  name: string;
  /** One outcome per op, in order (the kernel's outcome objects). */
  outcomes: unknown[];
  /** What a placeholder stands for, now that the batch is applied. */
  value(p: Placeholder): unknown;
}

/** A transactional batch: ops are collected, then `commit` applies them as one undo step. */
export class Batch {
  private ops: unknown[] = [];
  private done = false;
  readonly name: string;
  private commitFn: (name: string, ops: unknown[]) => Promise<unknown[]>;

  constructor(name: string, commitFn: (name: string, ops: unknown[]) => Promise<unknown[]>) {
    this.name = name;
    this.commitFn = commitFn;
  }

  get length(): number {
    return this.ops.length;
  }

  /** Any kernel op (docs/PLUGINS.md lists them). Ids may be placeholders from earlier ops. */
  op(op: { op: string; [k: string]: unknown }): Ref {
    if (this.done) throw new Error(`batch "${this.name}" was already committed`);
    if (!op || typeof op.op !== "string") throw new TypeError('an op is an object with an "op" field, like { op: "rename", item: { object: 3 }, name: "Top" }');
    const clean = plain(op) as { op: string };
    checkData(clean, `op ${this.ops.length}`);
    this.ops.push(clean);
    return new Ref(this.ops.length - 1, op.op);
  }

  /** A rectangular solid. `ref.object` is the new solid. */
  box(spec: BoxSpec): Ref {
    return this.op({ op: "import", items: [boxItem(spec.name ?? "Box", spec.min, spec.size, spec)] });
  }

  /** A component built from parts (each part its own component, so the cut list reads them). `ref.node` is the placed instance. */
  component(spec: ComponentSpec): Ref {
    return this.op(componentOp(spec) as { op: string });
  }

  /** Make a group of solids, groups or instances. `ref.node` is the group. */
  group(items: Item[], name?: string): Ref {
    return this.op({ op: "make_group", items, name: name ?? null });
  }

  /** Make a component of solids, groups or instances. `ref.node` is its instance. */
  makeComponent(items: Item[], name: string): Ref {
    return this.op({ op: "make_component", items, name });
  }

  rename(item: Item, name: string | null): Ref {
    return this.op({ op: "rename", item, name });
  }

  /** Delete a solid ({object}) or a group or instance ({node}). */
  delete(item: Item): Ref {
    return "node" in item ? this.op({ op: "delete_node", node: item.node }) : this.op({ op: "delete_object", object: item.object });
  }

  /** Move, turn or scale solids and groups or instances by a transform. */
  transform(items: Item[], xform: Transform): Ref {
    const objects = items.flatMap((i) => ("object" in i ? [i.object] : []));
    const nodes = items.flatMap((i) => ("node" in i ? [i.node] : []));
    return this.op({ op: "transform", objects, nodes, xform });
  }

  /** Paint a solid with a material defined in the model (null takes it off). */
  setMaterial(object: IdLike, material: string | null): Ref {
    return this.op({ op: "set_material", object, material });
  }

  /** Add or replace a material: a colour (sRGB 0..255 and alpha). */
  defineMaterial(name: string, color: [number, number, number, number]): Ref {
    return this.op({ op: "define_material", name, color });
  }

  /** Put solids, groups or instances on a tag (null: Untagged). */
  setTag(items: Item[], tag: string | null): Ref {
    return this.op({ op: "tag_items", items, tag });
  }

  /** Apply every op as one undo step. Rejects (and changes nothing) when the kernel refuses any of them. */
  async commit(): Promise<BatchResult> {
    if (this.done) throw new Error(`batch "${this.name}" was already committed`);
    if (!this.ops.length) throw new Error(`batch "${this.name}" has no ops`);
    this.done = true;
    const outcomes = await this.commitFn(this.name, this.ops);
    return {
      name: this.name,
      outcomes,
      value: (p: Placeholder) => outcomeValue(outcomes[p.$ref], p.path),
    };
  }
}

/** Plain data as JSON would make it: object fields that are undefined are left out, undefined in lists is null. */
export function plain(v: unknown): unknown {
  if (v === undefined) return null;
  if (v === null || typeof v !== "object" || v instanceof Uint8Array) return v;
  if (Array.isArray(v)) return v.map((x) => (x === undefined ? null : plain(x)));
  const proto = Object.getPrototypeOf(v);
  if (proto !== Object.prototype && proto !== null) return v;
  const out: Record<string, unknown> = {};
  for (const [k, x] of Object.entries(v)) if (x !== undefined) out[k] = plain(x);
  return out;
}

export interface Runtime {
  api: Record<string, unknown>;
  receive(msg: HostMessage): void;
  /** Send a log line (the sandbox's console goes here). */
  log(level: LogLevel, args: unknown[]): void;
}

interface ToolHandlers {
  onActivate?: (tool: unknown) => unknown;
  onDeactivate?: (tool: unknown) => unknown;
  onEvent?: (e: unknown, tool: unknown) => unknown;
  onValue?: (text: string, tool: unknown) => unknown;
  onCancel?: (tool: unknown) => unknown;
}

export function createRuntime(opts: RuntimeOptions): Runtime {
  let nextId = 1;
  const pending = new Map<number, { resolve: (v: Data) => void; reject: (e: Error) => void }>();
  let units = opts.units;
  const commands = new Map<string, Handler>();
  const panels = new Map<string, { onAction?: Handler; onChange?: Handler }>();
  const tools = new Map<string, { handlers: ToolHandlers; handle: unknown }>();
  const listeners = new Map<string, Set<Handler>>();

  const send = (msg: PluginMessage) => opts.send(msg);
  const request = (method: string, params: unknown = null): Promise<Data> => {
    const id = nextId++;
    // Lane 15: parameters that cannot cross reject the call (they used to throw at the caller).
    let data: Data;
    try {
      data = checkData(plain(params), `${method} params`);
    } catch (e) {
      return refuse(e, "invalid");
    }
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      try {
        send({ t: "req", id, method, params: data });
      } catch (e) {
        // The message could not be sent (the channel is gone, or the browser would not copy
        // it): the call fails now instead of waiting for an answer that cannot come.
        pending.delete(id);
        reject(new PluginApiError(`${method}: the request could not be sent (${e instanceof Error ? e.message : String(e)})`, "send_failed"));
      }
    });
  };

  const log = (level: LogLevel, args: unknown[]) => send({ t: "log", level, text: logText(args) });

  const model = {
    document: () => request("model.document"),
    stats: () => request("model.stats"),
    solids: () => request("model.solids"),
    solid: (id: number) => request("model.solid", { id }),
    faces: (solid: number) => request("model.faces", { solid }),
    nodes: () => request("model.nodes"),
    definitions: () => request("model.definitions"),
    tree: () => request("model.tree"),
    selection: () => request("model.selection"),
    select: (items: unknown[]) => request("model.select", { items }),
    bounds: (items?: unknown[]) => request("model.bounds", { items: items ?? null }),
    materials: () => request("model.materials"),
    cutList: (options?: { rounding?: string; visibleOnly?: boolean }) => request("model.cutList", options ?? {}),
    batch: (name: string) => {
      // A batch is built step by step before it is applied, so a bad name is a mistake in the
      // plugin's code at that line: it throws (apply() rejects on anything the host refuses).
      if (typeof name !== "string" || !name.trim() || name.length > 80) throw new TypeError("a batch needs a name of 1 to 80 characters (it names the undo step)");
      return new Batch(name, async (n, ops) => (await request("model.batch", { name: n, ops: ops as Data[] })) as unknown[]);
    },
  };

  const panelHandle = (id: string) => ({
    id,
    update: (patch: { fields?: Record<string, unknown>; values?: Record<string, unknown> }) => request("ui.updatePanel", { id, ...patch }),
    setBody: (body: unknown[]) => request("ui.updatePanel", { id, body }),
  });

  const toolHandle = (id: string) => ({
    id,
    /** What the tool shows: overlay lines, labels, the status line, the VCB value, busy (Esc cancels), inference constraints. */
    setView: (view: Record<string, unknown>) => request("ui.toolView", { id, view }),
  });

  const ui = {
    registerCommand: (spec: { id: string; run: Handler }) => {
      if (!spec || typeof spec.run !== "function") return refuse(new TypeError("registerCommand needs { id, run }"));
      commands.set(spec.id, spec.run);
      return request("ui.registerCommand", { id: spec.id });
    },
    registerPanel: (spec: { id: string; title?: string; body: unknown[]; onAction?: Handler; onChange?: Handler }) => {
      if (!spec || !Array.isArray(spec.body)) return refuse(new TypeError("registerPanel needs { id, body: [...] }"));
      panels.set(spec.id, { onAction: spec.onAction, onChange: spec.onChange });
      const handle = panelHandle(spec.id);
      return request("ui.registerPanel", { id: spec.id, title: spec.title ?? null, body: spec.body as Data[] }).then(() => handle);
    },
    registerTool: (spec: { id: string } & ToolHandlers) => {
      if (!spec || typeof spec.id !== "string") return refuse(new TypeError("registerTool needs { id, onEvent, ... }"));
      const handle = toolHandle(spec.id);
      tools.set(spec.id, { handlers: spec, handle });
      return request("ui.registerTool", { id: spec.id }).then(() => handle);
    },
    activateTool: (id: string) => request("ui.activateTool", { id }),
    notify: (text: string, options?: { kind?: "info" | "warning" | "error" }) => request("ui.notify", { text, kind: options?.kind ?? "info" }),
    prompt: (spec: { title: string; label?: string; value?: string; ok?: string }) => request("ui.prompt", spec),
    confirm: (spec: { title: string; text: string; ok?: string }) => request("ui.confirm", spec),
  };

  const unitsApi = {
    system: () => units,
    short: () => opts.unitsImpl.short(units),
    /** A typed length in metres ("3/4", "19mm", "1'6\""), or null when it cannot be read. */
    parse: (text: string): number | null => {
      const r = opts.unitsImpl.parse(String(text), units);
      return r.ok ? r.metres : null;
    },
    /** Like parse, with the reason when it cannot be read. */
    read: (text: string) => opts.unitsImpl.parse(String(text), units),
    format: (metres: number) => opts.unitsImpl.format(metres, units),
  };

  /** Each event's subscription with the host: resolves when the host accepts it, rejects
   * when it refuses (an event the plugin has no permission for) or it is not an event. */
  const subscriptions = new Map<string, Promise<void>>();
  const events = {
    /** Listen. Returns a function that stops listening; its `ready` promise resolves when the
     * host has accepted the subscription and rejects (PluginApiError) when it refuses it. */
    on: (name: string, fn: Handler): (() => void) & { ready: Promise<void> } => {
      let ready = subscriptions.get(name);
      if (typeof fn !== "function") ready = refuse(new TypeError("events.on needs a handler function"));
      else if (!(EVENT_NAMES as readonly string[]).includes(name)) ready = refuse(new PluginApiError(`events.on: no event "${String(name)}" (there are ${EVENT_NAMES.join(", ")})`, "invalid"));
      else if (!ready) {
        listeners.set(name, new Set());
        ready = request("events.subscribe", { name }).then(() => undefined);
        subscriptions.set(name, ready);
        // Refused: no listener stays, and the refusal is in the plugin's console too.
        ready.catch((e) => {
          listeners.delete(name);
          subscriptions.delete(name);
          log("error", [`events.on("${name}"): ${e instanceof Error ? e.message : String(e)}`]);
        });
      }
      const set = typeof fn === "function" ? listeners.get(name) : undefined;
      if (set) set.add(fn);
      const off = Object.assign(() => void set?.delete(fn), { ready });
      // Awaiting `ready` is optional: a refusal is logged either way, never unhandled.
      ready.catch(() => {});
      return off;
    },
  };

  const kv = (scope: "app" | "document") => ({
    get: (key: string) => request("storage.get", { scope, key }),
    set: (key: string, value: unknown) => request("storage.set", { scope, key, value: value as Data }),
    delete: (key: string) => request("storage.delete", { scope, key }),
    keys: () => request("storage.keys", { scope }),
  });

  const file = {
    save: (spec: { name: string; data: string | Uint8Array; mime?: string; description?: string }) => request("file.save", spec),
    open: (spec?: { extensions?: string[]; description?: string }) => request("file.open", spec ?? {}),
  };

  const net = {
    fetch: async (url: string, init?: { method?: string; headers?: Record<string, string>; body?: string | Uint8Array }) => {
      const r = (await request("net.fetch", { url: String(url), method: init?.method ?? "GET", headers: init?.headers ?? {}, body: init?.body ?? null })) as {
        status: number;
        statusText: string;
        headers: Record<string, string>;
        body: Uint8Array;
      };
      const R = (globalThis as { Response?: typeof Response }).Response;
      if (R) return new R(r.status === 204 || r.status === 304 ? null : (r.body as BlobPart), { status: r.status, statusText: r.statusText, headers: r.headers });
      return r;
    },
  };

  const api = Object.freeze({
    apiVersion: 1,
    plugin: Object.freeze({ ...opts.plugin, permissions: Object.freeze([...opts.plugin.permissions]) }),
    log: (...args: unknown[]) => log("log", args),
    model: Object.freeze(model),
    ui: Object.freeze(ui),
    units: Object.freeze(unitsApi),
    geometry: Object.freeze({ vec3: Object.freeze(vec3), plane: Object.freeze(plane), bbox: Object.freeze(bbox), transform: Object.freeze(transform) }),
    events: Object.freeze(events),
    storage: Object.freeze({ app: Object.freeze(kv("app")), document: Object.freeze(kv("document")) }),
    file: Object.freeze(file),
    net: Object.freeze(net),
    PluginApiError,
  });

  const run = async (fn: string, args: Data[]): Promise<unknown> => {
    switch (fn) {
      case "command": {
        const h = commands.get(String(args[0]));
        if (!h) throw new Error(`no command "${String(args[0])}" is registered`);
        return (h as () => unknown)();
      }
      case "panel.action":
      case "panel.change": {
        const p = panels.get(String(args[0]));
        const h = fn === "panel.action" ? p?.onAction : p?.onChange;
        if (!h) return null;
        return (h as (id: unknown, values: unknown) => unknown)(args[1], args[2]);
      }
      case "tool.activate":
      case "tool.deactivate":
      case "tool.event":
      case "tool.value":
      case "tool.cancel": {
        const t = tools.get(String(args[0]));
        if (!t) throw new Error(`no tool "${String(args[0])}" is registered`);
        const h = t.handlers;
        if (fn === "tool.activate") return h.onActivate?.(t.handle);
        if (fn === "tool.deactivate") return h.onDeactivate?.(t.handle);
        if (fn === "tool.event") return h.onEvent?.(args[1], t.handle);
        if (fn === "tool.value") return h.onValue?.(String(args[1]), t.handle);
        return h.onCancel?.(t.handle);
      }
      default:
        throw new Error(`unknown call ${fn}`);
    }
  };

  const receive = (msg: HostMessage) => {
    switch (msg.t) {
      case "res": {
        const p = pending.get(msg.id);
        if (!p) return;
        pending.delete(msg.id);
        if (msg.ok) p.resolve(msg.value);
        else p.reject(new PluginApiError(msg.error, msg.code ?? "error"));
        return;
      }
      case "ping":
        send({ t: "pong", n: msg.n });
        return;
      case "evt": {
        if (msg.name === "unitsChanged") {
          units = String((msg.payload as { units?: string } | null)?.units ?? units);
        }
        for (const fn of listeners.get(msg.name) ?? []) {
          try {
            const r = (fn as (p: unknown) => unknown)(msg.payload);
            if (r && typeof (r as Promise<unknown>).catch === "function") (r as Promise<unknown>).catch((e) => log("error", [`${msg.name} handler failed:`, e]));
          } catch (e) {
            log("error", [`${msg.name} handler failed:`, e]);
          }
        }
        return;
      }
      case "call": {
        const id = msg.id;
        Promise.resolve()
          .then(() => run(msg.fn, msg.args))
          .then(
            (value) => {
              let v: Data = null;
              try {
                v = checkData(value === undefined ? null : value, "result");
              } catch {
                v = null;
              }
              send({ t: "done", id, ok: true, value: v });
            },
            (e) => {
              const text = e instanceof Error ? e.message : String(e);
              log("error", [`${msg.fn} failed:`, e]);
              send({ t: "done", id, ok: false, error: text });
            },
          );
        return;
      }
      case "init":
        return;
    }
  };

  return { api, receive, log };
}

export type { Bounds, Transform, Vec3 };
