import { expect, test, type Page } from "@playwright/test";
import { strFromU8, unzipSync } from "fflate";
import { box } from "./helpers";
import { bridge, desktopBridge, DOCUMENTS, HOME, savedBytes, setBridge } from "./tauri";

// #71, #80: on Linux the desktop's Open and Save dialogs are given a folder to start in: the
// folder of the model's own file for Save As, else the folder of the last file opened or
// saved, else Documents, else home. GTK's chooser otherwise starts Save in the working
// directory (an AppImage's mount) and Open in Recent. Which folder is still there is the
// shell's to say (src-tauri/dialog-folder, tested with cargo); the page's own rules are in
// src/io/folder.test.ts. Here the page runs as it does inside the desktop shell, with the
// bridge stood in for (e2e/tauri.ts): dialog_folder, the dialog plugin's open and save, and
// save_file_atomic.
const requireKernel = !!process.env.REQUIRE_KERNEL;
const MODELS = "/home/maker/models";
const OTHER = "/home/maker/other";
const SHELF = `${MODELS}/shelf.shz`;
const BENCH = `${OTHER}/bench.shz`;
const STICK = "/media/stick/models/shelf.shz";

async function ready(page: Page, url = "/?autosave=0") {
  await page.goto(url);
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** Choose a File menu entry by its label; the entry's name also holds its keys ("Open… Ctrl+O"). */
async function menu(page: Page, top: string, label: string) {
  await page.getByRole("button", { name: top, exact: true }).click();
  await page.getByRole("menuitem").filter({ hasText: label }).first().click();
}

// One 12 x 12 x 6 in box, saved by the browser build.
let shelf = "";

test.beforeAll(async ({ browser }) => {
  const page = await browser.newPage();
  await page.goto(`http://localhost:${Number(process.env.PW_PORT || 4173)}/?io=basic&autosave=0`);
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if ((await vp.getAttribute("data-kernel")) !== "ready") {
    await page.close();
    return;
  }
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  const [d] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Control+s")]);
  const fs = await import("node:fs");
  shelf = fs.readFileSync(await d.path()).toString("base64");
  await page.close();
});

/** A desktop page on `platform`, started with `start`, with these folders on its disk. */
async function started(page: Page, platform: string, start: string | null, folders: string[], url?: string) {
  await desktopBridge(page, platform, { start, files: { [SHELF]: shelf, [BENCH]: shelf, [STICK]: shelf }, folders });
  return ready(page, url);
}

/** Everything this page keeps in IndexedDB, keys and values, as one text. */
async function everythingKept(page: Page): Promise<string> {
  return page.evaluate(async () => {
    const text = (v: unknown): string => {
      if (v instanceof ArrayBuffer) return text(new Uint8Array(v));
      if (ArrayBuffer.isView(v)) return Array.from(new Uint8Array(v.buffer, v.byteOffset, v.byteLength), (c) => String.fromCharCode(c)).join("");
      if (typeof v === "string") return v;
      if (v && typeof v === "object") return Object.entries(v).map(([k, x]) => `${k}=${text(x)}`).join("\n");
      return String(v);
    };
    const out: string[] = [];
    for (const { name } of await indexedDB.databases()) {
      if (!name) continue;
      const db = await new Promise<IDBDatabase>((ok, no) => {
        const r = indexedDB.open(name);
        r.onsuccess = () => ok(r.result);
        r.onerror = () => no(r.error);
      });
      for (const store of Array.from(db.objectStoreNames)) {
        const s = db.transaction(store, "readonly").objectStore(store);
        const all = <T,>(r: IDBRequest<T>) => new Promise<T>((ok, no) => ((r.onsuccess = () => ok(r.result)), (r.onerror = () => no(r.error))));
        const [keys, values] = await Promise.all([all(s.getAllKeys()), all(s.getAll())]);
        keys.forEach((k, i) => out.push(`${name}/${store}/${text(k)}: ${text(values[i])}`));
      }
      db.close();
    }
    return out.join("\n");
  });
}

test("Linux: Open starts in the folder of the file the app was started with, then in the folder of the file it opened", async ({ page }) => {
  const vp = await started(page, "Linux x86_64", SHELF, [MODELS, OTHER, DOCUMENTS, HOME]);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await setBridge(page, { pick: BENCH });
  await menu(page, "File", "Open…");
  await expect(page).toHaveTitle("bench - Sawhorse");
  let b = await bridge(page);
  expect(b.named).toEqual([[MODELS]]);
  expect(b.opens).toHaveLength(1);
  expect(b.opens[0].defaultPath).toBe(MODELS);
  // The next one starts where that file was.
  await setBridge(page, { pick: null });
  await menu(page, "File", "Open…");
  await expect.poll(async () => (await bridge(page)).opens.length).toBe(2);
  b = await bridge(page);
  expect(b.named[1]).toEqual([OTHER]);
  expect(b.opens[1].defaultPath).toBe(OTHER);
  // Cancelled: the model stays, and so does the folder kept.
  await expect(page).toHaveTitle("bench - Sawhorse");
  expect(await page.evaluate(() => localStorage.getItem("sawhorse.lastFolder"))).toBe(OTHER);
});

test("Linux: Save As starts in the folder of the model's own file, with its name, and the next dialog in the folder saved to", async ({ page }) => {
  await started(page, "Linux x86_64", SHELF, [MODELS, OTHER, DOCUMENTS, HOME]);
  await expect(page).toHaveTitle("shelf - Sawhorse");
  // Another file opened by a drop would move the folder kept; the model's own still comes first.
  await page.evaluate((f) => localStorage.setItem("sawhorse.lastFolder", f), OTHER);
  await setBridge(page, { saveAs: `${OTHER}/shelf v2.shz` });
  await menu(page, "File", "Save As…");
  await expect(page).toHaveTitle("shelf v2 - Sawhorse");
  const b = await bridge(page);
  expect(b.named).toEqual([[MODELS, OTHER]]);
  expect(b.saves).toHaveLength(1);
  expect(b.saves[0].defaultPath).toBe(`${MODELS}/shelf.shz`);
  expect(b.written).toEqual([`${OTHER}/shelf v2.shz`]);
  await setBridge(page, { pick: null });
  await menu(page, "File", "Open…");
  await expect.poll(async () => (await bridge(page)).opens.length).toBe(1);
  expect((await bridge(page)).opens[0].defaultPath).toBe(OTHER);
});

test("Linux: the first Save of a new model starts in Documents, or in home when there is no Documents", async ({ page }) => {
  const vp = await started(page, "Linux x86_64", null, [DOCUMENTS, HOME]);
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await setBridge(page, { saveAs: null });
  await page.keyboard.press("Control+s");
  await expect.poll(async () => (await bridge(page)).saves.length).toBe(1);
  let b = await bridge(page);
  expect(b.named).toEqual([[]]);
  expect(b.saves[0].defaultPath).toBe(`${DOCUMENTS}/Untitled.shz`);
  // Cancelled: nothing was written and no folder is kept.
  expect(b.written).toEqual([]);
  expect(await page.evaluate(() => localStorage.getItem("sawhorse.lastFolder"))).toBeNull();
  await setBridge(page, { folders: [HOME], saveAs: `${HOME}/box.shz` });
  await page.keyboard.press("Control+s");
  await expect(page).toHaveTitle("box - Sawhorse");
  b = await bridge(page);
  expect(b.saves[1].defaultPath).toBe(`${HOME}/Untitled.shz`);
  expect(b.written).toEqual([`${HOME}/box.shz`]);
  // Saved: Ctrl+S again writes the file with no dialog and asks for no folder.
  const dialogs = b.saves.length;
  const asked = b.named.length;
  await page.keyboard.press("Control+s");
  await expect.poll(async () => (await bridge(page)).written.length).toBe(2);
  b = await bridge(page);
  expect(b.saves).toHaveLength(dialogs);
  expect(b.named).toHaveLength(asked);
});

test("Linux: a folder that is gone is passed over, and the dialog starts in Documents", async ({ page }) => {
  // Started with a file on a stick that is then pulled out: its folder is no longer there.
  await started(page, "Linux x86_64", STICK, [DOCUMENTS, HOME]);
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await setBridge(page, { pick: null });
  await menu(page, "File", "Open…");
  await expect.poll(async () => (await bridge(page)).opens.length).toBe(1);
  const b = await bridge(page);
  expect(b.named).toEqual([["/media/stick/models"]]);
  expect(b.opens[0].defaultPath).toBe(DOCUMENTS);
});

test("Linux: Load Unpacked Folder starts where Open starts, and the folder kept stays the model's", async ({ page }) => {
  await started(page, "Linux x86_64", SHELF, [MODELS, OTHER, DOCUMENTS, HOME]);
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await setBridge(page, { pick: null });
  await menu(page, "Plugins", "Load Unpacked Folder…");
  await expect.poll(async () => (await bridge(page)).opens.length).toBe(1);
  const b = await bridge(page);
  expect(b.named).toEqual([[MODELS]]);
  expect(b.opens[0]).toEqual({ directory: true, multiple: false, recursive: true, title: "Load an unpacked plugin folder", defaultPath: MODELS });
  expect(await page.evaluate(() => localStorage.getItem("sawhorse.lastFolder"))).toBe(MODELS);
});

test("Linux: the folder is kept under its own key and nowhere else: not in the saved file, the unsaved work kept, or the console", async ({ page }) => {
  const said: string[] = [];
  page.on("console", (m) => said.push(m.text()));
  // Autosave on, so the unsaved work is kept as it is in the app; a folder kept by an earlier run.
  const vp = await started(page, "Linux x86_64", null, [MODELS, OTHER, DOCUMENTS, HOME], "/");
  await page.evaluate((f) => localStorage.setItem("sawhorse.lastFolder", f), MODELS);
  // An edit: the unsaved work is written to this browser's store.
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect.poll(async () => (await everythingKept(page)).length, { timeout: 15_000 }).toBeGreaterThan(200);
  expect(await everythingKept(page)).not.toContain("/home/maker");
  // Saved somewhere else: the dialog starts in the folder kept, the folder kept moves, and
  // the file holds none of it.
  await setBridge(page, { saveAs: `${OTHER}/box.shz` });
  await page.keyboard.press("Control+s");
  await expect(page).toHaveTitle("box - Sawhorse");
  const b = await bridge(page);
  expect(b.saves[0].defaultPath).toBe(`${MODELS}/Untitled.shz`);
  const file = await savedBytes(page);
  const entries = unzipSync(file);
  expect(Object.keys(entries).length).toBeGreaterThan(0);
  for (const [name, bytes] of Object.entries(entries)) {
    expect(name).not.toContain("maker");
    expect(strFromU8(bytes, true), name).not.toContain("/home/maker");
  }
  expect(strFromU8(file, true)).not.toContain("/home/maker");
  // One key holds it, and no other key holds a path.
  const storage = await page.evaluate(() => Object.fromEntries(Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i) as string).map((k) => [k, localStorage.getItem(k) ?? ""])));
  expect(storage["sawhorse.lastFolder"]).toBe(OTHER);
  for (const [k, v] of Object.entries(storage)) if (k !== "sawhorse.lastFolder") expect(`${k}=${v}`).not.toContain("/home/maker");
  expect(await everythingKept(page)).not.toContain("/home/maker");
  expect(said.join("\n")).not.toContain("/home/maker");
});

