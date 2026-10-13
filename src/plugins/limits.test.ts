// Lane 16: the limits that protect the app from a plugin, enforced on the app's side of the
// wire (a fake worker speaks as the plugin; the real sandbox is driven in Playwright by
// e2e/plugins-hostile.spec.ts). Every refusal is typed, settles the plugin's call, and leaves
// the document and the stores as they were.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { encodePluginMessage, type HostMessage } from "../../packages/plugin-api/src/rpc.ts";
import { FRAME_BUDGET } from "../../packages/plugin-api/src/wire.ts";
import { checkPackage, MAX_PROBLEMS } from "../../packages/plugin-api/src/source.ts";
import type { DocState, SceneData } from "../kernel/types";
import { fakeChannel, type FakeChannel } from "./fakeChannel";
import { LOG_TEXT_MAX } from "./host";
import { PluginManager, type AppBridge } from "./manager";
import { boundsOf, entitiesOfTargets, solidsOf } from "./model";
import { checkBody, imageSize, PANEL_BUDGET } from "./panel";
import { AppStore, APP_TOTAL, DocStore, DOC_TOTAL, jsonSize } from "./store";
import { checkToolView, TOOL_VIEW_POINTS } from "./toolView";

const flush = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};

const state = {
  stats: { objects: 1, faces: 6, edges: 12, vertices: 8, sketches: 1, regions: 0, holes: 0, shells: 1, watertight: true, euler: 2, genus: 0, volume_m3: 0.001, hash: "abc" },
  can_undo: false,
  can_redo: false,
  undo_depth: 0,
  redo_depth: 0,
  log_len: 1,
  log_hash: "l",
  objects: [7],
  solids: [{ id: 7, number: 1, min: [0, 0, 0], max: [0.1, 0.1, 0.1], volume: 0.001 }],
  sketches: [],
  groups: [],
  graph: { context: null, path: [], nodes: [], objects: [{ id: 7, parent: null, hidden: false, locked: false, visible: true, in_component: false }], meshes: [], sketches: [], definitions: [], tag_colors: {} },
  guides: [],
  dimensions: [],
  tags: {},
  object_info: [{ id: 7, name: "Leg", tag: null, material: null }],
  meshes: [],
  materials: [],
  scenes: [],
} as unknown as DocState;

function setup(hostOptions: Record<string, number> = {}) {
  const channels: FakeChannel[] = [];
  const statuses: { text: string; kind: string }[] = [];
  const signals: AbortSignal[] = [];
  const bridge = {
    scene: () => ({ state, mesh: {} }) as unknown as SceneData,
    kernel: () => null,
    settled: () => Promise.resolve(),
    units: () => "mm" as const,
    docName: () => null,
    selection: () => [],
    select: vi.fn(),
    applyBatch: vi.fn(async (_n: string, ops: unknown[], _p: string, signal?: AbortSignal) => {
      if (signal) signals.push(signal);
      return ops.map(() => ({ kind: "renamed" as const }));
    }),
    status: (text: string, kind: string) => void statuses.push({ text, kind }),
    dialog: vi.fn((_r: unknown, signal?: AbortSignal) => {
      if (signal) signals.push(signal);
      return new Promise<string | boolean | null>(() => undefined);
    }),
    saveFile: vi.fn(async (name: string) => ({ name })),
    pickFile: vi.fn(async () => null as never),
    activateTool: vi.fn(),
    activeTool: vi.fn(() => "select"),
    fetch: vi.fn(async () => new Response("hello", { status: 200 })),
  } satisfies AppBridge;
  const m = new PluginManager(bridge, null, {
    channel: () => {
      const c = fakeChannel();
      channels.push(c);
      return c;
    },
    hostOptions: { pingMs: 100000, budgetMs: 100000, killMs: 300000, callBudgetMs: 100000, ...hostOptions },
  });
  const send = (c: FakeChannel, method: string, params: unknown, id: number) => c.fromPlugin({ t: "req", id, method, params } as never);
  const req = async (c: FakeChannel, method: string, params: unknown = null, id = Math.floor(Math.random() * 1e9)) => {
    send(c, method, params, id);
    await flush();
    await flush();
    return c.answer(id) as Extract<HostMessage, { t: "res" }>;
  };
  return { m, bridge, channels, statuses, signals, send, req };
}

