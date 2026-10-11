import { expect, test, type Page } from "@playwright/test";

// The Phase-2 exit test, scripted (lane 3): a 36 x 72 x 11 1/4 in bookshelf, two sides,
// a top, four shelves in 3/4 in wide x 3/8 in deep dados, and a toe kick, built with the
// keyboard and the mouse only, through the same tools a person uses: Rect, Push/Pull,
// Tape (guides), Escape and the standard views. Every click is checked against the
// inference it should land on (endpoint, grid, intersection, guide point) before it is
// made, so the test fails where a person would fight the inference engine.
//
// Note on "3/4 in deep dado": in 3/4 in stock a 3/4 in deep groove would cut the side in
// two, so the dados are 3/4 in wide (the shelf thickness) and 3/8 in deep (half the side).
const requireKernel = !!process.env.REQUIRE_KERNEL;
type V3 = [number, number, number];
const IN = 0.0254;

const SIDE_H = 71.25; // the top sits on the sides: 71 1/4 + 3/4 = 72
const SHELVES = [3.5, 21.5, 39.5, 57.5]; // shelf bottoms; the bottom shelf sits on the toe kick
const RIGHT_X = 35.25; // the right side's inner face

async function ready(page: Page) {
  await page.goto("/");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** Page coordinates of a world point given in inches. */
async function at(page: Page, p: V3, dx = 0, dy = 0): Promise<[number, number]> {
  const box = await page.locator("#cv").boundingBox();
  const m: V3 = [p[0] * IN, p[1] * IN, p[2] * IN];
  const xy = await page.evaluate((m) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(m), m);
  return [box!.x + xy[0] + dx, box!.y + xy[1] + dy];
}

/** Move to a point (inches) and wait for the inference the click should land on. */
async function hover(page: Page, p: V3, kind: string | RegExp, dx = 1, dy = -1) {
  const [x, y] = await at(page, p, dx, dy);
  await page.mouse.move(x, y, { steps: 3 });
  await expect(page.locator("main.vp")).toHaveAttribute("data-inference", kind);
}

async function clickAt(page: Page, p: V3, kind: string | RegExp, dx = 1, dy = -1) {
  await hover(page, p, kind, dx, dy);
  const [x, y] = await at(page, p, dx, dy);
  await page.mouse.click(x, y);
}

/** Point the cursor somewhere (no inference expected), e.g. to show a direction. */
async function point(page: Page, p: V3) {
  const [x, y] = await at(page, p);
  await page.mouse.move(x, y, { steps: 3 });
}

async function type(page: Page, text: string) {
  await page.keyboard.type(text);
  await page.keyboard.press("Enter");
}

async function view(page: Page, name: string) {
  await page.getByRole("button", { name, exact: true }).click();
}

test("the bookshelf with dados, in under five minutes of steps", async ({ page }) => {
  test.setTimeout(240_000);
  const t0 = Date.now();
  const vp = await ready(page);
  const hint = page.locator("#hint");
  const objects = () => vp.getAttribute("data-objects");

  // 1. The left side: a 3/4 x 11 1/4 rectangle at the origin, pulled up 71 1/4.
  await page.keyboard.press("r");
  await type(page, "3/4, 11 1/4");
  await expect(hint).toContainText("Rectangle 3/4 × 11 1/4 in on the ground");
  await page.keyboard.press("p");
  await type(page, "71 1/4");
  await expect(hint).toContainText("Extruded 71 1/4 in");

  // 2. Shelf layout on its inner face, seen from the right: four guides measured up from
  //    the inner bottom edge, then a 3/4 in strip at each, pushed in 3/8 in.
  await view(page, "Right");
  await page.keyboard.press("Shift+Z");
  for (const z of SHELVES) {
    await page.keyboard.press("t");
    await clickAt(page, [0.75, 3, 0], "on-edge", 0, -1);
    await point(page, [0.75, 3, z + 4]);
    await type(page, `${fraction(z)}`);
    await expect(hint).toContainText(`Guide ${fraction(z)} in from the edge`);
  }
  for (const z of SHELVES) {
    await page.keyboard.press("r");
    await clickAt(page, [0.75, 0, z], "intersection", 0, 0);
    await point(page, [0.75, 5, z + 4]);
    await type(page, "11 1/4, 3/4");
    await expect(hint).toContainText("Rectangle 11 1/4 × 3/4 in on face");
    await page.keyboard.press("p");
    await type(page, "-3/8");
    await expect(hint).toContainText("Pushed 3/8 in");
  }

  // 3. The right side, from the grid 3 ft over, 3/4 in back toward the left side.
  await view(page, "Iso");
  await page.keyboard.press("r");
  await clickAt(page, [36, 0, 0], "grid", 1, 1);
  await point(page, [30, 6, 0]);
  await type(page, "3/4, 11 1/4");
  await expect(hint).toContainText("Rectangle 3/4 × 11 1/4 in on the ground");
  await page.keyboard.press("p");
  await type(page, "71 1/4");
  await expect(hint).toContainText("Extruded 71 1/4 in");
  await expect.poll(objects).toBe("2");

  // 4. Its dados, from the iso view (its inner face looks this way).
  await page.keyboard.press("Shift+Z");
  for (const z of SHELVES) {
    await page.keyboard.press("t");
    await clickAt(page, [RIGHT_X, 3, 0], "on-edge", 0, -1);
    await point(page, [RIGHT_X, 3, z + 4]);
    await type(page, `${fraction(z)}`);
  }
  for (const z of SHELVES) {
    await page.keyboard.press("r");
    await clickAt(page, [RIGHT_X, 0, z], "intersection", 0, 0);
    await point(page, [RIGHT_X, 5, z + 4]);
    await type(page, "11 1/4, 3/4");
    await expect(hint).toContainText("Rectangle 11 1/4 × 3/4 in on face");
    await page.keyboard.press("p");
    await type(page, "-3/8");
    await expect(hint).toContainText("Pushed 3/8 in");
  }

  // 5. Four shelves, each from the left dado's front-bottom corner, level, 35 1/4 x 11 1/4,
  //    pulled up 3/4 into the dados.
  for (const z of SHELVES) {
    await page.keyboard.press("r");
    await clickAt(page, [0.375, 0, z], "endpoint", 1, 1);
    await point(page, [18, 6, z]);
    await type(page, "35 1/4, 11 1/4");
    await expect(hint).toContainText(`Rectangle 35 1/4 × 11 1/4 in on a plane ${fraction(z)} up`);
    await page.keyboard.press("p");
    await type(page, "3/4");
    await expect(hint).toContainText("Extruded 3/4 in");
  }
  await expect.poll(objects).toBe("6");

  // 6. The toe kick, 2 in back: a guide point 2 in along the right side's inner bottom
  //    edge, then 34 1/2 x 3/4 toward the left side, 3 1/2 tall (under the bottom shelf).
  await page.keyboard.press("t");
  await clickAt(page, [RIGHT_X, 0, 0], "endpoint", 1, -1);
  await point(page, [RIGHT_X, 6, 0]);
  await type(page, "2");
  await expect(hint).toContainText("Guide point 2 in along");
  await page.keyboard.press("r");
  await clickAt(page, [RIGHT_X, 2, 0], "guide-point", 1, 0);
  await point(page, [18, 4, 0]);
  await type(page, "34 1/2, 3/4");
  await expect(hint).toContainText("Rectangle 34 1/2 × 3/4 in on the ground");
  await page.keyboard.press("p");
  await type(page, "3 1/2");
  await expect(hint).toContainText("Extruded 3 1/2 in");

  // 7. The top: from the left side's top front corner, level, 36 x 11 1/4, 3/4 thick.
  await page.keyboard.press("r");
  await clickAt(page, [0, 0, SIDE_H], "endpoint", -1, -1);
  await point(page, [18, 6, SIDE_H]);
  await type(page, "36, 11 1/4");
  await expect(hint).toContainText(`Rectangle 36 × 11 1/4 in on a plane 71 1/4 up`);
  await page.keyboard.press("p");
  await type(page, "3/4");
  await expect(hint).toContainText("Extruded 3/4 in");

  // 8. Clean up the guides, look at it.
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /Delete Guides/ }).click();
  await page.keyboard.press("Escape");
  await view(page, "Persp");

  // The result: 8 watertight solids of the right total volume.
  await expect.poll(objects).toBe("8");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  const side = 0.75 * 11.25 * SIDE_H - 4 * (11.25 * 0.75 * 0.375);
  const expected = 2 * side + 36 * 11.25 * 0.75 + 4 * (35.25 * 11.25 * 0.75) + 34.5 * 0.75 * 3.5;
  const volume = Number(await vp.getAttribute("data-volume")) / IN ** 3;
  expect(volume).toBeCloseTo(expected, 3);
  expect(expected).toBeCloseTo(2761.03125, 6);
  const state = await page.evaluate(() => (window as unknown as { __sawhorse: { refusals?: Record<string, number> } }).__sawhorse.refusals ?? {});
  const tilted = Object.entries(state).filter(([m]) => m.includes("not a valid solid")).reduce((n, [, c]) => n + c, 0);
  const seconds = (Date.now() - t0) / 1000;
  console.log(`BOOKSHELF scripted: ${seconds.toFixed(1)} s, volume ${volume.toFixed(5)} in³, refusals ${JSON.stringify(state)}, tilted-plane boolean refusals ${tilted}`);
  test.info().annotations.push({ type: "bookshelf", description: `${seconds.toFixed(1)} s; refusals ${JSON.stringify(state)}; tilted ${tilted}` });
  expect(Object.keys(state)).toEqual([]);
  await page.screenshot({ path: "test-results/bookshelf-scripted.png" });
});

/** 21.5 -> "21 1/2", 3.5 -> "3 1/2". */
function fraction(x: number): string {
  const whole = Math.floor(x);
  const half = x - whole;
  return half === 0.5 ? `${whole} 1/2` : half === 0 ? `${whole}` : String(x);
}
