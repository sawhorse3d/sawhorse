// The host side of plugins with a fake worker: permissions enforced on every request, the
// watchdog, crashes, contributions, storage and the network allowlist. The real sandbox
// (workers, lockdown) is driven in Playwright: e2e/plugins-sandbox.spec.ts.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HostMessage } from "../../packages/plugin-api/src/rpc.ts";
import type { DocState, SceneData } from "../kernel/types";
import { NeedsUserActivation, pickFile, saveFile } from "../io/files";
import { fakeChannel, type FakeChannel } from "./fakeChannel";
import { PERMISSION_TEXT } from "../../packages/plugin-api/src/manifest.ts";
import { fakeHost } from "../tools/fakeHost";
import { PluginTool } from "../tools/plugin";
import { METHODS } from "./host";
import { BATCH_MAX_OPS } from "../../packages/plugin-api/src/runtime.ts";
import { batchRefusedWithoutRead, MAX_BATCH_OPS, permissionLines, PluginManager, type AppBridge } from "./manager";
import { grantCovers, grantMatches, loadGrants, packageDigest, trimGrant } from "./store";
import { checkToolView } from "./toolView";

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

function setup(hostOptions: Record<string, number> = {}) {
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
    hostOptions: { pingMs: 100, budgetMs: 1000, killMs: 3000, callBudgetMs: 500, ...hostOptions },
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

async function running(over: Record<string, unknown> = {}, hostOptions: Record<string, number> = {}) {
  const t = setup(hostOptions);
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
    // Lane 17 (F5): without model.read the kernel's reason (which names the model) stays out.
    const bad = await req(c, "model.batch", { name: "Bad", ops: [{ op: "delete_object", object: 99 }] });
    expect(bad).toMatchObject({ ok: false, code: "refused" });
    expect(bad.ok === false && bad.error).toMatch(/^the batch was refused; nothing changed/);
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

  it("lane 16 fixes: a pause of the app's own thread is not the plugin's; a plugin still stuck after it is stopped", async () => {
    vi.useFakeTimers();
    const { m, c, id } = await running();
    c.alive = false;
    // The app's thread is held for 48 s (no timer runs, no pong is read), then runs again.
    vi.setSystemTime(Date.now() + 48_000);
    await vi.advanceTimersByTimeAsync(100);
    expect(m.get(id)!.status).toBe("running");
    expect(c.terminated).toBe(false);
    // The plugin answers again: nothing happened to it.
    c.alive = true;
    await vi.advanceTimersByTimeAsync(300);
    expect(m.get(id)!.status).toBe("running");
    // A plugin that is really stuck through a pause is still stopped, a budget after it.
    c.alive = false;
    vi.setSystemTime(Date.now() + 48_000);
    await vi.advanceTimersByTimeAsync(100);
    expect(c.terminated).toBe(false);
    await vi.advanceTimersByTimeAsync(3200);
    expect(c.terminated).toBe(true);
    expect(m.get(id)!.lastError).toContain("did not respond for 3 s");
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

describe("tool events carry the model only with model.read (lane 17, F1)", () => {
  const tooled = (permissions: string[]) => ({ permissions, contributes: { tools: [{ id: "t", title: "Tool" }] } });
  const KEY = "plugin:org.example.test/t";
  // A resolved click on solid 7's face 3, as the PluginTool would send it with the model in it.
  const event = {
    type: "click",
    x: 10,
    y: 20,
    shift: true,
    ctrl: false,
    alt: false,
    ray: { origin: [1, -5, 2], dir: [0, 1, 0] },
    snap: { point: [0.1, 0.05, 0.1], kind: "on-face", tip: "On Face", surface: { at: { kind: "face", solid: 7, face: 3 }, frame: { origin: [0, 0, 0.1], normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }, point: [0.1, 0.05, 0.1] }, entity: { kind: "face", solid: 7, face: 3 } },
    picked: { entity: { kind: "solid", id: 7 }, point: [0.1, 0.05, 0.1], surface: null },
    // Anything the app might add later is not passed on by default either.
    later: { solid: 7 },
  };
  const lastEvent = (c: FakeChannel) => c.posted.filter((p) => p.t === "call" && p.fn === "tool.event").pop() as Extract<HostMessage, { t: "call" }> | undefined;

  it("a ui-only plugin's tool gets the pointer and the eye ray, never a snap or a pick from the model", async () => {
    const { m, c, req } = await running(tooled(["ui"]));
    expect(await req(c, "ui.registerTool", { id: "t" })).toMatchObject({ ok: true });
    expect(m.toolSeesModel(KEY)).toBe(false);
    void m.toolCall(KEY, "tool.event", event as never);
    await flush();
    const sent = lastEvent(c)!.args[1];
    expect(sent).toEqual({ type: "click", x: 10, y: 20, shift: true, ctrl: false, alt: false, ray: { origin: [1, -5, 2], dir: [0, 1, 0] }, snap: null, picked: null });
    expect(JSON.stringify(sent)).not.toMatch(/solid|face|"id"/);
  });

  it("with model.read the same event arrives whole", async () => {
    const { m, c, req } = await running(tooled(["ui", "model.read"]));
    await req(c, "ui.registerTool", { id: "t" });
    expect(m.toolSeesModel(KEY)).toBe(true);
    void m.toolCall(KEY, "tool.event", event as never);
    await flush();
    expect(lastEvent(c)!.args[1]).toMatchObject({ snap: { kind: "on-face", entity: { kind: "face", solid: 7, face: 3 } }, picked: { entity: { kind: "solid", id: 7 } } });
  });

  it("a ui-only tool's own inference constraints still give it a point, worked out from the ray and its own values only", async () => {
    const { m, c, req } = await running(tooled(["ui"]));
    await req(c, "ui.registerTool", { id: "t" });
    // Its own plane: where the eye ray meets it, whatever the model holds there.
    await req(c, "ui.toolView", { id: "t", view: { inference: { plane: { origin: [0, 3, 0], normal: [0, 1, 0] } } } });
    void m.toolCall(KEY, "tool.event", event as never);
    await flush();
    expect(lastEvent(c)!.args[1]).toMatchObject({ snap: { point: [1, 3, 2], kind: "free", tip: "", surface: null, entity: null }, picked: null });
    // Its own locked line: the line's point nearest the ray.
    await req(c, "ui.toolView", { id: "t", view: { inference: { start: [1, 0, 0], lock: { dir: [0, 0, 1], kind: "axis-z" } } } });
    void m.toolCall(KEY, "tool.event", event as never);
    await flush();
    expect(lastEvent(c)!.args[1]).toMatchObject({ snap: { point: [1, 0, 2], kind: "axis-z", tip: "On Blue Axis", surface: null, entity: null }, picked: null });
    // A ray along the line, or one that misses the plane: no point.
    await req(c, "ui.toolView", { id: "t", view: { inference: { plane: { origin: [0, -9, 0], normal: [0, 1, 0] } } } });
    void m.toolCall(KEY, "tool.event", event as never);
    await flush();
    expect(lastEvent(c)!.args[1]).toMatchObject({ snap: null, picked: null });
  });

  it("other tool callbacks carry only what the person typed", async () => {
    const { m, c, req } = await running(tooled(["ui"]));
    await req(c, "ui.registerTool", { id: "t" });
    void m.toolCall(KEY, "tool.value", "3/4");
    await flush();
    expect(c.posted.filter((p) => p.t === "call").pop()).toMatchObject({ fn: "tool.value", args: ["t", "3/4"] });
  });
});

describe("files a plugin saves and opens (lane 17, F2)", () => {
  it("refuses names that would run when opened, whatever comes before the last dot", async () => {
    const { c, req, bridge } = await running({ permissions: ["file"] });
    for (const name of ["setup.exe", "SETUP.EXE", "report.txt.exe", "fix.bat", "fix.cmd", "a.ps1", "a.vbs", "a.js", "a.hta", "a.lnk", "a.scr", "a.msi", "a.jar", "run.sh", "a.command", "a.app", "page.html", "page.htm", "evil.exe.", "evil.exe . "]) {
      const r = await req(c, "file.save", { name, data: "x" });
      expect(r, name).toMatchObject({ ok: false, code: "invalid" });
    }
    expect(bridge.saveFile).not.toHaveBeenCalled();
    for (const name of ["report.md", "cut list.csv", "panel.svg", "plan.dxf", "sheet.pdf", "data.json", "notes"]) expect(await req(c, "file.save", { name, data: "x" }), name).toMatchObject({ ok: true });
  });

  it("asks the person, naming the plugin, the file and its size, before a browser download; no means nothing is saved", async () => {
    const { c, req, bridge } = await running({ permissions: ["file"] });
    // The bridge's browser fallback: ask, then download.
    const downloaded: string[] = [];
    bridge.saveFile.mockImplementation((async (name: string, _b: Uint8Array, _t: unknown, opts?: { confirmDownload?: () => Promise<boolean> }) => {
      if (opts?.confirmDownload && !(await opts.confirmDownload())) return null;
      downloaded.push(name);
      return { name };
    }) as never);
    bridge.dialog.mockResolvedValueOnce(false as never);
    expect(await req(c, "file.save", { name: "report.md", data: "fourteen bytes" })).toMatchObject({ ok: true, value: null });
    expect(bridge.dialog).toHaveBeenLastCalledWith({ kind: "download", plugin: "Test", name: "report.md", bytes: 14 }, expect.any(AbortSignal));
    expect(downloaded).toEqual([]);
    bridge.dialog.mockResolvedValueOnce(true as never);
    expect(await req(c, "file.save", { name: "report.md", data: "fourteen bytes" })).toMatchObject({ ok: true, value: { name: "report.md" } });
    expect(downloaded).toEqual(["report.md"]);
  });

  it("file.open without the person's click is refused with a typed error, not left waiting", async () => {
    const { c, req, bridge } = await running({ permissions: ["file"] });
    bridge.pickFile.mockRejectedValueOnce(new NeedsUserActivation() as never);
    const r = await req(c, "file.open", { extensions: [".csv"] });
    expect(r).toMatchObject({ ok: false, code: "user_activation" });
    expect(r.ok === false && r.error).toContain("right after the person clicks");
    // The slot is free: the next one is asked, not refused as busy.
    expect(await req(c, "file.open", { extensions: [".csv"] })).toMatchObject({ ok: true, value: null });
  });

  it("a request waiting on the person is given up after waitMs: answered timeout, its work aborted, its slot free", async () => {
    vi.useFakeTimers();
    const { c, req, bridge } = await running({ permissions: ["file", "ui"] }, { waitMs: 2000 });
    let seen: AbortSignal | undefined;
    bridge.pickFile.mockImplementationOnce(((_t: unknown, opts?: { signal?: AbortSignal }) => {
      seen = opts?.signal;
      return new Promise(() => undefined); // a file input that never settles
    }) as never);
    c.fromPlugin({ t: "req", id: 41, method: "file.open", params: { extensions: [".csv"] } });
    await flush();
    expect(c.answer(41)).toBeUndefined();
    expect(await req(c, "file.open", {}, 42)).toMatchObject({ ok: false, code: "busy" });
    await vi.advanceTimersByTimeAsync(2100);
    expect(c.answer(41)).toMatchObject({ ok: false, code: "timeout" });
    expect(seen?.aborted).toBe(true);
    expect(await req(c, "file.open", {}, 43)).toMatchObject({ ok: true, value: null });
    // A dialog the same way (it closes: its signal aborts).
    let dialogSignal: AbortSignal | undefined;
    bridge.dialog.mockImplementationOnce(((_r: unknown, s?: AbortSignal) => {
      dialogSignal = s;
      return new Promise(() => undefined);
    }) as never);
    c.fromPlugin({ t: "req", id: 44, method: "ui.confirm", params: { title: "Sure?", text: "?" } });
    await vi.advanceTimersByTimeAsync(2100);
    expect(c.answer(44)).toMatchObject({ ok: false, code: "timeout" });
    expect(dialogSignal?.aborted).toBe(true);
  });
});

describe("the browser's file paths (lane 17, F2)", () => {
  const type = { description: "Markdown", mime: "text/markdown", extensions: [".md"] };
  it("the download fallback asks first and downloads nothing on no", async () => {
    const created: unknown[] = [];
    const url = URL as unknown as { createObjectURL?: (b: unknown) => string; revokeObjectURL?: (u: string) => void };
    const was = { create: url.createObjectURL, revoke: url.revokeObjectURL };
    url.createObjectURL = (b) => (created.push(b), "blob:x");
    url.revokeObjectURL = () => undefined;
    // jsdom cannot follow a download link.
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    try {
      const ask = vi.fn(async () => false);
      expect(await saveFile("a.md", new Uint8Array(3), type, undefined, { confirmDownload: ask })).toBeNull();
      expect(ask).toHaveBeenCalledOnce();
      expect(created).toEqual([]);
      expect(await saveFile("a.md", new Uint8Array(3), type, undefined, { confirmDownload: async () => true })).toEqual({ name: "a.md" });
      expect(created.length).toBe(1);
      // The app's own saves (no confirmDownload) download as before.
      expect(await saveFile("b.md", new Uint8Array(3), type)).toEqual({ name: "b.md" });
      expect(created.length).toBe(2);
      expect(click).toHaveBeenCalledTimes(2);
    } finally {
      click.mockRestore();
      url.createObjectURL = was.create;
      url.revokeObjectURL = was.revoke;
    }
  });

  it("a picker asked for without a click is refused at once instead of a file input that never answers", async () => {
    const nav = navigator as Navigator & { userActivation?: { isActive: boolean } };
    Object.defineProperty(nav, "userActivation", { value: { isActive: false }, configurable: true });
    try {
      await expect(pickFile([type], { requireActivation: true })).rejects.toBeInstanceOf(NeedsUserActivation);
      const ctl = new AbortController();
      Object.defineProperty(nav, "userActivation", { value: { isActive: true }, configurable: true });
      const p = pickFile([type], { requireActivation: true, signal: ctl.signal });
      expect(document.querySelectorAll('input[type="file"]').length).toBe(1);
      ctl.abort();
      expect(await p).toBeNull();
      expect(document.querySelectorAll('input[type="file"]').length).toBe(0);
    } finally {
      delete (nav as { userActivation?: unknown }).userActivation;
    }
  });
});

describe("the network allowlist is by origin (lane 17, F3)", () => {
  it("checks the scheme, the host and the port, not the host alone", async () => {
    const { c, req, bridge } = await running({ permissions: ["net"], net: ["api.example.com", "http://localhost:3000"] });
    for (const url of ["https://api.example.com:8443/", "http://localhost:5432/", "http://localhost/", "https://localhost:3000/", "http://127.0.0.1:3000/"]) {
      const r = await req(c, "net.fetch", { url, method: "POST", body: "x" });
      expect(r, url).toMatchObject({ ok: false, code: "permission" });
    }
    expect(bridge.fetch).not.toHaveBeenCalled();
    expect(await req(c, "net.fetch", { url: "https://api.example.com/v1" })).toMatchObject({ ok: true });
    expect(await req(c, "net.fetch", { url: "http://localhost:3000/v1" })).toMatchObject({ ok: true });
    expect(bridge.fetch).toHaveBeenCalledTimes(2);
    const refused = await req(c, "net.fetch", { url: "https://api.example.com:8443/" });
    expect(refused.ok === false && refused.error).toContain("may not reach https://api.example.com:8443: plugin.json lists https://api.example.com, http://localhost:3000");
  });

  it("the enable dialog says what each entry reaches, \"any port on localhost\" among them", () => {
    const m = { ...JSON.parse(manifest({ permissions: ["net"], net: ["localhost", "*.example.org", "https://api.example.com:8443"] })), contributes: { commands: [], panels: [], tools: [], menus: [], shortcuts: [] }, entry: "main.js" };
    const net = permissionLines(m).find((l) => l.permission === "net")!;
    expect(net.detail).toBe("only the sites listed here: any port on localhost (http or https), https://*.example.org (any subdomain of example.org), https://api.example.com:8443");
  });

  it("an answer over 10 MB is refused even when it comes without a stream to cap", async () => {
    const { c, req, bridge } = await running({ permissions: ["net"], net: ["api.example.com"] });
    const big = { status: 200, statusText: "OK", headers: new Headers(), body: null, arrayBuffer: async () => new ArrayBuffer(10 * 1024 * 1024 + 1) };
    bridge.fetch.mockResolvedValueOnce(big as unknown as Response);
    expect(await req(c, "net.fetch", { url: "https://api.example.com/big" })).toMatchObject({ ok: false, code: "too_large" });
    const small = { ...big, arrayBuffer: async () => new ArrayBuffer(5) };
    bridge.fetch.mockResolvedValueOnce(small as unknown as Response);
    const r = await req(c, "net.fetch", { url: "https://api.example.com/small" });
    expect(r.ok && (r.value as { body: Uint8Array }).body.byteLength).toBe(5);
  });
});

describe("consent, attribution and grants (lane 17, F4 to F9)", () => {
  it("F4: the consent text for model.read says it changes the selection, and model.select needs model.read", async () => {
    expect(PERMISSION_TEXT["model.read"].detail).toContain("and change the selection");
    expect(METHODS["model.select"]).toBe("model.read");
    const { readFileSync } = await import("node:fs");
    expect(readFileSync("docs/PLUGINS.md", "utf8")).toMatch(/\| `model\.read` \|[^\n]*and change the selection/);
    const { c, req, bridge } = await running({ permissions: ["ui"] });
    expect(await req(c, "model.select", { items: [{ kind: "solid", id: 7 }] })).toMatchObject({ ok: false, code: "permission" });
    expect(bridge.select).not.toHaveBeenCalled();
  });

  it("F5: without model.read a refused batch says only which op was refused, never the kernel's reason", async () => {
    const reason = 'op 1 (push_pull): no face #12 on object 7 "Secret leg"';
    const w = await running({ permissions: ["model.write"] });
    w.bridge.applyBatch.mockRejectedValueOnce(new Error(reason));
    const r = await w.req(w.c, "model.batch", { name: "Probe", ops: [{ op: "rename", item: { object: 7 }, name: "x" }, { op: "push_pull", object: 7, face: 12, distance: 1 }] });
    expect(r).toMatchObject({ ok: false, code: "refused" });
    const text = r.ok === false ? r.error : "";
    expect(text).toContain("op 1 was refused");
    expect(text).not.toMatch(/face|#12|object 7|Secret|push_pull/);
    w.bridge.applyBatch.mockRejectedValueOnce(new Error("the kernel is busy with Secret leg"));
    const r2 = await w.req(w.c, "model.batch", { name: "Probe", ops: [{ op: "rename", item: { object: 7 }, name: "x" }] });
    expect(r2.ok === false && r2.error).toMatch(/^the batch was refused; nothing changed/);
    // With model.read the reason is the plugin's to read.
    const rw = await running({ permissions: ["model.read", "model.write"] });
    rw.bridge.applyBatch.mockRejectedValueOnce(new Error(reason));
    expect(await rw.req(rw.c, "model.batch", { name: "Probe", ops: [{ op: "push_pull", object: 7, face: 12, distance: 1 }] })).toMatchObject({ ok: false, code: "refused", error: reason });
    expect(batchRefusedWithoutRead("op 12: placeholder regions.9 is missing")).toBe('op 12 was refused; nothing changed (the reason names the model, and plugin.json does not ask for "model.read")');
  });

  it("F6: a plugin tool's status line and hint start with the plugin's name", async () => {
    const { m, c, req, bridge } = await running({ permissions: ["ui"], contributes: { tools: [{ id: "t", title: "Tool", hint: "Click a face." }] } });
    (bridge as { activeTool: () => string }).activeTool = () => "plugin:org.example.test/t";
    await req(c, "ui.registerTool", { id: "t" });
    const host = fakeHost();
    const tool = new PluginTool(host, m, "plugin:org.example.test/t");
    tool.activate();
    await req(c, "ui.toolView", { id: "t", view: { status: "Saved. Your document is safe." } });
    await new Promise((r) => setTimeout(r, 40));
    expect(host.statuses.at(-1)?.text).toBe("Test: Saved. Your document is safe.");
    expect(tool.variantHint()).toBe("Tool (Test): Click a face.");
    await req(c, "ui.toolView", { id: "t", view: { status: null } });
    await new Promise((r) => setTimeout(r, 40));
    expect(host.statuses.at(-1)?.text).toBeNull();
    tool.dispose();
  });

  it("F8: a grant shrinks to what the files ask for, so a permission dropped and asked for again needs the person again", async () => {
    const t = setup();
    const e = t.m.add(files({ permissions: ["model.read", "ui", "net"], net: ["api.example.com"] }), "bundled");
    await t.m.enable(e.id);
    expect(loadGrants()[e.id]).toMatchObject({ permissions: ["model.read", "ui", "net"], net: ["api.example.com"] });
    // The sample (it keeps its grant across updates) drops model.read and the network ...
    t.m.add(files({ permissions: ["ui"] }), "bundled");
    expect(loadGrants()[e.id]).toMatchObject({ permissions: ["ui"], net: [] });
    expect(t.m.get(e.id)!.enabled).toBe(true);
    // ... and asks for them again: off until the person says yes.
    const back = t.m.add(files({ permissions: ["model.read", "ui", "net"], net: ["api.example.com"] }), "bundled");
    expect(back.enabled).toBe(false);
    expect(back.lastError).toContain("asks for more (model.read, net, net api.example.com)");
    t.m.dispose();
  });

  it("F8: a new net entry is a new grant too, even with the same permissions", async () => {
    const t = setup();
    const e = t.m.add(files({ permissions: ["net"], net: ["api.example.com"] }), "bundled");
    await t.m.enable(e.id);
    const more = t.m.add(files({ permissions: ["net"], net: ["api.example.com", "localhost"] }), "bundled");
    expect(more.enabled).toBe(false);
    expect(more.lastError).toContain("asks for more (net localhost)");
    expect(grantCovers({ version: "1", permissions: ["net"] }, ["net"], ["api.example.com"])).toBe(false);
    expect(trimGrant({ version: "1", permissions: ["ui"] }, ["ui"])).toEqual({ version: "1", permissions: ["ui"] });
    t.m.dispose();
  });

  it("F9: names on Object.prototype are not methods or events, and a refusal never prints a function", async () => {
    const { c, req } = await running({ permissions: ["ui"] });
    for (const method of ["constructor", "toString", "__proto__", "hasOwnProperty", "valueOf", "__defineGetter__"]) {
      const r = await req(c, method);
      // "__proto__" and "__defineGetter__" are not method names on the wire ("invalid"); the rest
      // are refused by the method table.
      expect(r, method).toMatchObject({ ok: false, code: method.startsWith("__") ? "invalid" : "unknown" });
      expect(r.ok === false && r.error, method).not.toMatch(/function|native code|\{/);
    }
    for (const name of ["constructor", "toString", "__proto__"]) {
      const r = await req(c, "events.subscribe", { name });
      expect(r, name).toMatchObject({ ok: false, code: "invalid" });
      expect(r.ok === false && r.error, name).not.toMatch(/function|native code/);
    }
    // A tool view's colour or lock named after a prototype key is not one.
    const v = checkToolView({ overlay: [{ points: [[0, 0, 0], [1, 0, 0]], color: "constructor" }], inference: { start: [0, 0, 0], lock: { dir: [0, 0, 1], kind: "toString" } } });
    expect(v.overlay[0].color).toBe("--accent");
    expect(v.inference.lock).toBeUndefined();
  });
});

describe("one batch limit for the plugin and the app (lane 17)", () => {
  it("the runtime stops adding ops where the app would refuse the batch", () => {
    expect(BATCH_MAX_OPS).toBe(MAX_BATCH_OPS);
  });
});
