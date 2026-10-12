import { describe, expect, it } from "vitest";
import { boxItem, componentOp, createRuntime, outcomeValue } from "../src/runtime.ts";
import type { PluginMessage } from "../src/rpc.ts";

function setup(permissions = ["model.read", "model.write", "ui"]) {
  const sent: PluginMessage[] = [];
  const rt = createRuntime({
    send: (m) => sent.push(m),
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

  it("refuse arguments that are not plain data before sending", () => {
    const { api, sent } = setup();
    expect(() => (api.storage as unknown as { app: { set(k: string, v: unknown): unknown } }).app.set("k", () => 1)).toThrow("is a function");
    expect(sent).toHaveLength(0);
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
