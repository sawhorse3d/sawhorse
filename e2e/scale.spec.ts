import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";

// Scale (lane 4): grips on the selection's bounding box, a face grip with a typed factor,
// an absolute size with a unit, a mirror (-1), and a tapered leg: the top face of a post
// scaled about its centre (Ctrl) to half size, checked against the frustum's volume.
import { at, box, clickAt, near, point, ready, selectAt, solids, type, type V3 } from "./helpers";

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

/** #76: a 24 x 4 x 2 in board stretched to 36 in by its +red face grip, then Ctrl+Z with
 * Scale still the tool. */
async function scaledThenUndone(page: Page) {
  await box(page, "24,4", "2");
  await selectSolid(page, [12, 2, 2]);
  await page.keyboard.press("s");
  const g = await grip(page, [24, 2, 1]);
  await page.mouse.click(g.x, g.y);
  await page.mouse.move(g.x + 40, g.y, { steps: 3 });
  await type(page, "1.5");
  await expect(page.locator("#hint")).toContainText("×1.5 along red");
  near((await solids(page))[0].max, [36, 4, 2]);
  // The grips are on the scaled solid.
  await grip(page, [36, 2, 1]);
  await page.keyboard.press("Control+z");
  await expect(page.locator("#hint")).toContainText("Undone");
  near((await solids(page))[0].max, [24, 4, 2]);
}

test("after Undo the box and its grips are back round the shape; New leaves none over the empty model (#76)", async ({ page }) => {
  const vp = await ready(page);
  page.on("dialog", (d) => void d.accept());
  await scaledThenUndone(page);
  // The solid is 24 in again, and so is the box with its grips (before, they stayed at
  // 36 in, where nothing was).
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  await grip(page, [24, 2, 1]);
  // File > New with Scale still the tool: no box over the empty model.
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: "New", exact: true }).click();
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
});

test("a value typed after Undo scales the selection and undoes nothing more (#76)", async ({ page }) => {
  const vp = await ready(page);
  await scaledThenUndone(page);
  // About the centre, as when no grip has been picked. Before, the tool began with an Undo of
  // its own, as if the scale were still the last step, and that took the step before it.
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("×2 uniform");
  const s = await solids(page);
  expect(s).toHaveLength(1);
  near(s[0].min, [-12, -2, -1]);
  near(s[0].max, [36, 6, 3]);
  await expect(vp).toHaveAttribute("data-watertight", "true");
});

test("Edit > Restore Unsaved Work with Scale active leaves no box and nothing to redo (#76)", async ({ page }) => {
  // With autosave on: a 24 x 16 x 12 in box is kept, and offered back after a reload.
  await page.goto("/");
  const vp = page.locator("main.vp");
  await expect(vp).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  await box(page, "24,16", "12");
  const kept = () =>
    page.evaluate(
      () =>
        new Promise<number>((resolve) => {
          const req = indexedDB.open("sawhorse", 1);
          req.onupgradeneeded = () => req.result.createObjectStore("autosave");
          req.onsuccess = () => {
            const get = req.result.transaction("autosave", "readonly").objectStore("autosave").get("current-meta");
            get.onsuccess = () => resolve((get.result?.steps as number | undefined) ?? 0);
            get.onerror = () => resolve(0);
          };
          req.onerror = () => resolve(0);
        }),
    );
  await expect.poll(kept).toBe(3);
  await page.reload();
  await expect(vp).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  await expect(page.locator("#hint")).toContainText("Restore unsaved work?");
  // Meanwhile a board is drawn and stretched to 36 in, and Scale stays the tool.
  await box(page, "24,4", "2");
  await selectSolid(page, [12, 2, 2]);
  await page.keyboard.press("s");
  const g = await grip(page, [24, 2, 1]);
  await page.mouse.click(g.x, g.y);
  await page.mouse.move(g.x + 40, g.y, { steps: 3 });
  await type(page, "1.5");
  await expect(page.locator("#hint")).toContainText("×1.5 along red");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: "Restore Unsaved Work" }).click();
  await expect(page.locator("#hint")).toContainText("Restored your unsaved work");
  // Nothing is selected in the restored model, so no box (before, the board's stayed).
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  // Before, the tool began with an Undo of its own, which took the restored box's Push/Pull.
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("Select something");
  const s = await solids(page);
  expect(s).toHaveLength(1);
  near(s[0].max, [24, 16, 12]);
});

