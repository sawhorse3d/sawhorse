import { expect, test, type Page } from "@playwright/test";
import { strToU8, zipSync } from "fflate";
import { at, box, solids, type V3 } from "./helpers";

// Lane 16: a hostile plugin, loaded from a .zip for this session through the real UI, in a
// real Web Worker. Every message it sends is serialized once in its sandbox and measured by
// the app before the app reads anything; none of its attacks reaches the document, the
// stores or the app's thread, and every one comes back to the plugin as a typed refusal.
//
// - a 400 MB storage.set (100 references to one 4 M-character string),
// - a 1.5 GB buffer behind a 1-byte view,
// - a sparse array of length 2^32-1,
// - a 50 MB object key,
// - a batch of a million ops,
// - a panel of 10,000 controls,
// - and all of it again with Array.prototype, Object.keys / entries, Array.isArray and
//   JSON.stringify replaced inside the plugin.

const requireKernel = !!process.env.REQUIRE_KERNEL;
const ID = "org.example.hostile";

const manifest = {
  id: ID,
  name: "Hostile",
  version: "6.6.6",
  api: 1,
  description: "Tries to exhaust the app through the message path.",
  author: "Sawhorse",
  license: "MIT",
  permissions: ["model.read", "model.write", "ui", "storage"],
  contributes: { panels: [{ id: "p", title: "Hostile" }] },
};

const main = `import { model, storage, ui } from "sawhorse";
const J = JSON.stringify;
const report = (k, v) => console.log(k + "=" + J(v));
const attempt = async (k, f) => {
  const t0 = Date.now();
  try {
    await f();
    report(k, { ok: true, ms: Date.now() - t0 });
  } catch (e) {
    report(k, { ok: false, code: e && e.code !== undefined ? e.code : null, ms: Date.now() - t0, message: String((e && e.message) || e).slice(0, 200) });
  }
};
const attacks = async (tag) => {
  await attempt(tag + "storage400", () => storage.app.set("k", new Array(100).fill("x".repeat(4 * 1024 * 1024))));
  let view = null;
  try {
    view = new Uint8Array(new ArrayBuffer(1.5e9), 0, 1);
  } catch (e) {
    report(tag + "backingAlloc", { ok: false });
  }
  if (view) await attempt(tag + "backing", () => storage.app.set("v", view));
  view = null;
  await attempt(tag + "sparse", () => {
    const a = [];
    a.length = 2 ** 32 - 1;
    a[0] = { kind: "solid", id: 1 };
    return model.select(a);
  });
  await attempt(tag + "key", () => storage.app.set("o", { ["k".repeat(50 * 1024 * 1024)]: 1 }));
  await attempt(tag + "batch", () => {
    const b = model.batch("A million ops");
    for (let i = 0; i < 1e6; i++) b.op({ op: "rename", item: { object: 1 }, name: "x" });
    return b.commit();
  });
  await attempt(tag + "panel", () => ui.registerPanel({ id: "p", body: Array.from({ length: 10000 }, (_, i) => ({ type: "button", id: "b" + i, label: "x" })) }));
};
await attacks("");
// Replace the built-ins the runtime could lean on: the plugin can only confuse itself.
Array.prototype.forEach = function () {};
Array.prototype.map = function () { throw new Error("patched map"); };
Array.prototype.filter = function () { return []; };
Object.keys = () => [];
Object.entries = () => [["__proto__", 1]];
Array.isArray = () => false;
JSON.stringify = () => "{}";
await attacks("shadowed.");
await attempt("shadowed.read", () => model.solids());
report("keys", await storage.app.keys());
report("solids", (await model.solids()).length);
console.log("done");
`;

type Hook = { plugins: { list(): { id: string; status: string; logs: string[] }[] } };

async function logLine(page: Page, prefix: string): Promise<string> {
  let line = "";
  await expect
    .poll(
      async () => {
        const e = await page.evaluate((id) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((x) => x.id === id), ID);
        line = e?.logs.find((l) => l.includes(prefix)) ?? "";
        return line;
      },
      { timeout: 90_000 },
    )
    .not.toBe("");
  return line.slice(line.indexOf(prefix) + prefix.length);
}

