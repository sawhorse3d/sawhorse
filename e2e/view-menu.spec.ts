import { expect, test, type Page } from "@playwright/test";

// #107: a right-click in the 3D view (a right press let go without moving) opens a menu of
// the menu bar's own commands for what is under it; a right-drag still pans and opens none;
// Escape closes it with nothing changed; a choice is one undo step; a tool part way through a
// step ignores the right-click. Also Edit > Edit Group / Component, disabled on a locked one.
import { at, box, clickAt, hover, ready } from "./helpers";

const hint = (page: Page) => page.locator("#hint");
const menu = (page: Page) => page.getByRole("menu", { name: "View actions" });
const item = (page: Page, name: string | RegExp) => menu(page).getByRole("menuitem", { name });
const hud = (page: Page) => page.locator(".vp .hud");
const undoDepth = async (page: Page) => Number(/undo (\d+)/.exec(await hud(page).innerText())?.[1]);

type Graph = { context: number | null; nodes: { id: number; locked: boolean }[]; objects: { id: number; hidden: boolean }[]; sections?: { id: number; active: boolean }[] };
const graph = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { graph(): Graph } }).__sawhorse.graph());

/** A canvas point with nothing under it: near the top, the middle of the width (the iso view
 * looks down at the box, so above it is sky). */
async function emptySpot(page: Page): Promise<[number, number]> {
  const b = (await page.locator("#cv").boundingBox())!;
  return [b.x + b.width / 2, b.y + 40];
}

/** Right-click world point `p` (inches): press and let go where it went down. */
async function rightClick(page: Page, p: [number, number, number]) {
  const [x, y] = await at(page, p, 1, -1);
  await page.mouse.click(x, y, { button: "right" });
}

test("a right-click on a solid selects it and offers its commands; Hide is one undo step (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await hover(page, [6, 6, 6], "face", 1, -1, "data-hover");
  await rightClick(page, [6, 6, 6]);
  await expect(menu(page)).toBeVisible();
  // Not selected before: selected now, as Select's click would.
  await expect(vp).toHaveAttribute("data-selection", "1");
  for (const name of ["Delete", "Hide", "Lock", /Make Group/, "Make Component…", "Clear Paint"]) await expect(item(page, name)).toBeEnabled();
  await expect(item(page, "Make Unique")).toBeDisabled();
  await expect(item(page, /^Explode Ctrl/)).toBeDisabled();
  await expect(item(page, "Explode Curve")).toBeDisabled();
  await expect(item(page, "Edit Group / Component")).toBeDisabled();
  await expect(item(page, /Make Group/)).toContainText("Ctrl+G");
  const before = await undoDepth(page);
  await item(page, "Hide").click();
  await expect(menu(page)).toHaveCount(0);
  await expect(hint(page)).toContainText("Hid 1 item");
  await expect.poll(async () => (await graph(page)).objects[0].hidden).toBe(true);
  // One step: the undo depth goes up by one, and Ctrl+Z brings it back.
  await expect.poll(() => undoDepth(page)).toBe(before + 1);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await graph(page)).objects[0].hidden).toBe(false);
  await expect.poll(() => undoDepth(page)).toBe(before);
});

test("on empty space: Select All, Select None, Zoom Extents, Undo, Redo; Escape closes it with nothing changed (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  const hash = await vp.getAttribute("data-hash");
  const camera = await vp.getAttribute("data-camera");
  const depth = await undoDepth(page);
  const [x, y] = await emptySpot(page);
  await page.mouse.click(x, y, { button: "right" });
  await expect(menu(page)).toBeVisible();
  const names = await menu(page).getByRole("menuitem").allTextContents();
  expect(names.map((n) => n.replace(/\s+/g, " ").trim())).toEqual(["Select All Ctrl+A", "Select None Esc", "Zoom Extents Shift+Z", expect.stringMatching(/^Undo/), "Redo Ctrl+Y"]);
  await page.keyboard.press("Escape");
  await expect(menu(page)).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-hash", hash!);
  await expect(vp).toHaveAttribute("data-camera", camera!);
  await expect(vp).toHaveAttribute("data-selection", "0");
  expect(await undoDepth(page)).toBe(depth);
  // Select All from it selects the box.
  await page.mouse.click(x, y, { button: "right" });
  await item(page, /Select All/).click();
  await expect(vp).toHaveAttribute("data-selection", "1");
  // A right-click inside the selection keeps it.
  await rightClick(page, [6, 6, 6]);
  await expect(menu(page)).toBeVisible();
  await expect(vp).toHaveAttribute("data-selection", "1");
  // A left press outside closes it, as does the wheel.
  await page.mouse.click(x, y);
  await expect(menu(page)).toHaveCount(0);
});

