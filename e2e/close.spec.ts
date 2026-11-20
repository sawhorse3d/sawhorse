import { expect, test, type Page } from "@playwright/test";
import { answerUnsaved, box, unsavedDialog } from "./helpers";
import { answerSave, bridge, closeWindow, desktopBridge, savedBytes, setBridge } from "./tauri";

// Lane 35 (#44): closing the desktop window over unsaved changes asks first, File > Close
// closes the model and keeps Sawhorse running, and File > Exit asks as the window's X does.
// The desktop shell cannot run here: the page runs as it does inside it, with the bridge
// stood in for (e2e/tauri.ts). The shell's own rule, that a close request two seconds after
// an unanswered question closes the window whatever the page does, is tested in
// src-tauri/close-guard (cargo test -p sawhorse-close-guard).
//
// #120: the question has three answers, Save, Discard Changes and Cancel, and is the page's
// own dialog (what it is as a dialog: e2e/unsaved.spec.ts). Here: each answer at the window's
// X, at File > Exit and at File > Close, with a model that has a file and one that has none,
// a save dialog dismissed and a save that fails, and the "saving" the shell hears while a
// save dialog is up.
const requireKernel = !!process.env.REQUIRE_KERNEL;
const CLOSE_QUESTION = "Save the changes to this model before Sawhorse closes?";
const UNSAVED_QUESTION = "Save the changes to this model?";
const SHELF = "C:\\Users\\maker\\Documents\\Sawhorse\\shelf.shz";
const CABINET = "C:\\Users\\maker\\Documents\\Sawhorse\\cabinet.shz";
/** A person's Cancel comes later than this after the question (src/io/closing.ts, ANSWER_FLOOR_MS): sooner, and the page takes the dialog not to have stayed up. */
const ANSWER_FLOOR_MS = 150;