test("a hostile plugin from a session zip cannot exhaust the app: every attack is a typed refusal, the document and stores are unchanged, the app stays responsive", async ({ page }) => {
  test.setTimeout(180_000);
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await box(page);
  const before = await solids(page);
  const volume = await vp.getAttribute("data-volume");

  // Watch the app's own thread: the longest gap between frames while the plugin runs.
  await page.evaluate(() => {
    const w = window as unknown as { __gap: number };
    w.__gap = 0;
    let last = performance.now();
    const tick = (t: number) => {
      w.__gap = Math.max(w.__gap, t - last);
      last = t;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });

  await page.getByRole("tab", { name: "Plugins" }).click();
  const zip = zipSync({ "hostile/plugin.json": strToU8(JSON.stringify(manifest)), "hostile/main.js": strToU8(main) });
  const [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.locator('[data-do="load-session"]').click()]);
  await chooser.setFiles({ name: "hostile.zip", mimeType: "application/zip", buffer: Buffer.from(zip) });
  const row = page.locator(`.plugin-row[data-plugin="${ID}"]`);
  await expect(row).toContainText("this session");
  await row.locator('[data-do="enable"]').click();
  await page.getByRole("dialog", { name: "Enable Hostile" }).getByRole("button", { name: "Enable" }).click();

  await logLine(page, "done");
  const result = async (k: string) => JSON.parse(await logLine(page, `${k}=`)) as { ok: boolean; code: string | null; ms: number; message?: string };
  for (const tag of ["", "shadowed."]) {
    expect(await result(`${tag}storage400`)).toMatchObject({ ok: false, code: "too_large" });
    const alloc = await page.evaluate((id) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((x) => x.id === id)!.logs.some((l) => l.includes(`${"backingAlloc"}=`)), ID);
    // The 1-byte view crosses as one byte; the app refuses byte arrays in storage.
    if (!alloc) expect(await result(`${tag}backing`)).toMatchObject({ ok: false, code: "invalid" });
    expect(await result(`${tag}sparse`)).toMatchObject({ ok: false, code: "too_large" });
    expect(await result(`${tag}key`)).toMatchObject({ ok: false, code: "too_large" });
    expect(await result(`${tag}batch`)).toMatchObject({ ok: false, code: "too_large" });
    expect(await result(`${tag}panel`)).toMatchObject({ ok: false, code: "invalid", message: expect.stringContaining("at most 100 fields") });
  }
  expect(await result("shadowed.read")).toMatchObject({ ok: true });
  // Nothing was stored, nothing drawn, nothing changed.
  expect(JSON.parse(await logLine(page, "keys="))).toEqual([]);
  expect(JSON.parse(await logLine(page, "solids="))).toBe(1);
  expect(await solids(page)).toEqual(before);
  await expect(vp).toHaveAttribute("data-volume", volume ?? "");
  await expect(page.locator(`[data-panel="${ID}/p"]`)).toHaveCount(0);
  await expect(row).toHaveAttribute("data-status", "running");

  // The app kept drawing frames, and still edits: a second box.
  const gap = await page.evaluate(() => (window as unknown as { __gap: number }).__gap);
  expect(gap).toBeLessThan(1000);
  await page.locator("#cv").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("Escape");
  const t0 = Date.now();
  await page.evaluate(() => 1);
  expect(Date.now() - t0).toBeLessThan(500);
});

// Lane 17 (F1): a plugin with only "ui" makes its own tool the active one and logs every
// pointer event while the person works over a solid. It hears the pointer and the point of
// its own plane, never a solid's id, a face or anything else of the model.
const PEEK = "org.example.peek";
const peekManifest = {
  id: PEEK,
  name: "Peek",
  version: "1.0.0",
  api: 1,
  description: "A ui-only tool that logs what it is told about the pointer.",
  author: "Sawhorse",
  license: "MIT",
  permissions: ["ui"],
  contributes: { tools: [{ id: "peek", title: "Peek" }] },
};
const peekMain = `import { ui } from "sawhorse";
const tool = await ui.registerTool({ id: "peek", onEvent(e) { console.log("event=" + JSON.stringify(e)); } });
await tool.setView({ inference: { plane: { origin: [0, 0, 0], normal: [0, 0, 1] } } });
await ui.activateTool("peek");
console.log("peeking");
`;

