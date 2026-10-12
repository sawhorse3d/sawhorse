import { expect, test } from "@playwright/test";

// Follow Me (lane 4): a chamfer moulding routed round the top of a box. The profile is a
// corner of the box's end face cut off with the Line tool; the path is the top face's edge
// (select the face, press W, click the profile). Checked against the exact volume: the
// box less the triangle's area times its centroid's path round the top.
import { box, clickAt, ready, selectAt, solids } from "./helpers";

test("a moulding routed round the top of a box", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "24,12", "8");
  // The profile: a 2 in chamfer triangle on the right end (x = 24) at the top front
  // corner, drawn with the Line tool from edge to edge looking straight at that end. Each
  // click lands just inside the face so the line is drawn on it.
  await page.getByRole("button", { name: "Right", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await page.keyboard.press("l");
  await clickAt(page, [24, 2, 7.97], "on-edge", 0, 0);
  await clickAt(page, [24, 0.03, 6], "on-edge", 0, 0);
  await expect(vp).toHaveAttribute("data-parts", "7");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  // The path: the top face's edge.
  await page.keyboard.press("Space");
  await selectAt(page, [12, 6, 8], "face");
  await expect(page.locator("#hint")).toContainText("Selected face 2 of solid 1");
  await page.keyboard.press("w");
  const hint = page.locator("#hint");
  await expect(hint).toContainText("closed, 4 segments");
  await page.getByRole("button", { name: "Right", exact: true }).click();
  // Its centre (the snap may show the nearby midpoint; the face under the cursor is what counts).
  await clickAt(page, [24, 2 / 3, 8 - 2 / 3], /on-face|midpoint|on-edge/, 0, 0);
  await expect(hint).toContainText("cut from solid 1");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  const s = await solids(page);
  const inset = 2 / 3;
  const removed = 2 * (2 * (24 - 2 * inset) + 2 * (12 - 2 * inset));
  expect(s[0].volume).toBeCloseTo(24 * 12 * 8 - removed, 4);
  near2(s[0].max, [24, 12, 8]);
});

function near2(a: number[], b: number[]) {
  a.forEach((x, k) => expect(Math.abs(x - b[k])).toBeLessThan(1e-4));
}
