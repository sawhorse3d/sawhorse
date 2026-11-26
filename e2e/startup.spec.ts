import { expect, test, type Page } from "@playwright/test";
import { answerUnsaved, box, unsavedDialog } from "./helpers";
import { bridge, desktopBridge, dropFiles, restart } from "./tauri";

// Lane 37 (#45): a double-click on a .shz in Windows Explorer starts Sawhorse with the file's
// path, and so does a path after the program's name. The shell picks the file from its
// arguments (src-tauri/start-file, tested with cargo) and hands it over when the page asks
// (start_file, once); the page opens it the way it opens a dropped file. The desktop shell
// cannot run here: the page runs as it does inside it, with the bridge stood in for
// (e2e/tauri.ts), start_file and the fs plugin's reads included.
const requireKernel = !!process.env.REQUIRE_KERNEL;
const SHELF = "C:\\Users\\maker\\Documents\\Sawhorse\\shelf.shz";
const GONE = "C:\\Users\\maker\\Documents\\Sawhorse\\gone.shz";
const UNSAVED_QUESTION = "Save the changes to this model?";

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
  // Opening the file asks about the restored work, as File > Open does: Cancel keeps it.
  const open = hint.getByRole("button", { name: "Open shelf.shz" });
  await open.click();
  await expect(unsavedDialog(page).locator(".question")).toHaveText(UNSAVED_QUESTION);
  await answerUnsaved(page, "Cancel");
  await expect(unsavedDialog(page)).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-hash", work);
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  // Discard Changes opens it in place of the work.
  await open.click();
  await expect(unsavedDialog(page).locator(".question")).toHaveText(UNSAVED_QUESTION);
  await answerUnsaved(page, "Discard Changes");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
  // The question is the page's own (#120): the desktop's message dialog was not used.
  expect((await bridge(page)).asked).toEqual([]);
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

test("two pieces of unsaved work and a start-up file: a Discard offers the next at once, and the file opens after the last (#94)", async ({ page }) => {
  // Lane 64: the last run's work is offered first, the run before's after it, and the file
  // waits for both answers.
  const vp = await started(page, null);
  await box(page, "24,16", "12");
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, null);
  await loaded(page);
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await box(page);
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, SHELF);
  await loaded(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps, saved just now. 1 older set of unsaved work is set aside too. shelf.shz opens after you answer.");
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid, 3 steps, saved just now. shelf.shz opens after you answer.");
  await expect(vp).toHaveAttribute("data-objects", "0");
  expect((await bridge(page)).reads).toEqual([]);
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
  await expect.poll(() => slot(page, "offered-meta")).toBe(null);
});

// #115: the offer was only in the status line, so a double-clicked file looked like an empty
// model. While a file waits, the same question, buttons and file name are over the model.
const notice = (page: Page) => page.getByTestId("startup-notice");

/** The notice lies inside the model area's box (the sheet area's, in Sheets mode) and over
 * none of the viewport's chips along its top. */
async function expectOverModel(page: Page, area = "main.vp") {
  await expect(notice(page)).toBeVisible();
  const n = (await notice(page).boundingBox())!;
  const a = (await page.locator(area).boundingBox())!;
  expect(n.x).toBeGreaterThanOrEqual(a.x);
  expect(n.y).toBeGreaterThanOrEqual(a.y);
  expect(n.x + n.width).toBeLessThanOrEqual(a.x + a.width);
  expect(n.y + n.height).toBeLessThanOrEqual(a.y + a.height);
  // In the middle of it, where the empty model is.
  expect(Math.abs(n.x + n.width / 2 - (a.x + a.width / 2)), "centred across").toBeLessThan(2);
  expect(Math.abs(n.y + n.height / 2 - (a.y + a.height / 2)), "centred down").toBeLessThan(2);
  for (const chip of await page.locator(".vp .hud span").all()) {
    const c = await chip.boundingBox();
    if (c) expect(c.y + c.height <= n.y || c.y >= n.y + n.height || c.x + c.width <= n.x || c.x >= n.x + n.width, "a chip under the notice").toBe(true);
  }
}