async function running(manifest: Record<string, unknown> = {}, hostOptions: Record<string, number> = {}) {
  const t = setup(hostOptions);
  const files = { "plugin.json": JSON.stringify({ id: "org.example.hostile", name: "Hostile", version: "1.0.0", api: 1, description: "", author: "Sawhorse", license: "MIT", permissions: [], ...manifest }), "main.js": "export {};" };
  const e = await t.m.loadSession(files);
  await t.m.enable(e.id);
  await flush();
  expect(t.m.get(e.id)?.status).toBe("running");
  return { ...t, c: t.channels[t.channels.length - 1], id: e.id };
}

/** A frame by hand: a req header for `method` and a body of `size` bytes in all. */
function forgedReq(id: number, method: string, size: number): ArrayBuffer {
  const buf = new ArrayBuffer(size);
  const b = new Uint8Array(buf);
  b.set([0x53, 0x48, 0x57, 0x31, 3]);
  new DataView(buf).setFloat64(5, id, true);
  b[13] = method.length;
  for (let i = 0; i < method.length; i++) b[14 + i] = method.charCodeAt(i);
  return buf;
}

beforeEach(() => localStorage.clear());
afterEach(() => vi.useRealTimers());

describe("the message budget", () => {
  it("a request over its budget is refused on its size (too_large), answered, and nothing runs", async () => {
    const { c, id, m } = await running({ permissions: ["storage"] });
    // A 400 MB storage.set as one frame would be 400 MB; this one is just over 4 MiB.
    c.fromPluginRaw(forgedReq(77, "storage.set", FRAME_BUDGET.default + 1));
    await flush();
    expect(c.answer(77)).toMatchObject({ ok: false, code: "too_large" });
    expect(new AppStore(id).keys()).toEqual([]);
    expect(m.get(id)!.status).toBe("running");
  });

  it("a message that is not a frame (a structured clone, a shared or resizable buffer) is refused and logged", async () => {
    const { c, id, m } = await running();
    c.fromPluginRaw({ t: "req", id: 1, method: "model.stats", params: null });
    if (typeof SharedArrayBuffer !== "undefined") c.fromPluginRaw(new SharedArrayBuffer(64));
    c.fromPluginRaw(new Uint8Array(64));
    await flush();
    const log = m.get(id)!.logs.map((l) => l.text).join("\n");
    expect(log).toMatch(/refused a message: a message must be one ArrayBuffer/);
  });

  it("an answer too large to send settles the call as too_large (file.open of a 33 MiB file)", async () => {
    const { c, req, bridge } = await running({ permissions: ["file"] });
    bridge.pickFile.mockResolvedValueOnce({ name: "big.bin", bytes: new Uint8Array(33 * 1024 * 1024) } as never);
    const r = await req(c, "file.open", {});
    expect(r).toMatchObject({ ok: false, code: "too_large" });
    // The frame that went out is the small refusal, not 33 MiB.
    expect(c.frames.at(-1)!.byteLength).toBeLessThan(4096);
  });

  it("an event too large to deliver is dropped with a note in the plugin's console", async () => {
    const { c, req, m, id } = await running({ permissions: ["model.read"] });
    await req(c, "events.subscribe", { name: "selectionChanged" });
    m.emit("selectionChanged", { selection: "x".repeat(FRAME_BUDGET.default) });
    expect(m.get(id)!.logs.some((l) => /a selectionChanged event was not delivered/.test(l.text))).toBe(true);
  });

  it("log lines and the last error are cut, whoever wrote them", async () => {
    const { m, id } = await running();
    m.host(id)!.log("error", "k".repeat(5_000_000));
    const e = m.get(id)!;
    expect(e.logs.at(-1)!.text.length).toBeLessThan(LOG_TEXT_MAX + 100);
    expect(e.lastError!.length).toBeLessThan(LOG_TEXT_MAX + 100);
  });
});

