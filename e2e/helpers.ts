import { expect, test, type Page } from "@playwright/test";

// Shared helpers for the lane-4 specs (move, rotate, scale, offset, follow-me): every
// click waits for the inference it should land on, positions are given in inches, and
// results are read back from window.__sawhorse.solids() (bounds and volumes per solid).
const requireKernel = !!process.env.REQUIRE_KERNEL;
export type V3 = [number, number, number];
export const IN = 0.0254;

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

