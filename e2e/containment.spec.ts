import { expect, test, type Page } from "@playwright/test";
import { box, solids } from "./helpers";

// Lane 16, containment: a kernel that fails in the middle of an import (a Rust panic is a
// WebAssembly trap with the document borrowed: before, every later request was refused and
// Save stopped working) is replaced by a fresh worker that rebuilds the document from its
// log, and the app says so; a plugin that hangs or floods the app is stopped without
// touching another plugin or the document; and an autosave that cannot be read, or is too
// large, never blocks the start.

const requireKernel = !!process.env.REQUIRE_KERNEL;

async function ready(page: Page, url = "/?autosave=0&io=basic&kerneltest=1") {
  await page.goto(url);
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** A binary STL of one 20 mm box. */
function stlBox(): Buffer {
  const c = (i: number) => [i & 1 ? 20 : 0, (i & 2 ? 20 : 0) + 100, i & 4 ? 20 : 0];
  const tris: number[][][] = [];
  for (const q of [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [1, 3, 7, 5],
    [0, 4, 6, 2],
  ])
    tris.push([c(q[0]), c(q[1]), c(q[2])], [c(q[0]), c(q[2]), c(q[3])]);
  const b = Buffer.alloc(84 + tris.length * 50);
  b.writeUInt32LE(tris.length, 80);
  tris.forEach((t, k) => t.forEach((v, j) => v.forEach((x, a) => b.writeFloatLE(x, 84 + k * 50 + 12 + j * 12 + a * 4))));
  return b;
}

type Hook = {
  kernelTest: { failNextImport(how?: string): void; restarts(): number };
  lastRestart?: { reason: string; error: string | null; rebuilt: boolean };
  plugins: { list(): { id: string; status: string; lastError: string | null; logs: string[] }[]; loadSession(f: Record<string, string>): Promise<string>; enable(id: string): Promise<void>; run(key: string): Promise<void> };
};
const hook = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.kernelTest.restarts());

test("a kernel panic during an import leaves the open document intact: a fresh kernel rebuilds it and the app says so", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  const before = await solids(page);
  const volume = await vp.getAttribute("data-volume");
  expect(await hook(page)).toBe(0);

  await page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.kernelTest.failNextImport("panic"));
  await page.getByTestId("file-input").setInputFiles({ name: "box.stl", mimeType: "model/stl", buffer: stlBox() });

  const hint = page.locator("#hint");
  await expect(hint).toContainText("The kernel stopped", { timeout: 30_000 });
  await expect(hint).toContainText("restarted from your last step: nothing was lost");
  expect(await hook(page)).toBe(1);
  const restart = await page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.lastRestart);
  expect(restart).toMatchObject({ rebuilt: true, error: null });
  expect(restart?.reason).toMatch(/panic|unreachable|RuntimeError/i);
  // The document is exactly as it was, history included.
  expect(await solids(page)).toEqual(before);
  await expect(vp).toHaveAttribute("data-volume", volume ?? "");
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await page.keyboard.press("Control+y");
  await expect(vp).toHaveAttribute("data-objects", "1");

  // The new kernel works: the same import, without the fault, comes in.
  await page.getByTestId("file-input").setInputFiles({ name: "box.stl", mimeType: "model/stl", buffer: stlBox() });
  await expect(vp).toHaveAttribute("data-objects", "2", { timeout: 15_000 });
  // And Save still works (it did not after a trap: every call on the poisoned document failed).
  const [download] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Control+s")]);
  expect(download.suggestedFilename()).toMatch(/\.shz$/);
});

function plugin(id: string, main: string, extra: Record<string, unknown> = {}) {
  return {
    "plugin.json": JSON.stringify({ id, name: id.split(".").pop(), version: "1.0.0", api: 1, description: "containment", author: "Sawhorse", license: "MIT", permissions: ["ui", "storage", "model.write", "model.read"], ...extra }),
    "main.js": main,
  };
}

const steady = `import { ui, storage } from "sawhorse";
await ui.registerCommand({ id: "tick", async run() {
  const n = ((await storage.app.get("n")) ?? 0) + 1;
  await storage.app.set("n", n);
  console.log("tick " + n);
} });
`;

const hang = `import { ui, model } from "sawhorse";
await ui.registerCommand({ id: "hang", run() { for (;;) {} } });
await ui.registerCommand({ id: "flood", run() { for (;;) { ui.notify("x").catch(() => {}); } } });
`;

