import { expect, test, type Page } from "@playwright/test";

// Scale (lane 4): grips on the selection's bounding box, a face grip with a typed factor,
// an absolute size with a unit, a mirror (-1), and a tapered leg: the top face of a post
// scaled about its centre (Ctrl) to half size, checked against the frustum's volume.
import { at, box, near, ready, selectAt, solids, type, type V3 } from "./helpers";

/** Put the cursor on a grip (inches) and wait until it is the hot one. */
async function grip(page: Page, p: V3) {
  await expect(async () => {
    const [x, y] = await at(page, p);
    await page.mouse.move(x + 3, y + 2);
    await page.mouse.move(x, y, { steps: 2 });
    await expect(page.locator(".vlabel.grip.hot")).toHaveCount(1, { timeout: 800 });
  }).toPass({ timeout: 10_000 });
  const [x, y] = await at(page, p);
  return { x, y };
}

async function selectSolid(page: Page, p: V3) {
  await page.keyboard.press("Space");
  await selectAt(page, p, "face");
  const [x, y] = await at(page, p, 1, -1);
  await page.mouse.dblclick(x, y);
  await expect(page.locator("#hint")).toContainText("Selected solid");
}

test("a face grip with a typed factor, then an absolute size, then a mirror", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "24,4", "2");
  await selectSolid(page, [12, 2, 2]);
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  const hint = page.locator("#hint");
  const g = await grip(page, [24, 2, 1]);
  await expect(hint).toContainText("A face grip");
  await page.mouse.click(g.x, g.y);
  await page.mouse.move(g.x + 40, g.y, { steps: 3 });
  await type(page, "1.5");
  await expect(hint).toContainText("Scaled solid 1 ×1.5 along red about the opposite grip");
  let s = await solids(page);
  near(s[0].min, [0, 0, 0]);
  near(s[0].max, [36, 4, 2]);
  // Typed again with a unit: an absolute length along the grip.
  await type(page, "30in");
  await expect(hint).toContainText("×1.25 along red");
  s = await solids(page);
  near(s[0].max, [30, 4, 2]);
  // A mirror through the opposite face.
  await type(page, "-1");
  await expect(hint).toContainText("×-1 along red");
  s = await solids(page);
  near(s[0].min, [-24, 0, 0]);
  near(s[0].max, [0, 4, 2]);
  expect(s[0].volume).toBeCloseTo(24 * 4 * 2, 6);
  await expect(vp).toHaveAttribute("data-watertight", "true");
});

test("a tapered leg: the top face scaled about its centre", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "2,2", "28");
  // Look straight down on the post's top, zoomed to fit.
  await page.getByRole("button", { name: "Top", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await page.keyboard.press("Space");
  await selectAt(page, [1, 1, 28], "face");
  await expect(page.locator("#hint")).toContainText("Selected face 2 of solid 1");
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).toHaveCount(8);
  await page.keyboard.press("Control");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("about the centre");
  const g = await grip(page, [2, 2, 28]);
  await page.mouse.click(g.x, g.y);
  await page.mouse.move(g.x - 10, g.y + 10, { steps: 3 });
  await type(page, "0.5");
  await expect(hint).toContainText("Scaled face 2 of solid 1 ×0.5 along red and green about its centre");
  const s = await solids(page);
  // A frustum: h/3 (A1 + A2 + sqrt(A1 A2)) = 28/3 (4 + 1 + 2).
  expect(s[0].volume).toBeCloseTo((28 / 3) * 7, 4);
  near(s[0].min, [0, 0, 0]);
  near(s[0].max, [2, 2, 28]);
  await expect(vp).toHaveAttribute("data-watertight", "true");
});
