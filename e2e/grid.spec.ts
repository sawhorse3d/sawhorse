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