test("a right-drag still pans and opens no menu (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("Space");
  const camera = await vp.getAttribute("data-camera");
  const [x, y] = await at(page, [6, 6, 6], 1, -1);
  await page.mouse.move(x, y);
  await page.mouse.down({ button: "right" });
  await page.mouse.move(x + 60, y + 30, { steps: 6 });
  await page.mouse.up({ button: "right" });
  await expect(vp).not.toHaveAttribute("data-camera", camera!);
  await expect(menu(page)).toHaveCount(0);
});

test("a tool part way through a step ignores the right-click; between steps the menu opens (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  // The Line tool with its first point down: the right-click does nothing.
  await page.keyboard.press("l");
  const [x0, y0] = await at(page, [0, 0, 0], 1, 1);
  await page.mouse.click(x0, y0);
  const hash = await vp.getAttribute("data-hash");
  await rightClick(page, [6, 6, 6]);
  // Settled: a hover after it, read through the same kernel queue as the right-click's own
  // pick, has come back and been drawn, so a menu the right-click opened would be there.
  await hover(page, [12, 12, 6], "endpoint");
  await expect(menu(page)).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-hash", hash!);
  // Esc drops the line; the Line tool between steps opens the menu.
  await page.keyboard.press("Escape");
  await rightClick(page, [6, 6, 6]);
  await expect(menu(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu(page)).toHaveCount(0);
  // A right-click straight after the click that starts a line, before that click has
  // reached the tool, is ignored too: both clicks in one go, as fast hands make them.
  const [x1, y1] = await at(page, [6, 6, 6], 1, -1);
  await page.evaluate(
    ([a, b]) => {
      HTMLElement.prototype.setPointerCapture = () => undefined;
      HTMLElement.prototype.releasePointerCapture = () => undefined;
      const cv = document.getElementById("cv")!;
      const fire = (type: string, [x, y]: number[], button: number, buttons: number) =>
        cv.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, button, buttons, pointerId: 1, pointerType: "mouse", isPrimary: true }));
      fire("pointerdown", a, 0, 1);
      fire("pointerup", a, 0, 0);
      fire("pointerdown", b, 2, 2);
      fire("pointerup", b, 2, 0);
    },
    [
      [x0, y0],
      [x1, y1],
    ],
  );
  await hover(page, [12, 12, 6], "endpoint");
  await expect(hint(page)).toContainText("Line started");
  await expect(menu(page)).toHaveCount(0);
  // Push/Pull part way through a pull: the right-click does nothing either.
  await page.keyboard.press("Escape");
  await page.keyboard.press("p");
  await expect(hint(page)).toContainText(/^Push/);
  await clickAt(page, [6, 6, 6], /./);
  await expect(hint(page)).toContainText("Pushing");
  await rightClick(page, [6, 6, 6]);
  await hover(page, [12, 12, 6], /./);
  await expect(menu(page)).toHaveCount(0);
  await expect(hint(page)).toContainText("Pushing");
  await expect(vp).toHaveAttribute("data-hash", hash!);
});

