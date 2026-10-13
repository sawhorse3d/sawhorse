// The host side of plugins with a fake worker: permissions enforced on every request, the
// watchdog, crashes, contributions, storage and the network allowlist. The real sandbox
// (workers, lockdown) is driven in Playwright: e2e/plugins-sandbox.spec.ts.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HostMessage } from "../../packages/plugin-api/src/rpc.ts";
import type { DocState, SceneData } from "../kernel/types";
import { fakeChannel, type FakeChannel } from "./fakeChannel";
import { PluginManager, type AppBridge } from "./manager";
import { grantMatches, packageDigest } from "./store";

const flush = async () => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
};

function manifest(over: Record<string, unknown> = {}) {
  return JSON.stringify({ id: "org.example.test", name: "Test", version: "1.0.0", api: 1, description: "", author: "Sawhorse", license: "MIT", permissions: [], ...over });
}

const files = (over: Record<string, unknown> = {}) => ({ "plugin.json": manifest(over), "main.js": 'import { ui } from "sawhorse";\nexport {};' });

const state = {
  stats: { objects: 1, faces: 6, edges: 12, vertices: 8, sketches: 1, regions: 0, holes: 0, shells: 1, watertight: true, euler: 2, genus: 0, volume_m3: 0.001, hash: "abc" },
  can_undo: true,
  can_redo: false,
  undo_depth: 1,
  redo_depth: 0,
  log_len: 3,
  log_hash: "l",
  objects: [7],
  solids: [{ id: 7, number: 1, min: [0, 0, 0], max: [0.1, 0.1, 0.1], volume: 0.001 }],
  sketches: [],
  groups: [],
  graph: { context: null, path: [], nodes: [], objects: [{ id: 7, parent: null, hidden: false, locked: false, visible: true, in_component: false }], meshes: [], sketches: [], definitions: [], tag_colors: {} },
  guides: [],
  dimensions: [],
  tags: {},
  object_info: [{ id: 7, name: "Leg", tag: null, material: "Oak" }],
  meshes: [],
  materials: [{ name: "Oak", color: [200, 150, 90, 255], texture: null }],
  scenes: [],
} as unknown as DocState;

function setup() {
  const channels: FakeChannel[] = [];
  const statuses: { text: string; kind: string; actions?: { label: string }[] }[] = [];
  const bridge = {
    scene: () => ({ state, mesh: {} }) as unknown as SceneData,
    kernel: () => null,
    settled: () => Promise.resolve(),
    units: () => "frac-in" as const,
    docName: () => "Bench.shz",
    selection: () => [{ kind: "object" as const, object: 7 }],
    select: vi.fn(),
    applyBatch: vi.fn(async (_name: string, ops: unknown[]) => ops.map(() => ({ kind: "renamed" as const }))),
    status: (text: string, kind: string, actions?: { label: string; run: () => void }[]) => statuses.push({ text, kind, actions }),
    dialog: vi.fn(async () => "typed"),
    saveFile: vi.fn(async (name: string) => ({ name })),
    pickFile: vi.fn(async () => null),
    activateTool: vi.fn(),
    activeTool: () => "select",
    fetch: vi.fn(async () => new Response("hello", { status: 200 })),
  } satisfies AppBridge;
  const m = new PluginManager(bridge, null, {
    channel: () => {
      const c = fakeChannel();
      channels.push(c);
      return c;
    },
    hostOptions: { pingMs: 100, budgetMs: 1000, killMs: 3000, callBudgetMs: 500 },
  });
  /** Send a request as the plugin; resolves to the host's answer. */
  const req = async (c: FakeChannel, method: string, params: unknown = null, id = Math.floor(Math.random() * 1e6)) => {
    c.fromPlugin({ t: "req", id, method, params });
    await flush();
    await flush();
    return c.answer(id) as Extract<HostMessage, { t: "res" }>;
  };
  return { m, bridge, channels, statuses, req };
}

beforeEach(() => localStorage.clear());
afterEach(() => vi.useRealTimers());

