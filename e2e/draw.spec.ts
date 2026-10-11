import { expect, test, type Page } from "@playwright/test";

// Draw tools (lane 3): Line chains that close into regions, typed lengths, Rect across a
// face (a dado strip), Rotated Rect, Circle / Polygon with typed segments and sides, Arc
// with a typed radius, and a line cut across a face from edge to edge.
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

/** Click a world point (the inference snaps it exactly: grid, endpoint, edge...). */
async function click(page: Page, p: V3, dx = 0.6, dy = -0.4) {
  const [x, y] = await at(page, p, dx, dy);
  await page.mouse.move(x, y, { steps: 2 });
  await page.mouse.click(x, y);
}

test("Line chains close into a region; typed lengths are exact", async ({ page }) => {
  const vp = await ready(page);
  await page.getByRole("button", { name: "Top", exact: true }).click();
  await page.keyboard.press("l");
  for (const p of [
    [0, 0, 0],
    [12 * IN, 0, 0],
    [12 * IN, 8 * IN, 0],
    [0, 8 * IN, 0],
    [0, 0, 0],
  ] as V3[])
    await click(page, p);
  await expect(vp).toHaveAttribute("data-parts", "1");
  await expect(page.locator("#hint")).toContainText("a face formed");
  // A new chain: start on the grid, point along red, type 6 1/2.
  await click(page, [20 * IN, 0, 0]);
  const [x, y] = await at(page, [24 * IN, 0, 0], 0, 2);
  await page.mouse.move(x, y, { steps: 2 });
  await expect(vp).toHaveAttribute("data-inference", "axis-x");
  await page.keyboard.type("6 1/2");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Line 6 1/2 in on the ground");
  await page.keyboard.press("Escape");
});

test("Rect across a board's face cuts a dado strip; push it in", async ({ page }) => {
  const vp = await ready(page);
  // A board 24 x 6 x 3/4 lying flat.
  await page.keyboard.press("r");
  await page.keyboard.type("24,6");
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type("3/4");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-parts", "6");
  await page.getByRole("button", { name: "Top", exact: true }).click();
  // A 3/4 in strip across the top, from the front edge to the back edge, at x = 8 in:
  // first corner on the front edge, then type 3/4, 6 (toward +x, +y).
  await page.keyboard.press("r");
  await click(page, [8 * IN, 0, 0.75 * IN], 0, 1);
  await expect(page.locator("#hint")).toContainText("First corner set on face 2 of solid 1");
  const [x, y] = await at(page, [9 * IN, 3 * IN, 0.75 * IN]);
  await page.mouse.move(x, y, { steps: 2 });
  await page.keyboard.type("3/4, 6");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Rectangle 3/4 × 6 in on face 2 of solid 1");
  await expect(vp).toHaveAttribute("data-parts", "8");
  // Push the strip 3/8 in down: a dado.
  await page.keyboard.press("p");
  await click(page, [8.375 * IN, 3 * IN, 0.75 * IN], 0, 0);
  await page.keyboard.type("-3/8");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Pushed 3/8 in");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  const v = Number(await vp.getAttribute("data-volume")) / IN ** 3;
  expect(v).toBeCloseTo(24 * 6 * 0.75 - 0.75 * 6 * 0.375, 6);
});

test("Rotated rectangle, circle and polygon with typed counts, arc with a typed radius", async ({ page }) => {
  const vp = await ready(page);
  await page.getByRole("button", { name: "Top", exact: true }).click();
  // Circle: 48 segments, radius 3 in.
  await page.keyboard.press("c");
  await page.keyboard.type("48s");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("48 segments");
  await click(page, [-10 * IN, 0, 0]);
  const [cx, cy] = await at(page, [-8 * IN, 0, 0]);
  await page.mouse.move(cx, cy, { steps: 2 });
  await page.keyboard.type("3");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Circle r 3 in, 48 segments on the ground");
  await expect(vp).toHaveAttribute("data-parts", "1");
  // C again: Polygon, 6 sides by default.
  await page.keyboard.press("c");
  await expect(page.locator("#hint")).toContainText("Polygon (6 sides)");
  await click(page, [10 * IN, 0, 0]);
  await click(page, [12 * IN, 0, 0]);
  await expect(page.locator("#hint")).toContainText("Polygon r 2 in, 6 sides");
  await expect(vp).toHaveAttribute("data-parts", "2");
  // R twice: Rotated Rectangle on its first edge (0,10) -> (4,14), width typed.
  await page.keyboard.press("r");
  await page.keyboard.press("r");
  await expect(page.locator("#hint")).toContainText("Rotated Rectangle");
  await click(page, [0, 10 * IN, 0]);
  await click(page, [4 * IN, 14 * IN, 0]);
  const [rx, ry] = await at(page, [0, 14 * IN, 0]);
  await page.mouse.move(rx, ry, { steps: 2 });
  await page.keyboard.type("2");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Rotated rectangle");
  await expect(vp).toHaveAttribute("data-parts", "3");
  // Arc from (0,-10) to (6,-10) with a 3 in radius (a half circle), then close the chord.
  await page.keyboard.press("a");
  await click(page, [0, -10 * IN, 0]);
  await click(page, [6 * IN, -10 * IN, 0]);
  const [ax, ay] = await at(page, [3 * IN, -12 * IN, 0]);
  await page.mouse.move(ax, ay, { steps: 2 });
  await page.keyboard.type("3r");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Arc r 3 in, 12 segments");
  await page.keyboard.press("l");
  await click(page, [0, -10 * IN, 0]);
  await click(page, [6 * IN, -10 * IN, 0]);
  await expect(vp).toHaveAttribute("data-parts", "4");
});

test("a line across a solid's face from edge to edge splits it; a dangling line waits", async ({ page }) => {
  const vp = await ready(page);
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type("4");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-parts", "6");
  await page.getByRole("button", { name: "Top", exact: true }).click();
  await page.keyboard.press("l");
  await click(page, [10 * IN, 0, 4 * IN], 0, 1);
  await click(page, [10 * IN, 8 * IN, 4 * IN], 0.3, 0.3);
  await expect(page.locator("#hint")).toContainText("carry on to an edge");
  await expect(vp).toHaveAttribute("data-parts", "6");
  await click(page, [10 * IN, 16 * IN, 4 * IN], 0, -1);
  await expect(page.locator("#hint")).toContainText("Cut on face 2 of solid 1");
  await expect(vp).toHaveAttribute("data-parts", "7");
  await expect(vp).toHaveAttribute("data-watertight", "true");
});
