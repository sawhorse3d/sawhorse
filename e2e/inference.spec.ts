import { expect, test, type Page } from "@playwright/test";

// The inference engine in the running app (lane 3): our coloured marker and tooltip for
// endpoint / midpoint / on-edge / on-face / grid / axis, arrow-key locks, and clicks that
// land on the inferred point, so typed-free values come out exact (no "~").
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

async function hover(page: Page, p: V3, dx = 0, dy = 0) {
  const [x, y] = await at(page, p, dx, dy);
  await page.mouse.move(x, y, { steps: 2 });
}

test("our inference colours and tooltips on a box", async ({ page }) => {
  const vp = await ready(page);
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type("12");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  await page.keyboard.press("r");
  // Endpoint (3 px off the top corner), midpoint, on edge, on face.
  await hover(page, [24 * IN, 0, 12 * IN], 3, -2);
  await expect(vp).toHaveAttribute("data-inference", "endpoint");
  await expect(page.locator("#tip")).toHaveText("Endpoint");
  const lime = await page.evaluate(() => getComputedStyle(document.querySelector(".vp .snap")!).backgroundColor);
  expect(lime).toBe("rgb(123, 192, 67)"); // #7bc043, ours
  await hover(page, [12 * IN, 0, 12 * IN], 1, 1);
  await expect(vp).toHaveAttribute("data-inference", "midpoint");
  await expect(page.locator("#tip")).toHaveText("Midpoint");
  await hover(page, [5 * IN, 0, 12 * IN], 0, 2);
  await expect(vp).toHaveAttribute("data-inference", "on-edge");
  await hover(page, [8 * IN, 8 * IN, 12 * IN]);
  await expect(vp).toHaveAttribute("data-inference", "on-face");
  await expect(page.locator("#tip")).toHaveText("On Face");
  // The ground grid, away from the box.
  await page.getByRole("button", { name: "Top", exact: true }).click();
  await hover(page, [-10 * IN, -6 * IN, 0], 1, 1);
  await expect(vp).toHaveAttribute("data-inference", "grid");
});

test("axis inference, arrow-key locks, and exact values from inferred clicks", async ({ page }) => {
  const vp = await ready(page);
  await page.getByRole("button", { name: "Top", exact: true }).click();
  await page.keyboard.press("r");
  // First corner on the grid at (2, 3) in, second on the grid at (10, 7) in.
  const [ax, ay] = await at(page, [2 * IN, 3 * IN, 0], 1, -1);
  await page.mouse.move(ax, ay);
  await expect(vp).toHaveAttribute("data-inference", "grid");
  await page.mouse.click(ax, ay);
  await expect(page.locator("#hint")).toContainText("First corner set on the ground");
  // Moving straight along red from the corner shows the red axis.
  await hover(page, [9.5 * IN, 3 * IN, 0], 0, 3);
  await expect(vp).toHaveAttribute("data-inference", "axis-x");
  await expect(page.locator("#tip")).toHaveText("On Red Axis");
  // Lock green with the left arrow: the inference stays green wherever the mouse goes.
  await page.keyboard.press("ArrowLeft");
  await hover(page, [7 * IN, 9 * IN, 0]);
  await expect(vp).toHaveAttribute("data-inference", /axis-y|from-point/);
  await page.keyboard.press("ArrowLeft"); // unlock
  const [bx, by] = await at(page, [10 * IN, 7 * IN, 0], -1, 1);
  await page.mouse.move(bx, by);
  await expect(vp).toHaveAttribute("data-inference", "grid");
  // The live measurement is exact while dragging.
  await expect(page.locator("#vcb")).toHaveValue("8, 4");
  await page.mouse.click(bx, by);
  await expect(page.locator("#hint")).toContainText("Rectangle 8 × 4 in on the ground");
  await expect(page.locator("#hint")).not.toContainText("~");
  await expect(vp).toHaveAttribute("data-triangles", "2");
});
