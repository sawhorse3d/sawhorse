import { expect, test, type Page } from "@playwright/test";
import { box } from "./helpers";
import { bridge, desktopBridge, dropFiles, restart, setBridge } from "./tauri";

// Lane 37 (#45): a double-click on a .shz in Windows Explorer starts Sawhorse with the file's
// path, and so does a path after the program's name. The shell picks the file from its
// arguments (src-tauri/start-file, tested with cargo) and hands it over when the page asks
// (start_file, once); the page opens it the way it opens a dropped file. The desktop shell
// cannot run here: the page runs as it does inside it, with the bridge stood in for
// (e2e/tauri.ts), start_file and the fs plugin's reads included.
const requireKernel = !!process.env.REQUIRE_KERNEL;
const SHELF = "C:\\Users\\maker\\Documents\\Sawhorse\\shelf.shz";
const GONE = "C:\\Users\\maker\\Documents\\Sawhorse\\gone.shz";
const DISCARD_QUESTION = "Discard the unsaved changes to this model?";

async function ready(page: Page, url = "/") {
  await page.goto(url);
  return loaded(page);
}

async function loaded(page: Page) {
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** The steps kept in an autosave slot (src/lib/autosave.ts): "current-meta" (this session's) or "offered-meta" (set aside at start). */
function slot(page: Page, key: "current-meta" | "offered-meta") {
  return page.evaluate(
    (key) =>
      new Promise<number | null>((resolve) => {
        const req = indexedDB.open("sawhorse");
        req.onsuccess = () => {
          const db = req.result;
          if (!db.objectStoreNames.contains("autosave")) return resolve(null);
          const get = db.transaction("autosave").objectStore("autosave").get(key);
          get.onsuccess = () => {
            resolve(typeof get.result?.steps === "number" ? get.result.steps : null);
            db.close();
          };
          get.onerror = () => resolve(null);
        };
        req.onerror = () => resolve(null);
      }),
    key,
  );
}

/** How many times the page asked the shell for its start-up file (this page load). */
async function asked(page: Page) {
  return (await bridge(page)).log.filter((c) => c === "start_file").length;
}

// shelf.shz: one 12 x 12 x 6 in box, saved by the browser build, and the model's hash.
let shelf = "";
let shelfHash = "";

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
  shelfHash = (await vp.getAttribute("data-hash")) ?? "";
  const [d] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Control+s")]);
  const fs = await import("node:fs");
  shelf = fs.readFileSync(await d.path()).toString("base64");
  await page.close();
});

/** A desktop page started with `start`, with shelf.shz (and a file that cannot be read) on its disk. */
async function started(page: Page, start: string | null, url = "/") {
  await desktopBridge(page, "Win32", { start, files: { [SHELF]: shelf, [GONE]: null } });
  return ready(page, url);
}

test("a .shz the app was started with opens: its name in the title and the status line, read like a dropped file", async ({ page }) => {
  const vp = await started(page, SHELF);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(page.locator("#hint")).toContainText("Opened shelf.shz");
  const b = await bridge(page);
  expect(await asked(page)).toBe(1);
  // Exactly that one path went into the scope, and only it was read.
  expect(b.allowed).toEqual([SHELF]);
  expect([...new Set(b.reads)]).toEqual([SHELF]);
  // Opened from its file: nothing unsaved.
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("saved");
  expect((await bridge(page)).unsaved).toBe(false);
});

test("with no start-up file the app starts as it always has", async ({ page }) => {
  const vp = await started(page, null);
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect.poll(() => asked(page)).toBe(1);
  const b = await bridge(page);
  expect(b.allowed).toEqual([]);
  expect(b.reads).toEqual([]);
  await expect(page.locator("#hint")).not.toContainText("Opened");
});