test("a waiting file: the offer, its buttons and the file's name over the model, kept when the status line moves on; Discard there opens the file (#115)", async ({ page }) => {
  const { vp, hint } = await lastRunThenShelf(page);
  const n = notice(page);
  await expect(n).toBeVisible();
  // Over the model area: inside the viewport's box, clear of its chips along the top.
  await expectOverModel(page);
  await expect(n).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await expect(n).toContainText("shelf.shz opens after you answer.");
  await expect(n.getByRole("button")).toHaveText(["Restore", "Discard"]);
  // Another message takes the status line: the question stays over the model.
  await page.keyboard.press("Control+z");
  await expect(hint).not.toContainText("Restore unsaved work?");
  await expect(n).toContainText("shelf.shz opens after you answer.");
  await expect(n.getByRole("button")).toHaveText(["Restore", "Discard"]);
  // Discard from it: the work goes, the file opens, and the question goes with it.
  await n.getByRole("button", { name: "Discard" }).click();
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
  await expect(n).toHaveCount(0);
  await expect.poll(() => slot(page, "offered-meta")).toBe(null);
});

test("a waiting file: Restore over the model brings the work back and keeps the file's Open button there (#115)", async ({ page }) => {
  const { vp, work } = await lastRunThenShelf(page);
  const n = notice(page);
  await n.getByRole("button", { name: "Restore" }).click();
  await expect(vp).toHaveAttribute("data-hash", work);
  await expect(n).toContainText("shelf.shz is not open yet.");
  await expect(n.getByRole("button")).toHaveText(["Open shelf.shz"]);
  expect((await bridge(page)).reads).toEqual([]);
  // Open from it asks about the restored work, as the status line's button does.
  await n.getByRole("button", { name: "Open shelf.shz" }).click();
  await expect(unsavedDialog(page).locator(".question")).toHaveText(UNSAVED_QUESTION);
  await answerUnsaved(page, "Discard Changes");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
  await expect(n).toHaveCount(0);
});

test("a waiting file at a start in Sheets mode: the notice over the sheet area, which hides the 3D view (#115)", async ({ page }) => {
  const vp = await started(page, null);
  await box(page, "24,16", "12");
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  // The mode is kept between runs: this start comes up in Sheets mode.
  await page.locator('.scenes:not(.sheet-tabs) .mode-switch [data-mode="sheets"]').click();
  await expect(page.locator(".app")).toHaveClass(/sheets-mode/);
  await restart(page, SHELF);
  await loaded(page);
  await expect(page.locator(".app")).toHaveClass(/sheets-mode/);
  await expect(page.locator("#hint")).toContainText("shelf.shz opens after you answer.");
  await expectOverModel(page, ".sheet-area");
  await expect(notice(page)).toContainText("shelf.shz opens after you answer.");
  await notice(page).getByRole("button", { name: "Discard" }).click();
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
  await expect(notice(page)).toHaveCount(0);
});

test("two pieces of unsaved work and a waiting file: Discard twice from the notice, and the file opens after the last (#115)", async ({ page }) => {
  const vp = await started(page, null);
  await box(page, "24,16", "12");
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, null);
  await loaded(page);
  await expect(page.locator("#hint")).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await box(page);
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, SHELF);
  await loaded(page);
  const n = notice(page);
  await expect(n).toContainText("1 older set of unsaved work is set aside too. shelf.shz opens after you answer.");
  await n.getByRole("button", { name: "Discard" }).click();
  await expect(n).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid, 3 steps, saved just now. shelf.shz opens after you answer.");
  await expect(vp).toHaveAttribute("data-objects", "0");
  expect((await bridge(page)).reads).toEqual([]);
  await n.getByRole("button", { name: "Discard" }).click();
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
  await expect(n).toHaveCount(0);
});

test("two pieces of unsaved work and a waiting file: a double-click on the notice's Discard discards only the first (#115)", async ({ page }) => {
  // The next offer's Discard is drawn where the first one was: the second click of a
  // double-click must not delete work the person has not read.
  const vp = await started(page, null);
  await box(page, "24,16", "12");
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, null);
  await loaded(page);
  await expect(page.locator("#hint")).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await box(page);
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, SHELF);
  await loaded(page);
  const n = notice(page);
  await expect(n).toContainText("1 older set of unsaved work is set aside too.");
  await n.getByRole("button", { name: "Discard" }).dblclick();
  await expect(n).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid, 3 steps, saved just now. shelf.shz opens after you answer.");
  await expect(page.locator("#hint")).toContainText("Unsaved work discarded. Restore unsaved work?");
  await expect.poll(() => slot(page, "offered-meta")).toBe(3);
  await expect(vp).toHaveAttribute("data-objects", "0");
  expect((await bridge(page)).reads).toEqual([]);
  // A single click then answers the second offer.
  await n.getByRole("button", { name: "Discard" }).click();
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(n).toHaveCount(0);
});