async function running(over: Record<string, unknown> = {}) {
  const t = setup();
  const e = await t.m.loadSession(files(over));
  await t.m.enable(e.id);
  await flush();
  expect(t.m.get(e.id)?.status).toBe("running");
  return { ...t, c: t.channels[t.channels.length - 1], id: e.id };
}

describe("permissions are enforced by the host", () => {
  it("a plugin without model.write cannot change the model", async () => {
    const { c, req, bridge } = await running({ permissions: ["model.read"] });
    const r = await req(c, "model.batch", { name: "Sneaky", ops: [{ op: "delete_object", object: 7 }] });
    expect(r).toMatchObject({ ok: false, code: "permission" });
    expect(r.ok === false && r.error).toContain('does not ask for the "model.write" permission');
    expect(bridge.applyBatch).not.toHaveBeenCalled();
  });

  it("a plugin without model.read reads nothing", async () => {
    const { c, req } = await running({ permissions: [] });
    for (const method of ["model.solids", "model.selection", "model.cutList", "model.tree", "model.stats"]) expect(await req(c, method)).toMatchObject({ ok: false, code: "permission" });
    expect(await req(c, "events.subscribe", { name: "documentChanged" })).toMatchObject({ ok: false, code: "permission" });
  });

  it("a plugin without net has no network, and with net only its listed hosts over https", async () => {
    const none = await running({ permissions: [] });
    expect(await none.req(none.c, "net.fetch", { url: "https://example.com/" })).toMatchObject({ ok: false, code: "permission" });
    expect(none.bridge.fetch).not.toHaveBeenCalled();
    const net = await running({ permissions: ["net"], net: ["api.example.com"] });
    expect(await net.req(net.c, "net.fetch", { url: "https://evil.example.org/" })).toMatchObject({ ok: false, code: "permission" });
    expect(await net.req(net.c, "net.fetch", { url: "http://api.example.com/" })).toMatchObject({ ok: false, code: "permission" });
    const ok = await net.req(net.c, "net.fetch", { url: "https://api.example.com/v1?q=1", headers: { Cookie: "x", Accept: "text/plain" } });
    expect(ok.ok).toBe(true);
    const [url, init] = net.bridge.fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("https://api.example.com/v1?q=1");
    expect(init).toMatchObject({ credentials: "omit", redirect: "error", headers: { Accept: "text/plain" } });
    expect(ok.ok && new TextDecoder().decode((ok.value as { body: Uint8Array }).body)).toBe("hello");
  });

  it("unknown methods and events are refused with a code", async () => {
    const { c, req } = await running({ permissions: ["model.read"] });
    expect(await req(c, "model.dropTables")).toMatchObject({ ok: false, code: "unknown" });
    expect(await req(c, "events.subscribe", { name: "keystrokes" })).toMatchObject({ ok: false, code: "invalid" });
  });

  it("malformed messages are refused and logged, not obeyed", async () => {
    const { m, c, id } = await running({ permissions: ["model.write"] });
    c.fromPlugin({ t: "req", id: 1, method: "model.batch", params: { f: () => 1 } });
    c.fromPlugin({ t: "nonsense" });
    await flush();
    expect(m.get(id)!.logs.map((l) => l.text).join("\n")).toContain("refused a message");
  });
});

