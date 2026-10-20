import { expect, test, type Page } from "@playwright/test";

// Lane 21 (issue #4): View > Grid and the Styles pane's Ground grid checkbox show and hide
// the ground grid. The switch is part of the document's style, so it saves with the .shz
// and comes back when the file is opened; a file without it shows the grid.
const requireKernel = !!process.env.REQUIRE_KERNEL;

type Grid = { minor: boolean; major: boolean; enabled: boolean };
const grid = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { grid(): Grid } }).__sawhorse.grid());

async function ready(page: Page) {
  await page.goto("/?io=basic&autosave=0");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

const gridItem = (page: Page) => page.getByRole("menuitemcheckbox", { name: /^\W*Grid$/ });

async function viewGrid(page: Page) {
  await page.getByRole("button", { name: "View", exact: true }).click();
  await gridItem(page).click();
}

async function download(page: Page, action: () => Promise<void>) {
  const [d] = await Promise.all([page.waitForEvent("download"), action()]);
  const fs = await import("node:fs");
  return fs.readFileSync((await d.path())!);
}

test("View > Grid hides the grid; the Styles pane agrees; saved and reopened it stays hidden; on again restores it", async ({ page }) => {
  const vp = await ready(page);
  // On by default, with the menu item checked.
  expect(await grid(page)).toMatchObject({ major: true, enabled: true });
  await page.getByRole("button", { name: "View", exact: true }).click();
  await expect(gridItem(page)).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Escape");

  // Off from the View menu: both levels hidden at every zoom.
  await viewGrid(page);
  await expect.poll(async () => (await grid(page)).enabled).toBe(false);
  expect(await grid(page)).toMatchObject({ minor: false, major: false });
  await expect(page.locator(".hud")).toContainText("undo 1");
  const cv = (await page.locator("#cv").boundingBox())!;
  await page.mouse.move(cv.x + cv.width / 2, cv.y + cv.height / 2);
  for (let i = 0; i < 4; i++) await page.mouse.wheel(0, -400);
  expect(await grid(page)).toMatchObject({ minor: false, major: false });
  await page.keyboard.press("Shift+Z");
  // The Styles pane's checkbox says the same.
  await page.getByRole("tab", { name: "Styles" }).click();
  const box = page.getByLabel("Ground grid");
  await expect(box).not.toBeChecked();

  // Save, start a new model (grid on), open the file: the grid is hidden again.
  const saved = await download(page, () => page.keyboard.press("Control+s"));
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: "New", exact: true }).click();
  await expect.poll(async () => (await grid(page)).enabled).toBe(true);
  await expect(box).toBeChecked();
  await page.getByTestId("file-input").setInputFiles({ name: "nogrid.shz", mimeType: "application/vnd.sawhorse.shz", buffer: saved });
  await expect(page).toHaveTitle(/nogrid/);
  await expect.poll(async () => (await grid(page)).enabled).toBe(false);
  expect(await grid(page)).toMatchObject({ minor: false, major: false });
  await expect(box).not.toBeChecked();
  await page.getByRole("button", { name: "View", exact: true }).click();
  await expect(gridItem(page)).toHaveAttribute("aria-checked", "false");
  await page.keyboard.press("Escape");

  // On again from the Styles pane: the grid is back (the major lines at this zoom).
  await box.click();
  await expect(box).toBeChecked();
  await expect.poll(async () => (await grid(page)).enabled).toBe(true);
  expect((await grid(page)).major).toBe(true);
  await page.getByRole("button", { name: "View", exact: true }).click();
  await expect(gridItem(page)).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Escape");
  // ... and undo hides it again (one undo step per switch).
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await grid(page)).enabled).toBe(false);
  await expect(vp).toHaveAttribute("data-kernel", "ready");
});

// What docs/releases/v0.1.0-alpha.3.md and v0.1.0-alpha.4.md (Known limits) and CHANGELOG lane 21 say about scenes
// and the grid, checked as written. A scene keeps the style it was saved with only when that
// style is not the default; the default is stored as no style, and going to such a scene
// leaves the style (grid included) as it is. Updating that scene while the grid shows stores
// the default again, so it does not bring the grid back either (the alpha.3 notes first said
// it did). View > Grid does, and so does a scene saved with any other style.
test("scenes and the grid: a default-style scene leaves it alone; View > Grid and a styled scene bring it back", async ({ page }) => {
  await ready(page);
  const hint = page.locator("#hint");
  // The hint names the scene once the kernel has gone to it; two frames later the looks it
  // set (if any) are on the engine, so a grid that stays hidden is really staying hidden.
  const go = async (name: string) => {
    await page.getByRole("tab", { name, exact: true }).click();
    await expect(hint).toContainText(name);
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  };
  const hide = async () => {
    await viewGrid(page);
    await expect(hint).toContainText("Grid hidden");
  };
  const show = async () => {
    await viewGrid(page);
    await expect(hint).toContainText("Grid shown");
  };
  const addScene = async (name: string) => {
    await page.getByRole("button", { name: /Add a scene/ }).click();
    await expect(page.getByRole("tab", { name, exact: true })).toBeVisible();
  };
  const enabled = async () => (await grid(page)).enabled;

  // Saved with the default style (grid shown): going to it leaves a hidden grid hidden.
  await addScene("Scene 1");
  await hide();
  await go("Scene 1");
  expect(await enabled()).toBe(false);

  // Updated while the grid shows: still the default style, so still no grid on return.
  await show();
  await page.getByRole("tab", { name: "Scene 1", exact: true }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Update to this view" }).click();
  await expect(hint).toContainText("updated to this view");
  await hide();
  await go("Scene 1");
  expect(await enabled()).toBe(false);

  // View > Grid shows it again.
  await show();
  expect(await enabled()).toBe(true);

  // Saved with another style (X-ray faces, grid shown): going to it brings the grid back.
  await page.getByRole("tab", { name: "Styles" }).click();
  await page.getByRole("radio", { name: "X-ray" }).click();
  await addScene("Scene 2");
  await hide();
  await go("Scene 2");
  await expect.poll(enabled).toBe(true);

  // Saved with the grid hidden: going to it hides the grid.
  await hide();
  await addScene("Scene 3");
  await show();
  await go("Scene 3");
  await expect.poll(enabled).toBe(false);
  await go("Scene 2");
  await expect.poll(enabled).toBe(true);
});
