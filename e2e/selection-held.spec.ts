import { expect, test, type Page } from "@playwright/test";

// #110: after an edit, Undo or Redo the selection keeps what the model still has. These are
// the cases where the selection names something the view mesh does not hold as picked (a
// section plane's cut) or that a step moved (a solid's edge), which must stay selected.
import { box, clickAt, IN, point, ready, selectAt, type } from "./helpers";
import { buildShz, openShz } from "./shz";

function rect(apply: (op: object) => Record<string, unknown>, x: number, y: number, w: number, d: number, h: number) {
  const drawn = apply({ op: "draw", target: { on: "plane", origin: [0, 0, 0], normal: [0, 0, 1] }, points: [[x, y, 0], [x + w, y, 0], [x + w, y + d, 0], [x, y + d, 0]], closed: true });
  return apply({ op: "extrude", sketch: drawn.sketch, region: drawn.inside, distance: h });
}

/** A 12 x 12 x 6 in box and a 6 x 6 x 12 in one at (30, 0), cut at 6 in by an active plane
 * keeping what is below it; the plane itself not drawn, so a click reaches the cut. */
async function sectioned(page: Page) {
  const shz = await buildShz((apply) => {
    rect(apply, 0, 0, 12 * IN, 12 * IN, 6 * IN);
    rect(apply, 30 * IN, 0, 6 * IN, 6 * IN, 12 * IN);
    apply({ op: "add_section", origin: [0, 0, 6 * IN], normal: [0, 0, -1], active: true, hidden: true });
  });
  await openShz(page, shz, 2, "cut.shz");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await page.keyboard.press("Space");
  return page.locator("main.vp");
}

/** An edit that touches nothing selected: the Styles pane's Endpoints switch. */
async function styleEdit(page: Page) {
  const vp = page.locator("main.vp");
  const h = await vp.getAttribute("data-hash");
  await page.getByRole("tab", { name: "Styles" }).click();
  await page.getByRole("checkbox", { name: "Endpoints", exact: true }).click();
  await expect(vp).not.toHaveAttribute("data-hash", h!);
}

for (const [what, p, kind] of [
  ["an edge the plane cuts short", [30, 0, 3], "edge"],
  ["an edge of the cut's outline", [33, 0, 6], "edge"],
  ["a point where the plane cuts an edge", [36, 0, 6], "vertex"],
  ["an edge the plane does not cut", [33, 0, 0], "edge"],
] as const) {
  test(`under a section plane, ${what} stays selected across an edit (#110)`, async ({ page }) => {
    const vp = await sectioned(page);
    await selectAt(page, [...p] as [number, number, number], kind, 0, 0);
    await expect(vp).toHaveAttribute("data-selection", "1");
    await styleEdit(page);
    await expect(vp).toHaveAttribute("data-selection", "1");
    // And Undo of the edit keeps it too.
    await page.keyboard.press("Control+z");
    await expect(page.locator("#hint")).toContainText("Undone");
    await expect(vp).toHaveAttribute("data-selection", "1");
  });
}

test("a solid's edge moved with Move stays selected where it went, and Undo and Redo put it back (#110)", async ({ page }) => {
  const vp = await ready(page);
  const hint = page.locator("#hint");
  await box(page);
  await page.keyboard.press("Space");
  await selectAt(page, [6, 0, 6], "edge", 0, 0);
  await expect(vp).toHaveAttribute("data-selection", "1");
  // The box's top front edge lifted 2 in up blue.
  await page.keyboard.press("m");
  await clickAt(page, [6, 0, 6], /on-edge|midpoint|endpoint/, 0, 0);
  await page.keyboard.press("ArrowUp");
  await point(page, [6, 0, 10]);
  await type(page, "2");
  await expect(hint).toContainText("Moved");
  await expect(vp).toHaveAttribute("data-selection", "1");
  // It is the edge where it went that is selected: Follow Me reads it as a path.
  await page.keyboard.press("Space");
  await page.keyboard.press("w");
  await expect(hint).toContainText("Path: 1 edge");
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+z");
  await expect(hint).toContainText("Undone");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("w");
  await expect(hint).toContainText("Path: 1 edge");
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+y");
  await expect(hint).toContainText("Redone");
  await expect(vp).toHaveAttribute("data-selection", "1");
});
