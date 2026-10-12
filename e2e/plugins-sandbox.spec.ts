import { expect, test, type Page } from "@playwright/test";
import { box, ready, solids } from "./helpers";

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
});

test("code that would load from outside is refused before it runs", async ({ page }) => {
  await ready(page);
  const dynamic = await load(page, plugin("org.example.dynamic", [], 'const m = await import("https://example.com/x.js");'), false);
  const remote = await load(page, plugin("org.example.remote", [], 'import x from "https://example.com/x.js";'), false);
  const blob = await load(page, plugin("org.example.sneaky", [], 'import/* hi */("data:text/javascript,1");'), false);
  const scripts = await load(page, plugin("org.example.scripts", [], 'importScripts("https://example.com/x.js");'), false);
  for (const [id, want] of [
    [dynamic, "dynamic import() is not allowed"],
    [remote, 'may import only "sawhorse"'],
    [blob, "dynamic import() is not allowed"],
    [scripts, "importScripts is not available"],
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
