import { expect, test, type Page } from "@playwright/test";

// Move (lane 4): click-move-click and drag with inference, typed distances, Ctrl copies,
// arrays (x3, /2) that replace the copy, a distance typed after the move, and moving part
// of a solid (a face, a corner) with autofold off and on. Every click waits for the
// inference it should land on; results are checked against each solid's bounding box.
const requireKernel = !!process.env.REQUIRE_KERNEL;
type V3 = [number, number, number];
const IN = 0.0254;

export async function ready(page: Page) {
  await page.goto("/?autosave=0");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

export async function at(page: Page, p: V3, dx = 0, dy = 0): Promise<[number, number]> {
  const box = await page.locator("#cv").boundingBox();
  const m: V3 = [p[0] * IN, p[1] * IN, p[2] * IN];
  const xy = await page.evaluate((m) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(m), m);
  return [box!.x + xy[0] + dx, box!.y + xy[1] + dy];
}

/**
 * Move to a point (inches) and wait for the inference the click should land on. The view
 * may refit right after an edit (it grows to fit new solids), so the screen position is
 * recomputed and the cursor nudged until the inference shows.
 */
export async function hover(page: Page, p: V3, kind: string | RegExp, dx = 1, dy = -1, attr = "data-inference") {
  await expect(async () => {
    const [x, y] = await at(page, p, dx, dy);
    await page.mouse.move(x + 3, y + 2);
    await page.mouse.move(x, y, { steps: 2 });
    await expect(page.locator("main.vp")).toHaveAttribute(attr, kind, { timeout: 800 });
  }).toPass({ timeout: 10_000 });
}

/** Click a point with the Select tool once the entity of `kind` is under the cursor. */
export async function selectAt(page: Page, p: V3, kind: string, dx = 1, dy = -1) {
  await hover(page, p, kind, dx, dy, "data-hover");
  const [x, y] = await at(page, p, dx, dy);
  await page.mouse.click(x, y);
}

export async function clickAt(page: Page, p: V3, kind: string | RegExp, dx = 1, dy = -1) {
  await hover(page, p, kind, dx, dy);
  const [x, y] = await at(page, p, dx, dy);
  await page.mouse.click(x, y);
}

export async function point(page: Page, p: V3) {
  const [x, y] = await at(page, p);
  await page.mouse.move(x, y, { steps: 4 });
}

export async function type(page: Page, text: string) {
  await page.keyboard.type(text);
  await page.keyboard.press("Enter");
}

export type Solid = { id: number; number: number; min: V3; max: V3; volume: number };
export async function solids(page: Page): Promise<Solid[]> {
  const all = await page.evaluate(() => (window as unknown as { __sawhorse: { solids(): Solid[] } }).__sawhorse.solids());
  return all.map((s) => ({ ...s, min: s.min.map((m) => m / IN) as V3, max: s.max.map((m) => m / IN) as V3, volume: s.volume / IN ** 3 }));
}

export function near(a: V3, b: V3, tol = 1e-4) {
  for (let k = 0; k < 3; k++) expect(Math.abs(a[k] - b[k]), `${JSON.stringify(a)} vs ${JSON.stringify(b)}`).toBeLessThan(tol);
}

/** A 12 x 12 x 6 in box at the origin. */
export async function box(page: Page, size = "12,12", h = "6") {
  await page.keyboard.press("r");
  await type(page, size);
  await page.keyboard.press("p");
  await type(page, h);
  await expect(page.locator("#hint")).toContainText("Extruded");
}

test("click, move, click with a typed distance; then change it by typing again", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("m");
  await clickAt(page, [12, 0, 6], "endpoint");
  await point(page, [20, 0, 6]);
  await expect(vp).toHaveAttribute("data-inference", "axis-x");
  await type(page, "24");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Moved solid 1 24 in along +red");
  let s = await solids(page);
  near(s[0].min, [24, 0, 0]);
  near(s[0].max, [36, 12, 6]);
  // Typed after the move: the move becomes 30 in.
  await type(page, "30");
  await expect(hint).toContainText("Moved solid 1 30 in along +red");
  s = await solids(page);
  near(s[0].min, [30, 0, 0]);
  // One undo step per move.
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await solids(page))[0].min[0]).toBeCloseTo(0, 6);
});