/** A 24 x 4 x 2 in board selected, with Scale the tool and its 26 grips shown. */
async function scaleOnBoard(page: Page) {
  await box(page, "24,4", "2");
  await selectSolid(page, [12, 2, 2]);
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
}

test("Delete clears the box and grips of the solid it removes (#85)", async ({ page }) => {
  const vp = await ready(page);
  await scaleOnBoard(page);
  const [x, y] = await at(page, [24, 2, 1]);
  await page.keyboard.press("Delete");
  await expect(page.locator("#hint")).toContainText("Erased 1 item");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  // The issue's last step: a press, a move and a click where the right end's grip was. Before,
  // the grip lit up and the click was refused with "that solid no longer exists"; now there is
  // no grip there, and the press gets the words a press off a grip always gets.
  const hash = await vp.getAttribute("data-hash");
  await page.mouse.move(x, y, { steps: 2 });
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  await page.mouse.down();
  await page.mouse.up();
  await page.mouse.move(x + 40, y, { steps: 3 });
  await page.mouse.click(x + 40, y);
  await expect(page.locator("#hint")).toContainText("Press on a grip");
  await expect(page.locator("#hint")).not.toContainText("no longer exists");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-hash", hash!);
  await expect(vp).toHaveAttribute("data-objects", "0");
});

test("Edit > Delete clears the box too, and Undo brings the board back unselected, its box on hover (#85)", async ({ page }) => {
  const vp = await ready(page);
  await scaleOnBoard(page);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /^Delete( Delete)?$/ }).click();
  await expect(page.locator("#hint")).toContainText("Erased 1 item");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  // Undo with Scale still the tool: the board is back, not selected (Delete had emptied the
  // selection), so no box until the cursor is over it, as with nothing selected.
  await page.keyboard.press("Control+z");
  await expect(page.locator("#hint")).toContainText("Undone");
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  await grip(page, [24, 2, 1]);
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
});

test("a factor typed after Delete does not undo the Delete (#85)", async ({ page }) => {
  const vp = await ready(page);
  await scaleOnBoard(page);
  const g = await grip(page, [24, 2, 1]);
  await page.mouse.click(g.x, g.y);
  await page.mouse.move(g.x + 40, g.y, { steps: 3 });
  await type(page, "1.5");
  await expect(page.locator("#hint")).toContainText("×1.5 along red");
  await page.keyboard.press("Delete");
  await expect(page.locator("#hint")).toContainText("Erased 1 item");
  await expect(vp).toHaveAttribute("data-objects", "0");
  // Before, the tool took this as the last scale typed again: it began with an Undo, which
  // took the Delete back, and scaled the board to 72 in.
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("Select something");
  await expect(vp).toHaveAttribute("data-objects", "0");
  expect(await solids(page)).toHaveLength(0);
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
});

test("Delete in the middle of a scale drops it, and the box's factor goes too (#85)", async ({ page }) => {
  const vp = await ready(page);
  await scaleOnBoard(page);
  const g = await grip(page, [24, 2, 1]);
  await page.mouse.click(g.x, g.y);
  await page.mouse.move(g.x + 60, g.y, { steps: 3 });
  // The live factor is in the measurements box while the grip moves.
  await expect(page.locator("#vcb")).not.toHaveValue("");
  await page.keyboard.press("Delete");
  await expect(page.locator("#hint")).toContainText("Erased 1 item");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  await expect(page.locator("#vcb")).toHaveValue("");
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("Select something");
  expect(await solids(page)).toHaveLength(0);
});

/** The Outliner's row for a key ("o:1" is solid 1). */
const outlinerRow = (page: Page, key: string) => page.getByTestId("outliner").locator(`[data-row="${key}"]`);

test("the Outliner's Delete clears the box and grips of the solid it removes (#111)", async ({ page }) => {
  const vp = await ready(page);
  await scaleOnBoard(page);
  await page.getByRole("tab", { name: "Outliner" }).click();
  await outlinerRow(page, "o:1").click({ button: "right" });
  await page.getByRole("menuitem", { name: /^Delete/ }).click();
  await expect(page.locator("#hint")).toContainText("Deleted 1 item");
  await expect(vp).toHaveAttribute("data-objects", "0");
  // Before, the dashed box and its 26 grips stayed over the empty model.
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("Select something");
  await expect(vp).toHaveAttribute("data-objects", "0");
});

