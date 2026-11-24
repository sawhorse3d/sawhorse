import { expect, test, type Page } from "@playwright/test";
import { IN, at, box, clickAt, hover, ready, solids, type, type V3 } from "./helpers";

// #130: a rectangle started on a corner of a box goes on the face under the cursor. A guide
// line that leaves the face (the red or green axis through the first corner, the line of a
// point the cursor rested on earlier, also one under the board) no longer pulls the second
// corner off the face onto a new sketch beside it; the marker shows the face before the
// click. Both corners on one line say why nothing is drawn, the first corner stays; typed
// sizes with the cursor still on the corner keep the kernel's reason and say the way was a
// guess. The cases that never touched a face holding the corner (up into the air, on the
// ground, a corner or midpoint of the face) draw as before. Iso view, 1440 x 900.

type Inf = { kind: string; point: V3; surface: { at: { kind: string; object?: number; face?: number }; point: V3; frame: { normal: V3 } } | null };
const hint = (page: Page) => page.locator("#hint");
const vp = (page: Page) => page.locator("main.vp");
const hash = (page: Page) => vp(page).getAttribute("data-hash");
const lastInf = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { lastInference: Inf | null } }).__sawhorse.lastInference);
type Region = { rings: V3[][]; normal: V3 | null; origin: V3 | null };
const regions = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { regions(): Promise<Region[]> } }).__sawhorse.regions());

/** A box, the Select tool, the Iso view, then the Rectangle tool. */
async function model(page: Page, size: string, h: string) {
  await ready(page);
  await box(page, size, h);
  await page.keyboard.press(" ");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await expect(vp(page)).toHaveAttribute("data-view", "iso");
  await page.keyboard.press("r");
  await expect(hint(page)).toContainText("Rect.");
}

/** Move straight to a model point (inches) and wait until the tool shows `marker` there. */
async function moveTo(page: Page, p: V3, marker: string | RegExp) {
  const [x, y] = await at(page, p);
  await page.mouse.move(x, y);
  await expect(vp(page)).toHaveAttribute("data-inference", marker, { timeout: 3_000 });
  return [x, y] as [number, number];
}

/** Undo until the model is back to `h0`. */
async function undoTo(page: Page, h0: string | null) {
  await page.keyboard.press("Control+z");
  await expect.poll(() => hash(page)).toBe(h0);
}

test("a remembered point's line under the board does not take the rectangle off the top face; the rabbet cuts the board", async ({ page }) => {
  await model(page, "24,6", "3/4");
  // Rest on the bottom edge's midpoint at the left end (its axis line runs under the board).
  await hover(page, [0, 3, 0], "midpoint");
  await clickAt(page, [24, 0, 0.75], "endpoint");
  await expect(hint(page)).toContainText("First corner set");
  // Over the top face, right where that remembered line runs (under the board).
  const q = await moveTo(page, [15, 3, 0], "on-face");
  const inf = (await lastInf(page))!;
  // The engine's own answer is still the remembered line; the tool keeps to the face.
  expect(inf.kind).toBe("from-point");
  expect(inf.surface?.at).toEqual({ kind: "face", object: 1, face: 2 });
  const hit = inf.surface!.point.map((m) => m / IN) as V3;
  expect(hit[2]).toBeCloseTo(0.75, 6);
  await page.mouse.click(q[0], q[1]);
  await expect(hint(page)).toContainText("on face 2 of solid 1");
  await expect(hint(page)).toContainText("from (24, 0, 3/4)");
  // Push/Pull the new strip 3/8 down: one board with a rabbet, not a second board.
  await page.keyboard.press("p");
  await type(page, "-3/8");
  await expect(hint(page)).toContainText("Pushed");
  const want = 108 - (24 - hit[0]) * hit[1] * 0.375;
  await expect.poll(async () => (await solids(page)).map((s) => +s.volume.toFixed(3))).toEqual([+want.toFixed(3)]);
});

for (const [name, normal, grid] of [
  ["top", [0, 0, 1], (u: number, v: number): V3 => [12 * u, 12 * v, 12]],
  ["front (facing -green)", [0, -1, 0], (u: number, v: number): V3 => [12 * u, 0, 12 * v]],
  ["left (facing -red)", [-1, 0, 0], (u: number, v: number): V3 => [0, 12 * u, 12 * v]],
] as [string, V3, (u: number, v: number) => V3][]) {
  test(`from the near top corner of a 12 in cube, 25 second clicks on its ${name} face all draw on that face`, async ({ page }) => {
    test.setTimeout(120_000);
    await model(page, "12,12", "12");
    const h0 = await hash(page);
    const missed: string[] = [];
    for (const u of [0.1, 0.3, 0.5, 0.7, 0.9]) {
      for (const v of [0.1, 0.3, 0.5, 0.7, 0.9]) {
        const p = grid(u, v);
        await clickAt(page, [0, 0, 12], "endpoint");
        await expect(hint(page)).toContainText("First corner set");
        const [x, y] = await at(page, p);
        await page.mouse.move(x, y);
        // The cursor is over that face (whatever the engine snapped to).
        await expect.poll(async () => (await lastInf(page))?.surface?.frame.normal.map((n) => Math.round(n))).toEqual(normal.map((n) => n || 0));
        const face = (await lastInf(page))!.surface!.at.face;
        await page.mouse.click(x, y);
        await expect(hint(page)).not.toContainText("First corner set");
        const said = (await hint(page).innerText()).trim();
        if (!said.includes(`on face ${face} of solid 1`) || (await hash(page)) === h0) missed.push(`${p.join(",")}: ${said}`);
        if ((await hash(page)) !== h0) await undoTo(page, h0);
        await page.keyboard.press("Escape");
      }
    }
    expect(missed).toEqual([]);
  });
}