test("the start-up file is asked for once: a reload of the page does not open it again", async ({ page }) => {
  const vp = await started(page, SHELF);
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await box(page, "24,16", "12");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await expect.poll(() => slot(page, "current-meta")).not.toBe(null);
  await page.reload();
  await loaded(page);
  // Asked again by the new page, answered with nothing: the file is not opened a second time
  // (the work is offered back from the autosave instead, as after any reload).
  await expect.poll(() => asked(page)).toBe(1);
  await expect(page.locator("#hint")).toContainText("Restore unsaved work? 2 solids");
  await expect(page.locator("#hint")).not.toContainText("opens after you answer");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  const b = await bridge(page);
  expect(b.allowed).toEqual([]);
  expect(b.reads).toEqual([]);
});

test("a start-up file that cannot be read says what a failed drop says, and the app stays usable", async ({ page }) => {
  const vp = await started(page, GONE);
  const hint = page.locator("#hint");
  await expect(hint).toContainText("cannot find the file");
  const said = (await hint.innerText()).trim();
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  // The same file dropped on the window: the same message.
  await dropFiles(page, [GONE]);
  await expect(hint).toContainText("cannot find the file");
  expect((await hint.innerText()).trim()).toBe(said);
  await expect(vp).toHaveAttribute("data-objects", "1");
  expect((await bridge(page)).allowed).toEqual([GONE, GONE]);
});

/** Unsaved work from the last run (a 24 x 16 x 12 in box autosaved), then a start with shelf.shz. */
async function lastRunThenShelf(page: Page) {
  const vp = await started(page, null);
  await box(page, "24,16", "12");
  await expect(vp).toHaveAttribute("data-objects", "1");
  const work = await vp.getAttribute("data-hash");
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, SHELF);
  await loaded(page);
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await expect(hint).toContainText("shelf.shz opens after you answer.");
  // The file waits: the model is the empty one, and the file has not even been read.
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  expect(await asked(page)).toBe(1);
  expect((await bridge(page)).reads).toEqual([]);
  // The last run's work is set aside, and nothing of this start is saved over it, however long
  // the answer takes (the autosave runs 300 ms after a change).
  await page.waitForTimeout(800);
  expect(await slot(page, "offered-meta")).toBe(3);
  expect(await slot(page, "current-meta")).toBe(null);
  return { vp, hint, work: work! };
}

test("unsaved work from the last run is offered first; Restore brings it back and offers the file beside it", async ({ page }) => {
  const { vp, hint, work } = await lastRunThenShelf(page);
  await hint.getByRole("button", { name: "Restore" }).click();
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(vp).toHaveAttribute("data-hash", work);
  await expect(hint).toContainText("Restored your unsaved work: 1 solid, 3 steps (undo works as before). shelf.shz is not open yet.");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  expect((await bridge(page)).reads).toEqual([]);
  // Opening the file asks about the restored work, as File > Open does: No keeps it.
  const open = hint.getByRole("button", { name: "Open shelf.shz" });
  await setBridge(page, { answer: "Cancel", delay: 300 });
  await open.click();
  await expect.poll(async () => (await bridge(page)).asked).toEqual([DISCARD_QUESTION]);
  await page.waitForTimeout(600);
  await expect(vp).toHaveAttribute("data-hash", work);
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  // Yes opens it in place of the work.
  await setBridge(page, { answer: "Ok" });
  await open.click();
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
  expect((await bridge(page)).asked).toEqual([DISCARD_QUESTION, DISCARD_QUESTION]);
});

test("unsaved work from the last run is offered first; Discard deletes it and opens the file", async ({ page }) => {
  const { vp, hint } = await lastRunThenShelf(page);
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
  await expect(hint).toContainText("Opened shelf.shz");
  // Nothing was unsaved, so nothing was asked.
  expect((await bridge(page)).asked).toEqual([]);
  await expect.poll(() => slot(page, "offered-meta")).toBe(null);
  // The offer is answered: Edit > Restore Unsaved Work is off, and a reload offers nothing.
  await page.reload();
  await loaded(page);
  await page.waitForTimeout(500);
  await expect(hint).not.toContainText("Restore unsaved work?");
});

test("with the autosave off (?autosave=0) a start-up file opens at once", async ({ page }) => {
  const vp = await started(page, SHELF, "/?autosave=0");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
});