test("a plugin that hangs or floods the app is stopped; another plugin and the document are untouched", async ({ page }) => {
  test.setTimeout(90_000);
  const vp = await ready(page);
  await box(page);
  const before = await solids(page);
  const call = (fn: string, ...args: unknown[]) =>
    page.evaluate(
      ([f, a]) => {
        const p = (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins as unknown as Record<string, (...x: unknown[]) => unknown>;
        return p[f as string](...(a as unknown[]));
      },
      [fn, args] as const,
    );
  const steadyId = (await call("loadSession", plugin("org.example.steady", steady, { contributes: { commands: [{ id: "tick", title: "Tick" }] } }))) as string;
  await call("enable", steadyId);
  const badId = (await call("loadSession", plugin("org.example.bad", hang, { contributes: { commands: [{ id: "hang", title: "Hang" }, { id: "flood", title: "Flood" }] } }))) as string;
  await call("enable", badId);
  const entry = (id: string) => page.evaluate((i) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((e) => e.id === i)!, id);
  await expect.poll(async () => (await entry(badId)).status).toBe("running");

  // Hang: its worker spins; the app is not blocked, and Stop ends it.
  void call("run", `${badId}/hang`);
  await call("run", `${steadyId}/tick`);
  await expect.poll(async () => (await entry(steadyId)).logs.some((l) => l.includes("tick 1"))).toBe(true);
  await page.getByRole("tab", { name: "Plugins" }).click();
  await page.locator(`.plugin-row[data-plugin="${badId}"] [data-do="stop"]`).click();
  await expect.poll(async () => (await entry(badId)).status).toBe("stopped");
  await call("run", `${steadyId}/tick`);
  await expect.poll(async () => (await entry(steadyId)).logs.some((l) => l.includes("tick 2"))).toBe(true);

  // Flood: thousands of messages a second; the app stops it by itself.
  await page.locator(`.plugin-row[data-plugin="${badId}"] [data-do="reload"]`).click().catch(() => undefined);
  await call("enable", badId);
  await expect.poll(async () => (await entry(badId)).status).toBe("running");
  void call("run", `${badId}/flood`);
  await expect.poll(async () => (await entry(badId)).status, { timeout: 30_000 }).toBe("stopped");
  expect((await entry(badId)).lastError).toMatch(/messages in a second|not respond/);
  await call("run", `${steadyId}/tick`);
  await expect.poll(async () => (await entry(steadyId)).logs.some((l) => l.includes("tick 3"))).toBe(true);
  expect((await entry(steadyId)).status).toBe("running");

  // The document never changed.
  expect(await solids(page)).toEqual(before);
  await expect(vp).toHaveAttribute("data-objects", "1");
});

/** Write raw autosave records (as a broken or old browser profile would hold them). */
async function plant(page: Page, records: Record<string, unknown>) {
  await page.evaluate(
    (recs) =>
      new Promise<void>((resolve) => {
        const req = indexedDB.open("sawhorse", 1);
        req.onupgradeneeded = () => req.result.createObjectStore("autosave");
        req.onsuccess = () => {
          const db = req.result;
          const tx = db.transaction("autosave", "readwrite");
          for (const [k, v] of Object.entries(recs)) tx.objectStore("autosave").put(v, k);
          tx.oncomplete = () => {
            db.close();
            resolve();
          };
        };
      }),
    records,
  );
}

test("an autosave that cannot be read, or is too large, never blocks the start: it is set aside and offered for discarding", async ({ page }) => {
  const vp = await ready(page, "/?io=basic");
  // Unreadable: a record that is not a record, a log that is not a log.
  await plant(page, { "current-meta": "garbage", current: 12345 });
  await page.reload();
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("cannot be read; it is set aside, not loaded");
  await expect(hint.getByRole("button", { name: "Restore" })).toHaveCount(0);
  // The app works while the offer stands: a box, and the record is still kept aside.
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await page.reload();
  await expect(hint).toContainText("cannot be read");
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded");

  // Too large: 100 MB of history is offered for discarding only, never loaded.
  await plant(page, { "current-meta": { savedAt: Date.now(), solids: 3, steps: 9, bytes: 100 * 1024 * 1024, worth: true }, current: "[]" });
  await page.reload();
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  await expect(hint).toContainText("100 MB of history: too large to restore here");
  await expect(hint.getByRole("button", { name: "Restore" })).toHaveCount(0);
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded");
});
