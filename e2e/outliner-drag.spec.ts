import { expect, test, type Page } from "@playwright/test";

// #117: drag rows in the Outliner onto a group to move them into it, or onto the strip
// below the rows to move them out to the model. Read back from window.__sawhorse.graph().
import { box, clickAt, point, ready, type } from "./helpers";

const hud = (page: Page) => page.locator(".vp .hud");
const hint = (page: Page) => page.locator("#hint");
const row = (page: Page, key: string) => page.locator(`.outline li[data-row="${key}"]`);

type Graph = { context: number | null; nodes: { id: number; parent: number | null }[]; objects: { id: number; parent: number | null }[] };
const graph = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { graph(): Graph } }).__sawhorse.graph());
const parentOf = async (page: Page, object: number) => (await graph(page)).objects.find((o) => o.id === object)?.parent ?? null;

/** Every edit the page has queued has landed, twice over with a pause between, so a step a
 * drag would start after another (open the rows' parent, then move) has landed too. */
async function settled(page: Page) {
  await page.evaluate(async () => {
    const w = window as unknown as { __sawhorse: { cutList(): Promise<unknown> } };
    await w.__sawhorse.cutList();
    await new Promise((r) => setTimeout(r, 250));
    await w.__sawhorse.cutList();
  });
}

/** The undo depth the viewport's HUD shows. */
const undoDepth = async (page: Page) => Number((await hud(page).textContent())?.match(/undo (\d+)/)?.[1]);

