import { expect, test, type Page } from "@playwright/test";
import { box } from "./helpers";
import { bridge, closeWindow, desktopBridge, setBridge } from "./tauri";

// Lane 35 (#44): closing the desktop window over unsaved changes asks first, File > Close
// closes the model and keeps Sawhorse running, and File > Exit asks as the window's X does.
// The desktop shell cannot run here: the page runs as it does inside it, with the bridge
// stood in for (e2e/tauri.ts). The shell's own rule, that a close request two seconds after
// an unanswered question closes the window whatever the page does, is tested in
// src-tauri/close-guard (cargo test -p sawhorse-close-guard).
const requireKernel = !!process.env.REQUIRE_KERNEL;
const CLOSE_QUESTION = "Discard the unsaved changes to this model and close Sawhorse?";
const DISCARD_QUESTION = "Discard the unsaved changes to this model?";

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

test("over unsaved changes, No keeps the window, the model and the unsaved mark", async ({ page }) => {
  const vp = await dirty(page);
  await closeWindow(page);
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  const b = await bridge(page);
  expect(b.asked).toEqual([CLOSE_QUESTION]);
  expect(b.ended).toBeNull();
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  // Asked again on the next close.
  await closeWindow(page);
  await expect.poll(async () => (await bridge(page)).asked).toEqual([CLOSE_QUESTION, CLOSE_QUESTION]);
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
});

test("over unsaved changes, Yes deletes the autosave as File > New does, then closes", async ({ page }) => {
  await dirty(page, "/");
  await expect.poll(() => autosaved(page)).not.toBeNull();
  await setBridge(page, { answer: "Ok" });
  await closeWindow(page);
  await expect.poll(async () => (await bridge(page)).ended).toBe("destroyed");
  expect((await bridge(page)).asked).toEqual([CLOSE_QUESTION]);
  expect(await autosaved(page)).toBeNull();
});

test("a second request while the question is up asks nothing more", async ({ page }) => {
  await dirty(page);
  await setBridge(page, { delay: 1500 });
  await closeWindow(page);
  await expect.poll(async () => (await bridge(page)).asked).toEqual([CLOSE_QUESTION]);
  // The shell sends no second request within two seconds; even if one came, one question.
  await closeWindow(page);
  await closeWindow(page);
  await expect.poll(async () => (await bridge(page)).said.at(-1), { timeout: 5000 }).toBe("keep");
  const b = await bridge(page);
  expect(b.asked).toEqual([CLOSE_QUESTION]);
  expect(b.said.filter((w) => w === "keep")).toHaveLength(1);
});

for (const [mode, what] of [
  ["throw", "the question failing to open"],
  ["throw-sync", "the handler throwing"],
] as const) {
  test(`${what} says so, keeps the model, and tells the shell to close on the next request`, async ({ page }) => {
    const vp = await dirty(page);
    await setBridge(page, { dialog: mode });
    await closeWindow(page);
    await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("failed");
    await expect(page.locator("#hint")).toContainText("Sawhorse could not ask about the unsaved changes");
    await expect(page.locator("#hint")).toContainText("Close the window again to close it without saving");
    expect((await bridge(page)).ended).toBeNull();
    await expect(vp).toHaveAttribute("data-objects", "1");
  });
}

test("a No that comes back at once (the dialog never showed) counts as the question failing", async ({ page }) => {
  await dirty(page);
  await setBridge(page, { delay: 0 });
  await closeWindow(page);
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("failed");
  expect((await bridge(page)).said).not.toContain("keep");
});

test("an answer that cannot reach the shell still tells it the question failed", async ({ page }) => {
  await dirty(page);
  await setBridge(page, { failWords: ["keep"] });
  await closeWindow(page);
  await expect.poll(async () => (await bridge(page)).said.slice(-2)).toEqual(["keep", "failed"]);
});

test("File > Close on a clean model starts an empty untitled one at once, Sawhorse still running", async ({ page }) => {
  await desktopBridge(page);
  const vp = await ready(page);
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await expect(page.locator("#hint")).toContainText("Closed Untitled.");
  await expect(vp).toHaveAttribute("data-objects", "0");
  const b = await bridge(page);
  expect(b.asked).toEqual([]);
  expect(b.ended).toBeNull();
  expect(b.said).not.toContain("exit");
});

test("File > Close over unsaved changes asks File > New's question: No keeps the model, Yes closes it", async ({ page }) => {
  const vp = await dirty(page);
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await expect.poll(async () => (await bridge(page)).asked).toEqual([DISCARD_QUESTION]);
  await page.waitForTimeout(800);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  await setBridge(page, { answer: "Ok" });
  await (await fileMenu(page)).getByText("Close", { exact: true }).click();
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  await expect.poll(async () => (await bridge(page)).unsaved).toBe(false);
  const b = await bridge(page);
  expect(b.asked).toEqual([DISCARD_QUESTION, DISCARD_QUESTION]);
  expect(b.ended).toBeNull();
});

test("File > Exit goes through the window's close: at once when clean, the same question over unsaved changes", async ({ page }) => {
  await desktopBridge(page);
  await ready(page);
  await expect.poll(async () => (await bridge(page)).said).toContain("saved");
  await (await fileMenu(page)).getByText("Exit", { exact: true }).click();
  await expect.poll(async () => (await bridge(page)).ended).toBe("closed");
  expect((await bridge(page)).asked).toEqual([]);
});

test("File > Exit over unsaved changes asks as the X does, and No keeps everything", async ({ page }) => {
  const vp = await dirty(page);
  await (await fileMenu(page)).getByText("Exit", { exact: true }).click();
  await expect.poll(async () => (await bridge(page)).said.at(-1)).toBe("keep");
  const b = await bridge(page);
  expect(b.said).toContain("exit");
  expect(b.asked).toEqual([CLOSE_QUESTION]);
  expect(b.ended).toBeNull();
  await expect(vp).toHaveAttribute("data-objects", "1");
});