for (const platform of ["Win32", "MacIntel"]) {
  test(`${platform}: the shell names no folder, and Save and Open are given what they were given before`, async ({ page }) => {
    const vp = await started(page, platform, null, [DOCUMENTS, HOME]);
    await setBridge(page, { pick: null, saveAs: null });
    await menu(page, "File", "Open…");
    await expect.poll(async () => (await bridge(page)).opens.length).toBe(1);
    await box(page);
    await expect(vp).toHaveAttribute("data-objects", "1");
    await page.keyboard.press("Control+s");
    await expect.poll(async () => (await bridge(page)).saves.length).toBe(1);
    const b = await bridge(page);
    // The page still asks; this shell answers nothing.
    expect(b.named).toEqual([[], []]);
    expect(b.opens[0]).not.toHaveProperty("defaultPath");
    expect(b.saves[0]).toEqual({ defaultPath: "Untitled.shz", filters: [{ name: "Sawhorse document", extensions: ["shz"] }] });
    expect(b.written).toEqual([]);
  });
}

test("a shell without dialog_folder: the dialogs open as they did", async ({ page }) => {
  const vp = await started(page, "Linux x86_64", null, [DOCUMENTS, HOME]);
  await setBridge(page, { folderFails: true, pick: null, saveAs: null });
  await menu(page, "File", "Open…");
  await expect.poll(async () => (await bridge(page)).opens.length).toBe(1);
  expect((await bridge(page)).opens[0]).not.toHaveProperty("defaultPath");
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await page.keyboard.press("Control+s");
  await expect.poll(async () => (await bridge(page)).saves.length).toBe(1);
  expect((await bridge(page)).saves[0].defaultPath).toBe("Untitled.shz");
  // The page said nothing went wrong: a missing command is not an error to show.
  await expect(page.locator("#hint")).not.toContainText("dialog_folder");
});
