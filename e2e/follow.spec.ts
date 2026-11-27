import { expect, test } from "@playwright/test";

// Follow Me (lane 4): a chamfer moulding routed round the top of a box. The profile is a
// corner of the box's end face cut off with the Line tool; the path is the top face's edge
// (select the face, press W, click the profile). Checked against the exact volume: the
// box less the triangle's area times its centroid's path round the top.
import { box, clickAt, near, point, ready, selectAt, solids, type } from "./helpers";

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

test("a path that Undo removed is not swept (#86)", async ({ page }) => {
  const vp = await ready(page);
  const hint = page.locator("#hint");
  // The issue's steps: a 12 x 12 x 6 in box, a 6 x 6 in rectangle on the ground at (30, 0),
  // its region selected as the path, W, then Ctrl+Z, which removes the rectangle.
  await box(page, "12,12", "6");
  await page.keyboard.press("r");
  await clickAt(page, [30, 0, 0], "grid", 1, 1);
  await point(page, [33, 3, 0]);
  await type(page, "6,6");
  await expect(hint).toContainText(/Rectangle 6 \S 6 in on the ground/);
  await page.keyboard.press("Space");
  await selectAt(page, [33, 3, 0], "region");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("w");
  await expect(hint).toContainText("Path: the edge of region 1 (closed, 4 segments)");
  await page.keyboard.press("Control+z");
  await expect(hint).toContainText("Undone");
  await expect(vp).toHaveAttribute("data-can-redo", "true");
  const hash = await vp.getAttribute("data-hash");
  // The box's front face as the profile. Before, it was swept along the edge of the
  // rectangle that Undo had removed, and added to the box.
  await clickAt(page, [6, 0, 3], /on-face|grid|from-point/, 0, 0);
  await expect(hint).toContainText("Select the path first");
  await expect(vp).toHaveAttribute("data-hash", hash!);
  await expect(vp).toHaveAttribute("data-can-redo", "true");
  const s = await solids(page);
  expect(s).toHaveLength(1);
  near(s[0].max, [12, 12, 6]);
});

test("a path that Delete removed is not swept (#85, #86)", async ({ page }) => {
  const vp = await ready(page);
  const hint = page.locator("#hint");
  // The steps of #86 with Delete in place of Ctrl+Z: the region chosen as the path is still
  // the selection, so Delete erases it.
  await box(page, "12,12", "6");
  await page.keyboard.press("r");
  await clickAt(page, [30, 0, 0], "grid", 1, 1);
  await point(page, [33, 3, 0]);
  await type(page, "6,6");
  await expect(hint).toContainText(/Rectangle 6 \S 6 in on the ground/);
  await page.keyboard.press("Space");
  await selectAt(page, [33, 3, 0], "region");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("w");
  await expect(hint).toContainText("Path: the edge of region 1 (closed, 4 segments)");
  await page.keyboard.press("Delete");
  await expect(hint).toContainText("Erased 1 item");
  await expect(vp).toHaveAttribute("data-selection", "0");
  const hash = await vp.getAttribute("data-hash");
  // Before, the box's front face was swept along the erased rectangle's edge and added to
  // the box, which grew to (0, -30, 0)..(66, 36, 6).
  await clickAt(page, [6, 0, 3], /on-face|grid|from-point/, 0, 0);
  await expect(hint).toContainText("Select the path first");
  await expect(vp).toHaveAttribute("data-hash", hash!);
  const s = await solids(page);
  expect(s).toHaveLength(1);
  near(s[0].min, [0, 0, 0]);
  near(s[0].max, [12, 12, 6]);
});