async function ready(page: Page, url = "/?autosave=0") {
  await page.goto(url);
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** A desktop page with a box drawn: unsaved changes, and the shell told so. */
async function dirty(page: Page, url?: string) {
  await desktopBridge(page);
  const vp = await ready(page, url);
  await expect.poll(async () => (await bridge(page)).said).toContain("saved");
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  await expect.poll(async () => (await bridge(page)).unsaved).toBe(true);
  return vp;
}

async function fileMenu(page: Page) {
  await page.getByRole("button", { name: "File", exact: true }).click();
  return page.getByRole("menu", { name: "File" }).locator(":scope > button, :scope > .subwrap > button");
}

/** The autosave's log, or null (src/lib/autosave.ts: IndexedDB "sawhorse", store "autosave", key "current"). */
function autosaved(page: Page) {
  return page.evaluate(
    () =>
      new Promise<number | null>((resolve) => {
        const req = indexedDB.open("sawhorse");
        req.onsuccess = () => {
          const db = req.result;
          if (!db.objectStoreNames.contains("autosave")) return resolve(null);
          const get = db.transaction("autosave").objectStore("autosave").get("current");
          get.onsuccess = () => {
            resolve(typeof get.result === "string" ? get.result.length : null);
            db.close();
          };
          get.onerror = () => resolve(null);
        };
        req.onerror = () => resolve(null);
      }),
  );
}

test("the desktop File menu has Close next to Open and ends with Exit, named for the platform, and listens for close requests", async ({ page }) => {
  await desktopBridge(page);
  await ready(page);
  await expect(await fileMenu(page)).toHaveText([/New/, /Open…\s*Ctrl\+O/, "Close", /Save\s*Ctrl\+S/, /Save As…/, /Import…\s*Ctrl\+I/, /Import units/, /Export/, /Print Sheets…/, "Exit"]);
  expect(await page.evaluate(() => (window as unknown as { __tauriMock: { listeners(e: string): number } }).__tauriMock.listeners("sawhorse://close-requested"))).toBe(1);
  // The page never registers Tauri's own close-requested listener, which would have the shell
  // hold every close until the page destroyed the window.
  expect(await page.evaluate(() => (window as unknown as { __tauriMock: { listeners(e: string): number } }).__tauriMock.listeners("tauri://close-requested"))).toBe(0);
});

test("on macOS the last item is Quit Sawhorse", async ({ page }) => {
  await desktopBridge(page, "MacIntel");
  await ready(page);
  await expect((await fileMenu(page)).last()).toHaveText("Quit Sawhorse");
});

test("the browser build has File > Close and no Exit (a page cannot close its tab)", async ({ page }) => {
  await ready(page);
  const items = await fileMenu(page);
  await expect(items.nth(2)).toHaveText("Close");
  await expect(items.last()).toHaveText(/Print Sheets…/);
  await expect(page.getByRole("menuitem", { name: /^(Exit|Quit)/ })).toHaveCount(0);
});

test("closing the window with nothing unsaved closes it without a question", async ({ page }) => {
  await desktopBridge(page);
  await ready(page);
  await expect.poll(async () => (await bridge(page)).said).toContain("saved");
  await closeWindow(page);
  const b = await bridge(page);
  expect(b.ended).toBe("closed");
  expect(b.asked).toEqual([]);
});

test("the page saved since the shell looked: the request closes without a question", async ({ page }) => {
  // The shell sent the request (it last heard "unsaved"); the page finds nothing unsaved.
  await desktopBridge(page);
  await ready(page);
  await page.evaluate(() => (window as unknown as { __tauriMock: { emit(e: string): void } }).__tauriMock.emit("sawhorse://close-requested"));
  await expect.poll(async () => (await bridge(page)).ended).toBe("destroyed");
  expect((await bridge(page)).asked).toEqual([]);
});

/** The close question is up, and has been for longer than a person's fastest answer. */
async function closeQuestion(page: Page, text = CLOSE_QUESTION) {
  await expect(unsavedDialog(page).locator(".question")).toHaveText(text);
  await page.waitForTimeout(ANSWER_FLOOR_MS + 100);
}

/** How many times the page has said "saving" to the shell. */
async function savings(page: Page) {
  return (await bridge(page)).said.filter((w) => w === "saving").length;
}

/** A desktop page started with shelf.shz (a box, saved in this test's own page), then a second box drawn: a model with a file, and unsaved changes. */
async function dirtyShelf(page: Page) {
  // The file: a box saved through the stand-in's Save dialog, read back from its disk.
  await desktopBridge(page);
  await ready(page);
  await expect.poll(async () => (await bridge(page)).said).toContain("saved");
  await box(page);
  await setBridge(page, { saveAs: SHELF });
  await page.keyboard.press("Control+s");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect.poll(async () => (await bridge(page)).unsaved).toBe(false);
  await box(page, "24,16", "12");
  const vp = page.locator("main.vp");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await expect(page).toHaveTitle("• shelf - Sawhorse");
  await expect.poll(async () => (await bridge(page)).unsaved).toBe(true);
  return vp;
}

// ---- the window's X ---------------------------------------------------------------------

test("the X over unsaved changes: Cancel keeps the window, the model and the unsaved mark, and the next close asks again", async ({ page }) => {
  const vp = await dirty(page);
  await closeWindow(page);
  await closeQuestion(page);
  await answerUnsaved(page, "Cancel");
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  await expect(unsavedDialog(page)).toHaveCount(0);
  const b = await bridge(page);
  expect(b.ended).toBeNull();
  // The question is the page's own: the desktop's message dialog was not used, nor a save.
  expect(b.asked).toEqual([]);
  expect(b.saves).toEqual([]);
  expect(b.said).not.toContain("saving");
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  // Asked again on the next close.
  await closeWindow(page);
  await closeQuestion(page);
  await answerUnsaved(page, "Cancel");
  await expect.poll(async () => (await bridge(page)).said.filter((w) => w === "keep")).toHaveLength(2);
});

test("the X over unsaved changes: Escape is Cancel", async ({ page }) => {
  const vp = await dirty(page);
  await closeWindow(page);
  await closeQuestion(page);
  await page.keyboard.press("Escape");
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  await expect(unsavedDialog(page)).toHaveCount(0);
  expect((await bridge(page)).ended).toBeNull();
  await expect(vp).toHaveAttribute("data-objects", "1");
});

test("the X over unsaved changes: Discard Changes deletes the autosave as File > New does, then closes", async ({ page }) => {
  await dirty(page, "/");
  await expect.poll(() => autosaved(page)).not.toBeNull();
  await closeWindow(page);
  await closeQuestion(page);
  await answerUnsaved(page, "Discard Changes");
  await expect.poll(async () => (await bridge(page)).ended).toBe("destroyed");
  const b = await bridge(page);
  expect(b.said.at(-1)).toBe("close");
  expect(b.said).not.toContain("saving");
  expect(b.written).toEqual([]);
  expect(await autosaved(page)).toBeNull();
});

test("the X over unsaved changes, a model with a file: Save writes the file and closes, with no dialog to choose a name", async ({ page }) => {
  await dirtyShelf(page);
  const before = (await bridge(page)).written.length;
  await closeWindow(page);
  await closeQuestion(page, "Save the changes to shelf.shz before Sawhorse closes?");
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).ended).toBe("destroyed");
  const b = await bridge(page);
  expect(b.written.slice(before)).toEqual([SHELF]);
  // One Save dialog, the one that named the file at the start.
  expect(b.saves).toHaveLength(1);
  // "saved" (the unsaved mark going) and "close" (the answer) reach the shell in either order.
  expect(b.said).toContain("close");
  expect(b.said).toContain("saving");
  expect(b.said).not.toContain("keep");
});

