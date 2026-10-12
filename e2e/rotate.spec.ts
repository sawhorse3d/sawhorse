import { expect, test } from "@playwright/test";

// Rotate (lane 4): the protractor locked by an arrow key, centre / start / typed angle, a
// typed angle after the turn, Ctrl copies with x3 around a centre (four parts in a ring),
// and turning part of a solid (the top face about its front edge: a wedge; a twist that
// would bend the sides is refused, typed).
import { at, box, clickAt, near, point, ready, selectAt, solids, type } from "./helpers";

test("turn a board a quarter about its corner, then change the angle", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "24,4", "2");
  await page.keyboard.press("q");
  await page.keyboard.press("ArrowUp");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Protractor locked level");
  await clickAt(page, [0, 0, 0], "endpoint", 1, 1);
  await clickAt(page, [24, 0, 0], "endpoint", -1, 1);
  await point(page, [0, 20, 0]);
  await type(page, "90");
  await expect(hint).toContainText("Turned solid 1 90° about the blue axis");
  let s = await solids(page);
  near(s[0].min, [-4, 0, 0]);
  near(s[0].max, [0, 24, 2]);
  // Typed after the turn: it becomes 45 degrees.
  await type(page, "45");
  await expect(hint).toContainText("Turned solid 1 45° about the blue axis");
  s = await solids(page);
  near(s[0].max, [24 * Math.SQRT1_2, (24 + 4) * Math.SQRT1_2, 2]);
  await expect(vp).toHaveAttribute("data-watertight", "true");
});

test("Ctrl copies a turn; x3 makes a ring of four about a centre", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "2,2", "2");
  // Select the post first: the centre of the ring is off the solid.
  await page.keyboard.press("Space");
  await selectAt(page, [1, 1, 2], "face");
  const [x, y] = await at(page, [1, 1, 2], 1, -1);
  await page.mouse.dblclick(x, y);
  await expect(page.locator("#hint")).toContainText("Selected solid 1");
  await page.keyboard.press("q");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Control");
  await clickAt(page, [10, 10, 0], "grid", 0, 0);
  await clickAt(page, [0, 0, 0], "endpoint", -1, 1);
  await point(page, [20, 0, 0]);
  await type(page, "90");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Copied solid 1 turned 90° about the blue axis");
  await type(page, "x3");
  await expect(hint).toContainText("Array: 3 turned copies of solid 1, 90° apart");
  await expect(vp).toHaveAttribute("data-objects", "4");
  const s = await solids(page);
  const boxes = s.map((x) => [x.min[0], x.min[1]].map((v) => Math.round(v) + 0));
  expect(boxes).toEqual([
    [0, 0],
    [18, 0],
    [18, 18],
    [0, 18],
  ]);
});

test("turn the top face about its front edge into a wedge; a twist is refused", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "12,12", "6");
  await page.keyboard.press("Space");
  await selectAt(page, [6, 6, 6], "face");
  await expect(page.locator("#hint")).toContainText("Selected face 2 of solid 1");
  await page.keyboard.press("q");
  // A twist about the blue axis would bend every side: refused, typed.
  await page.keyboard.press("ArrowUp");
  await clickAt(page, [0, 0, 6], /endpoint|midpoint/, 1, 1);
  await clickAt(page, [12, 0, 6], "endpoint", -1, 1);
  await point(page, [12, 6, 6]);
  await type(page, "10");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("would bend face");
  // About the front edge (the protractor upright, facing red): a wedge.
  await page.keyboard.press("ArrowRight");
  await expect(hint).toContainText("Protractor locked upright, facing red");
  await clickAt(page, [0, 0, 6], /endpoint|midpoint/, 1, 1);
  await clickAt(page, [0, 12, 6], "endpoint", 1, -1);
  await point(page, [0, 12, 3]);
  await type(page, "10");
  await expect(hint).toContainText("Turned face 2 of solid 1 -10° about the red axis");
  const s = await solids(page);
  // The side profile (y, z): (0,0) (12,0) (12 cos 10, 6 - 12 sin 10) (0,6), times 12 wide.
  const a = (10 * Math.PI) / 180;
  const pts = [
    [0, 0],
    [12, 0],
    [12 * Math.cos(a), 6 - 12 * Math.sin(a)],
    [0, 6],
  ];
  let area = 0;
  for (let k = 0; k < 4; k++) area += pts[k][0] * pts[(k + 1) % 4][1] - pts[(k + 1) % 4][0] * pts[k][1];
  expect(s[0].volume).toBeCloseTo((Math.abs(area) / 2) * 12, 4);
  await expect(vp).toHaveAttribute("data-watertight", "true");
});
