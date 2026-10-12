import { expect, test, type Page } from "@playwright/test";

// Groups, components, tags and scenes (lane 5), driven like a person would: the keyboard,
// the Edit menu, the Outliner and Tags panes and the scene tabs. Solids are read back from
// window.__sawhorse.solids(); the editing context from the HUD trail.
import { at, box, clickAt, point, ready, selectAt, solids, type } from "./helpers";

const hud = (page: Page) => page.locator(".vp .hud");
const hint = (page: Page) => page.locator("#hint");
const row = (page: Page, key: string) => page.locator(`.outline li[data-row="${key}"]`);

async function dblclickAt(page: Page, p: [number, number, number], kind: string) {
  await selectAt(page, p, kind);
  const [x, y] = await at(page, p, 1, -1);
  await page.mouse.dblclick(x, y);
}

test("Ctrl+G groups; a click selects the group; double-click opens it, Esc closes it; undo returns inside", async ({ page }) => {
  await ready(page);
  await box(page);
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Control+g");
  await expect(hint(page)).toContainText("Grouped 1 item");
  await expect(row(page, "n:1")).toBeVisible();
  await expect(row(page, "n:1")).toHaveAttribute("aria-selected", "true");
  // A click on the solid selects the whole group, and the Outliner shows it.
  await page.keyboard.press("Escape");
  await expect(row(page, "n:1")).toHaveAttribute("aria-selected", "false");
  await selectAt(page, [6, 6, 6], "node");
  await expect(row(page, "n:1")).toHaveAttribute("aria-selected", "true");
  // Push/Pull refuses a face of a closed group, with a reason.
  await page.keyboard.press("p");
  await clickAt(page, [6, 6, 6], /on-face|endpoint|midpoint|on-edge/);
  await expect(hint(page)).toContainText("double-click it");
  // Open it, pull its top 3 in.
  await page.keyboard.press("Space");
  await dblclickAt(page, [6, 6, 6], "node");
  await expect(hud(page)).toContainText("Edit: Model › Group");
  await expect(row(page, "n:1")).toHaveClass(/open/);
  await selectAt(page, [6, 6, 6], "face");
  await page.keyboard.press("p");
  await type(page, "3");
  await expect.poll(async () => (await solids(page))[0].max[2]).toBeCloseTo(9, 6);
  // Esc clears the selection, then closes the group.
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await expect(hud(page)).toContainText("Edit: Model ·");
  // Undo returns into the group, where the pull was made.
  await page.keyboard.press("Control+z");
  await expect(hud(page)).toContainText("Edit: Model › Group");
  await expect.poll(async () => (await solids(page))[0].max[2]).toBeCloseTo(6, 6);
  // Ctrl+Shift+G explodes it again (from the top level).
  await page.keyboard.press("Escape");
  await selectAt(page, [6, 6, 6], "node");
  await page.keyboard.press("Control+Shift+g");
  await expect(hint(page)).toContainText("Exploded it");
  await expect(row(page, "n:1")).toHaveCount(0);
});

test("a component's copies are instances: an edit inside one changes them all; Make Unique forks", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "2,10", "20");
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /Make Component/ }).click();
  const name = page.getByRole("dialog", { name: "Make Component" }).getByLabel("Name");
  await name.fill("Side");
  await name.press("Enter");
  await expect(hint(page)).toContainText("Made component Side");
  await expect(row(page, "n:1")).toContainText("Side");
  // Ctrl+Move a copy 30 in along red: an instance of the same component.
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt(page, [0, 0, 0], "endpoint", 1, 1);
  await page.keyboard.press("ArrowRight");
  await point(page, [10, 0, 0]);
  await type(page, "30");
  await expect(hint(page)).toContainText("Copied component Side");
  await expect(vp).toHaveAttribute("data-objects", "2");
  await expect(row(page, "n:1")).toContainText("component · 1 of 2");
  await expect(row(page, "n:2")).toContainText("component · 2 of 2");
  // Open the copy and pull its top up 4 in: both get taller.
  await page.keyboard.press("Space");
  await dblclickAt(page, [31, 5, 20], "node");
  await expect(hud(page)).toContainText("Edit: Model › Side");
  await selectAt(page, [31, 5, 20], "face");
  await page.keyboard.press("p");
  await type(page, "4");
  await expect.poll(async () => (await solids(page)).map((s) => Math.round(s.max[2] * 1000) / 1000)).toEqual([24, 24]);
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await expect(hud(page)).toContainText("Edit: Model ·");
  // Make Unique on the copy (Outliner, right-click), then push it back: only it changes.
  await row(page, "n:2").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Make Unique" }).click();
  await expect(hint(page)).toContainText("Made unique");
  await expect(row(page, "n:2")).toContainText("Side (2)");
  await dblclickAt(page, [31, 5, 24], "node");
  await selectAt(page, [31, 5, 24], "face");
  await page.keyboard.press("p");
  await type(page, "-4");
  await expect.poll(async () => (await solids(page)).map((s) => Math.round(s.max[2] * 1000) / 1000)).toEqual([24, 20]);
  await page.screenshot({ path: test.info().outputPath("components.png") });
});