test("the X over unsaved changes, an untitled model: Save asks for a name, the shell hears saving while the dialog is up, and the window closes once the file is written", async ({ page }) => {
  const vp = await dirty(page);
  await setBridge(page, { saveHold: true });
  await closeWindow(page);
  await closeQuestion(page);
  await answerUnsaved(page, "Save");
  // The Save dialog is up, offering the untitled name; the question shows what is going on.
  await expect.poll(async () => (await bridge(page)).saves.map((s) => s.defaultPath)).toEqual(["Untitled.shz"]);
  await expect(unsavedDialog(page).getByRole("status")).toHaveText("Saving…");
  await expect(unsavedDialog(page).getByRole("button")).toHaveText(["Cancel"]);
  // The word keeps coming for as long as the dialog is up.
  const first = await savings(page);
  expect(first).toBeGreaterThanOrEqual(1);
  await expect.poll(() => savings(page)).toBeGreaterThanOrEqual(first + 4);
  // A second close request meanwhile (a second click on the X): no second question, no answer yet.
  await closeWindow(page);
  await expect.poll(() => savings(page)).toBeGreaterThanOrEqual(first + 6);
  let b = await bridge(page);
  expect(b.ended).toBeNull();
  expect(b.said).not.toContain("keep");
  expect(b.said).not.toContain("close");
  expect(b.written).toEqual([]);
  await expect(vp).toHaveAttribute("data-objects", "1");
  // A name chosen: the model is saved and the window closes.
  await answerSave(page, CABINET);
  await expect.poll(async () => (await bridge(page)).ended).toBe("destroyed");
  b = await bridge(page);
  expect(b.written).toEqual([CABINET]);
  expect((await savedBytes(page)).length).toBeGreaterThan(100);
  expect(b.said).toContain("close");
  expect(b.said).not.toContain("keep");
  // No more of the word once the save has ended.
  const after = await savings(page);
  await page.waitForTimeout(600);
  expect(await savings(page)).toBe(after);
});

test("the X over unsaved changes, an untitled model: Save, and the save dialog dismissed, keeps the window, the model and the autosave", async ({ page }) => {
  const vp = await dirty(page, "/");
  await expect.poll(() => autosaved(page)).not.toBeNull();
  await setBridge(page, { saveHold: true });
  await closeWindow(page);
  await closeQuestion(page);
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).saves).toHaveLength(1);
  await answerSave(page, null);
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  await expect(unsavedDialog(page)).toHaveCount(0);
  const b = await bridge(page);
  expect(b.ended).toBeNull();
  expect(b.written).toEqual([]);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  expect(await autosaved(page)).not.toBeNull();
  // The word stops with the save.
  const after = await savings(page);
  await page.waitForTimeout(600);
  expect(await savings(page)).toBe(after);
  // And the next close asks again.
  await closeWindow(page);
  await closeQuestion(page);
});