describe("model requests", () => {
  it("reads the model in the API's shapes", async () => {
    const { c, req } = await running({ permissions: ["model.read"] });
    expect((await req(c, "model.solids")).ok && (await req(c, "model.solids", null, 9)).ok).toBe(true);
    const solids = await req(c, "model.solids", null, 10);
    expect(solids.ok && solids.value).toEqual([expect.objectContaining({ id: 7, label: "Leg", material: "Oak", editable: true })]);
    const sel = await req(c, "model.selection", null, 11);
    expect(sel.ok && sel.value).toEqual([{ kind: "solid", id: 7 }]);
    const doc = await req(c, "model.document", null, 12);
    expect(doc.ok && doc.value).toMatchObject({ name: "Bench.shz", units: "frac-in", hash: "abc" });
    expect(await req(c, "model.select", { items: [{ kind: "solid", id: 99 }] }, 13)).toMatchObject({ ok: false, code: "invalid" });
    const b = await req(c, "model.bounds", { items: null }, 14);
    expect(b.ok && b.value).toEqual({ min: [0, 0, 0], max: [0.1, 0.1, 0.1] });
  });

  it("checks a batch before the kernel sees it", async () => {
    const { c, req, bridge } = await running({ permissions: ["model.write"] });
    expect(await req(c, "model.batch", { name: "x", ops: [{ op: "undo" }] })).toMatchObject({ ok: false, code: "invalid" });
    expect(await req(c, "model.batch", { name: "x", ops: [{ op: "set_context", node: 1 }] })).toMatchObject({ ok: false, code: "invalid" });
    expect(await req(c, "model.batch", { name: "", ops: [{ op: "delete_guides" }] })).toMatchObject({ ok: false, code: "invalid" });
    expect(await req(c, "model.batch", { name: "x", ops: [] })).toMatchObject({ ok: false, code: "invalid" });
    expect(bridge.applyBatch).not.toHaveBeenCalled();
    const ok = await req(c, "model.batch", { name: "Rename", ops: [{ op: "rename", item: { object: 7 }, name: "Leg" }] });
    expect(ok).toMatchObject({ ok: true, value: [{ kind: "renamed" }] });
    expect(bridge.applyBatch).toHaveBeenCalledWith("Rename", [{ op: "rename", item: { object: 7 }, name: "Leg" }], "Test", expect.any(AbortSignal));
    bridge.applyBatch.mockRejectedValueOnce(new Error("no object #99"));
    expect(await req(c, "model.batch", { name: "Bad", ops: [{ op: "delete_object", object: 99 }] })).toMatchObject({ ok: false, code: "refused", error: "no object #99" });
  });
});

describe("contributions", () => {
  const ui = { permissions: ["ui"], contributes: { commands: [{ id: "go", title: "Go" }], panels: [{ id: "p", title: "Panel" }], tools: [{ id: "t", title: "Tool", vcb: "Count" }], shortcuts: [{ command: "go", key: "Alt+Shift+G" }] } };

  it("only what plugin.json declares can be registered, and commands run in the plugin", async () => {
    const { m, c, req } = await running(ui);
    expect(await req(c, "ui.registerCommand", { id: "other" })).toMatchObject({ ok: false, code: "invalid" });
    expect(await req(c, "ui.registerCommand", { id: "go" })).toMatchObject({ ok: true });
    expect([...m.commands.values()][0]).toMatchObject({ title: "Go", shortcut: "Alt+Shift+G" });
    void m.runCommand("org.example.test/go");
    await flush();
    expect(c.posted.find((p) => p.t === "call")).toMatchObject({ fn: "command", args: ["go"] });
  });

  it("panels are checked field by field", async () => {
    const { m, c, req } = await running(ui);
    expect(await req(c, "ui.registerPanel", { id: "p", body: [{ type: "html", html: "<b>x</b>" }] })).toMatchObject({ ok: false, code: "invalid" });
    expect(await req(c, "ui.registerPanel", { id: "p", body: [{ type: "image", src: "javascript:alert(1)", alt: "x" }] })).toMatchObject({ ok: false, code: "invalid" });
    expect(await req(c, "ui.registerPanel", { id: "p", body: [{ type: "number", id: "w", label: "Width", unit: "length", value: 0.5 }, { type: "button", id: "go", label: "Go" }] })).toMatchObject({ ok: true });
    void m.panelEvent("org.example.test/p", "change", "w", 0.75);
    await flush();
    expect(m.panels.get("org.example.test/p")!.fields[0]).toMatchObject({ value: 0.75 });
    const call = c.posted.filter((p) => p.t === "call").pop();
    expect(call).toMatchObject({ fn: "panel.change", args: ["p", "w", { w: 0.75 }] });
  });

  it("a plugin's contributions go when it stops", async () => {
    const { m, c, req, id } = await running(ui);
    await req(c, "ui.registerTool", { id: "t" });
    await req(c, "ui.registerCommand", { id: "go" });
    expect(m.tools.size + m.commands.size).toBe(2);
    m.disable(id);
    expect(m.tools.size + m.commands.size).toBe(0);
    expect(c.terminated).toBe(true);
    expect(m.get(id)!.status).toBe("disabled");
  });
});