// #145: answering the offer puts the next set's offer in the same place at once, so a second
// click or key press meant for the first must not answer the next, which is not read yet.

/** Two sets of unsaved work set aside (24 x 16 x 12 in, then 12 x 12 x 6 in, each 3 steps),
 * and a start with `start`. */
async function twoSets(page: Page, start: string | null) {
  const vp = await started(page, null);
  await box(page, "24,16", "12");
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, null);
  await loaded(page);
  await expect(page.locator("#hint")).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await box(page);
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, start);
  await loaded(page);
  const hint = page.locator("#hint");
  await expect(hint).toContainText("1 older set of unsaved work is set aside too.");
  return { vp, hint };
}

/** After one answer of Discard: the next set is offered, still kept, and nothing else changed. */
async function oneDiscarded(page: Page, vp: ReturnType<Page["locator"]>, hint: ReturnType<Page["locator"]>) {
  await expect(hint).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid, 3 steps");
  await expect.poll(() => slot(page, "offered-meta")).toBe(3);
  await expect(vp).toHaveAttribute("data-objects", "0");
  expect((await bridge(page)).reads).toEqual([]);
}

/** The next offer, answered by a click of its own after the extra click or key presses, still
 * restores its set: so those presses had been taken, and had answered nothing, by then. */
async function nextStillOffered(vp: ReturnType<Page["locator"]>, hint: ReturnType<Page["locator"]>) {
  await hint.getByRole("button", { name: "Restore", exact: true }).click();
  await expect(hint).toContainText("Restored your unsaved work: 1 solid, 3 steps");
  await expect(vp).toHaveAttribute("data-objects", "1");
}

// Where the next offer's buttons land depends on the window's width and the two texts, so a
// real double-click hits them only in some windows (336 px wide in the issue, not 1440). The
// second click is sent to the next offer's button itself, as it lands when the layout puts it
// under the pointer: a click whose detail is 2.
async function secondClickOn(button: ReturnType<Page["locator"]>) {
  await button.dispatchEvent("click", { detail: 2, bubbles: true });
}

test("two sets kept: a double-click on the status line's Discard whose second click lands on the next Discard discards only the first (#145)", async ({ page }) => {
  const { vp, hint } = await twoSets(page, null);
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded.");
  await secondClickOn(hint.getByRole("button", { name: "Discard" }));
  await oneDiscarded(page, vp, hint);
  await nextStillOffered(vp, hint);
});

test("two sets and a waiting file: a double-click on the status line's Discard whose second click lands on the next Restore restores nothing (#145)", async ({ page }) => {
  const { vp, hint } = await twoSets(page, SHELF);
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded.");
  await secondClickOn(hint.getByRole("button", { name: "Restore" }));
  await oneDiscarded(page, vp, hint);
  await expect(hint).toContainText("shelf.shz opens after you answer.");
  await nextStillOffered(vp, hint);
});

/** The window widths tried, in order, for a real double-click whose second click lands on the
 * next offer's button. */
const DOUBLE_CLICK_WIDTHS = [896, 464, 336, 384, 432, 512, 576, 640, 768, 1024];