/** Press on `from`, move a few steps onto `to`, release. */
async function drag(page: Page, from: ReturnType<Page["locator"]>, to: ReturnType<Page["locator"]>) {
  const a = (await from.boundingBox())!;
  const b = (await to.boundingBox())!;
  await page.mouse.move(a.x + 20, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(a.x + 20, a.y + a.height / 2 + 8, { steps: 3 });
  await page.mouse.move(b.x + 20, b.y + b.height / 2, { steps: 6 });
  await page.mouse.up();
}

/** Four 12 x 12 x 6 in boxes 30 in apart along red (solids 1 to 4; Ctrl+A then a Ctrl copy,
 * twice), and solid 1 in a group (node 1). */
async function threeBoxesOneGrouped(page: Page) {
  const vp = await ready(page);
  await box(page);
  for (const [by, n] of [["30", "2"], ["60", "4"]]) {
    await page.keyboard.press("Space");
    await page.keyboard.press("Control+a");
    await page.keyboard.press("m");
    await page.keyboard.press("Control");
    await clickAt(page, [0, 0, 0], "endpoint", 1, 1);
    await page.keyboard.press("ArrowRight");
    await point(page, [10, 0, 0]);
    await type(page, by);
    await expect(vp).toHaveAttribute("data-objects", n);
  }
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await page.getByRole("tab", { name: "Outliner" }).click();
  await row(page, "o:1").click({ button: "right" });
  await page.getByRole("menuitem", { name: /Make Group/ }).click();
  await expect(hint(page)).toContainText("Grouped 1 item");
  await expect(row(page, "n:1")).toBeVisible();
  return vp;
}

test("a solid dragged onto a group goes into it; dragged to the strip below, it comes out (#117)", async ({ page }) => {
  await threeBoxesOneGrouped(page);
  const before = await page.evaluate(() => (window as unknown as { __sawhorse: { solids(): { min: number[] }[] } }).__sawhorse.solids().map((s) => s.min));
  await drag(page, row(page, "o:2"), row(page, "n:1"));
  await expect(hint(page)).toContainText("Moved 1 item into");
  await expect.poll(() => parentOf(page, 2)).toBe(1);
  // Nothing moved in the model, and the open context is the model again.
  const after = await page.evaluate(() => (window as unknown as { __sawhorse: { solids(): { min: number[] }[] } }).__sawhorse.solids().map((s) => s.min));
  expect(after).toEqual(before);
  await expect(hud(page)).toContainText("Edit: Model ·");
  // Undo takes it back out.
  await page.keyboard.press("Control+z");
  await expect.poll(() => parentOf(page, 2)).toBe(null);
  await page.keyboard.press("Control+y");
  await expect.poll(() => parentOf(page, 2)).toBe(1);
  // Out again to the model: unfold the group and drag the row to the strip below.
  await page.getByRole("button", { name: /Unfold/ }).first().click();
  await expect(row(page, "o:2")).toBeVisible();
  const a = (await row(page, "o:2").boundingBox())!;
  await page.mouse.move(a.x + 20, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(a.x + 20, a.y + a.height / 2 + 8, { steps: 3 });
  const strip = page.getByTestId("outliner-drop-model");
  await expect(strip).toBeVisible();
  const s = (await strip.boundingBox())!;
  await page.mouse.move(s.x + 20, s.y + s.height / 2, { steps: 6 });
  await page.mouse.up();
  await expect(hint(page)).toContainText("Moved 1 item into the model");
  await expect.poll(() => parentOf(page, 2)).toBe(null);
  await expect(hud(page)).toContainText("Edit: Model ·");
});

test("the last solid out of a group: the group goes, and the status line names it (#117)", async ({ page }) => {
  await threeBoxesOneGrouped(page);
  await page.getByRole("button", { name: /Unfold/ }).first().click();
  const a = (await row(page, "o:1").boundingBox())!;
  await page.mouse.move(a.x + 20, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(a.x + 20, a.y + a.height / 2 + 8, { steps: 3 });
  const s = (await page.getByTestId("outliner-drop-model").boundingBox())!;
  await page.mouse.move(s.x + 20, s.y + s.height / 2, { steps: 6 });
  await page.mouse.up();
  await expect(hint(page)).toContainText("'Group' was left empty and removed, with any section plane in it.");
  await expect(row(page, "n:1")).toHaveCount(0);
  await expect.poll(() => parentOf(page, 1)).toBe(null);
});

test("the selected rows move together; a drop beside rows in their own group moves nothing (#117)", async ({ page }) => {
  const vp = await threeBoxesOneGrouped(page);
  await row(page, "o:2").click();
  await row(page, "o:3").click({ modifiers: ["Control"] });
  await expect(row(page, "o:3")).toHaveAttribute("aria-selected", "true");
  // Dropped on one of their own: beside it, where they are already. Nothing happens.
  const hash = await vp.getAttribute("data-hash");
  const depth = await undoDepth(page);
  await drag(page, row(page, "o:2"), row(page, "o:3"));
  await settled(page);
  await expect(vp).toHaveAttribute("data-hash", hash!);
  expect(await undoDepth(page)).toBe(depth);
  await expect(hint(page)).not.toContainText("Can't do that");
  // A row not selected, dropped on another row in the model: beside it, where it is. Nothing
  // happens, and the kernel is not asked (it would refuse: "that is already there").
  await drag(page, row(page, "o:4"), row(page, "o:2"));
  await settled(page);
  await expect(vp).toHaveAttribute("data-hash", hash!);
  expect(await undoDepth(page)).toBe(depth);
  await expect(hint(page)).not.toContainText("Can't do that");
  expect(await parentOf(page, 4)).toBe(null);
  // Onto the group: both go in, as one step.
  await row(page, "o:2").click();
  await row(page, "o:3").click({ modifiers: ["Control"] });
  await drag(page, row(page, "o:3"), row(page, "n:1"));
  await expect(hint(page)).toContainText("Moved 2 items into");
  await expect.poll(() => parentOf(page, 2)).toBe(1);
  expect(await parentOf(page, 3)).toBe(1);
  await page.keyboard.press("Control+z");
  await expect.poll(() => parentOf(page, 2)).toBe(null);
  expect(await parentOf(page, 3)).toBe(null);
});

test("Escape cancels a drag; a locked row cannot be dragged or take a drop (#117)", async ({ page }) => {
  const vp = await threeBoxesOneGrouped(page);
  const hash = await vp.getAttribute("data-hash");
  const depth = await undoDepth(page);
  // Escape in the middle: nothing moves.
  const a = (await row(page, "o:2").boundingBox())!;
  const b = (await row(page, "n:1").boundingBox())!;
  await page.mouse.move(a.x + 20, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(b.x + 20, b.y + b.height / 2, { steps: 6 });
  await expect(row(page, "n:1")).toHaveClass(/drop/);
  await page.keyboard.press("Escape");
  await page.mouse.up();
  await settled(page);
  await expect(vp).toHaveAttribute("data-hash", hash!);
  expect(await parentOf(page, 2)).toBe(null);
  expect(await undoDepth(page)).toBe(depth);
  // A locked solid's row is not draggable.
  await page.getByRole("button", { name: "Lock Solid 2" }).click();
  await expect(row(page, "o:2")).not.toHaveAttribute("data-draggable", "true");
  await settled(page);
  const locked = await vp.getAttribute("data-hash");
  await drag(page, row(page, "o:2"), row(page, "n:1"));
  await settled(page);
  await expect(vp).toHaveAttribute("data-hash", locked!);
  expect(await parentOf(page, 2)).toBe(null);
  // A locked group takes no drop.
  await page.getByRole("button", { name: "Lock Group" }).click();
  await expect(hint(page)).toContainText("Locked 1 item");
  await settled(page);
  const lockedGroup = await vp.getAttribute("data-hash");
  await drag(page, row(page, "o:3"), row(page, "n:1"));
  await settled(page);
  await expect(vp).toHaveAttribute("data-hash", lockedGroup!);
  expect(await parentOf(page, 3)).toBe(null);
});

test("a drag tells the active tool: Scale's box goes with the selection, a line half drawn starts again (#117)", async ({ page }) => {
  const vp = await threeBoxesOneGrouped(page);
  // Scale on solid 2, its box and grips shown.
  await row(page, "o:2").click();
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  await drag(page, row(page, "o:2"), row(page, "n:1"));
  await expect(hint(page)).toContainText("Moved 1 item into");
  await expect.poll(() => parentOf(page, 2)).toBe(1);
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  const hash = await vp.getAttribute("data-hash");
  await type(page, "2");
  await expect(hint(page)).toContainText("Select something");
  await expect(vp).toHaveAttribute("data-hash", hash!);
  // A line half drawn, then a drag: the next click starts a new line, drawing nothing.
  await page.keyboard.press("l");
  await clickAt(page, [0, -10, 0], /grid|axis|from-point/, 1, 1);
  await expect(hint(page)).not.toContainText("Click to start");
  await drag(page, row(page, "o:3"), row(page, "n:1"));
  await expect.poll(() => parentOf(page, 3)).toBe(1);
  const moved = await vp.getAttribute("data-hash");
  const depth = await undoDepth(page);
  await clickAt(page, [12, -10, 0], /grid|axis|from-point/, 1, 1);
  await expect(hint(page)).not.toContainText("Click to start");
  await settled(page);
  await expect(vp).toHaveAttribute("data-hash", moved!);
  expect(await undoDepth(page)).toBe(depth);
});

test("Esc in a drag leaves a line half drawn alone: the next click draws it (#117)", async ({ page }) => {
  const vp = await threeBoxesOneGrouped(page);
  await page.keyboard.press("l");
  await clickAt(page, [0, -10, 0], /grid|axis|from-point/, 1, 1);
  await expect(hint(page)).not.toContainText("Click to start");
  const a = (await row(page, "o:2").boundingBox())!;
  const b = (await row(page, "n:1").boundingBox())!;
  await page.mouse.move(a.x + 20, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(b.x + 20, b.y + b.height / 2, { steps: 6 });
  await expect(row(page, "n:1")).toHaveClass(/drop/);
  await page.keyboard.press("Escape");
  await page.mouse.up();
  await settled(page);
  expect(await parentOf(page, 2)).toBe(null);
  const hash = await vp.getAttribute("data-hash");
  await clickAt(page, [12, -10, 0], /grid|axis|from-point/, 1, 1);
  // The Esc went to the drag only: the line goes on from its first point and is drawn.
  await expect(vp).not.toHaveAttribute("data-hash", hash!);
});

test("a drag with a group open puts that group back; a locked row among the selected refuses the drop (#117)", async ({ page }) => {
  await threeBoxesOneGrouped(page);
  await row(page, "o:4").click({ button: "right" });
  await page.getByRole("menuitem", { name: /Make Group/ }).click();
  await expect(hint(page)).toContainText("Grouped 1 item");
  await expect(row(page, "n:2")).toBeVisible();
  await row(page, "n:2").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Edit Group / Component" }).click();
  await expect(hud(page)).toContainText("Edit: Model › Group ·");
  // A row of the model dragged onto the other group: the group that was open is open again.
  await drag(page, row(page, "o:3"), row(page, "n:1"));
  await expect(hint(page)).toContainText("Moved 1 item into");
  await expect.poll(() => parentOf(page, 3)).toBe(1);
  await settled(page);
  await expect(hud(page)).toContainText("Edit: Model › Group ·");
  expect((await graph(page)).context).toBe(2);
  await page.keyboard.press("Escape");
  await expect(hud(page)).toContainText("Edit: Model ·");
  // Solid 2 and the group n:2 selected, n:2 locked: dragging solid 2 moves neither.
  await row(page, "n:2").getByRole("button", { name: /^Lock/ }).click();
  await expect(hint(page)).toContainText("Locked 1 item");
  await row(page, "o:2").click();
  await row(page, "n:2").click({ modifiers: ["Control"] });
  await expect(row(page, "n:2")).toHaveAttribute("aria-selected", "true");
  await settled(page);
  const depth = await undoDepth(page);
  await drag(page, row(page, "o:2"), row(page, "n:1"));
  await expect(hint(page)).toContainText("Can't do that. that is locked: unlock it first");
  await settled(page);
  expect(await parentOf(page, 2)).toBe(null);
  expect((await graph(page)).nodes.find((n) => n.id === 2)?.parent).toBe(null);
  expect(await undoDepth(page)).toBe(depth);
});
