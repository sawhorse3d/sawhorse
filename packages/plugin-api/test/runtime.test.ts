import { describe, expect, it } from "vitest";
import { BATCH_MAX_OPS, boxItem, componentOp, createRuntime, outcomeValue, PluginApiError } from "../src/runtime.ts";
import { decodePluginFrame, type PluginMessage } from "../src/rpc.ts";

function setup(permissions = ["model.read", "model.write", "ui"]) {
  const sent: PluginMessage[] = [];
  const rt = createRuntime({
    // What the sandbox posts, read back as the app reads it.
    post: (f) => void sent.push(decodePluginFrame(f)),
    plugin: { id: "org.example.t", name: "T", version: "1.0.0", permissions },
    units: "mm",
    unitsImpl: {
      parse: (t, s) => (/^\d+(\.\d+)?$/.test(t.trim()) ? { ok: true, metres: Number(t) * (s === "mm" ? 0.001 : 0.0254) } : { ok: false, error: `Can't read "${t}"` }),
      format: (m, s) => (s === "mm" ? `${m * 1000}` : `${m / 0.0254}`),
      short: (s) => s,
    },
  });
  const api = rt.api as Record<string, Record<string, (...a: unknown[]) => unknown>>;
  const last = () => sent[sent.length - 1];
  const answer = (value: unknown) => {
    const m = last();
    if (m.t !== "req") throw new Error("no request to answer");
    rt.receive({ t: "res", id: m.id, ok: true, value: value as never });
  };
  return { rt, api, sent, last, answer };
}

describe("requests", () => {
  it("send a req and resolve with the res, in any order", async () => {
    const { rt, api, sent } = setup();
    const a = api.model.stats() as Promise<unknown>;
    const b = api.model.solids() as Promise<unknown>;
    const [ra, rb] = sent as Extract<PluginMessage, { t: "req" }>[];
    expect(ra.method).toBe("model.stats");
    rt.receive({ t: "res", id: rb.id, ok: true, value: [] });
    rt.receive({ t: "res", id: ra.id, ok: true, value: { solids: 2 } });
    await expect(a).resolves.toEqual({ solids: 2 });
    await expect(b).resolves.toEqual([]);
  });

  it("reject with the host's error and its code", async () => {
    const { rt, api, last } = setup();
    const p = api.model.solids() as Promise<unknown>;
    const m = last() as Extract<PluginMessage, { t: "req" }>;
    rt.receive({ t: "res", id: m.id, ok: false, error: "needs model.read", code: "permission" });
    await expect(p).rejects.toMatchObject({ message: "needs model.read", code: "permission" });
  });

  it("leave out undefined fields, as JSON would", () => {
    const { api, last } = setup();
    void api.ui.notify("hi", undefined);
    void (api.ui.registerTool as (s: unknown) => Promise<{ setView(v: unknown): unknown }>)({ id: "t" });
    expect(last()).toMatchObject({ t: "req", method: "ui.registerTool", params: { id: "t" } });
    const m = last() as Extract<PluginMessage, { t: "req" }>;
    expect(m.params).toEqual({ id: "t" });
  });

  it("refuse arguments that are not plain data before sending: the call rejects, it never throws (lane 15)", async () => {
    const { api, sent } = setup();
    let p: Promise<unknown> | undefined;
    expect(() => {
      p = (api.storage as unknown as { app: { set(k: string, v: unknown): Promise<unknown> } }).app.set("k", () => 1);
    }).not.toThrow();
    await expect(p).rejects.toMatchObject({ name: "PluginApiError", code: "invalid", message: expect.stringContaining("is a function") });
    expect(sent).toHaveLength(0);
    // The same for registering with a missing handler.
    await expect((api.ui.registerCommand as (s: unknown) => Promise<unknown>)({ id: "x" })).rejects.toMatchObject({ code: "invalid" });
    await expect((api.ui.registerPanel as (s: unknown) => Promise<unknown>)({ id: "x" })).rejects.toMatchObject({ code: "invalid" });
    await expect((api.ui.registerTool as (s: unknown) => Promise<unknown>)(null)).rejects.toMatchObject({ code: "invalid" });
  });

  it("a request that cannot be sent rejects with a typed error at once (lane 15)", async () => {
    const rt = createRuntime({
      post: (f) => {
        if (decodePluginFrame(f).t === "req") throw new DOMException("could not be cloned", "DataCloneError");
      },
      plugin: { id: "org.example.t", name: "T", version: "1.0.0", permissions: [] },
      units: "mm",
      unitsImpl: { parse: () => ({ ok: false, error: "" }), format: () => "", short: () => "" },
    });
    const api = rt.api as unknown as { model: { stats(): Promise<unknown> }; PluginApiError: new (m: string, c?: string) => Error };
    const p = api.model.stats();
    await expect(p).rejects.toBeInstanceOf(api.PluginApiError);
    await expect(p).rejects.toMatchObject({ code: "send_failed", message: expect.stringContaining("model.stats: the request could not be sent") });
  });

  it("every host refusal is a PluginApiError with its code", async () => {
    const { rt, api, last } = setup();
    const p = api.model.solids() as Promise<unknown>;
    const m = last() as Extract<PluginMessage, { t: "req" }>;
    rt.receive({ t: "res", id: m.id, ok: false, error: "nope", code: "busy" });
    await expect(p).rejects.toMatchObject({ name: "PluginApiError", code: "busy" });
  });
});