test("two sets kept: a real double-click on the status line's Discard, where the next offer's button lands under the pointer, answers only the first (#145)", async ({ page }) => {
  // Where the next offer's buttons land depends on the window's width, the two texts and the
  // fonts, so the window is found from the live layout, not assumed. For each width tried, the
  // page is loaded at that width, the first offer is answered once with Enter (no pointer), and
  // the next offer's buttons are measured; the width taken is the first where a point 2 px or
  // more inside the first offer's Discard is as far inside one of the next offer's buttons.
  // The first set is then made again (the same box), and the double-click goes to that point.
  test.setTimeout(90_000);
  type Box = { x: number; y: number; width: number; height: number };
  const hitIn = (a: Box, b: Box): [number, number] | null => {
    const x0 = Math.max(a.x, b.x) + 2;
    const x1 = Math.min(a.x + a.width, b.x + b.width) - 2;
    const y0 = Math.max(a.y, b.y) + 2;
    const y1 = Math.min(a.y + a.height, b.y + b.height) - 2;
    return x1 > x0 && y1 > y0 ? [(x0 + x1) / 2, (y0 + y1) / 2] : null;
  };
  await page.setViewportSize({ width: DOUBLE_CLICK_WIDTHS[0], height: 700 });
  const { vp, hint } = await twoSets(page, null);
  const discard = hint.getByRole("button", { name: "Discard", exact: true });
  /** The newest set made again, and the page loaded at `width`: two sets offered. */
  const again = async (width: number) => {
    await box(page);
    await expect.poll(() => slot(page, "current-meta")).toBe(3);
    await page.setViewportSize({ width, height: 700 });
    await restart(page, null);
    await loaded(page);
    await expect(hint).toContainText("1 older set of unsaved work is set aside too.");
  };
  let aim: { at: [number, number]; label: string; first: Box } | null = null;
  for (let i = 0; i < DOUBLE_CLICK_WIDTHS.length && !aim; i++) {
    if (i > 0) await again(DOUBLE_CLICK_WIDTHS[i]);
    const first = (await discard.boundingBox())!;
    await discard.focus();
    await page.keyboard.press("Enter");
    await expect(hint).toContainText("Unsaved work discarded.");
    for (const label of ["Restore", "Discard"]) {
      const at = hitIn(first, (await hint.getByRole("button", { name: label, exact: true }).boundingBox())!);
      if (at && !aim) aim = { at, label, first };
    }
    if (aim) {
      test.info().annotations.push({ type: "window", description: `${DOUBLE_CLICK_WIDTHS[i]} px wide, the second click on the next ${aim.label}` });
      await again(DOUBLE_CLICK_WIDTHS[i]);
    }
  }
  if (!aim) throw new Error(`no window width of ${DOUBLE_CLICK_WIDTHS.join(", ")} px puts a button of the next offer under the first offer's Discard`);
  // The same layout as when it was measured.
  expect(await discard.boundingBox()).toEqual(aim.first);
  const [x, y] = aim.at;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.up();
  await expect(hint).toContainText("Unsaved work discarded.");
  // The second click of the double-click lands on the next offer's button.
  expect(await page.evaluate(([x, y]) => document.elementFromPoint(x, y)?.textContent, [x, y])).toBe(aim.label);
  await page.mouse.down({ clickCount: 2 });
  await page.mouse.up({ clickCount: 2 });
  await oneDiscarded(page, vp, hint);
  // The second press gave the next offer's button the focus; the click that answered nothing
  // gave it up again, so Enter now would not answer that offer either.
  expect(await page.evaluate(() => document.activeElement === document.body)).toBe(true);
  await nextStillOffered(vp, hint);
});

test("two sets kept: Enter pressed twice on a focused Discard discards only the first (#145)", async ({ page }) => {
  const { vp, hint } = await twoSets(page, null);
  await hint.getByRole("button", { name: "Discard" }).focus();
  await page.keyboard.press("Enter");
  await expect(hint).toContainText("Unsaved work discarded.");
  await page.keyboard.press("Enter");
  await oneDiscarded(page, vp, hint);
  // The focus is given up, to the page itself: not to the next offer's Discard, nor to
  // anything else Enter would press or commit (the measurements box).
  expect(await page.evaluate(() => document.activeElement === document.body)).toBe(true);
  await nextStillOffered(vp, hint);
});

test("two sets kept: Enter held down on a focused Discard discards only the first (#145)", async ({ page }) => {
  const { vp, hint } = await twoSets(page, null);
  await hint.getByRole("button", { name: "Discard" }).focus();
  // Held: the first press, then the key's repeats, then the release.
  await page.keyboard.down("Enter");
  await expect(hint).toContainText("Unsaved work discarded.");
  for (let k = 0; k < 6; k++) await page.keyboard.down("Enter");
  await page.keyboard.up("Enter");
  await oneDiscarded(page, vp, hint);
  await nextStillOffered(vp, hint);
});