test("on a section plane: its three commands; inside an open group, Close Group / Component (#107)", async ({ page }) => {
  await ready(page);
  await box(page);
  // A plane through the box's middle, square to red.
  await page.getByRole("button", { name: "Tools" }).click();
  await page.getByRole("menuitemcheckbox", { name: "Section Plane" }).click();
  await hover(page, [6, 6, 6], "on-face");
  await page.keyboard.press("ArrowRight");
  const [sx, sy] = await at(page, [6, 6, 6], 1, -1);
  await page.mouse.click(sx, sy);
  await expect(hint(page)).toContainText("Section plane placed");
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  // Above the box, where only the plane's rectangle is (it runs 8% past what it cuts).
  await rightClick(page, [6, 6, 6.5]);
  await expect(menu(page)).toBeVisible();
  const names = (await menu(page).getByRole("menuitem").allTextContents()).map((n) => n.trim());
  expect(names.map((n) => n.replace(/\s+/g, " "))).toEqual(["Deactivate Cut", "Reverse Section Plane", "Create Group from Slice", "Delete Delete"]);
  await item(page, "Deactivate Cut").click();
  await expect.poll(async () => (await graph(page)).sections?.[0]?.active).toBe(false);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await graph(page)).sections?.[0]?.active).toBe(true);
  // Delete from it takes the plane away in one step, as Edit > Delete does.
  const depth = await undoDepth(page);
  await rightClick(page, [6, 6, 6.5]);
  await item(page, /^Delete/).click();
  await expect.poll(async () => (await graph(page)).sections?.length ?? 0).toBe(0);
  await expect.poll(() => undoDepth(page)).toBe(depth + 1);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await graph(page)).sections?.length ?? 0).toBe(1);
  // Group the box, open it: the menu ends with Close Group / Component.
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Control+g");
  await expect(hint(page)).toContainText("Grouped");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: "Edit Group / Component" }).click();
  await expect(hud(page)).toContainText("Edit: Model › Group");
  const [x, y] = await emptySpot(page);
  await page.mouse.click(x, y, { button: "right" });
  await expect(item(page, /Close Group \/ Component/)).toBeVisible();
  await item(page, /Close Group \/ Component/).click();
  await expect(hud(page)).toContainText("Edit: Model ·");
  await expect.poll(async () => (await graph(page)).context).toBeNull();
});

test("a right-click on one of two selected solids keeps both selected, and Hide hides both (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  // A copy 30 in along red, then both selected.
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  const [x0, y0] = await at(page, [0, 0, 0], 1, 1);
  await page.mouse.move(x0, y0);
  await page.mouse.click(x0, y0);
  await page.keyboard.press("ArrowRight");
  await page.keyboard.type("30");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await expect(vp).toHaveAttribute("data-selection", "2");
  await rightClick(page, [6, 6, 6]);
  await expect(menu(page)).toBeVisible();
  await expect(vp).toHaveAttribute("data-selection", "2");
  await item(page, "Hide").click();
  await expect(hint(page)).toContainText("Hid 2 items");
  await expect.poll(async () => (await graph(page)).objects.map((o) => o.hidden)).toEqual([true, true]);
});

test("a right-click on a solid outside the open group closes back to it and selects it, as Select's click does (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  // A copy 30 in along red, then the copy grouped and opened.
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  const [x0, y0] = await at(page, [0, 0, 0], 1, 1);
  await page.mouse.move(x0, y0);
  await page.mouse.click(x0, y0);
  await page.keyboard.press("ArrowRight");
  await page.keyboard.type("30");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.locator('.outline li[data-row="o:2"]').click({ button: "right" });
  await page.getByRole("menuitem", { name: /Make Group/ }).click();
  await expect(hint(page)).toContainText("Grouped 1 item");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: "Edit Group / Component" }).click();
  await expect(hud(page)).toContainText("Edit: Model › Group");
  // The first box is outside the open group.
  await rightClick(page, [6, 6, 6]);
  await expect(menu(page)).toBeVisible();
  await expect(hud(page)).toContainText("Edit: Model ·");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(item(page, "Hide")).toBeEnabled();
  await page.keyboard.press("Escape");
});