describe("the watchdog", () => {
  it("reports a plugin stuck in a loop, then stops it; the app carries on", async () => {
    vi.useFakeTimers();
    const { m, c, statuses, id } = await running({ permissions: ["ui"], contributes: { commands: [{ id: "spin", title: "Spin" }] } });
    c.fromPlugin({ t: "req", id: 1, method: "ui.registerCommand", params: { id: "spin" } });
    await flush();
    const pending = m.runCommand("org.example.test/spin");
    c.alive = false; // while(true) {} in the command: no more pongs
    await vi.advanceTimersByTimeAsync(1200);
    expect(m.get(id)!.status).toBe("unresponsive");
    expect(statuses.some((s) => s.text.includes('is not responding (running "Spin")') && s.actions?.[0].label === "Stop")).toBe(true);
    await vi.advanceTimersByTimeAsync(2000);
    expect(c.terminated).toBe(true);
    expect(m.get(id)!.status).toBe("stopped");
    expect(m.get(id)!.lastError).toContain("did not respond");
    expect(m.commands.size).toBe(0);
    expect(statuses.some((s) => s.actions?.[0].label === "Reload")).toBe(true);
    await pending; // the command's promise settled (failed), nothing hangs
  });

  it("a plugin that answers again is running again", async () => {
    vi.useFakeTimers();
    const { m, c, id } = await running();
    c.alive = false;
    await vi.advanceTimersByTimeAsync(1200);
    expect(m.get(id)!.status).toBe("unresponsive");
    c.alive = true;
    await vi.advanceTimersByTimeAsync(300);
    expect(m.get(id)!.status).toBe("running");
  });

  it("Stop ends an unresponsive plugin at once", async () => {
    vi.useFakeTimers();
    const { m, c, id } = await running();
    c.alive = false;
    await vi.advanceTimersByTimeAsync(1200);
    m.stopNow(id);
    expect(c.terminated).toBe(true);
    expect(m.get(id)!.status).toBe("stopped");
  });

  it("a worker that fails is crashed, and one whose entry throws did not start", async () => {
    const { m, c, id } = await running();
    c.failWorker("out of memory");
    await flush();
    expect(m.get(id)!.status).toBe("crashed");
    expect(m.get(id)!.lastError).toContain("out of memory");

    const t = setup();
    const e = await t.m.loadSession(files({ id: "org.example.broken" }));
    const start = t.m.enable(e.id);
    t.channels[0].onInit = "SyntaxError: Unexpected token";
    // The fake answers at post time: send the failure by hand.
    t.channels[0].fromPlugin({ t: "fail", error: "SyntaxError: Unexpected token" });
    await start;
    expect(t.m.get(e.id)!.status).toBe("failed");
    expect(t.m.get(e.id)!.lastError).toContain("Unexpected token");
  });
});

