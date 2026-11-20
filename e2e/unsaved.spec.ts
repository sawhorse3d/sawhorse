import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { answerUnsaved, box, unsavedDialog } from "./helpers";
import { answerSave, bridge, closeWindow, desktopBridge, dropFiles, setBridge } from "./tauri";

// #120: the question over unsaved changes has three answers, Save, Discard Changes and
// Cancel, and is the app's own dialog in both builds (src/io/unsaved.ts,
// src/components/UnsavedDialog.tsx). Here:
// - the browser build: each answer at File > New, File > Close and File > Open, a file
//   dropped on the page, a save that fails, the keys, and the dialog as a modal;
// - the page as it runs in the desktop shell (the bridge stood in for, e2e/tauri.ts): a Save
//   dialog for an untitled model at File > New, a file dropped on the window, giving up on a
//   save, one question at a time, and the focus put back.
// The window's X, File > Exit and File > Close in the desktop app are in e2e/close.spec.ts.
// `?io=basic`: Save is a download the test can catch.
const requireKernel = !!process.env.REQUIRE_KERNEL;
const QUESTION = "Save the changes to this model?";
const SHELF = "C:\\Users\\maker\\Documents\\Sawhorse\\shelf.shz";
const CABINET = "C:\\Users\\maker\\Documents\\Sawhorse\\cabinet.shz";

/** shelf.shz: a 24 x 16 x 12 in box, saved by the app in beforeAll; and the hash the model has when it is open. */
let shelf: Buffer;
let shelfHash: string;

