import { expect, test } from "@playwright/test";

// Offset (lane 4): a face inward (imprint, then a tenon pulled up) and outward (a sketch
// on its plane), a sketch region inward, and a selected edge offset across a face (a
// cut). Results through the kernel, checked by parts and volumes.
import { box, clickAt, point, ready, selectAt, solids, type } from "./helpers";

test("a face inward imprints (and pulls up into a tenon); outward makes a sketch", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "12,8", "4");
  await expect(vp).toHaveAttribute("data-parts", "6");
  await page.keyboard.press("f");
  await clickAt(page, [4, 0, 4], "on-edge", 0, -1);
  await point(page, [4, 3, 4]);
  await type(page, "1");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Offset face 2 of solid 1 1 in inward, on the solid");
  await expect(vp).toHaveAttribute("data-parts", "7");
  // The inner face is the Push/Pull target: a 10 x 6 tenon 1 in tall.
  await page.keyboard.press("p");
  await type(page, "1");
  await expect(hint).toContainText("Pulled 1 in");
  let s = await solids(page);
  expect(s[0].volume).toBeCloseTo(12 * 8 * 4 + 10 * 6, 6);
  // Outward from the tenon's top: off the solid, so a new sketch on that plane.
  await page.keyboard.press("f");
  await clickAt(page, [4, 1, 5], "on-edge", 0, -1);
  await point(page, [4, -2, 5]);
  await type(page, "1/2");
  await expect(hint).toContainText("1/2 in outward, into a sketch on its plane");
  s = await solids(page);
  expect(s).toHaveLength(1);
});

test("a sketch region inward stays in its sketch", async ({ page }) => {
  const vp = await ready(page);
  await page.keyboard.press("r");
  await type(page, "10,10");
  await expect(vp).toHaveAttribute("data-parts", "1");
  await page.keyboard.press("f");
  await clickAt(page, [3.3, 2.2, 0], /on-face|grid/, 0, 0);
  await point(page, [3, 4, 0]);
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("Offset region 1 2 in inward, into a sketch on its plane");
  await expect(vp).toHaveAttribute("data-parts", "2");
});

test("a selected edge offsets across the face as a cut", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "12,8", "4");
  await page.keyboard.press("Space");
  await selectAt(page, [4, 0, 4], "edge", 0, -1);
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("f");
  await clickAt(page, [4, 0, 4], "on-edge", 0, -1);
  await point(page, [4, 3, 4]);
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("Offset 1 edge of solid 1 2 in to the");
  await expect(vp).toHaveAttribute("data-parts", "7");
  await expect(vp).toHaveAttribute("data-watertight", "true");
});
