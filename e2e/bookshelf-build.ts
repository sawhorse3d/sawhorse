// The user guide's bookshelf built from components through the UI, like a person would
// (lane 7; shared with lane 9's nesting test): Rect, Push/Pull, Move with typed distances,
// Make Component, Ctrl+Move copies and an array. Every part's bounds are checked.
import { expect, test, type Page } from "@playwright/test";
import { at, clickAt, point, selectAt, solids, type, type V3 } from "./helpers";

const requireKernel = !!process.env.REQUIRE_KERNEL;
export const hint = (page: Page) => page.locator("#hint");

/** The app with the kernel loaded; io=basic, so saving is a download Playwright can catch. */
export async function ready(page: Page) {
  // io=basic: saving is a download Playwright can catch (no native file picker).
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

async function dblAt(page: Page, p: V3, kind: string) {
  await selectAt(page, p, kind);
  const [x, y] = await at(page, p, 1, -1);
  await page.mouse.dblclick(x, y);
}

/** Zoom in toward a point (the wheel zooms toward the cursor), so thin faces are easy to hit. */
async function zoomTo(page: Page, p: V3) {
  await page.keyboard.press("Shift+Z");
  const [x, y] = await at(page, p);
  await page.mouse.move(x, y);
  for (let k = 0; k < 4; k++) await page.mouse.wheel(0, -240);
  await page.waitForTimeout(100);
}

export async function escapeAll(page: Page) {
  await page.keyboard.press("Space");
  for (let k = 0; k < 4; k++) await page.keyboard.press("Escape");
}

export async function component(page: Page, name: string) {
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /Make Component/ }).click();
  const field = page.getByRole("dialog", { name: "Make Component" }).getByLabel("Name");
  await field.fill(name);
  await field.press("Enter");
  await expect(hint(page)).toContainText(`Made component '${name}'`);
}

/** A board at the origin: a typed rectangle on the ground, pulled up. */
async function board(page: Page, size: string, h: string) {
  await escapeAll(page);
  await page.keyboard.press("r");
  await type(page, size);
  await expect(hint(page)).toContainText("on the ground");
  await page.keyboard.press("p");
  await type(page, h);
  await expect(hint(page)).toContainText(`Extruded ${h} in`);
  await page.keyboard.press("Shift+Z");
}

/** Select the solid with a face through `p` (double-click), then move it by a typed
 * distance along an arrow-locked axis, the cursor showing the way. */
async function nudge(page: Page, p: V3, arrow: "ArrowRight" | "ArrowLeft" | "ArrowUp", toward: V3, dist: string) {
  await escapeAll(page);
  await zoomTo(page, p);
  await dblAt(page, p, "face");
  await expect(hint(page)).toContainText("Selected solid");
  await page.keyboard.press("m");
  // Any snap will do: the move is the typed distance along the locked axis.
  await clickAt(page, p, /.+/, 0, 0);
  await page.keyboard.press(arrow);
  await point(page, toward);
  await type(page, dist);
  await expect(hint(page)).toContainText("Moved solid");
}

/**
 * The bookshelf of the user guide, as components: Side x2 (3/4 x 11 1/4 x 71 1/4), Top
 * (3/4 x 11 1/4 x 36) on the sides, Toe kick (3/4 x 3 1/2 x 34 1/2) 2 in back, Shelf x4
 * (3/4 x 11 x 34 1/2) from 3 1/2 in up every 18 in, and a 1/4 in Back panel behind the
 * shelves (built first, while there is room at the origin).
 */