test("Edit > Edit Group / Component opens the selected group, and is disabled on a locked one (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Control+g");
  await expect(hint(page)).toContainText("Grouped");
  await expect(vp).toHaveAttribute("data-selection", "1");
  const edit = page.getByRole("button", { name: "Edit", exact: true });
  await edit.click();
  await page.getByRole("menuitem", { name: "Lock" }).click();
  await expect.poll(async () => (await graph(page)).nodes[0]?.locked).toBe(true);
  await edit.click();
  await expect(page.getByRole("menuitem", { name: "Edit Group / Component" })).toBeDisabled();
  await page.keyboard.press("Escape");
  // Unlocked: it opens the group, as a double-click does.
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await graph(page)).nodes[0]?.locked).toBe(false);
  await rightClick(page, [6, 6, 6]);
  await item(page, "Edit Group / Component").click();
  await expect(hud(page)).toContainText("Edit: Model › Group");
  // Two groups selected: greyed out, in the Edit menu and in the right-click menu.
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await expect.poll(async () => (await graph(page)).context).toBeNull();
  await page.keyboard.press("Control+a");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  const [x0, y0] = await at(page, [0, 0, 0], 1, 1);
  await page.mouse.move(x0, y0);
  await page.mouse.click(x0, y0);
  await page.keyboard.press("ArrowRight");
  await page.keyboard.type("30");
  await page.keyboard.press("Enter");
  await expect.poll(async () => (await graph(page)).nodes.length).toBe(2);
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await expect(vp).toHaveAttribute("data-selection", "2");
  await edit.click();
  await expect(page.getByRole("menuitem", { name: "Edit Group / Component" })).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu", { name: "Edit" })).toHaveCount(0);
  await page.keyboard.press("Control+a");
  await expect(vp).toHaveAttribute("data-selection", "2");
  await rightClick(page, [6, 6, 6]);
  await expect(menu(page)).toBeVisible();
  await expect(vp).toHaveAttribute("data-selection", "2");
  await expect(item(page, "Edit Group / Component")).toBeDisabled();
});

test("a right-click on a model dimension's number selects it as Select's click does; Delete from the menu is one undo step, and the Delete key closes the menu (#107, #129)", async ({ page }) => {
  const vp = await ready(page);
  await page.getByRole("button", { name: "Top", exact: true }).click();
  await expect(vp).toHaveAttribute("data-view", "top");
  await page.keyboard.press("r");
  await page.keyboard.type("24,12");
  await page.keyboard.press("Enter");
  await expect(hint(page)).toContainText("Rectangle 24 × 12 in");
  // A dimension along the rectangle's front edge, 6 in in front of it, over the bare ground.
  await page.keyboard.press("d");
  await clickAt(page, [0, 0, 0], "endpoint");
  await clickAt(page, [24, 0, 0], "endpoint");
  await clickAt(page, [12, -6, 0], /grid|free|/);
  await expect(hint(page)).toContainText("Dimension 24 in");
  const dims = page.locator(".vp .vlabel.dim");
  await expect(dims).toHaveCount(1);
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  const hash = await vp.getAttribute("data-hash");
  const depth = await undoDepth(page);
  /** Right-click inside the number's box, 6 px or more off the dimension line it sits on,
   * once Select shows the dimension under the pointer there. */
  const onNumber = async () => {
    let xy: [number, number] = [0, 0];
    await expect(async () => {
      const b = (await dims.boundingBox())!;
      const [, ly] = await at(page, [12, -6, 0]);
      xy = [b.x + b.width - 3, b.y + b.height - 2];
      expect(xy[1] - ly).toBeGreaterThanOrEqual(6);
      await page.mouse.move(xy[0] + 3, xy[1] + 2);
      await page.mouse.move(xy[0], xy[1], { steps: 2 });
      await expect(vp).toHaveAttribute("data-hover", "dimension", { timeout: 800 });
    }).toPass({ timeout: 10_000 });
    await page.mouse.click(xy[0], xy[1], { button: "right" });
    await expect(menu(page)).toBeVisible();
  };
  await onNumber();
  // Selected, as Select's click on the number selects it, and offered what the menu bar
  // offers it: Delete. Nothing else on the list acts on a dimension.
  await expect(hint(page)).toContainText("Selected a dimension.");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(item(page, "Delete")).toBeEnabled();
  for (const name of ["Hide", "Lock", /Make Group/, "Make Component…", "Make Unique", /^Explode Ctrl/, "Explode Curve", "Clear Paint", "Edit Group / Component"]) await expect(item(page, name)).toBeDisabled();
  await item(page, "Delete").click();
  await expect(menu(page)).toHaveCount(0);
  await expect(dims).toHaveCount(0);
  await expect.poll(() => undoDepth(page)).toBe(depth + 1);
  await page.keyboard.press("Control+z");
  await expect(dims).toHaveCount(1);
  await expect(vp).toHaveAttribute("data-hash", hash!);
  // The Delete key with the menu open: the dimension goes, and the menu goes with what it
  // was opened on (no menu left over nothing, its items acting on the next selection).
  await onNumber();
  await page.keyboard.press("Delete");
  await expect(dims).toHaveCount(0);
  await expect(menu(page)).toHaveCount(0);
  await expect.poll(() => undoDepth(page)).toBe(depth + 1);
});