test("Edit > Hide clears the box and grips of the solid it hides (#111)", async ({ page }) => {
  const vp = await ready(page);
  await scaleOnBoard(page);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: "Hide", exact: true }).click();
  await expect(page.locator("#hint")).toContainText("Hid 1 item");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  // The hidden board is not scaled by a typed factor either.
  const hash = await vp.getAttribute("data-hash");
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("Select something");
  await expect(vp).toHaveAttribute("data-hash", hash!);
});

test("the Outliner's eye clears the box and grips of the solid it hides, and Show leaves it unselected (#111)", async ({ page }) => {
  const vp = await ready(page);
  await scaleOnBoard(page);
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.getByRole("button", { name: "Hide Solid 1" }).click();
  await expect(page.locator("#hint")).toContainText("Hid 1 item");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  // Shown again: still not selected, so no box until the cursor is over it.
  await page.getByRole("button", { name: "Show Solid 1" }).click();
  await expect(page.locator("#hint")).toContainText("Showed 1 item");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  await grip(page, [24, 2, 1]);
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
});

/** The board of {@link scaleOnBoard} on a new tag "Carcass", with the Tags pane open. */
async function taggedBoard(page: Page) {
  await box(page, "24,4", "2");
  await page.getByRole("tab", { name: "Tags" }).click();
  await page.getByRole("button", { name: /New tag/ }).click();
  const tagName = page.getByLabel("New name for tag Tag 1");
  await tagName.fill("Carcass");
  await tagName.press("Enter");
  await selectSolid(page, [12, 2, 2]);
  await page.getByRole("button", { name: "Put the selection on Carcass" }).click();
  await expect(page.locator("#hint")).toContainText("on tag 'Carcass'");
}

test("hiding the tag of the solid being scaled clears its box and grips (#111)", async ({ page }) => {
  const vp = await ready(page);
  await taggedBoard(page);
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  await page.getByRole("button", { name: "Hide tag Carcass" }).click();
  await expect(page.locator("#hint")).toContainText("Tag 'Carcass' hidden");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
});

test("putting the solid being scaled on a hidden tag clears its box and grips (#111)", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "24,4", "2");
  await page.getByRole("tab", { name: "Tags" }).click();
  await page.getByRole("button", { name: /New tag/ }).click();
  const tagName = page.getByLabel("New name for tag Tag 1");
  await tagName.fill("Carcass");
  await tagName.press("Enter");
  await page.getByRole("button", { name: "Hide tag Carcass" }).click();
  await selectSolid(page, [12, 2, 2]);
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  await page.getByRole("button", { name: "Put the selection on Carcass" }).click();
  await expect(page.locator("#hint")).toContainText("on tag 'Carcass'");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
});

test("a scene that hides the solid being scaled clears its box and grips (#111)", async ({ page }) => {
  const vp = await ready(page);
  await taggedBoard(page);
  // Scene 1 with the tag hidden, Scene 2 with it shown.
  await page.getByRole("button", { name: "Hide tag Carcass" }).click();
  await page.getByRole("button", { name: /Add a scene/ }).click();
  await page.getByRole("button", { name: "Show tag Carcass" }).click();
  await page.getByRole("button", { name: /Add a scene/ }).click();
  await selectSolid(page, [12, 2, 2]);
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  await page.getByRole("tab", { name: "Scene 1" }).click();
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
});

test("Explode Curve clears the box and grips of the circle it makes plain (#111)", async ({ page }) => {
  const vp = await ready(page);
  await page.keyboard.press("c");
  await clickAt(page, [0, 0, 0], /origin|endpoint|grid/);
  await point(page, [6, 0, 0]);
  await type(page, "6");
  await page.keyboard.press("Space");
  // The middle of the circle's first segment.
  const a = Math.PI / 24;
  await selectAt(page, [6 * Math.cos(a) * Math.cos(a), 6 * Math.cos(a) * Math.sin(a), 0], "curve");
  await expect(page.locator("#hint")).toContainText("Selected a circle");
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).not.toHaveCount(0);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /Explode Curve/ }).click();
  await expect(page.locator("#hint")).toContainText("Exploded the curve");
  await expect(vp).toHaveAttribute("data-selection", "0");
  // Before, the box and its grips stayed round edges no longer selected.
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
});