describe("rates and work in flight", () => {
  it("more than the rate is refused with busy; ten seconds' worth in one second stops the plugin", async () => {
    const { c, id, m } = await running({ permissions: ["model.read"] }, { ratePerSec: 10, burst: 5 });
    for (let i = 1; i <= 20; i++) c.fromPlugin({ t: "req", id: i, method: "model.stats", params: null });
    await flush();
    await flush();
    const answers = Array.from({ length: 20 }, (_, i) => c.answer(i + 1) as Extract<HostMessage, { t: "res" }> | undefined);
    expect(answers.filter((a) => a?.ok === false && a.code === "busy").length).toBeGreaterThanOrEqual(10);
    expect(m.get(id)!.status).toBe("running");
    for (let i = 100; i < 260; i++) c.fromPlugin({ t: "req", id: i, method: "model.stats", params: null });
    await flush();
    expect(m.get(id)!.status).toBe("stopped");
    expect(m.get(id)!.lastError).toMatch(/sent more than 100 messages in a second/);
  });

  it("batches are capped per second and in flight", async () => {
    const { c, req, bridge } = await running({ permissions: ["model.write"] }, { batchesPerSec: 2 });
    const op = { name: "R", ops: [{ op: "rename", item: { object: 7 }, name: "A" }] };
    expect((await req(c, "model.batch", op)).ok).toBe(true);
    expect((await req(c, "model.batch", op)).ok).toBe(true);
    expect(await req(c, "model.batch", op)).toMatchObject({ ok: false, code: "busy" });
    expect(bridge.applyBatch).toHaveBeenCalledTimes(2);
  });

  it("one dialog at a time per plugin; stopping the plugin takes its dialog and its queued batch away", async () => {
    const { c, send, signals, m, id, bridge } = await running({ permissions: ["ui", "model.write"] });
    bridge.applyBatch.mockImplementationOnce(async (_n, _o, _p, signal) => {
      signals.push(signal!);
      return new Promise(() => undefined);
    });
    send(c, "ui.prompt", { title: "Name?" }, 1);
    send(c, "model.batch", { name: "Slow", ops: [{ op: "rename", item: { object: 7 }, name: "A" }] }, 2);
    await flush();
    send(c, "ui.confirm", { title: "Sure?", text: "?" }, 3);
    await flush();
    expect(c.answer(3)).toMatchObject({ ok: false, code: "busy" });
    expect(signals.length).toBe(2);
    expect(signals.every((s) => !s.aborted)).toBe(true);
    m.disable(id);
    expect(signals.every((s) => s.aborted)).toBe(true);
  });

  it("time spent on a plugin's messages is budgeted: a plugin that keeps the app busy is stopped", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 9, 13, 12, 0, 0));
    const { c, m, id } = await running({ permissions: ["model.read"] }, { cpuMsPerSec: 0, cpuStrikes: 2 });
    for (let s = 0; s < 3 && m.get(id)!.status === "running"; s++) {
      c.fromPlugin({ t: "req", id: 10 + s, method: "model.stats", params: null });
      await flush();
      vi.setSystemTime(Date.now() + 1100);
    }
    expect(m.get(id)!.status).toBe("stopped");
    expect(m.get(id)!.lastError).toMatch(/kept the app busy/);
  });
});

describe("storage", () => {
  it("measures a value without stringifying it; byte arrays and over-quota values are refused at once", () => {
    const s = new AppStore("org.example.a");
    const t0 = performance.now();
    expect(() => s.set("k", new Uint8Array(32 * 1024 * 1024))).toThrow(/not byte arrays/);
    expect(() => s.set("k", "x".repeat(4 * 1024 * 1024))).toThrow(/over 256 KB/);
    expect(() => s.set("k", Array.from({ length: 100 }, () => "x".repeat(4 * 1024 * 1024)))).toThrow(/over 256 KB/);
    expect(performance.now() - t0).toBeLessThan(200);
    expect(s.keys()).toEqual([]);
    expect(jsonSize({ a: [1, "x\"y", null, true] }, 1e9)).toBe(JSON.stringify({ a: [1, 'x"y', null, true] }).length);
  });

  it("caps every plugin's stores together, app and document", () => {
    for (let i = 0; i < 4; i++) new AppStore(`org.example.p${i}`).set("k", "x".repeat(250 * 1024));
    expect(() => new AppStore("org.example.p9").set("k", "x".repeat(250 * 1024))).toThrow(/together would be over 1024 KB/);
    expect(APP_TOTAL).toBe(1024 * 1024);
    const d = new DocStore();
    for (let i = 0; i < 16; i++) d.set(`org.example.d${i}`, "k", "y".repeat(63 * 1024));
    expect(() => d.set("org.example.d99", "k", "y".repeat(63 * 1024))).toThrow(/together would be over 1024 KB/);
    expect(DOC_TOTAL).toBe(1024 * 1024);
  });

  it("a __proto__ key in a document's plugin data is a key, not a prototype", () => {
    const d = new DocStore();
    d.load(JSON.parse('{"org.example.a": {"__proto__": {"polluted": true}, "k": 1}, "__proto__": {"x": {}}}'));
    expect(d.get("org.example.a", "k")).toBe(1);
    expect(d.get("org.example.a", "__proto__")).toEqual({ polluted: true });
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
    expect(Object.getPrototypeOf(d.snapshot()["org.example.a"])).toBe(Object.prototype);
  });
});