describe("events (lane 15)", () => {
  it("an event the host refuses rejects `ready`, drops the listener and logs it", async () => {
    const { rt, api, sent, last } = setup();
    const got: unknown[] = [];
    const off = (api.events.on as (n: string, f: (p: unknown) => void) => (() => void) & { ready: Promise<void> })("selectionChanged", (p) => got.push(p));
    const m = last() as Extract<PluginMessage, { t: "req" }>;
    expect(m.method).toBe("events.subscribe");
    rt.receive({ t: "res", id: m.id, ok: false, error: "T may not use events.subscribe", code: "permission" });
    await expect(off.ready).rejects.toMatchObject({ name: "PluginApiError", code: "permission" });
    await new Promise((r) => setTimeout(r, 0));
    expect(sent.some((x) => x.t === "log" && x.level === "error" && /events.on\("selectionChanged"\)/.test(x.text))).toBe(true);
    rt.receive({ t: "evt", name: "selectionChanged", payload: { selection: [] } } as never);
    expect(got).toEqual([]);
  });

  it("an event that does not exist rejects without asking the host; an accepted one resolves", async () => {
    const { rt, api, sent, last } = setup();
    const on = api.events.on as (n: string, f: unknown) => (() => void) & { ready: Promise<void> };
    const before = sent.length;
    await expect(on("selectionChange", () => 0).ready).rejects.toMatchObject({ code: "invalid", message: expect.stringContaining('no event "selectionChange"') });
    await expect(on("documentChanged", 5).ready).rejects.toMatchObject({ code: "invalid" });
    expect(sent.length).toBe(before);
    const ok = on("documentChanged", () => 0);
    rt.receive({ t: "res", id: (last() as Extract<PluginMessage, { t: "req" }>).id, ok: true, value: null });
    await expect(ok.ready).resolves.toBeUndefined();
    expect(typeof ok).toBe("function");
  });
});

describe("batches", () => {
  it("collect ops with placeholders and commit them as one request", async () => {
    const { api, last, answer } = setup();
    const b = (api.model.batch as (n: string) => {
      op(o: unknown): { region(i?: number): unknown; object: unknown };
      makeComponent(i: unknown[], n: string): { node: unknown };
      rename(i: unknown, n: string): unknown;
      commit(): Promise<{ value(p: unknown): unknown; outcomes: unknown[] }>;
      length: number;
    })("Make a box");
    const rect = b.op({ op: "add_rectangle", sketch: 1, x: 0, y: 0, w: 1, h: 1 });
    const ext = b.op({ op: "extrude", sketch: 1, region: rect.region(0), distance: 1 });
    const comp = b.makeComponent([{ object: ext.object }], "Cube");
    b.rename({ node: comp.node }, "The cube");
    expect(b.length).toBe(4);
    const done = b.commit();
    const req = last() as Extract<PluginMessage, { t: "req" }>;
    expect(req.method).toBe("model.batch");
    expect(req.params).toEqual({
      name: "Make a box",
      ops: [
        { op: "add_rectangle", sketch: 1, x: 0, y: 0, w: 1, h: 1 },
        { op: "extrude", sketch: 1, region: { $ref: 0, path: "regions.0" }, distance: 1 },
        { op: "make_component", items: [{ object: { $ref: 1, path: "object" } }], name: "Cube" },
        { op: "rename", item: { node: { $ref: 2, path: "instance" } }, name: "The cube" },
      ],
    });
    answer([{ kind: "sketch_edited", regions: [5] }, { kind: "object_created", object: 9 }, { kind: "component_created", instance: 3, definition: 1 }, { kind: "renamed" }]);
    const r = await done;
    expect(r.value(comp.node)).toBe(3);
    expect(r.value(ext.object)).toBe(9);
    await expect(b.commit()).rejects.toThrow("already committed");
  });

  it("say which ops make what", () => {
    const { api } = setup();
    const b = (api.model.batch as (n: string) => { op(o: unknown): { object: unknown; node: unknown } })("x");
    const r = b.op({ op: "rename", item: { object: 1 }, name: "A" });
    expect(() => r.object).toThrow("a rename op makes no object");
    expect(() => (api.model.batch as (n: string) => unknown)("")).toThrow("name of 1 to 80");
  });

  it("build boxes and components as import ops", () => {
    const item = boxItem("Side", [1, 2, 3], [0.5, 0.25, 2]) as { positions: number[][]; faces: number[][][] };
    expect(item.positions[0]).toEqual([1, 2, 3]);
    expect(item.positions[7]).toEqual([1.5, 2.25, 5]);
    expect(item.faces).toHaveLength(6);
    expect(() => boxItem("Flat", [0, 0, 0], [1, 0, 1])).toThrow("positive");
    const op = componentOp({ name: "Cabinet", at: [1, 0, 0], instanceName: "Base", parts: [{ name: "Side", boxes: [{ min: [0, 0, 0], size: [1, 1, 1] }], copies: [{ at: [0, 0, 0], name: "Left side" }, { at: [2, 0, 0], name: "Right side" }] }] }) as {
      definitions: { name: string; children?: { definition: number; name?: string; xform: { translation: number[] } }[] }[];
      nodes: { definition: number; name: string; xform: { translation: number[] } }[];
    };
    expect(op.definitions.map((d) => d.name)).toEqual(["Side", "Cabinet"]);
    expect(op.definitions[1].children!.map((c) => [c.definition, c.name, c.xform.translation[0]])).toEqual([
      [0, "Left side", 0],
      [0, "Right side", 2],
    ]);
    expect(op.nodes[0]).toMatchObject({ definition: 1, name: "Base", xform: { translation: [1, 0, 0] } });
  });

  it("looks values up by path", () => {
    expect(outcomeValue({ a: [{ b: 4 }] }, "a.0.b")).toBe(4);
    expect(outcomeValue({ a: 1 }, "a.b")).toBeUndefined();
    expect(outcomeValue({ a: 1 }, "")).toEqual({ a: 1 });
  });
});