test("the Outliner's eye hiding the solid whose box shows on hover clears it too (#111)", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "24,4", "2");
  // Nothing selected: Scale's box goes round the solid under the cursor and stays there.
  await page.keyboard.press("Space");
  await page.keyboard.press("s");
  await grip(page, [24, 2, 1]);
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  await expect(vp).toHaveAttribute("data-selection", "0");
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.getByRole("button", { name: "Hide Solid 1" }).click();
  await expect(page.locator("#hint")).toContainText("Hid 1 item");
  // The selection was already empty: what tells the tool is that a solid stopped being drawn.
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
});

test("Make Group, then Explode, with Scale the tool: the box is on what each selects (#111)", async ({ page }) => {
  const vp = await ready(page);
  await scaleOnBoard(page);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /^Make Group/ }).click();
  await expect(page.locator("#hint")).toContainText("Grouped 1 item");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  // Before, the box stayed on the solid now inside the closed group, and a factor typed was
  // refused: "that is inside a group or component that is not open".
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("×2 uniform about its centre");
  let s = await solids(page);
  near(s[0].min, [-12, -2, -1]);
  near(s[0].max, [36, 6, 3]);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /^Explode(?! Curve)/ }).click();
  await expect(page.locator("#hint")).toContainText("Exploded it");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  // Before: "that group or component no longer exists".
  await type(page, "0.5");
  await expect(page.locator("#hint")).toContainText("Scaled solid 1 ×0.5 uniform about its centre");
  s = await solids(page);
  near(s[0].min, [0, 0, 0]);
  near(s[0].max, [24, 4, 2]);
});

test("Make Component with Scale the tool: the box is on the new instance (#111)", async ({ page }) => {
  await ready(page);
  await scaleOnBoard(page);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /^Make Component/ }).click();
  await page.getByRole("dialog", { name: "Make Component" }).getByLabel("Name").press("Enter");
  await expect(page.locator("#hint")).toContainText("Made component 'Component 1'");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("×2 uniform about its centre");
  const s = await solids(page);
  near(s[0].min, [-12, -2, -1]);
  near(s[0].max, [36, 6, 3]);
});

test("Close Group / Component with Scale the tool takes the box with the selection (#111)", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "24,4", "2");
  await selectSolid(page, [12, 2, 2]);
  await page.keyboard.press("Control+g");
  await expect(page.locator("#hint")).toContainText("Grouped 1 item");
  // Open the group and select the board inside it.
  await page.keyboard.press("Space");
  await selectAt(page, [12, 2, 2], "node");
  const [x, y] = await at(page, [12, 2, 2], 1, -1);
  await page.mouse.dblclick(x, y);
  await expect(page.locator("#hint")).toContainText("Editing");
  await selectSolid(page, [12, 2, 2]);
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /^Close Group/ }).click();
  await expect(page.locator("#hint")).toContainText("Closed it. Editing Model");
  await expect(vp).toHaveAttribute("data-selection", "0");
  // Before, the box and its grips stayed round the board in the closed group.
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  const hash = await vp.getAttribute("data-hash");
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("Select something");
  await expect(vp).toHaveAttribute("data-hash", hash!);
});

test("a scene that only moves the camera leaves a line half drawn alone (#111)", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "24,4", "2");
  await page.getByRole("button", { name: /Add a scene/ }).click();
  await expect(page.getByRole("tab", { name: "Scene 1" })).toBeVisible();
  await page.keyboard.press("l");
  await clickAt(page, [0, -10, 0], /grid|axis|from-point/, 1, 1);
  await expect(page.locator("#hint")).not.toContainText("Click to start");
  // The scene tab clicked with the line started: it hides nothing, so the line goes on.
  await page.getByRole("tab", { name: "Scene 1" }).click();
  await expect(page.locator("#hint")).toContainText("Scene 'Scene 1'.");
  const hash = await vp.getAttribute("data-hash");
  await clickAt(page, [12, -10, 0], /grid|axis|from-point/, 1, 1);
  // Before this was narrowed, the click after the scene started a new line and drew nothing.
  await expect(vp).not.toHaveAttribute("data-hash", hash!);
  await page.keyboard.press("Escape");
  // One step: a single Undo takes the line back.
  await page.keyboard.press("Control+z");
  await expect(page.locator("#hint")).toContainText("Undone");
  await expect(vp).toHaveAttribute("data-hash", hash!);
});