test("two sets and a waiting file: Enter twice on the notice's focused Discard discards only the first (#145)", async ({ page }) => {
  const { vp, hint } = await twoSets(page, SHELF);
  const n = notice(page);
  await n.getByRole("button", { name: "Discard" }).focus();
  await page.keyboard.press("Enter");
  await expect(n).toContainText("Unsaved work discarded.");
  await page.keyboard.press("Enter");
  await oneDiscarded(page, vp, hint);
  await expect(n).toContainText("shelf.shz opens after you answer.");
  expect(await page.evaluate(() => document.activeElement === document.body)).toBe(true);
  await nextStillOffered(vp, hint);
});

test("a waiting file whose name has no place to break: the notice wraps it and stays inside the model area (#115)", async ({ page }) => {
  const LONG = `C:\\Users\\maker\\Documents\\Sawhorse\\${"x".repeat(196)}.shz`;
  await desktopBridge(page, "Win32", { start: null, files: { [LONG]: shelf } });
  const vp = await ready(page);
  await box(page, "24,16", "12");
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, LONG);
  await loaded(page);
  await expect(notice(page)).toContainText(`${"x".repeat(196)}.shz opens after you answer.`);
  const n = (await notice(page).boundingBox())!;
  const a = (await vp.boundingBox())!;
  expect(n.x).toBeGreaterThanOrEqual(a.x);
  expect(n.x + n.width).toBeLessThanOrEqual(a.x + a.width);
  const text = notice(page).locator("p");
  expect(await text.evaluate((p) => p.scrollWidth <= p.clientWidth)).toBe(true);
});

test("no file waiting: the offer stays in the status line only, as before (#115)", async ({ page }) => {
  const vp = await started(page, null);
  await box(page, "24,16", "12");
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, null);
  await loaded(page);
  await expect(page.locator("#hint")).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await box(page);
  await expect.poll(() => slot(page, "current-meta")).toBe(3);
  await restart(page, null);
  await loaded(page);
  await expect(page.locator("#hint")).toContainText("Restore unsaved work? 1 solid, 3 steps, saved just now. 1 older set of unsaved work is set aside too.");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(notice(page)).toHaveCount(0);
  // Restore and then Discard with no file waiting: the messages after them are the status
  // line's alone.
  await page.locator("#hint").getByRole("button", { name: "Restore" }).click();
  await expect(page.locator("#hint")).toContainText("Restored your unsaved work: 1 solid, 3 steps (undo works as before).");
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(notice(page)).toHaveCount(0);
  await page.getByTestId("hint-offers").click();
  await page.locator("#hint").getByRole("button", { name: "Discard" }).click();
  await expect(page.locator("#hint")).toContainText("Unsaved work discarded.");
  await expect(page.locator("#hint")).not.toContainText("Restore unsaved work?");
  await expect(notice(page)).toHaveCount(0);
});

test("a waiting file: the view takes the pointer round the notice, the notice over it (#115)", async ({ page }) => {
  const { vp } = await lastRunThenShelf(page);
  const n = (await notice(page).boundingBox())!;
  const at = (x: number, y: number) =>
    page.evaluate(([x, y]) => {
      const e = document.elementFromPoint(x, y);
      return e?.closest('[data-testid="startup-notice"]') ? "notice" : e?.closest("main.vp") ? "view" : (e?.className ?? null);
    }, [x, y]);
  const midY = n.y + n.height / 2;
  expect(await at(n.x + n.width / 2, midY)).toBe("notice");
  for (const [x, y] of [
    [n.x - 20, midY],
    [n.x + n.width + 20, midY],
    [n.x + n.width / 2, n.y - 20],
    [n.x + n.width / 2, n.y + n.height + 20],
  ])
    expect(await at(x, y), `beside the notice at ${x}, ${y}`).toBe("view");
  // And a wheel there reaches it.
  const camera = await vp.getAttribute("data-camera");
  await page.mouse.move(n.x - 20, midY);
  await page.mouse.wheel(0, -300);
  await expect(vp).not.toHaveAttribute("data-camera", camera!);
});

test("with the autosave off (?autosave=0) a start-up file opens at once", async ({ page }) => {
  const vp = await started(page, SHELF, "/?autosave=0");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
});
