import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";

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