test("a second click on the edge through the first corner says why, keeps the corner, and the next click draws", async ({ page }) => {
  await model(page, "24,6", "3/4");
  const h0 = await hash(page);
  await clickAt(page, [24, 0, 0.75], "endpoint");
  // Hovering the edge says nothing: only the click does.
  await hover(page, [12, 0, 0.75], "midpoint");
  await expect(hint(page)).toContainText("First corner set");
  await clickAt(page, [12, 0, 0.75], "midpoint");
  await expect(hint(page)).toHaveText("Can't do that. On the drawing plane both corners are on one line, so the rectangle has no width. Click off the line, or type width, depth.");
  await expect(hint(page)).toHaveClass(/err/);
  expect(await hash(page)).toBe(h0);
  await clickAt(page, [12, 3, 0.75], /on-face|from-point/);
  await expect(hint(page)).toContainText(/Rectangle 12 × (~ )?3 in on face 2 of solid 1/);
  await expect(hint(page)).toContainText("from (24, 0, 3/4)");
});

test("typed sizes with the cursor still on the first corner: the kernel's words, then the guess named; on the face they cut the rabbet", async ({ page }) => {
  await model(page, "24,6", "3/4");
  const h0 = await hash(page);
  await clickAt(page, [24, 0, 0.75], "endpoint");
  await expect(hint(page)).toContainText("First corner set");
  await type(page, "24, 3/8");
  await expect(hint(page)).toHaveText("Can't do that. the path must run strictly inside the face. The cursor did not show which way the rectangle runs, so that was a guess: move the cursor onto the face to choose it.");
  expect(await hash(page)).toBe(h0);
  // A size the kernel calls too small: its words alone.
  await type(page, "24, 0");
  await expect(hint(page)).toHaveText("Can't do that. that shape is too small (under 0.01 mm)");
  expect(await hash(page)).toBe(h0);
  // The first corner is still placed: onto the top face, the same sizes.
  await moveTo(page, [12, 3, 0.75], /on-face|from-point/);
  await type(page, "24, 3/8");
  await expect(hint(page)).toContainText("Rectangle 24 × 3/8 in on face 2 of solid 1");
  await expect(hint(page)).toContainText("from (24, 0, 3/4)");
  await page.keyboard.press("p");
  await type(page, "-3/8");
  await expect(hint(page)).toContainText("Pushed");
  await expect.poll(async () => (await solids(page)).map((s) => +s.volume.toFixed(3))).toEqual([104.625]);
});

test("what never had a face under the cursor holding the corner draws as before: up into the air, on the ground, a corner or midpoint of the face", async ({ page }) => {
  await model(page, "12,12", "12");
  const h0 = await hash(page);
  // Up into the air: rest on the midpoint of the near right upright edge, then from the
  // near top corner along that point's red line, out past the cube.
  await hover(page, [12, 0, 6], "midpoint");
  await clickAt(page, [0, 0, 12], "endpoint");
  const s = await moveTo(page, [18, 0, 6], "from-point");
  await page.mouse.click(s[0], s[1]);
  await expect(hint(page)).toContainText("on a vertical plane");
  const up = await regions(page);
  expect(up).toHaveLength(1);
  expect(up[0].normal?.map((n) => Math.abs(Math.round(n)))).toEqual([0, 1, 0]);
  await undoTo(page, h0);

  // On the ground from a bottom corner.
  await clickAt(page, [12, 0, 0], "endpoint");
  const g = await moveTo(page, [20, -6, 0], /grid|free|from-point|axis-x|axis-y/);
  await page.mouse.click(g[0], g[1]);
  await expect(hint(page)).toContainText("on the ground");
  const ground = await regions(page);
  expect(ground).toHaveLength(1);
  expect(ground[0].normal).toEqual([0, 0, 1]);
  expect(ground[0].origin?.[2] ?? 0).toBe(0);
  await undoTo(page, h0);

  // On the top face to a midpoint of its far edge, and from a midpoint to its far corner.
  await clickAt(page, [0, 0, 12], "endpoint");
  await clickAt(page, [12, 6, 12], "midpoint");
  await expect(hint(page)).toContainText("Rectangle 12 × 6 in on face 2 of solid 1");
  await undoTo(page, h0);
  await clickAt(page, [6, 0, 12], "midpoint");
  await clickAt(page, [12, 12, 12], "endpoint");
  await expect(hint(page)).toContainText("Rectangle 6 × 12 in on face 2 of solid 1");
  await undoTo(page, h0);
});
