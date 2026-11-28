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

test("with a section plane active, edges that Undo removed still leave the selection (#110)", async ({ page }) => {
  // The issue's steps in a model with an active plane at 50 in that cuts nothing: the plane
  // clips everything under it in the view, but an edge that is gone is still gone.
  const shz = await buildShz((apply) => {
    rect(apply, 0, 0, 12 * IN, 12 * IN, 6 * IN);
    rect(apply, 44 * IN, -12 * IN, 2 * IN, 2 * IN, 2 * IN);
    apply({ op: "add_section", origin: [0, 0, 50 * IN], normal: [0, 0, -1], active: true, hidden: true });
  });
  await openShz(page, shz, 2, "plane.shz");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  const vp = page.locator("main.vp");
  const hint = page.locator("#hint");
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
  await page.keyboard.press("Control+z");
  await expect(hint).toContainText("Undone");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await page.keyboard.press("w");
  await expect(hint).not.toContainText("Path:");
});

// Above the box, cutting nothing; and on its top face, as the Section Plane tool puts one when
// that face is clicked (lying along the edge, so cutting it nowhere).
for (const [where, z] of [
  ["above the box", 50],
  ["on the box's top face", 6],
] as const) {
  test(`with a section plane ${where}, half of an edge a line split leaves the selection when the line is undone (#110)`, async ({ page }) => {
    // A line from the middle of the box's top front edge to the middle of its back edge splits
    // the front edge in two. One half and the middle point are selected, then the line is
    // undone: the halves are gone, though both lie along the whole edge that is back.
    const shz = await buildShz((apply) => {
      rect(apply, 0, 0, 12 * IN, 12 * IN, 6 * IN);
      apply({ op: "add_section", origin: [0, 0, z * IN], normal: [0, 0, -1], active: true, hidden: true });
    });
    await openShz(page, shz, 1, "plane.shz");
    await page.getByRole("button", { name: "Iso", exact: true }).click();
    await page.keyboard.press("Shift+Z");
    const vp = page.locator("main.vp");
    const hint = page.locator("#hint");
    await page.keyboard.press("l");
    await clickAt(page, [6, 0, 6], "midpoint", 0, 0);
    await clickAt(page, [6, 12, 6], "midpoint", 0, 0);
    // The line has landed before Esc ends it.
    await expect(vp).toHaveAttribute("data-can-undo", "true");
    await page.keyboard.press("Escape");
    await page.keyboard.press("Space");
    await selectAt(page, [3, 0, 6], "edge", 0, 0);
    await page.keyboard.down("Shift");
    await selectAt(page, [6, 0, 6], "vertex", 0, 0);
    await page.keyboard.up("Shift");
    await expect(vp).toHaveAttribute("data-selection", "2");
    await page.keyboard.press("Control+z");
    await expect(hint).toContainText("Undone");
    await expect(vp).toHaveAttribute("data-selection", "0");
    await page.keyboard.press("w");
    await expect(hint).not.toContainText("Path:");
  });
}

test("a whole edge leaves the selection when Redo puts back the line that splits it (#110)", async ({ page }) => {
  // A line from the middle of the box's top front edge to the middle of its back edge, undone;
  // the whole front edge selected, then the line redone: the edge is two pieces again.
  const vp = await ready(page);
  const hint = page.locator("#hint");
  await box(page);
  const boxed = await vp.getAttribute("data-hash");
  await page.keyboard.press("l");
  await clickAt(page, [6, 0, 6], "midpoint", 0, 0);
  await clickAt(page, [6, 12, 6], "midpoint", 0, 0);
  // The line has landed before Esc ends it.
  await expect(vp).not.toHaveAttribute("data-hash", boxed!);
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+z");
  await expect(hint).toContainText("Undone");
  await expect(vp).toHaveAttribute("data-hash", boxed!);
  await page.keyboard.press("Space");
  await selectAt(page, [3, 0, 6], "edge", 0, 0);
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("Control+y");
  await expect(hint).toContainText("Redone");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await page.keyboard.press("w");
  await expect(hint).not.toContainText("Path:");
});

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
