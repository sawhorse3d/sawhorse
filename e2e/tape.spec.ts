import { expect, test, type Page } from "@playwright/test";

// Tape measure, guides and dimensions (lane 3).
const requireKernel = !!process.env.REQUIRE_KERNEL;
type V3 = [number, number, number];
const IN = 0.0254;

async function ready(page: Page) {
  await page.goto("/");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

async function at(page: Page, p: V3, dx = 0, dy = 0): Promise<[number, number]> {
  const box = await page.locator("#cv").boundingBox();
  const xy = await page.evaluate((p) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(p), p);
  return [box!.x + xy[0] + dx, box!.y + xy[1] + dy];
}

async function click(page: Page, p: V3, dx = 0, dy = 0) {
  const [x, y] = await at(page, p, dx, dy);
  await page.mouse.move(x, y, { steps: 2 });
  await page.mouse.click(x, y);
}

test("measure, place a guide from an edge, snap to its crossing, dimension, erase the guide", async ({ page }) => {
  const vp = await ready(page);
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type("12");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-watertight", "true");

  // Measure the top's right edge: 16 in.
  await page.keyboard.press("t");
  await click(page, [24 * IN, 0, 12 * IN]);
  await click(page, [24 * IN, 16 * IN, 12 * IN]);
  await expect(page.locator("#hint")).toContainText("Distance 16 in");

  // A guide 4 in down from the top front edge, across the front face.
  await click(page, [6 * IN, 0, 12 * IN], 0, 1);
  await expect(page.locator("#hint")).toContainText("Guide from this edge");
  const [mx, my] = await at(page, [6 * IN, 0, 7 * IN]);
  await page.mouse.move(mx, my, { steps: 3 });
  await page.keyboard.type("4");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Guide 4 in from the edge");

  // The guide crosses the front-left vertical edge at (0, 0, 8): an intersection snap.
  await page.keyboard.press("r");
  const [ix, iy] = await at(page, [0, 0, 8 * IN], 2, 1);
  await page.mouse.move(ix, iy, { steps: 2 });
  await expect(vp).toHaveAttribute("data-inference", "intersection");
  await page.keyboard.press("Escape");

  // A dimension on the top front edge, placed above it.
  await page.keyboard.press("d");
  await click(page, [0, 0, 12 * IN]);
  await click(page, [24 * IN, 0, 12 * IN]);
  await click(page, [12 * IN, 0, 16 * IN]);
  await expect(page.locator(".vp .vlabel.dim")).toHaveText("24");
  await expect(page.locator("#hint")).toContainText("Dimension 24 in");

  // Select the guide and delete it; Edit > Delete Guides is then disabled.
  await page.keyboard.press("Space");
  await click(page, [16 * IN, 0, 8 * IN]);
  await expect(page.locator("#hint")).toContainText("Selected a guide");
  await page.keyboard.press("Delete");
  await expect(page.locator("#hint")).toContainText("Erased 1 item");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await expect(page.getByRole("menuitem", { name: /Delete Guides/ })).toBeDisabled();
});