test("a left press, a middle drag or the wheel on the view only closes the menu; a right press there opens it again there (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await rightClick(page, [6, 6, 6]);
  await expect(menu(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(vp).toHaveAttribute("data-selection", "1");
  const camera = await vp.getAttribute("data-camera");
  const [x, y] = await emptySpot(page);
  const [gx, gy] = await at(page, [20, -4, 0]);
  /** Settled: a hover read through the kernel after the press has come back (Select names
   * the corner under the pointer, the Line tool the point it would snap to). */
  const settled = (line = false) => (line ? hover(page, [12, 12, 6], "endpoint") : hover(page, [12, 12, 6], "vertex", 1, -1, "data-hover"));
  // A left click on empty space with Select would clear the selection.
  await page.mouse.click(x, y, { button: "right" });
  await expect(menu(page)).toBeVisible();
  await page.mouse.click(gx, gy);
  await expect(menu(page)).toHaveCount(0);
  await settled();
  await expect(vp).toHaveAttribute("data-selection", "1");
  // A middle drag would orbit, the wheel zoom.
  await page.mouse.click(x, y, { button: "right" });
  await expect(menu(page)).toBeVisible();
  await page.mouse.move(gx, gy);
  await page.mouse.down({ button: "middle" });
  await page.mouse.move(gx + 40, gy + 10, { steps: 4 });
  await page.mouse.up({ button: "middle" });
  await expect(menu(page)).toHaveCount(0);
  await page.mouse.click(x, y, { button: "right" });
  await expect(menu(page)).toBeVisible();
  await page.mouse.move(gx, gy);
  await page.mouse.wheel(0, -200);
  await expect(menu(page)).toHaveCount(0);
  await settled();
  await expect(vp).toHaveAttribute("data-camera", camera!);
  // The Line tool between steps: the click that closes the menu starts no line.
  await page.keyboard.press("l");
  await expect(hint(page)).toContainText("Line. Click to start");
  await page.mouse.click(x, y, { button: "right" });
  await expect(menu(page)).toBeVisible();
  await page.mouse.click(gx, gy);
  await expect(menu(page)).toHaveCount(0);
  await settled(true);
  await expect(hint(page)).toContainText("Line. Click to start");
  // A right press elsewhere on the view: the menu opens there, and only one.
  await page.mouse.click(x, y, { button: "right" });
  await expect(menu(page)).toBeVisible();
  const first = (await menu(page).boundingBox())!;
  await page.mouse.click(gx, gy, { button: "right" });
  await expect.poll(async () => (await menu(page).boundingBox())?.y).not.toBe(first.y);
  await expect(menu(page)).toHaveCount(1);
});

test("the menu closes when only the selection, or only the tool, changes under it (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  const hash = await vp.getAttribute("data-hash");
  const [x, y] = await emptySpot(page);
  // Ctrl+A: the selection changes, the model does not.
  await page.mouse.click(x, y, { button: "right" });
  await expect(menu(page)).toBeVisible();
  await page.keyboard.press("Control+a");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(menu(page)).toHaveCount(0);
  // A tool key: the tool changes, the model and the selection do not.
  await page.mouse.click(x, y, { button: "right" });
  await expect(menu(page)).toBeVisible();
  await page.keyboard.press("l");
  await expect(hint(page)).toContainText(/^Line/);
  await expect(menu(page)).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(vp).toHaveAttribute("data-hash", hash!);
});

