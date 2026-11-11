import { expect, test, type Page } from "@playwright/test";
import { box } from "./helpers";
import { bridge, desktopBridge, setBridge } from "./tauri";

// Lane 39: in the desktop app the window's title bar follows the page's title, the model's
// name with the unsaved mark, so two windows can be told apart. The page sends the text it
// gives document.title to the shell's window_title command (src/io/title.ts); the shell's
// cleaning of it is tested in src-tauri/window-title. The desktop shell cannot run here: the
// page runs as it does inside it, with the bridge stood in for (e2e/tauri.ts), which records
// every title the page sent.
const requireKernel = !!process.env.REQUIRE_KERNEL;
const SHELF = "C:\\Users\\maker\\Documents\\Sawhorse\\shelf.shz";
const DISCARD_QUESTION = "Discard the unsaved changes to this model?";

async function ready(page: Page, url = "/") {
  await page.goto(url);
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

async function fileMenu(page: Page) {
  await page.getByRole("button", { name: "File", exact: true }).click();
  return page.getByRole("menu", { name: "File" }).locator(":scope > button, :scope > .subwrap > button");
}

/** The page's title, and the last title it sent the shell, once they agree on `want`. */
async function titled(page: Page, want: string) {
  await expect(page).toHaveTitle(want);
  await expect.poll(async () => (await bridge(page)).titles.at(-1)).toBe(want);
}

/** The titles sent, with a title sent twice in a row counted once. */
async function sent(page: Page) {
  return (await bridge(page)).titles.filter((t, i, all) => i === 0 || all[i - 1] !== t);
}

// shelf.shz: one 12 x 12 x 6 in box, saved by the browser build.
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

test("the title bar follows the model: open, an edit's unsaved mark, File > Close", async ({ page }) => {
  await desktopBridge(page, "Win32", { start: SHELF, files: { [SHELF]: shelf } });
  const vp = await ready(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await titled(page, "shelf - Sawhorse");
  await box(page, "24,16", "12");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await titled(page, "• shelf - Sawhorse");
  await setBridge(page, { answer: "Ok" });
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await expect.poll(async () => (await bridge(page)).asked).toEqual([DISCARD_QUESTION]);
  await expect(vp).toHaveAttribute("data-objects", "0");
  await titled(page, "Untitled - Sawhorse");
  // Only titles the page itself had, in the order it had them.
  expect(await sent(page)).toEqual(["Untitled - Sawhorse", "shelf - Sawhorse", "• shelf - Sawhorse", "Untitled - Sawhorse"]);
});

test("an untitled model: the mark comes with the first edit and goes with File > Close", async ({ page }) => {
  await desktopBridge(page);
  const vp = await ready(page);
  await titled(page, "Untitled - Sawhorse");
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await titled(page, "• Untitled - Sawhorse");
  // No keeps the model and the mark; OK closes it and the mark goes.
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await expect.poll(async () => (await bridge(page)).asked).toEqual([DISCARD_QUESTION]);
  await page.waitForTimeout(800);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await titled(page, "• Untitled - Sawhorse");
  await setBridge(page, { answer: "Ok" });
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await expect(vp).toHaveAttribute("data-objects", "0");
  await titled(page, "Untitled - Sawhorse");
  for (const t of (await bridge(page)).titles) expect(["Untitled - Sawhorse", "• Untitled - Sawhorse"]).toContain(t);
});

test("a title the shell cannot set is never mentioned, and the page's own title still follows", async ({ page }) => {
  await desktopBridge(page);
  const vp = await ready(page);
  await titled(page, "Untitled - Sawhorse");
  await setBridge(page, { titleFails: true });
  const hint = page.locator("#hint");
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  await expect.poll(async () => (await bridge(page)).log.filter((c) => c === "window_title").length).toBeGreaterThan(1);
  await page.waitForTimeout(300);
  await expect(hint).toContainText("Extruded");
  await expect(hint).not.toContainText("window_title");
  await expect(hint).not.toContainText("Can't do that");
  expect((await bridge(page)).titles).toEqual(["Untitled - Sawhorse"]);
});

test("the browser build never calls the shell for its title", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const vp = await ready(page);
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  expect(await page.evaluate(() => "__TAURI_INTERNALS__" in window)).toBe(false);
  expect(errors).toEqual([]);
});