describe("text and dialogs", () => {
  it("prompt, confirm and notify take text only, refused over their limits (never coerced or cut)", async () => {
    const { c, req, bridge } = await running({ permissions: ["ui"] });
    expect(await req(c, "ui.prompt", { title: ["x"] })).toMatchObject({ ok: false, code: "invalid" });
    expect(await req(c, "ui.confirm", { title: "x".repeat(81), text: "y" })).toMatchObject({ ok: false, code: "invalid" });
    expect(await req(c, "ui.notify", { text: "z".repeat(501) })).toMatchObject({ ok: false, code: "invalid" });
    expect(bridge.dialog).not.toHaveBeenCalled();
  });
});

describe("the network", () => {
  it("an answer that says it is over 10 MB is refused before reading; a stream is cut at 10 MB", async () => {
    const { c, req, bridge } = await running({ permissions: ["net"], net: ["api.example.com"] });
    let pulled = 0;
    const endless = () =>
      new ReadableStream<Uint8Array>({
        pull(ctl) {
          pulled++;
          ctl.enqueue(new Uint8Array(1024 * 1024));
        },
      });
    bridge.fetch.mockResolvedValueOnce(new Response(endless(), { headers: { "content-length": String(20 * 1024 * 1024) } }));
    expect(await req(c, "net.fetch", { url: "https://api.example.com/a" })).toMatchObject({ ok: false, code: "too_large" });
    expect(pulled).toBeLessThanOrEqual(2);
    bridge.fetch.mockResolvedValueOnce(new Response(endless()));
    const r = await new Promise<HostMessage | undefined>((resolve) => {
      c.fromPlugin({ t: "req", id: 555, method: "net.fetch", params: { url: "https://api.example.com/b" } });
      const tick = () => (c.answer(555) ? resolve(c.answer(555)) : setTimeout(tick, 5));
      tick();
    });
    expect(r).toMatchObject({ ok: false, code: "too_large" });
    expect(pulled).toBeLessThan(16);
  });

  it("at most four fetches wait at once per plugin", async () => {
    const { c, send, bridge } = await running({ permissions: ["net"], net: ["api.example.com"] });
    bridge.fetch.mockImplementation(() => new Promise(() => undefined));
    for (let i = 1; i <= 5; i++) send(c, "net.fetch", { url: "https://api.example.com/x" }, i);
    await flush();
    expect(c.answer(5)).toMatchObject({ ok: false, code: "busy" });
    expect(bridge.fetch).toHaveBeenCalledTimes(4);
  });
});