test("the X over unsaved changes: Save, and a save that fails, says why and keeps the window and the model", async ({ page }) => {
  const vp = await dirty(page);
  await setBridge(page, { saveAs: CABINET, saveFails: "No space left on device (os error 28)" });
  await closeWindow(page);
  await closeQuestion(page);
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  await expect(page.locator("#hint")).toContainText("Could not save: No space left on device (os error 28)");
  await expect(unsavedDialog(page)).toHaveCount(0);
  const b = await bridge(page);
  expect(b.ended).toBeNull();
  expect(b.written).toEqual([]);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
});

test("a second request while the question is up asks nothing more", async ({ page }) => {
  await dirty(page);
  await closeWindow(page);
  await closeQuestion(page);
  // The shell sends no second request within two seconds; even if one came, one question.
  await closeWindow(page);
  await closeWindow(page);
  await expect(unsavedDialog(page)).toHaveCount(1);
  await answerUnsaved(page, "Cancel");
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  await expect(unsavedDialog(page)).toHaveCount(0);
  expect((await bridge(page)).said.filter((w) => w === "keep")).toHaveLength(1);
});

test("an answer that cannot reach the shell still tells it the question failed", async ({ page }) => {
  await dirty(page);
  await setBridge(page, { failWords: ["keep"] });
  await closeWindow(page);
  await closeQuestion(page);
  await answerUnsaved(page, "Cancel");
  await expect.poll(async () => (await bridge(page)).said.slice(-2)).toEqual(["keep", "failed"]);
  await expect(page.locator("#hint")).toContainText("Sawhorse could not ask about the unsaved changes");
  await expect(page.locator("#hint")).toContainText("Close the window again to close it without saving");
});

// ---- File > Close ------------------------------------------------------------------------

test("File > Close on a clean model starts an empty untitled one at once, Sawhorse still running", async ({ page }) => {
  await desktopBridge(page);
  const vp = await ready(page);
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await expect(page.locator("#hint")).toContainText("Closed Untitled.");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(unsavedDialog(page)).toHaveCount(0);
  const b = await bridge(page);
  expect(b.asked).toEqual([]);
  expect(b.ended).toBeNull();
  expect(b.said).not.toContain("exit");
});

test("File > Close over unsaved changes asks File > New's question: Cancel keeps the model, Discard Changes closes it", async ({ page }) => {
  const vp = await dirty(page);
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await expect(unsavedDialog(page).locator(".question")).toHaveText(UNSAVED_QUESTION);
  await answerUnsaved(page, "Cancel");
  await expect(unsavedDialog(page)).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await answerUnsaved(page, "Discard Changes");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page.locator("#hint")).toContainText("Closed Untitled.");
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  await expect.poll(async () => (await bridge(page)).unsaved).toBe(false);
  const b = await bridge(page);
  expect(b.asked).toEqual([]);
  expect(b.written).toEqual([]);
  expect(b.ended).toBeNull();
  // Not a close of the window: the shell hears nothing of it but the unsaved mark.
  expect(b.said.filter((w) => w !== "saved" && w !== "unsaved")).toEqual([]);
});

test("File > Close over unsaved changes, an untitled model: Save asks for a name, saves, then closes the model; the window stays", async ({ page }) => {
  const vp = await dirty(page);
  await setBridge(page, { saveHold: true });
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).saves.map((s) => s.defaultPath)).toEqual(["Untitled.shz"]);
  // Nothing is closed while the name is being chosen.
  await expect(vp).toHaveAttribute("data-objects", "1");
  await answerSave(page, CABINET);
  await expect(page.locator("#hint")).toContainText("Closed cabinet.");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  await expect(unsavedDialog(page)).toHaveCount(0);
  const b = await bridge(page);
  expect(b.written).toEqual([CABINET]);
  expect(b.ended).toBeNull();
  expect(b.said).not.toContain("close");
  expect(b.said).not.toContain("keep");
});

test("File > Close over unsaved changes: Save, and the save dialog dismissed, closes nothing", async ({ page }) => {
  const vp = await dirty(page);
  await setBridge(page, { saveHold: true });
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).saves).toHaveLength(1);
  await answerSave(page, null);
  await expect(unsavedDialog(page)).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  await expect(page.locator("#hint")).not.toContainText("Closed");
  expect((await bridge(page)).written).toEqual([]);
});