export async function buildBookshelf(page: Page) {
  const vp = page.locator("main.vp");
  // Back panel first, while the model is empty: a 34 1/2 x 71 1/4 slab 1/4 in thick, turned
  // upright about the red axis, then moved behind where the shelves go (3/4 in along red,
  // to 11 in back along green).
  await board(page, "34 1/2, 71 1/4", "1/4");
  await escapeAll(page);
  await zoomTo(page, [17, 35, 0.25]);
  await dblAt(page, [17, 35, 0.25], "face");
  await page.keyboard.press("q");
  await page.keyboard.press("ArrowRight");
  await clickAt(page, [0, 0, 0], "endpoint", -1, 1);
  await clickAt(page, [0, 30, 0], /on-edge|midpoint|endpoint/, -1, 1);
  await type(page, "90");
  await expect(hint(page)).toContainText("Turned solid 1 90°");
  let slab = (await solids(page))[0];
  if (slab.max[2] < 1) {
    await type(page, "-90");
    await expect.poll(async () => (await solids(page))[0].max[2]).toBeGreaterThan(70);
    slab = (await solids(page))[0];
  }
  const y0 = slab.min[1];
  await page.keyboard.press("Shift+Z");
  await nudge(page, [17, y0, 35], "ArrowRight", [25, y0, 35], "3/4");
  await nudge(page, [17.75, y0, 35], "ArrowLeft", [17.75, y0 + 6, 35], `${11 - y0}`);
  await escapeAll(page);
  await zoomTo(page, [17.75, 11, 35]);
  await dblAt(page, [17.75, 11, 35], "face");
  await component(page, "Back panel");
  // Sides.
  await board(page, "3/4, 11 1/4", "71 1/4");
  await escapeAll(page);
  await page.keyboard.press("Shift+Z");
  await dblAt(page, [0, 5.6, 35], "face");
  await component(page, "Side");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt(page, [0, 0, 0], "endpoint", 1, -1);
  await page.keyboard.press("ArrowRight");
  await point(page, [10, 0, 0]);
  await type(page, "35 1/4");
  await expect(hint(page)).toContainText("Copied component 'Side'");
  await page.keyboard.press("Shift+Z");
  // Top: drawn at the origin, lifted onto the sides.
  await board(page, "36, 11 1/4", "3/4");
  await nudge(page, [18, 6, 0.75], "ArrowUp", [18, 6, 20], "71 1/4");
  await escapeAll(page);
  await zoomTo(page, [18, 6, 72]);
  await dblAt(page, [18, 6, 72], "face");
  await component(page, "Top");
  // Toe kick: 3/4 in along red, 2 in back along green.
  await board(page, "34 1/2, 3/4", "3 1/2");
  await nudge(page, [18, 0, 1.75], "ArrowRight", [24, 0, 1.75], "3/4");
  await nudge(page, [18.75, 0, 1.75], "ArrowLeft", [18.75, 6, 1.75], "2");
  await escapeAll(page);
  await zoomTo(page, [18.75, 2, 1.75]);
  await dblAt(page, [18.75, 2, 1.75], "face");
  await component(page, "Toe kick");
  // Shelves: the bottom one on the toe kick, three more every 18 in.
  await board(page, "34 1/2, 11", "3/4");
  // (Points well back on the shelf: the toe kick stands in front of its front half.)
  await nudge(page, [18, 10, 0.75], "ArrowRight", [24, 10, 0.75], "3/4");
  await nudge(page, [18.75, 10, 0.75], "ArrowUp", [18.75, 10, 10], "3 1/2");
  await escapeAll(page);
  await zoomTo(page, [18.75, 10, 4.25]);
  await dblAt(page, [18.75, 10, 4.25], "face");
  await component(page, "Shelf");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt(page, [35.25, 0, 4.25], "endpoint", -1, -1);
  await page.keyboard.press("ArrowUp");
  await point(page, [35.25, 0, 14]);
  await type(page, "18");
  await expect(hint(page)).toContainText("Copied component 'Shelf'");
  await type(page, "x3");
  await expect(hint(page)).toContainText("Array: 3 copies of component 'Shelf'");
  await escapeAll(page);
  await page.keyboard.press("Shift+Z");
  await expect(vp).toHaveAttribute("data-objects", "9");
  // Every part where it belongs.
  const boxes: [string, V3, V3][] = [
    ["left side", [0, 0, 0], [0.75, 11.25, 71.25]],
    ["right side", [35.25, 0, 0], [36, 11.25, 71.25]],
    ["top", [0, 0, 71.25], [36, 11.25, 72]],
    ["toe kick", [0.75, 2, 0], [35.25, 2.75, 3.5]],
    ...[3.5, 21.5, 39.5, 57.5].map((z, i): [string, V3, V3] => [`shelf ${i + 1}`, [0.75, 0, z], [35.25, 11, z + 0.75]]),
    ["back panel", [0.75, 11, 0], [35.25, 11.25, 71.25]],
  ];
  const got = await solids(page);
  const near = (a: V3, b: V3) => a.every((x, k) => Math.abs(x - b[k]) < 1e-4);
  for (const [name, lo, hi] of boxes) {
    expect(
      got.some((s) => near(s.min, lo) && near(s.max, hi)),
      `${name}: nothing spans ${JSON.stringify(lo)} to ${JSON.stringify(hi)}; ${JSON.stringify(got.map((s) => [s.min, s.max]))}`,
    ).toBe(true);
  }
}
