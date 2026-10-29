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

type Looks = { face: string };
const looks = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { looks(): Looks } }).__sawhorse.looks());

/** The scene tabs, View > Grid and the hint, as the steps of issue #6 use them. */
function sceneSteps(page: Page) {
  const hint = page.locator("#hint");
  const enabled = async () => (await grid(page)).enabled;
  return {
    hint,
    enabled,
    // The hint names the scene once the kernel has gone to it; two frames later the looks it
    // set (if any) are on the engine, so a grid that stays as it is really stays.
    go: async (name: string) => {
      await page.getByRole("tab", { name, exact: true }).click();
      await expect(hint).toContainText(name);
      await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    },
    hide: async () => {
      await viewGrid(page);
      await expect(hint).toContainText("Grid hidden");
    },
    show: async () => {
      await viewGrid(page);
      await expect(hint).toContainText("Grid shown");
    },
    addScene: async (name: string) => {
      await page.getByRole("button", { name: /Add a scene/ }).click();
      await expect(page.getByRole("tab", { name, exact: true })).toBeVisible();
    },
  };
}

// Lane 26 (issue #6, a known limit of alpha.3 and alpha.4): a scene stores the grid switch
// itself, so a scene saved with the default style (stored as no style) brings the grid back.
// The issue's steps, then the same after saving the .shz and opening it again; and a scene
// saved with another style and the grid hidden brings both back.
test("issue #6: a scene saved with the default style brings the grid back, also after save and reopen", async ({ page }) => {
  const vp = await ready(page);
  const { hint, enabled, go, hide, show, addScene } = sceneSteps(page);

  // Grid off (View > Grid), save Scene 1; grid on, save Scene 2 (the default style).
  await hide();
  await addScene("Scene 1");
  await show();
  await addScene("Scene 2");
  // Go to Scene 1, then Scene 2: the grid is hidden, then shown, and the hint says so.
  await go("Scene 1");
  await expect.poll(enabled).toBe(false);
  await expect(hint).toContainText("Scene 'Scene 1': grid hidden.");
  await go("Scene 2");
  await expect.poll(enabled).toBe(true);
  await expect(hint).toContainText("Scene 'Scene 2': grid shown.");
  // View > Grid follows the style the scene set.
  await page.getByRole("button", { name: "View", exact: true }).click();
  await expect(gridItem(page)).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Escape");

  // X-ray faces and the grid hidden, saved as Scene 3; back to the default look, grid shown.
  await page.getByRole("tab", { name: "Styles" }).click();
  await page.getByRole("radio", { name: "X-ray" }).click();
  await expect.poll(async () => (await looks(page)).face).toBe("x_ray");
  await hide();
  await addScene("Scene 3");
  await page.getByRole("button", { name: /^Sawhorse/ }).click();
  await expect.poll(async () => (await looks(page)).face).toBe("textured");
  await show();
  await go("Scene 3");
  await expect.poll(enabled).toBe(false);
  await expect.poll(async () => (await looks(page)).face).toBe("x_ray");
  await expect(page.getByLabel("Ground grid")).not.toBeChecked();
  // Scene 2 sets only the grid: saved with the default style, it leaves the look as it is.
  await go("Scene 2");
  await expect.poll(enabled).toBe(true);
  await expect.poll(async () => (await looks(page)).face).toBe("x_ray");
  await expect(page.getByLabel("Ground grid")).toBeChecked();

  // Save as .shz, start a new model, open the file: the same scenes do the same.
  const saved = await download(page, () => page.keyboard.press("Control+s"));
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: "New", exact: true }).click();
  await expect(page.getByRole("tab", { name: "Scene 1", exact: true })).toHaveCount(0);
  await page.getByTestId("file-input").setInputFiles({ name: "scenegrid.shz", mimeType: "application/vnd.sawhorse.shz", buffer: saved });
  await expect(page).toHaveTitle(/scenegrid/);
  await expect(page.getByRole("tab", { name: "Scene 3", exact: true })).toBeVisible();
  // Scene 1's style is the default look with the grid hidden; Scene 2 keeps the look it
  // finds and shows the grid.
  for (const [name, on, face] of [
    ["Scene 1", false, "textured"],
    ["Scene 2", true, "textured"],
    ["Scene 1", false, "textured"],
    ["Scene 3", false, "x_ray"],
    ["Scene 2", true, "x_ray"],
  ] as const) {
    await go(name);
    await expect.poll(enabled, { message: name }).toBe(on);
    await expect.poll(async () => (await looks(page)).face, { message: name }).toBe(face);
  }
  // Going to a scene is one undo step, the grid included: back to Scene 3's hidden grid.
  await page.keyboard.press("Control+z");
  await expect.poll(enabled).toBe(false);
  await expect.poll(async () => (await looks(page)).face).toBe("x_ray");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
});

// Updating a scene saves the grid that shows then, under any style; View > Grid still hides
// and shows it between scenes.
test("scenes and the grid: updating a scene rewrites its grid; View > Grid works between scenes", async ({ page }) => {
  await ready(page);
  const { hint, enabled, go, hide, show, addScene } = sceneSteps(page);

  // Saved with the default style and the grid shown: going to it shows a hidden grid again.
  await addScene("Scene 1");
  await hide();
  await go("Scene 1");
  await expect.poll(enabled).toBe(true);

  // Updated while the grid is hidden: going to it hides the grid.
  await hide();
  await page.getByRole("tab", { name: "Scene 1", exact: true }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Update to this view" }).click();
  await expect(hint).toContainText("updated to this view");
  await show();
  await go("Scene 1");
  await expect.poll(enabled).toBe(false);

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
