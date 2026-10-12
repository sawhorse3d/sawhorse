import { expect, test, type Page } from "@playwright/test";

// Lane-3 verifier conditions (lane 4, step 1): the dev autosave survives a reload and is
// offered back; the status line names solids by display number, never by raw id; the
// rectangle preview and its commit message say which way the rectangle went.
const requireKernel = !!process.env.REQUIRE_KERNEL;
type V3 = [number, number, number];
const IN = 0.0254;

async function ready(page: Page, url = "/") {
  await page.goto(url);
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

async function at(page: Page, p: V3): Promise<[number, number]> {
  const box = await page.locator("#cv").boundingBox();
  const xy = await page.evaluate((p) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(p), p);
  return [box!.x + xy[0], box!.y + xy[1]];
}

async function box(page: Page, size = "24,16", height = "12") {
  await page.keyboard.press("r");
  await page.keyboard.type(size);
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type(height);
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Extruded");
}

/** The autosave record in IndexedDB (null until the first save). */
async function autosaved(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      new Promise<number>((resolve) => {
        const req = indexedDB.open("sawhorse", 1);
        req.onupgradeneeded = () => req.result.createObjectStore("autosave");
        req.onsuccess = () => {
          const db = req.result;
          const get = db.transaction("autosave", "readonly").objectStore("autosave").get("current");
          get.onsuccess = () => {
            db.close();
            resolve((get.result?.log?.length as number | undefined) ?? 0);
          };
          get.onerror = () => resolve(0);
        };
        req.onerror = () => resolve(0);
      }),
  );
}

test("unsaved work comes back after a reload", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect.poll(() => autosaved(page)).toBe(3);
  await page.reload();
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  await expect(vp).toHaveAttribute("data-objects", "0");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await hint.getByRole("button", { name: "Restore" }).click();
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  await expect(hint).toContainText("Restored your unsaved work: 1 solid, 3 steps");
  // The history came back too: undo removes the solid, redo brings it back.
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await page.keyboard.press("Control+y");
  await expect(vp).toHaveAttribute("data-objects", "1");
});

test("discarding forgets the unsaved work; ?autosave=0 never offers it", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await expect.poll(() => autosaved(page)).toBe(3);
  await page.reload();
  const hint = page.locator("#hint");
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded");
  await expect.poll(() => autosaved(page)).toBe(0);
  await ready(page, "/?autosave=0");
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  await page.waitForTimeout(600);
  expect(await autosaved(page)).toBe(0);
});

test("solids are named by display number, never by raw id", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  // Delete it: the next solid reuses the slot, so its public id is 2^32 + 1.
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Delete");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await box(page, "12,12", "6");
  const ids = await page.evaluate(() => (window as unknown as { __sawhorse: { solids(): { id: number; number: number }[] } }).__sawhorse.solids());
  expect(ids).toEqual([{ id: 2 ** 32 + 1, number: 2, min: expect.anything(), max: expect.anything(), volume: expect.anything() }]);
  await page.keyboard.press("Space");
  // Wait for the view to settle on the new solid (it refits after an edit), then click.
  await expect(async () => {
    const t = await at(page, [6 * IN, 6 * IN, 6 * IN]);
    await page.mouse.move(t[0] + 3, t[1] + 2);
    await page.mouse.move(t[0], t[1], { steps: 2 });
    await expect(vp).toHaveAttribute("data-hover", "face", { timeout: 800 });
  }).toPass({ timeout: 10_000 });
  const top = await at(page, [6 * IN, 6 * IN, 6 * IN]);
  await page.mouse.click(top[0], top[1]);
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Selected face 2 of solid 2");
  await page.mouse.dblclick(top[0], top[1]);
  await expect(hint).toContainText("Selected solid 2");
  const text = (await hint.textContent()) ?? "";
  expect(text).not.toMatch(/#\d|\d{10,}/);
});

test("the rectangle preview and message say which way it went", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("r");
  const a = await at(page, [0, 0, 0]);
  const b = await at(page, [-10 * IN, 6 * IN, 0]);
  await page.mouse.click(a[0], a[1]);
  await page.mouse.move(b[0], b[1], { steps: 4 });
  await expect(page.locator(".vlabel.dim", { hasText: "−red" })).toBeVisible();
  await expect(page.locator(".vlabel.dim", { hasText: "+green" })).toBeVisible();
  await page.keyboard.type("10,6");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("10 in along −red and 6 in along +green from (0, 0, 0)");
});