describe("callbacks", () => {
  it("run commands, panel handlers and tool handlers when the host calls, and answer with done", async () => {
    const { rt, api, sent, answer } = setup();
    let ran = 0;
    void api.ui.registerCommand({ id: "go", run: () => ++ran });
    answer(null);
    const seen: unknown[] = [];
    void api.ui.registerPanel({ id: "p", body: [], onAction: (id: unknown, values: unknown) => seen.push(["action", id, values]) });
    answer(null);
    void api.ui.registerTool({ id: "t", onEvent: (e: unknown) => seen.push(["event", e]), onValue: (text: unknown) => seen.push(["value", text]) });
    answer(null);
    rt.receive({ t: "call", id: 50, fn: "command", args: ["go"] });
    rt.receive({ t: "call", id: 51, fn: "panel.action", args: ["p", "generate", { width: 0.5 }] });
    rt.receive({ t: "call", id: 52, fn: "tool.event", args: ["t", { type: "move" }] });
    rt.receive({ t: "call", id: 53, fn: "tool.value", args: ["t", "8, 32"] });
    rt.receive({ t: "call", id: 54, fn: "command", args: ["missing"] });
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    expect(ran).toBe(1);
    expect(seen).toEqual([["action", "generate", { width: 0.5 }], ["event", { type: "move" }], ["value", "8, 32"]]);
    const dones = sent.filter((m) => m.t === "done");
    expect(dones.map((d) => [d.id, d.ok])).toEqual([
      [50, true],
      [51, true],
      [52, true],
      [53, true],
      [54, false],
    ]);
    expect(sent.some((m) => m.t === "log" && m.level === "error" && m.text.includes('no command "missing"'))).toBe(true);
  });

  it("answer pings and deliver events to listeners, subscribing once per event", () => {
    const { rt, api, sent } = setup();
    const got: unknown[] = [];
    api.events.on("documentChanged", (p: unknown) => got.push(p));
    const off = api.events.on("documentChanged", () => got.push("second")) as () => void;
    expect(sent.filter((m) => m.t === "req" && m.method === "events.subscribe")).toHaveLength(1);
    rt.receive({ t: "evt", name: "documentChanged", payload: { hash: "a" } });
    off();
    rt.receive({ t: "evt", name: "documentChanged", payload: { hash: "b" } });
    expect(got).toEqual([{ hash: "a" }, "second", { hash: "b" }]);
    rt.receive({ t: "ping", n: 4 });
    expect(sent[sent.length - 1]).toEqual({ t: "pong", n: 4 });
  });

  it("follow the document's units", () => {
    const { rt, api } = setup();
    expect(api.units.parse("19")).toBeCloseTo(0.019);
    expect(api.units.parse("nope")).toBeNull();
    rt.receive({ t: "evt", name: "unitsChanged", payload: { units: "in" } });
    expect(api.units.system()).toBe("in");
    expect(api.units.parse("2")).toBeCloseTo(0.0508);
  });

  it("is frozen", () => {
    const { api } = setup();
    expect(Object.isFrozen(api)).toBe(true);
    expect(Object.isFrozen(api.model)).toBe(true);
  });
});