test("tags hide and show; scenes save the view and the tags; the Outliner's eye and lock", async ({ page }) => {
  const vp = await ready(page);
  await box(page, "12,12", "6");
  await page.keyboard.press("r");
  await clickAt(page, [24, 0, 0], "grid", 1, 1);
  await point(page, [30, 6, 0]);
  await type(page, "6,6");
  await page.keyboard.press("p");
  await type(page, "3");
  await expect(vp).toHaveAttribute("data-objects", "2");
  const both = Number(await vp.getAttribute("data-triangles"));
  // A new tag named Carcass, with the first box on it.
  await page.getByRole("tab", { name: "Tags" }).click();
  await page.getByRole("button", { name: /New tag/ }).click();
  const tagName = page.getByLabel("New name for tag Tag 1");
  await tagName.fill("Carcass");
  await tagName.press("Enter");
  await expect(page.locator('.tree.tags li[data-tag="Carcass"]')).toBeVisible();
  await page.keyboard.press("Space");
  await selectAt(page, [6, 6, 6], "face");
  await page.getByRole("button", { name: "Put the selection on Carcass" }).click();
  await expect(hint(page)).toContainText("1 item on Carcass");
  // A scene with everything shown; hide the tag; a second scene.
  await page.getByRole("button", { name: /Add a scene/ }).click();
  await expect(page.getByRole("tab", { name: "Scene 1" })).toHaveAttribute("aria-selected", "true");
  await page.getByRole("button", { name: "Hide tag Carcass" }).click();
  await expect(vp).not.toHaveAttribute("data-triangles", String(both));
  const hidden = await vp.getAttribute("data-triangles");
  await page.keyboard.press("Shift+z");
  await page.getByRole("button", { name: /Add a scene/ }).click();
  await expect(page.getByRole("tab", { name: "Scene 2" })).toHaveAttribute("aria-selected", "true");
  // Back to scene 1: the tag shows again; scene 2 hides it.
  await page.getByRole("tab", { name: "Scene 1" }).click();
  await expect(vp).toHaveAttribute("data-triangles", String(both));
  await page.getByRole("tab", { name: "Scene 2" }).click();
  await expect(vp).toHaveAttribute("data-triangles", hidden ?? "");
  // Rename a scene by double-clicking its tab.
  await page.getByRole("tab", { name: "Scene 2" }).dblclick();
  const sceneName = page.getByLabel("New name for scene Scene 2");
  await sceneName.fill("Iso");
  await sceneName.press("Enter");
  await expect(page.getByRole("tab", { name: "Iso" })).toBeVisible();
  // The Outliner's eye hides the second box; its lock stops Move.
  await page.getByRole("tab", { name: "Scene 1" }).click();
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.getByRole("button", { name: "Hide Solid 2" }).click();
  await expect(vp).not.toHaveAttribute("data-triangles", String(both));
  await page.getByRole("button", { name: "Show Solid 2" }).click();
  await expect(vp).toHaveAttribute("data-triangles", String(both));
  await page.getByRole("button", { name: "Lock Solid 1" }).click();
  await page.keyboard.press("m");
  await clickAt(page, [12, 0, 6], "endpoint");
  await point(page, [20, 0, 6]);
  await type(page, "6");
  await expect(hint(page)).toContainText("locked");
  await page.screenshot({ path: test.info().outputPath("tags-scenes.png") });
});