describe("panels and tool views", () => {
  const ui = { permissions: ["ui"], contributes: { panels: [{ id: "p", title: "P" }], tools: [{ id: "t", title: "T" }], commands: [{ id: "go", title: "Go" }] } };

  it("a panel has a budget in all: cells, text, images; a panel of 10,000 controls is refused before any is read", () => {
    const t0 = performance.now();
    expect(() => checkBody(Array.from({ length: 10_000 }, (_, i) => ({ type: "button", id: `b${i}`, label: "x" })))).toThrow(/at most 100 fields/);
    expect(performance.now() - t0).toBeLessThan(20);
    const rows = Array.from({ length: 500 }, () => Array.from({ length: 12 }, () => "c"));
    expect(() => checkBody([{ type: "table", columns: Array.from({ length: 12 }, (_, i) => `c${i}`), rows }])).toThrow(new RegExp(`at most ${PANEL_BUDGET.cells} table cells`));
    // 3,600 cells of 100 characters: within the cells, over the text.
    const wordy = Array.from({ length: 300 }, () => Array.from({ length: 12 }, () => "t".repeat(100)));
    expect(() => checkBody([{ type: "table", columns: Array.from({ length: 12 }, (_, i) => `c${i}`), rows: wordy }])).toThrow(/characters of text in all/);
  });

  it("images: SVG is refused, and a header bigger than 2048 px a side is refused before the browser decodes it", () => {
    const png = (w: number, h: number) => {
      const b = new Uint8Array(33);
      b.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
      new DataView(b.buffer).setUint32(16, w);
      new DataView(b.buffer).setUint32(20, h);
      return `data:image/png;base64,${btoa(String.fromCharCode(...b))}`;
    };
    expect(imageSize(Uint8Array.from(atob(png(16384, 16384).split(",")[1]), (ch) => ch.charCodeAt(0)))).toEqual({ width: 16384, height: 16384 });
    expect(() => checkBody([{ type: "image", src: png(16384, 16384), alt: "bomb" }])).toThrow(/16384 x 16384 pixels/);
    expect(() => checkBody([{ type: "image", src: png(64, 64), alt: "ok" }])).not.toThrow();
    expect(() => checkBody([{ type: "image", src: "data:image/svg+xml;base64,PHN2Zy8+", alt: "svg" }])).toThrow(/png, jpeg, gif or webp/);
  });

  it("registering the same command or tool again redraws nothing", async () => {
    const { c, req, m } = await running(ui);
    await req(c, "ui.registerCommand", { id: "go" });
    const v = m.version();
    await req(c, "ui.registerCommand", { id: "go" });
    await req(c, "ui.registerCommand", { id: "go" });
    expect(m.version()).toBe(v);
  });

  it("a tool view is checked once, only for the active tool, within its point budget", async () => {
    const { c, req, m, bridge } = await running(ui);
    await req(c, "ui.registerTool", { id: "t" });
    const key = "plugin:org.example.hostile/t";
    const heard: string[] = [];
    m.onToolView((k) => heard.push(k));
    const overlay = Array.from({ length: 200 }, () => ({ points: Array.from({ length: 1000 }, () => [0, 0, 0]) }));
    await req(c, "ui.toolView", { id: "t", view: { overlay } });
    expect(m.tools.get(key)!.checked).toBeNull();
    bridge.activeTool.mockReturnValue(key);
    await req(c, "ui.toolView", { id: "t", view: { overlay } });
    await req(c, "ui.toolView", { id: "t", view: { overlay } });
    await new Promise((r) => setTimeout(r, 40));
    expect(heard).toEqual([key]);
    const pts = m.toolView(key).overlay.reduce((n, o) => n + o.points.length, 0);
    expect(pts).toBe(TOOL_VIEW_POINTS);
    expect(checkToolView({ overlay }).overlay.reduce((n, o) => n + o.points.length, 0)).toBe(TOOL_VIEW_POINTS);
  });
});

describe("model reads stay linear", () => {
  it("bounds of 10,000 targets naming one group of 20,000 solids, and every solid's record, in well under a second", () => {
    const n = 20_000;
    const objects = Array.from({ length: n }, (_, i) => ({ id: i + 1, parent: 1, hidden: false, locked: false, visible: true, in_component: false }));
    const big = {
      ...state,
      objects: objects.map((o) => o.id),
      solids: objects.map((o) => ({ id: o.id, number: o.id, min: [o.id, 0, 0], max: [o.id + 1, 1, 1], volume: 1 })),
      object_info: objects.map((o) => ({ id: o.id, name: null, tag: null, material: null })),
      graph: { ...state.graph, nodes: [{ id: 1, kind: "group", parent: null, label: "G", name: "G", hidden: false, locked: false, visible: true, in_component: false }], objects },
    } as unknown as DocState;
    const t0 = performance.now();
    const targets = Array.from({ length: 10_000 }, () => ({ kind: "group", id: 1 }));
    const b = boundsOf(entitiesOfTargets(targets, big), big);
    expect(b).toEqual({ min: [1, 0, 0], max: [n + 1, 1, 1] });
    expect(solidsOf(big)).toHaveLength(n);
    expect(performance.now() - t0).toBeLessThan(1000);
  });
});

describe("plugin packages", () => {
  it("a module of 100,000 forbidden names is checked in well under a second, with at most 20 problems listed", () => {
    const src = "importScripts;\n".repeat(100_000);
    const t0 = performance.now();
    const r = checkPackage({ "main.js": src }, "main.js");
    expect(performance.now() - t0).toBeLessThan(2000);
    expect(r.problems.length).toBeLessThanOrEqual(MAX_PROBLEMS + 1);
    expect(r.problems.at(-1)).toMatch(/and 99980 more problems like these/);
  });

  it("a module over 4 MB of code is refused without being parsed", () => {
    const r = checkPackage({ "main.js": `// ${"x".repeat(4 * 1024 * 1024)}` }, "main.js");
    expect(r.problems[0]).toMatch(/larger than 4 MB of code/);
  });

  it("encodes as the sandbox does (the codec is the one the tests use)", () => {
    expect(encodePluginMessage({ t: "ready" }).byteLength).toBe(15);
  });
});