describe("the wire (lane 16)", () => {
  it("a call over its budget rejects with too_large and sends nothing", async () => {
    const { api, sent } = setup(["storage"]);
    const store = (api.storage as unknown as { app: { set(k: string, v: unknown): Promise<unknown> } }).app;
    // 100 strings of 4 M characters: each within the per-string limit, 400 MB in all.
    const big = Array.from({ length: 100 }, () => "x".repeat(4 * 1024 * 1024));
    await expect(store.set("k", big)).rejects.toMatchObject({ name: "PluginApiError", code: "too_large" });
    expect(sent).toHaveLength(0);
  });

  it("an answer the app could not fit settles the call with a typed error instead of hanging", async () => {
    const { rt, api, last } = setup(["model.read"]);
    const p = api.model.solids() as Promise<unknown>;
    const m = last() as Extract<PluginMessage, { t: "req" }>;
    // A res frame whose body claims more items than it holds: unreadable on this side.
    const buf = new ArrayBuffer(14 + 11 + 5);
    const b = new Uint8Array(buf);
    b.set([0x53, 0x48, 0x57, 0x31, 12]);
    new DataView(buf).setFloat64(5, m.id, true);
    b[13] = 11;
    b.set([..."model.solid"].map((c) => c.charCodeAt(0)), 14);
    b.set([7, 0xff, 0xff, 0xff, 0x7f], 25);
    rt.receiveFrame(buf);
    await expect(p).rejects.toMatchObject({ name: "PluginApiError", code: "too_large" });
  });

  it("an app call it cannot read is answered with a failure, not dropped", () => {
    const { rt, sent } = setup();
    const buf = new ArrayBuffer(14 + 7 + 1);
    const b = new Uint8Array(buf);
    b.set([0x53, 0x48, 0x57, 0x31, 13]);
    new DataView(buf).setFloat64(5, 5, true);
    b[13] = 7;
    b.set([..."command"].map((c) => c.charCodeAt(0)), 14);
    b[21] = 99; // an unknown value tag
    rt.receiveFrame(buf);
    expect(sent.at(-1)).toMatchObject({ t: "done", id: 5, ok: false, error: expect.stringContaining("could not be read") });
  });

  it("a batch op is serialized once, when it is added (a getter is read once)", async () => {
    const { api, last } = setup();
    let reads = 0;
    const op = {
      op: "rename",
      item: { object: 1 },
      get name() {
        reads++;
        return `A${reads}`;
      },
    };
    const b = (api.model.batch as (n: string) => { op(o: unknown): unknown; commit(): Promise<unknown> })("Rename");
    b.op(op);
    void b.commit();
    const m = last() as Extract<PluginMessage, { t: "req" }>;
    expect(m.params).toEqual({ name: "Rename", ops: [{ op: "rename", item: { object: 1 }, name: "A1" }] });
    expect(reads).toBe(1);
    // And a bad op throws at the line that adds it.
    const c = (api.model.batch as (n: string) => { op(o: unknown): unknown })("Bad");
    expect(() => c.op({ op: "rename", name: () => 1 })).toThrow(/op 0\.name is a function/);
  });
});

describe("a batch's limits hold while it is built (lane 17)", () => {
  it("the op past 2,000 throws at once with too_large, after microseconds, not a million serializations", () => {
    const { api, sent } = setup();
    const b = (api.model.batch as (n: string) => { op(o: unknown): unknown; length: number })("A million ops");
    const t0 = performance.now();
    let thrown: unknown = null;
    try {
      for (let i = 0; i < 1e6; i++) b.op({ op: "rename", item: { object: 1 }, name: "x" });
    } catch (e) {
      thrown = e;
    }
    expect(thrown).toBeInstanceOf(PluginApiError);
    expect((thrown as PluginApiError).code).toBe("too_large");
    expect((thrown as Error).message).toContain(`at most ${BATCH_MAX_OPS} ops`);
    expect(b.length).toBe(BATCH_MAX_OPS);
    expect(performance.now() - t0).toBeLessThan(2000);
    expect(sent.length).toBe(0);
  });

  it("the ops together must fit one message: the one that would pass 4 MB throws", () => {
    const { api } = setup();
    const b = (api.model.batch as (n: string) => { op(o: unknown): unknown; length: number })("Big");
    const name = "n".repeat(1024 * 1024);
    let code: string | null = null;
    try {
      for (let i = 0; i < 10; i++) b.op({ op: "rename", item: { object: 1 }, name });
    } catch (e) {
      code = (e as { code?: string }).code ?? null;
    }
    expect(code).toBe("too_large");
    expect(b.length).toBeLessThan(4);
  });
});