test("edges that Undo removed leave the selection and are not read as a path (#110)", async ({ page }) => {
  const vp = await ready(page);
  const hint = page.locator("#hint");
  // The issue's steps: a 12 x 12 x 6 in box, a 6 x 6 in rectangle on the ground from (30, 0)
  // toward (36, -6), its left and bottom edges selected, W, then Ctrl+Z.
  await box(page, "12,12", "6");
  await page.keyboard.press("r");
  await clickAt(page, [30, 0, 0], "grid", 1, 1);
  await point(page, [36, -6, 0]);
  await type(page, "6,6");
  await expect(hint).toContainText(/Rectangle 6 \S 6 in on the ground/);
  await page.keyboard.press("Space");
  await selectAt(page, [30, -3, 0], "edge", 0, 0);
  await page.keyboard.down("Shift");
  await selectAt(page, [33, -6, 0], "edge", 0, 0);
  await page.keyboard.up("Shift");
  await expect(vp).toHaveAttribute("data-selection", "2");
  await page.keyboard.press("w");
  await expect(hint).toContainText("Path: 2 edges");
  await page.keyboard.press("Control+z");
  await expect(hint).toContainText("Undone");
  // Before, both edges stayed selected though the model no longer had them.
  await expect(vp).toHaveAttribute("data-selection", "0");
  const hash = await vp.getAttribute("data-hash");
  // W again reads the selection: before, "Path: 2 edges (open, 2 segments)", and a click on
  // the box's front face swept it along them, growing the box to (0, -36, 0)..(36, 12, 6).
  await page.keyboard.press("Space");
  await page.keyboard.press("w");
  await expect(hint).not.toContainText("Path:");
  await clickAt(page, [6, 0, 3], /on-face|grid|from-point/, 0, 0);
  await expect(hint).toContainText("Select the path first");
  await expect(vp).toHaveAttribute("data-hash", hash!);
  const s = await solids(page);
  expect(s).toHaveLength(1);
  near(s[0].min, [0, 0, 0]);
  near(s[0].max, [12, 12, 6]);
  // And Follow Me still takes a path chosen now: the box's top face.
  await page.keyboard.press("Space");
  await selectAt(page, [6, 6, 6], "face");
  await page.keyboard.press("w");
  await expect(hint).toContainText("closed, 4 segments");
});

test("an edge the model still has stays selected across Undo and Redo (#110)", async ({ page }) => {
  const vp = await ready(page);
  const hint = page.locator("#hint");
  await box(page, "12,12", "6");
  // An edge and a corner of the box, then an unrelated step: a rectangle on the ground.
  await page.keyboard.press("Space");
  await selectAt(page, [6, 0, 6], "edge", 0, 0);
  await page.keyboard.down("Shift");
  await selectAt(page, [12, 12, 6], "vertex", 0, 0);
  await page.keyboard.up("Shift");
  await expect(vp).toHaveAttribute("data-selection", "2");
  await page.keyboard.press("r");
  await clickAt(page, [30, 0, 0], "grid", 1, 1);
  await point(page, [36, -6, 0]);
  await type(page, "6,6");
  await expect(hint).toContainText(/Rectangle 6 \S 6 in on the ground/);
  await page.keyboard.press("Space");
  await expect(vp).toHaveAttribute("data-selection", "2");
  await page.keyboard.press("Control+z");
  await expect(hint).toContainText("Undone");
  await expect(vp).toHaveAttribute("data-selection", "2");
  await page.keyboard.press("Control+y");
  await expect(hint).toContainText("Redone");
  await expect(vp).toHaveAttribute("data-selection", "2");
  // The kept edge is still a path Follow Me reads, as it is selected: the corner, no path, is
  // taken out with Ctrl (a toggle, which leaves the kept edge as it is).
  await page.keyboard.down("ControlOrMeta");
  await selectAt(page, [12, 12, 6], "vertex", 0, 0);
  await page.keyboard.up("ControlOrMeta");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("w");
  await expect(hint).toContainText("Path: 1 edge");
});

function near2(a: number[], b: number[]) {
  a.forEach((x, k) => expect(Math.abs(x - b[k])).toBeLessThan(1e-4));
}