describe("loading and grants", () => {
  it("lists a broken package with its errors and never starts it", async () => {
    const { m, channels } = setup();
    const e = await m.loadSession({ "plugin.json": manifest({ id: "Bad Id" }), "main.js": "" });
    expect(e.status).toBe("invalid");
    expect(e.errors.join()).toContain("reverse-DNS");
    await expect(m.enable(e.id)).rejects.toThrow();
    const f = await m.loadSession({ "plugin.json": manifest({ id: "org.example.remote" }), "main.js": 'import x from "https://cdn.example.com/x.js";' });
    expect(f.status).toBe("invalid");
    expect(f.errors.join()).toContain('may import only "sawhorse"');
    expect(channels).toHaveLength(0);
  });

  it("remembers what was granted; an update asking for more is off until granted again", async () => {
    const { m, id } = await running({ permissions: ["ui"] });
    expect(JSON.parse(localStorage.getItem("sawhorse.plugins.enabled")!)[id].permissions).toEqual(["ui"]);
    const again = setup();
    const same = await again.m.loadSession(files({ permissions: ["ui"] }));
    expect(same.enabled).toBe(true);
    const more = await again.m.loadSession(files({ permissions: ["ui", "model.write"] }));
    expect(more.enabled).toBe(false);
    expect(more.lastError).toContain("asks for more (model.write)");
    m.dispose();
  });

  // The verifier: a zip with the same id and the same permissions as an enabled plugin
  // started Running at once with the old grant and no dialog. A grant is now bound to the
  // files the person enabled.
  it("a grant is for the files enabled: other files with the same id and permissions are off until enabled again", async () => {
    const { m, id } = await running({ permissions: ["model.read"] });
    const grant = JSON.parse(localStorage.getItem("sawhorse.plugins.enabled")!)[id];
    expect(grant).toMatchObject({ permissions: ["model.read"], source: "session" });
    expect(grant.digest).toMatch(/^[0-9a-f]{64}$/);
    const again = setup();
    const evil = await again.m.loadSession({ "plugin.json": manifest({ permissions: ["model.read"] }), "main.js": 'import { model } from "sawhorse";\nawait model.solids();' });
    expect(evil.enabled).toBe(false);
    expect(evil.status).toBe("disabled");
    expect(evil.lastError).toContain("not the files you enabled");
    expect(again.channels).toHaveLength(0);
    // The person can still enable it, which binds the grant to these files.
    await again.m.enable(id);
    await flush();
    expect(again.m.get(id)!.status).toBe("running");
    expect(JSON.parse(localStorage.getItem("sawhorse.plugins.enabled")!)[id].digest).toBe(again.m.get(id)!.digest);
    expect(again.m.get(id)!.digest).not.toBe(grant.digest);
    m.dispose();
    again.m.dispose();
  });

  it("a zip claiming a bundled sample's id does not inherit the sample's grant", async () => {
    const SAMPLE = "com.sawhorse3d.model-report";
    const first = setup();
    await first.m.init();
    await first.m.enable(SAMPLE);
    await flush();
    expect(first.m.get(SAMPLE)!.status).toBe("running");
    first.m.dispose();
    // Next start: the sample is enabled and starts again.
    const t = setup();
    await t.m.init();
    await flush();
    expect(t.m.get(SAMPLE)!.status).toBe("running");
    const sample = t.m.get(SAMPLE)!;
    const evilFiles = { ...sample.pkg!.files, "plugin.json": sample.pkg!.files["plugin.json"].replace('"Model report"', '"Model report (evil)"'), "main.js": 'import { model } from "sawhorse";\nconst s = await model.solids();' };
    const evil = await t.m.loadSession(evilFiles);
    expect(evil.name).toBe("Model report (evil)");
    expect(evil.enabled).toBe(false);
    expect(evil.status).toBe("disabled");
    expect(evil.replaces).toBe("Model report");
    expect(evil.lastError).toContain("takes the place of the bundled Model report");
    // The sample's worker was stopped and nothing new started.
    expect(t.channels.filter((c) => !c.terminated)).toHaveLength(0);
    t.m.dispose();
  });

  it("a bundled sample keeps its grant when the app updates it; an unpacked folder keeps its grant while it is edited", () => {
    const digest = "a".repeat(64);
    // Same files: always.
    expect(grantMatches({ version: "1", permissions: [], digest, source: "installed" }, { digest, source: "installed" })).toBe(true);
    // Installed or session: only the same files.
    expect(grantMatches({ version: "1", permissions: [], digest, source: "installed" }, { digest: "b".repeat(64), source: "installed" })).toBe(false);
    expect(grantMatches({ version: "1", permissions: [], digest, source: "bundled" }, { digest: "b".repeat(64), source: "session" })).toBe(false);
    // Bundled: the app's own code, updated with the app.
    expect(grantMatches({ version: "1", permissions: [], digest, source: "bundled" }, { digest: "b".repeat(64), source: "bundled" })).toBe(true);
    // A grant from before digests: bundled only.
    expect(grantMatches({ version: "1", permissions: [] }, { digest, source: "bundled" })).toBe(true);
    expect(grantMatches({ version: "1", permissions: [] }, { digest, source: "installed" })).toBe(false);
    // Unpacked: the same folder, whatever its files are now.
    expect(grantMatches({ version: "1", permissions: [], digest, source: "unpacked", location: "/plugins/a" }, { digest: "c".repeat(64), source: "unpacked", location: "/plugins/a" })).toBe(true);
    expect(grantMatches({ version: "1", permissions: [], digest, source: "unpacked", location: "/plugins/a" }, { digest: "c".repeat(64), source: "unpacked", location: "/plugins/b" })).toBe(false);
    expect(grantMatches(undefined, { digest, source: "bundled" })).toBe(false);
  });

  it("the digest covers every file, its path and its contents", () => {
    const base = { "plugin.json": "{}", "main.js": "export {};", "README.md": "# Hi" };
    const d = packageDigest(base);
    expect(packageDigest({ ...base })).toBe(d);
    expect(packageDigest({ "README.md": "# Hi", "main.js": "export {};", "plugin.json": "{}" })).toBe(d);
    expect(packageDigest({ ...base, "main.js": "export {}; " })).not.toBe(d);
    expect(packageDigest({ ...base, "README.md": "# Hi!" })).not.toBe(d);
    expect(packageDigest({ ...base, "lib/x.js": "" })).not.toBe(d);
    // Moving text between a path and its contents changes it too.
    expect(packageDigest({ ab: "c" })).not.toBe(packageDigest({ a: "bc" }));
  });
});