test("the X while a save chosen at File > Close's question has its dialog up: the window waits for the save, then closes with nothing left to ask", async ({ page }) => {
  const vp = await dirty(page);
  await setBridge(page, { saveHold: true });
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).saves).toHaveLength(1);
  const first = await savings(page);
  await closeWindow(page);
  // The page is asked, and answers nothing while the save runs: the word is what the shell hears.
  await expect.poll(() => savings(page)).toBeGreaterThanOrEqual(first + 4);
  let b = await bridge(page);
  expect(b.ended).toBeNull();
  expect(b.said).not.toContain("keep");
  await expect(unsavedDialog(page).getByRole("status")).toHaveText("Saving…");
  await answerSave(page, CABINET);
  // Saved, the model closed (File > Close), and the close request finds nothing unsaved.
  await expect.poll(async () => (await bridge(page)).ended).toBe("destroyed");
  b = await bridge(page);
  expect(b.written).toEqual([CABINET]);
  expect(b.said).toContain("close");
  expect(b.said).not.toContain("keep");
  await expect(vp).toHaveAttribute("data-objects", "0");
});

// ---- File > Exit -------------------------------------------------------------------------

test("File > Exit goes through the window's close: at once when clean", async ({ page }) => {
  await desktopBridge(page);
  await ready(page);
  await expect.poll(async () => (await bridge(page)).said).toContain("saved");
  await (await fileMenu(page)).getByText("Exit", { exact: true }).click();
  await expect.poll(async () => (await bridge(page)).ended).toBe("closed");
  expect((await bridge(page)).asked).toEqual([]);
  await expect(unsavedDialog(page)).toHaveCount(0);
});

test("File > Exit over unsaved changes asks as the X does, and Cancel keeps everything", async ({ page }) => {
  const vp = await dirty(page);
  await (await fileMenu(page)).getByText("Exit", { exact: true }).click();
  await closeQuestion(page);
  await answerUnsaved(page, "Cancel");
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  const b = await bridge(page);
  expect(b.said).toContain("exit");
  expect(b.ended).toBeNull();
  await expect(vp).toHaveAttribute("data-objects", "1");
});

test("File > Exit over unsaved changes: Discard Changes closes the window", async ({ page }) => {
  await dirty(page);
  await (await fileMenu(page)).getByText("Exit", { exact: true }).click();
  await closeQuestion(page);
  await answerUnsaved(page, "Discard Changes");
  await expect.poll(async () => (await bridge(page)).ended).toBe("destroyed");
  expect((await bridge(page)).written).toEqual([]);
});

test("File > Exit over unsaved changes, a model with a file: Save writes it and closes the window", async ({ page }) => {
  await dirtyShelf(page);
  const before = (await bridge(page)).written.length;
  await (await fileMenu(page)).getByText("Exit", { exact: true }).click();
  await closeQuestion(page, "Save the changes to shelf.shz before Sawhorse closes?");
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).ended).toBe("destroyed");
  const b = await bridge(page);
  expect(b.written.slice(before)).toEqual([SHELF]);
  expect(b.said).toContain("exit");
  expect(b.said).toContain("close");
});

test("File > Exit over unsaved changes, an untitled model: Save asks for a name; dismissed, the window stays; chosen, it closes", async ({ page }) => {
  const vp = await dirty(page);
  await setBridge(page, { saveHold: true });
  await (await fileMenu(page)).getByText("Exit", { exact: true }).click();
  await closeQuestion(page);
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).saves).toHaveLength(1);
  await answerSave(page, null);
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  expect((await bridge(page)).ended).toBeNull();
  await expect(vp).toHaveAttribute("data-objects", "1");
  await (await fileMenu(page)).getByText("Exit", { exact: true }).click();
  await closeQuestion(page);
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).saves).toHaveLength(2);
  await answerSave(page, CABINET);
  await expect.poll(async () => (await bridge(page)).ended).toBe("destroyed");
  expect((await bridge(page)).written).toEqual([CABINET]);
});