test("a right-click overtaken by Esc or a right-drag while its pick is on its way opens no menu (#107)", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  const [x, y] = await at(page, [6, 6, 6], 1, -1);
  // In one go, so the right-click's pick (an answer from the kernel worker) is still on its
  // way when Esc, then a right-drag, come: the pick is held back behind them.
  const fire = (steps: "esc" | "drag") =>
    page.evaluate(
      ([x, y, steps]) => {
        HTMLElement.prototype.setPointerCapture = () => undefined;
        HTMLElement.prototype.releasePointerCapture = () => undefined;
        const cv = document.getElementById("cv")!;
        const p = (type: string, px: number, button: number, buttons: number) =>
          cv.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: px, clientY: y, button, buttons, pointerId: 1, pointerType: "mouse", isPrimary: true }));
        p("pointerdown", x, 2, 2);
        p("pointerup", x, 2, 0);
        if (steps === "esc") window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
        else {
          p("pointerdown", x, 2, 2);
          p("pointermove", x + 30, -1, 2);
          p("pointerup", x + 30, 2, 0);
        }
      },
      [x, y, steps] as const,
    );
  for (const steps of ["esc", "drag"] as const) {
    await fire(steps);
    // Settled: a hover read after the pick has come back.
    await hover(page, [12, 12, 6], "vertex", 1, -1, "data-hover");
    await expect(menu(page)).toHaveCount(0);
    await expect(vp).toHaveAttribute("data-selection", "0");
  }
});

test("the menu opens at the page point of the right-click and stays inside the window (#107)", async ({ page }) => {
  await ready(page);
  await box(page);
  await page.keyboard.press("Space");
  // The canvas does not start at the page's top left (the tool rail and the bars are there),
  // so a menu at the canvas pixel would be off by that much.
  const cv = (await page.locator("#cv").boundingBox())!;
  expect(cv.x).toBeGreaterThan(10);
  expect(cv.y).toBeGreaterThan(10);
  const [x, y] = await at(page, [6, 6, 6], 1, -1);
  await page.mouse.click(x, y, { button: "right" });
  await expect(menu(page)).toBeVisible();
  const m = (await menu(page).boundingBox())!;
  expect(Math.abs(m.x - x)).toBeLessThanOrEqual(1);
  expect(Math.abs(m.y - y)).toBeLessThanOrEqual(1);
  await page.keyboard.press("Escape");
  // Near the right and bottom edges of a small window: moved left and up to fit.
  await page.setViewportSize({ width: 800, height: 600 });
  await expect.poll(async () => (await page.locator("#cv").boundingBox())?.width).toBeLessThan(810);
  const c2 = (await page.locator("#cv").boundingBox())!;
  const near: [number, number] = [Math.min(795, c2.x + c2.width - 4), Math.min(560, c2.y + c2.height - 40)];
  await page.mouse.click(near[0], near[1], { button: "right" });
  await expect(menu(page)).toBeVisible();
  const n = (await menu(page).boundingBox())!;
  expect(n.x + n.width).toBeLessThanOrEqual(800);
  expect(n.y + n.height).toBeLessThanOrEqual(600);
  expect(n.x).toBeLessThan(near[0]);
  expect(n.y).toBeLessThan(near[1]);
});