test("press, drag and let go onto another solid's corner", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("r");
  await clickAt(page, [36, 0, 0], "grid", 1, 1);
  await point(page, [40, 6, 0]);
  await type(page, "6,6");
  await page.keyboard.press("p");
  await type(page, "3");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await page.keyboard.press("m");
  // From the big box's top corner to the small box's top corner, by dragging.
  await hover(page, [12, 0, 6], "endpoint");
  await page.mouse.down();
  const [x1, y1] = await at(page, [36, 0, 3], 1, -1);
  await page.mouse.move(x1, y1, { steps: 12 });
  await hover(page, [36, 0, 3], "endpoint");
  await page.mouse.up();
  await expect(page.locator("#hint")).toContainText("Moved solid 1");
  const s = await solids(page);
  near(s[0].min, [24, 0, -3]);
  near(s[0].max, [36, 12, 3]);
});

test("Ctrl copies; x3 makes an array; /2 divides it; copies are separate solids", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "3,3", "30");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt(page, [0, 0, 0], "endpoint", 1, 1);
  // Lock the green axis with the arrow key, then show the way with the cursor.
  await page.keyboard.press("ArrowLeft");
  await expect(page.locator("#hint")).toContainText("Moving along the green axis");
  await point(page, [0, 10, 0]);
  await type(page, "18");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Copied solid 1 18 in along +green");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await type(page, "x3");
  await expect(hint).toContainText("Array: 3 copies of solid 1, 18 in along +green apart");
  await expect(vp).toHaveAttribute("data-objects", "4");
  let s = await solids(page);
  expect(s.map((x) => Math.round(x.min[1]))).toEqual([0, 18, 36, 54]);
  expect(s.map((x) => x.number)).toEqual([1, 2, 3, 4]);
  await type(page, "/2");
  await expect(vp).toHaveAttribute("data-objects", "3");
  s = await solids(page);
  expect(s.map((x) => Math.round(x.min[1]))).toEqual([0, 27, 54]);
  // One undo takes the array back to nothing copied.
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-objects", "1");
});

test("moving part of a solid: a face slides, a corner bends a face unless autofold is on", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  // Select the top face and lift it 2 in.
  await page.keyboard.press("Space");
  await selectAt(page, [6, 6, 6], "face");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(page.locator("#hint")).toContainText("Selected face 2 of solid 1");
  await page.keyboard.press("m");
  // Any start point: the typed distance only needs the direction (locked to blue).
  await clickAt(page, [0, 0, 6], /endpoint|midpoint|on-edge|on-face/, 1, 1);
  await page.keyboard.press("ArrowUp");
  await point(page, [0, 0, 10]);
  await type(page, "2");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Moved face 2 of solid 1 2 in along +blue");
  let s = await solids(page);
  expect(s[0].volume).toBeCloseTo(12 * 12 * 8, 6);
  // Select a top corner and lift it: the top would bend, refused (typed).
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await selectAt(page, [12, 0, 8], "vertex");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("m");
  await clickAt(page, [12, 0, 8], "endpoint");
  await page.keyboard.press("ArrowUp");
  await point(page, [12, 0, 12]);
  await type(page, "1");
  await expect(hint).toContainText("would bend face");
  await expect(hint).not.toContainText("#");
  // Alt turns autofold on: the same move folds the faces.
  await page.keyboard.press("Alt");
  await expect(hint).toContainText("Autofold on");
  await clickAt(page, [12, 0, 8], "endpoint");
  await page.keyboard.press("ArrowUp");
  await point(page, [12, 0, 12]);
  await type(page, "1");
  await expect(hint).toContainText("Moved a point of solid 1 1 in along +blue");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  s = await solids(page);
  near(s[0].max, [12, 12, 9]);
});