async function ready(page: Page, url = "/?io=basic&autosave=0") {
  await page.goto(url);
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

test.beforeAll(async ({ browser }) => {
  const page = await browser.newPage();
  await page.goto("/?io=basic&autosave=0");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if ((await vp.getAttribute("data-kernel")) !== "ready") {
    await page.close();
    return;
  }
  await box(page, "24,16", "12");
  await expect(vp).toHaveAttribute("data-objects", "1");
  shelfHash = (await vp.getAttribute("data-hash"))!;
  const [d] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Control+s")]);
  shelf = readFileSync((await d.path())!);
  await page.close();
});

async function menu(page: Page, top: string, item: string) {
  await page.getByRole("button", { name: top, exact: true }).click();
  await page.getByRole("menuitem", { name: item, exact: true }).click();
}

async function pick(page: Page, name: string, buffer: Buffer) {
  await page.getByTestId("file-input").setInputFiles({ name, mimeType: "application/octet-stream", buffer });
}

/** The browser build with a box drawn: an untitled model with unsaved changes. Returns its hash and the downloads seen from here on. */
async function work(page: Page, url?: string) {
  const vp = await ready(page, url);
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  const hash = (await vp.getAttribute("data-hash"))!;
  const downloads: string[] = [];
  page.on("download", (d) => downloads.push(d.suggestedFilename()));
  return { vp, hash, downloads };
}

/** The work is as it was: the model, its unsaved mark, no dialog. */
async function kept(page: Page, hash: string) {
  const vp = page.locator("main.vp");
  await expect(unsavedDialog(page)).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(vp).toHaveAttribute("data-hash", hash);
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
}

// ---- every answer at New, Close and Open, in the browser build ----------------------------

const PATHS: { name: string; go(page: Page): Promise<void>; ahead(page: Page): Promise<void> }[] = [
  {
    name: "File > New",
    go: (page) => menu(page, "File", "New"),
    async ahead(page) {
      await expect(page.locator("#hint")).toContainText("New model.");
      await expect(page.locator("main.vp")).toHaveAttribute("data-objects", "0");
      await expect(page).toHaveTitle("Untitled - Sawhorse");
    },
  },
  {
    name: "File > Close",
    go: (page) => menu(page, "File", "Close"),
    async ahead(page) {
      await expect(page.locator("#hint")).toContainText("Closed Untitled.");
      await expect(page.locator("main.vp")).toHaveAttribute("data-objects", "0");
      await expect(page).toHaveTitle("Untitled - Sawhorse");
    },
  },
  {
    name: "File > Open",
    go: (page) => pick(page, "shelf.shz", shelf),
    async ahead(page) {
      await expect(page).toHaveTitle("shelf - Sawhorse");
      await expect(page.locator("main.vp")).toHaveAttribute("data-hash", shelfHash);
    },
  },
];

for (const path of PATHS) {
  test(`${path.name} over unsaved changes: Cancel keeps the model and saves nothing`, async ({ page }) => {
    const w = await work(page);
    await path.go(page);
    await expect(unsavedDialog(page).locator(".question")).toHaveText(QUESTION);
    // Nothing happens until the answer.
    await expect(w.vp).toHaveAttribute("data-hash", w.hash);
    await answerUnsaved(page, "Cancel");
    await kept(page, w.hash);
    expect(w.downloads).toEqual([]);
  });

  test(`${path.name} over unsaved changes: Discard Changes goes ahead and saves nothing`, async ({ page }) => {
    const w = await work(page);
    await path.go(page);
    await answerUnsaved(page, "Discard Changes");
    await path.ahead(page);
    await expect(unsavedDialog(page)).toHaveCount(0);
    expect(w.downloads).toEqual([]);
  });

  test(`${path.name} over unsaved changes: Save saves the model, then goes ahead`, async ({ page }) => {
    const w = await work(page);
    await path.go(page);
    const [d] = await Promise.all([page.waitForEvent("download"), answerUnsaved(page, "Save")]);
    expect(d.suggestedFilename()).toBe("Untitled.shz");
    const saved = readFileSync((await d.path())!);
    await path.ahead(page);
    await expect(unsavedDialog(page)).toHaveCount(0);
    expect(w.downloads).toEqual(["Untitled.shz"]);
    // What was saved is the work as it was: the file opens as the same model (nothing is
    // unsaved now, so nothing is asked).
    await pick(page, "saved.shz", saved);
    await expect(page).toHaveTitle("saved - Sawhorse");
    await expect(w.vp).toHaveAttribute("data-hash", w.hash);
    await expect(unsavedDialog(page)).toHaveCount(0);
  });
}

test("a clean model is never asked: New, Close and Open go ahead at once", async ({ page }) => {
  const vp = await ready(page);
  for (const path of PATHS) {
    await path.go(page);
    await path.ahead(page);
    await expect(unsavedDialog(page)).toHaveCount(0);
  }
  await expect(vp).toHaveAttribute("data-hash", shelfHash);
});

test("the question names the model by its file", async ({ page }) => {
  const vp = await ready(page);
  await pick(page, "shelf.shz", shelf);
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await box(page, "30,20", "15");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await expect(page).toHaveTitle("• shelf - Sawhorse");
  await menu(page, "File", "New");
  await expect(unsavedDialog(page).locator(".question")).toHaveText("Save the changes to shelf.shz?");
  await answerUnsaved(page, "Cancel");
  await expect(unsavedDialog(page)).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-objects", "2");
});

test("a file dropped on the page over unsaved changes asks: Cancel keeps the model, Discard Changes opens the file", async ({ page }) => {
  const w = await work(page);
  const drop = async () => {
    const dt = await page.evaluateHandle((b64) => {
      const d = new DataTransfer();
      d.items.add(new File([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], "shelf.shz", { type: "application/octet-stream" }));
      return d;
    }, shelf.toString("base64"));
    await page.dispatchEvent("#app", "drop", { dataTransfer: dt });
  };
  await drop();
  await expect(unsavedDialog(page).locator(".question")).toHaveText(QUESTION);
  await answerUnsaved(page, "Cancel");
  await kept(page, w.hash);
  await drop();
  await answerUnsaved(page, "Discard Changes");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(w.vp).toHaveAttribute("data-hash", shelfHash);
  expect(w.downloads).toEqual([]);
});

test("Save, and a save that fails: says why, as File > Save does, and nothing else happens", async ({ page }) => {
  const w = await work(page, "/?io=basic&autosave=0&kerneltest=1");
  await page.evaluate(() => (window as unknown as { __sawhorse: { kernelTest: { failNextSave(): void } } }).__sawhorse.kernelTest.failNextSave());
  await menu(page, "File", "New");
  await answerUnsaved(page, "Save");
  await expect(page.locator("#hint")).toContainText("Could not save: the saved copy would not have opened as the same model, so nothing was written");
  await kept(page, w.hash);
  expect(w.downloads).toEqual([]);
  // The next Save at the question works, and New goes ahead.
  await menu(page, "File", "New");
  const [d] = await Promise.all([page.waitForEvent("download"), answerUnsaved(page, "Save")]);
  expect(d.suggestedFilename()).toBe("Untitled.shz");
  await expect(page.locator("#hint")).toContainText("New model.");
  await expect(w.vp).toHaveAttribute("data-objects", "0");
});

// ---- the keys -----------------------------------------------------------------------------

test("Save has the focus: Enter saves", async ({ page }) => {
  const w = await work(page);
  await menu(page, "File", "New");
  await expect(unsavedDialog(page).getByRole("button", { name: "Save", exact: true })).toBeFocused();
  const [d] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Enter")]);
  expect(d.suggestedFilename()).toBe("Untitled.shz");
  await expect(page.locator("#hint")).toContainText("New model.");
  await expect(w.vp).toHaveAttribute("data-objects", "0");
});

test("Escape is Cancel", async ({ page }) => {
  const w = await work(page);
  await menu(page, "File", "New");
  await expect(unsavedDialog(page).getByRole("button", { name: "Save", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await kept(page, w.hash);
  expect(w.downloads).toEqual([]);
});

test("Discard Changes is never the default: it takes Tab and then Enter", async ({ page }) => {
  const w = await work(page);
  await menu(page, "File", "New");
  const d = unsavedDialog(page);
  await expect(d.getByRole("button", { name: "Save", exact: true })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(d.getByRole("button", { name: "Discard Changes", exact: true })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("New model.");
  await expect(w.vp).toHaveAttribute("data-objects", "0");
  expect(w.downloads).toEqual([]);
});

test("Tab goes Save, Discard Changes, Cancel and stays in the dialog; Shift+Tab goes back", async ({ page }) => {
  const w = await work(page);
  await menu(page, "File", "New");
  const d = unsavedDialog(page);
  const button = (name: string) => d.getByRole("button", { name, exact: true });
  await expect(button("Save")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(button("Discard Changes")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(button("Cancel")).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(button("Discard Changes")).toBeFocused();
  // Round and round: the focus never lands on anything in the page behind.
  for (let i = 0; i < 7; i++) {
    await page.keyboard.press("Tab");
    expect(
      await page.evaluate(() => {
        const a = document.activeElement;
        return a === null || a === document.body || !!a.closest("dialog.unsaved-dialog");
      }),
      `after ${i + 1} more Tab`,
    ).toBe(true);
  }
  await page.keyboard.press("Escape");
  await kept(page, w.hash);
});

// ---- the dialog as a modal ------------------------------------------------------------------

test("it is a real modal: named by its question, the page behind takes no click and no key", async ({ page }) => {
  const w = await work(page);
  await menu(page, "File", "New");
  const d = page.getByRole("dialog", { name: QUESTION });
  await expect(d).toBeVisible();
  await expect(d).toHaveAttribute("aria-modal", "true");
  await expect(d).toHaveJSProperty("open", true);
  expect(await d.evaluate((e) => e.matches(":modal"))).toBe(true);
  await expect(d.getByRole("button")).toHaveText(["Save", "Discard Changes", "Cancel"]);

  // A click on the File menu behind it lands on the dialog, and no menu opens.
  const file = page.getByRole("button", { name: "File", exact: true });
  const at = (await file.boundingBox())!;
  const x = at.x + at.width / 2;
  const y = at.y + at.height / 2;
  expect(await page.evaluate(([x, y]) => !!document.elementFromPoint(x, y)?.closest("dialog.unsaved-dialog"), [x, y])).toBe(true);
  await page.mouse.click(x, y);
  await expect(page.getByRole("menu")).toHaveCount(0);
  await expect(d).toBeVisible();

  // Undo pressed while it is up is not the app's. If it were, the Undo and Redo after the
  // dialog would leave the box undone: one step back twice, one forward.
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Delete");
  await page.keyboard.press("Escape");
  await kept(page, w.hash);
  await page.keyboard.press("Control+z");
  await expect(w.vp).toHaveAttribute("data-objects", "0");
  await page.keyboard.press("Control+y");
  await expect(w.vp).toHaveAttribute("data-objects", "1");
});

test("the naming dialog is a real modal too: aria-modal, the focus in its field and held inside, the page behind out of reach", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "2,10", "20");
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  const makeComponent = async () => {
    await page.getByRole("button", { name: "Edit", exact: true }).click();
    await page.getByRole("menuitem", { name: /Make Component/ }).click();
  };
  await makeComponent();
  const d = page.getByRole("dialog", { name: "Make Component" });
  await expect(d).toHaveAttribute("aria-modal", "true");
  expect(await d.evaluate((e) => e.matches(":modal"))).toBe(true);
  const name = d.getByLabel("Name");
  await expect(name).toBeFocused();
  // The suggested name is selected: typing replaces it.
  expect(await name.evaluate((e: HTMLInputElement) => e.selectionEnd! - e.selectionStart!)).toBe((await name.inputValue()).length);
  for (let i = 0; i < 6; i++) {
    await page.keyboard.press("Tab");
    expect(
      await page.evaluate(() => {
        const a = document.activeElement;
        return a === null || a === document.body || !!a.closest("dialog.name-dialog");
      }),
      `after ${i + 1} Tab`,
    ).toBe(true);
  }
  const file = page.getByRole("button", { name: "File", exact: true });
  const at = (await file.boundingBox())!;
  expect(await page.evaluate(([x, y]) => !!document.elementFromPoint(x, y)?.closest("dialog.name-dialog"), [at.x + at.width / 2, at.y + at.height / 2])).toBe(true);
  await page.keyboard.press("Escape");
  await expect(d).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-objects", "1");
  // And it still makes the component.
  await makeComponent();
  await expect(name).toBeFocused();
  await name.fill("Side");
  await name.press("Enter");
  await expect(page.locator("#hint")).toContainText("Made component 'Side'");
  await expect(d).toHaveCount(0);
});

// ---- the page as it runs in the desktop shell ----------------------------------------------

/** A desktop page with a box drawn: unsaved changes, and the shell told so. */
async function desktopWork(page: Page) {
  await desktopBridge(page);
  const vp = await ready(page, "/?autosave=0");
  await expect.poll(async () => (await bridge(page)).said).toContain("saved");
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  await expect.poll(async () => (await bridge(page)).unsaved).toBe(true);
  return vp;
}

async function savings(page: Page) {
  return (await bridge(page)).said.filter((w) => w === "saving").length;
}

test("desktop, File > New on an untitled model: Save asks for a name, saves, then starts the new model", async ({ page }) => {
  const vp = await desktopWork(page);
  await setBridge(page, { saveHold: true });
  await menu(page, "File", "New");
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).saves.map((s) => s.defaultPath)).toEqual(["Untitled.shz"]);
  await expect(unsavedDialog(page).getByRole("status")).toHaveText("Saving…");
  await expect(vp).toHaveAttribute("data-objects", "1");
  await answerSave(page, CABINET);
  await expect(page.locator("#hint")).toContainText("New model.");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  await expect(unsavedDialog(page)).toHaveCount(0);
  const b = await bridge(page);
  expect(b.written).toEqual([CABINET]);
  // The shell heard of the save, and nothing about closing.
  expect(b.said).toContain("saving");
  expect(b.said.filter((w) => w === "keep" || w === "close" || w === "failed")).toEqual([]);
  expect(b.ended).toBeNull();
  // The desktop's message dialog was not used.
  expect(b.asked).toEqual([]);
});

test("desktop: Cancel while the save runs gives up on it: nothing is written, nothing else happens, and the shell hears no more", async ({ page }) => {
  const vp = await desktopWork(page);
  await setBridge(page, { saveHold: true });
  await menu(page, "File", "New");
  await answerUnsaved(page, "Save");
  await expect.poll(async () => (await bridge(page)).saves).toHaveLength(1);
  const d = unsavedDialog(page);
  await expect(d.getByRole("button")).toHaveText(["Cancel"]);
  // No button has the focus while saving (the dialog itself has it): a second Enter does nothing.
  await expect(d).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(d.getByRole("status")).toHaveText("Saving…");
  await d.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(d).toHaveCount(0);
  const after = await savings(page);
  // The name chosen after all: too late, nothing is written and New does not happen.
  await answerSave(page, CABINET);
  await page.waitForTimeout(600);
  expect(await savings(page)).toBe(after);
  expect((await bridge(page)).written).toEqual([]);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  await expect(page.locator("#hint")).not.toContainText("New model.");
});

test("desktop, a file dropped on the window over unsaved changes: Save writes the model to its file, then the dropped file opens", async ({ page }) => {
  // Two files on the stand-in's disk, saved by the app: shelf.shz (one box), then cabinet.shz
  // (another), which stays open and gets a second box.
  await desktopBridge(page);
  const vp = await ready(page, "/?autosave=0");
  await box(page, "24,16", "12");
  await setBridge(page, { saveAs: SHELF });
  await page.keyboard.press("Control+s");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  const shelfModel = (await vp.getAttribute("data-hash"))!;
  await menu(page, "File", "New");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await box(page);
  await setBridge(page, { saveAs: CABINET });
  await page.keyboard.press("Control+s");
  await expect(page).toHaveTitle("cabinet - Sawhorse");
  await box(page, "24,16", "12");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await expect(page).toHaveTitle("• cabinet - Sawhorse");
  const before = (await bridge(page)).written.length;

  // Cancel first: the model stays, nothing is read into it.
  await dropFiles(page, [SHELF]);
  await expect(unsavedDialog(page).locator(".question")).toHaveText("Save the changes to cabinet.shz?");
  await answerUnsaved(page, "Cancel");
  await expect(unsavedDialog(page)).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-objects", "2");
  await expect(page).toHaveTitle("• cabinet - Sawhorse");

  await dropFiles(page, [SHELF]);
  await answerUnsaved(page, "Save");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", shelfModel);
  const b = await bridge(page);
  // Written to its own file, with no dialog to choose a name (two were shown, at the start).
  expect(b.written.slice(before)).toEqual([CABINET]);
  expect(b.saves).toHaveLength(2);
});

test("desktop, one question at a time: a close request while File > Close's question is up takes its place", async ({ page }) => {
  const vp = await desktopWork(page);
  await menu(page, "File", "Close");
  const q = unsavedDialog(page).locator(".question");
  await expect(q).toHaveText(QUESTION);
  await closeWindow(page);
  await expect(q).toHaveText("Save the changes to this model before Sawhorse closes?");
  await expect(unsavedDialog(page)).toHaveCount(1);
  // Later than a person's fastest answer (src/io/closing.ts, ANSWER_FLOOR_MS).
  await page.waitForTimeout(250);
  await answerUnsaved(page, "Cancel");
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  await expect(unsavedDialog(page)).toHaveCount(0);
  // File > Close's question ended as Cancel: the model was not closed.
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  await expect(page.locator("#hint")).not.toContainText("Closed");
  expect((await bridge(page)).ended).toBeNull();
});

test("desktop: the focus goes back where it was when the question goes", async ({ page }) => {
  await desktopWork(page);
  const file = page.getByRole("button", { name: "File", exact: true });
  await file.focus();
  await expect(file).toBeFocused();
  await closeWindow(page);
  await expect(unsavedDialog(page).getByRole("button", { name: "Save", exact: true })).toBeFocused();
  await page.waitForTimeout(250);
  await page.keyboard.press("Escape");
  await expect(unsavedDialog(page)).toHaveCount(0);
  await expect(file).toBeFocused();
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
});