test("a ui-only plugin's tool never sees a solid id: pointer events over a solid carry no snap or pick from the model", async ({ page }) => {
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await box(page);
  const ids = (await solids(page)).map((s) => s.id);
  expect(ids.length).toBe(1);

  await page.getByRole("tab", { name: "Plugins" }).click();
  const zip = zipSync({ "peek/plugin.json": strToU8(JSON.stringify(peekManifest)), "peek/main.js": strToU8(peekMain) });
  const [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.locator('[data-do="load-session"]').click()]);
  await chooser.setFiles({ name: "peek.zip", mimeType: "application/zip", buffer: Buffer.from(zip) });
  await page.locator(`.plugin-row[data-plugin="${PEEK}"] [data-do="enable"]`).click();
  await page.getByRole("dialog", { name: "Enable Peek" }).getByRole("button", { name: "Enable" }).click();
  const logs = async () => (await page.evaluate((id) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((x) => x.id === id)?.logs ?? [], PEEK)) as string[];
  await expect.poll(async () => (await logs()).some((l) => l.includes("peeking")), { timeout: 15_000 }).toBe(true);

  // Over the box's top face, an edge and a corner, and a click on the face.
  for (const p of [[6, 6, 6], [12, 6, 6], [12, 12, 6], [0, 0, 6], [6, 0, 3]] as V3[]) {
    const [x, y] = await at(page, p);
    await page.mouse.move(x, y, { steps: 3 });
  }
  const [cx, cy] = await at(page, [6, 6, 6]);
  await page.mouse.click(cx, cy);
  // The click goes after the moves (moves are coalesced while the plugin is busy): wait for it.
  await expect.poll(async () => (await logs()).some((l) => l.includes("event=") && l.includes('"type":"click"')), { timeout: 15_000 }).toBe(true);

  const events = (await logs()).filter((l) => l.includes("event=")).map((l) => l.slice(l.indexOf("event=") + 6));
  let ownPoints = 0;
  for (const raw of events) {
    // Nothing of the model by name: no solid, face, object or region, no entity, no surface.
    expect(raw).not.toMatch(/"(solid|face|object|region|sketch|owner|frame)"/);
    const e = JSON.parse(raw) as { picked: unknown; snap: { point: V3; kind: string; surface: unknown; entity: unknown } | null; ray: { origin: V3; dir: V3 } };
    expect(e.picked).toBeNull();
    expect(Object.keys(e).sort()).toEqual(["alt", "ctrl", "picked", "ray", "shift", "snap", "type", "x", "y"]);
    if (e.snap) {
      expect(e.snap).toMatchObject({ kind: "free", surface: null, entity: null });
      // The ground plane it set, not the box's top 6 inches up.
      expect(Math.abs(e.snap.point[2])).toBeLessThan(1e-9);
      ownPoints++;
    }
  }
  expect(ownPoints).toBeGreaterThan(0);
  expect(events.some((r) => r.includes('"type":"click"'))).toBe(true);
});

// Lane 17 (F2b): a plugin asks for a file long after the person last clicked. The browser
// would not open the picker (and a file input opened then never answered, holding the
// plugin's file.open slot for good): the call is refused at once with "user_activation".
test("file.open without the person's click is refused with a typed error instead of hanging", async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  const id = "org.example.sneaky-open";
  const files = {
    "plugin.json": JSON.stringify({ id, name: "Sneaky open", version: "1.0.0", api: 1, description: "Opens a picker on its own.", author: "Sawhorse", license: "MIT", permissions: ["file"] }),
    // The browser's transient activation lasts about five seconds after a click.
    "main.js": `import { file } from "sawhorse";
setTimeout(async () => {
  for (const n of [1, 2]) {
    const t0 = Date.now();
    try { await file.open({ extensions: [".csv"] }); console.log("open" + n + "=opened"); }
    catch (e) { console.log("open" + n + "=" + JSON.stringify({ code: e.code, ms: Date.now() - t0 })); }
  }
}, 7000);
console.log("waiting");`,
  };
  let chooser = false;
  page.on("filechooser", () => (chooser = true));
  await page.evaluate((f) => (window as unknown as { __sawhorse: { plugins: { loadSession(f: Record<string, string>): Promise<string> } } }).__sawhorse.plugins.loadSession(f), files);
  await page.getByRole("tab", { name: "Plugins" }).click();
  await page.locator(`.plugin-row[data-plugin="${id}"] [data-do="enable"]`).click();
  await page.getByRole("dialog", { name: "Enable Sneaky open" }).getByRole("button", { name: "Enable" }).click();
  const logs = async () => (await page.evaluate((x) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((e) => e.id === x)?.logs ?? [], id)) as string[];
  // Hands off while the plugin waits: Playwright's page.evaluate runs as a user gesture, so
  // polling now would hand the page the activation the plugin must not have.
  await page.waitForTimeout(9_000);
  await expect.poll(async () => (await logs()).filter((l) => /open\d=/.test(l)).length, { timeout: 20_000 }).toBe(2);
  for (const n of [1, 2]) {
    const line = (await logs()).find((l) => l.includes(`open${n}=`))!;
    const r = JSON.parse(line.slice(line.indexOf("=") + 1)) as { code: string; ms: number };
    // Refused at once, and the second is refused the same way (not "busy": no slot was held).
    expect(r.code).toBe("user_activation");
    expect(r.ms).toBeLessThan(2000);
  }
  expect(chooser).toBe(false);
});