test("showing a hidden solid and a tag leaves a move under way alone (#111)", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "24,4", "2");
  // A second solid, hidden, and an empty tag, hidden.
  await page.keyboard.press("r");
  await clickAt(page, [40, 0, 0], /grid|axis|from-point/, 1, 1);
  await type(page, "6,6");
  await page.keyboard.press("p");
  await type(page, "2");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await page.keyboard.press("Space");
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.getByRole("button", { name: "Hide Solid 2" }).click();
  await expect(page.locator("#hint")).toContainText("Hid 1 item");
  await page.getByRole("tab", { name: "Tags" }).click();
  await page.getByRole("button", { name: /New tag/ }).click();
  const tagName = page.getByLabel("New name for tag Tag 1");
  await tagName.fill("Spare");
  await tagName.press("Enter");
  await page.getByRole("button", { name: "Hide tag Spare" }).click();
  await expect(page.locator("#hint")).toContainText("Tag 'Spare' hidden");
  // The board picked up by its corner.
  await selectSolid(page, [12, 2, 2]);
  await page.keyboard.press("m");
  await clickAt(page, [0, 0, 0], /endpoint|origin/, 0, 0);
  await expect(page.locator("#hint")).toContainText("Moving solid 1");
  await page.getByRole("button", { name: "Show tag Spare" }).click();
  await expect(page.locator("#hint")).toContainText("Tag 'Spare' shown");
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.getByRole("button", { name: "Show Solid 2" }).click();
  await expect(page.locator("#hint")).toContainText("Showed 1 item");
  // The move goes on: the next click puts the board down 10 in along green.
  await clickAt(page, [0, -10, 0], /grid|axis|from-point/, 1, 1);
  await expect(page.locator("#hint")).toContainText("Moved solid 1 10 in along −green");
  const s = await solids(page);
  near(s[0].min, [0, -10, 0]);
});

test("after Redo the box and its grips are round the scaled shape again (#76)", async ({ page }) => {
  await ready(page);
  await scaledThenUndone(page);
  await page.keyboard.press("Control+y");
  await expect(page.locator("#hint")).toContainText("Redone");
  near((await solids(page))[0].max, [36, 4, 2]);
  // The box follows the shape out to 36 in again (without the app telling the tool, it
  // stayed at 24 in, round the shape as it was before the Redo).
  await expect(page.locator(".vlabel.grip")).toHaveCount(26);
  await grip(page, [36, 2, 1]);
});

test("File > Close with Scale active leaves no box over the empty model (#76)", async ({ page }) => {
  const vp = await ready(page);
  page.on("dialog", (d) => void d.accept());
  await scaleOnBoard(page);
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: "Close", exact: true }).click();
  await expect(page.locator("#hint")).toContainText("Closed Untitled.");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
});

test("Open with Scale active leaves no box, and what was selected scales nothing in the opened model (#76)", async ({ page }) => {
  // io=basic: Save is a download the test can catch.
  await page.goto("/?io=basic&autosave=0");
  const vp = page.locator("main.vp");
  await expect(vp).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  page.on("dialog", (d) => void d.accept());
  // A 12 x 12 x 6 in box saved. Its solid has the number the board drawn next will have.
  await box(page);
  const [d] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Control+s")]);
  const saved = readFileSync((await d.path())!);
  const number = (await solids(page))[0].number;
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: "New", exact: true }).click();
  await expect(vp).toHaveAttribute("data-objects", "0");
  await scaleOnBoard(page);
  expect((await solids(page))[0].number).toBe(number);
  await page.getByTestId("file-input").setInputFiles({ name: "box.shz", mimeType: "application/vnd.sawhorse.shz", buffer: saved });
  await expect(page).toHaveTitle("box - Sawhorse");
  near((await solids(page))[0].max, [12, 12, 6]);
  // Nothing is selected in the opened model, so no box: not the board's, and not one round
  // the box, which the board's selection would name if it outlived the Open.
  await expect(page.locator(".vlabel.grip")).toHaveCount(0);
  await type(page, "2");
  await expect(page.locator("#hint")).toContainText("Select something");
  const s = await solids(page);
  expect(s).toHaveLength(1);
  near(s[0].min, [0, 0, 0]);
  near(s[0].max, [12, 12, 6]);
});