describe("storage", () => {
  it("keeps an app store per plugin and a document store that travels in the .shz", async () => {
    const { m, c, req } = await running({ permissions: ["storage"] });
    expect(await req(c, "storage.set", { scope: "app", key: "units", value: "mm" })).toMatchObject({ ok: true });
    expect(await req(c, "storage.get", { scope: "app", key: "units" }, 2)).toMatchObject({ ok: true, value: "mm" });
    expect(await req(c, "storage.set", { scope: "app", key: "big", value: "x".repeat(300 * 1024) }, 3)).toMatchObject({ ok: false, code: "invalid" });
    expect(await req(c, "storage.set", { scope: "document", key: "last", value: { id: 4 } }, 4)).toMatchObject({ ok: true });
    // Lane 16: the text the kernel writes as plugin-data.json, and reads back on open.
    const saved = m.doc.json()!;
    expect(JSON.parse(saved)).toEqual({ version: 1, plugins: { "org.example.test": { last: { id: 4 } } } });
    m.doc.clear();
    expect(await req(c, "storage.get", { scope: "document", key: "last" }, 5)).toMatchObject({ ok: true, value: null });
    m.doc.loadJson(saved);
    expect(await req(c, "storage.get", { scope: "document", key: "last" }, 6)).toMatchObject({ ok: true, value: { id: 4 } });
    m.doc.loadJson("not json");
    expect(await req(c, "storage.get", { scope: "document", key: "last" }, 7)).toMatchObject({ ok: true, value: null });
    m.doc.clear();
    expect(m.doc.json()).toBeNull();
  });

  it("saves files only through the app's dialog, with a plain name", async () => {
    const { c, req, bridge } = await running({ permissions: ["file"] });
    expect(await req(c, "file.save", { name: "../../evil.sh", data: "x" })).toMatchObject({ ok: false, code: "invalid" });
    const r = await req(c, "file.save", { name: "report.md", data: "# Report", mime: "text/markdown" }, 2);
    expect(r).toMatchObject({ ok: true, value: { name: "report.md" } });
    const [, bytes, type] = bridge.saveFile.mock.calls[0] as unknown as [string, Uint8Array, { extensions: string[]; mime: string }];
    expect(new TextDecoder().decode(bytes)).toBe("# Report");
    expect(type).toMatchObject({ extensions: [".md"], mime: "text/markdown" });
  });
});
