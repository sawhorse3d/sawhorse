import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { at, hover } from "./helpers";
import { goldenShz, openShz } from "./shz";

// Section planes (lane 12) through the browser build: the bookshelf opened as a .shz, cut
// down the middle with the Section Plane tool (locked square to red), the cut filled and
// outlined while the left half is gone, reversed from the Outliner, the Styles pane's
// switches, a scene bringing the active plane back, Create Group from Slice, the slice
// exported as SVG, and the plane deleted.

type SectionsState = { planes: number; active: number; clipped: number; caps: number; outline: number; fill: boolean };
type W = {
  __sawhorse: {
    sections(): SectionsState;
    graph(): { sections?: { id: number; name: string; active: boolean; normal: [number, number, number] }[] };
    pixel(x: number, y: number): [number, number, number];
    worldToScreen(p: [number, number, number]): [number, number];
  };
};
const sections = (page: Page) => page.evaluate(() => (window as unknown as W).__sawhorse.sections());
const planes = (page: Page) => page.evaluate(() => (window as unknown as W).__sawhorse.graph().sections ?? []);
const hint = (page: Page) => page.locator("#hint");

/** The canvas pixel showing world point `p` (inches). */
async function pixelAt(page: Page, p: [number, number, number]): Promise<[number, number, number]> {
  return page.evaluate((m) => {
    const s = (window as unknown as W).__sawhorse;
    const [x, y] = s.worldToScreen(m);
    return s.pixel(x, y);
  }, p.map((v) => v * 0.0254) as [number, number, number]);
}

test("cut the bookshelf with a section plane, reverse it, switch the style, keep it in a scene, slice and export it", async ({ page }) => {
  test.setTimeout(120_000);
  // The cut list's bookshelf, without its hidden shelf and mirrored side copy.
  const shz = await goldenShz("cutlist", [], (o) => o.op !== "set_hidden" && !(o.op === "copy" && (o as unknown as { xform: { linear: number[][] } }).xform.linear[0][0] < 0));
  await openShz(page, shz, 9, "bookshelf.shz");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  // The left side's outer face, before the cut.
  const left: [number, number, number] = [0, 5.6, 40];
  const before = await pixelAt(page, left);

  // Tools > Section Plane: hover the top shelf, lock square to red, click.
  await page.getByRole("button", { name: "Tools" }).click();
  await page.getByRole("menuitemcheckbox", { name: "Section Plane" }).click();
  await expect(page.locator('.tool[data-tool="section"]')).toHaveAttribute("aria-pressed", "true");
  await hover(page, [18, 5.5, 58.25], "on-face");
  await page.keyboard.press("ArrowRight");
  await expect(hint(page)).toContainText("locked square to red");
  const [x, y] = await at(page, [18, 5.5, 58.25], 1, -1);
  await page.mouse.click(x, y);
  await expect(hint(page)).toContainText("Section plane placed");
  await expect.poll(async () => (await sections(page)).active).toBe(1);
  const s = await sections(page);
  // Every solid is under the model's plane; the shelves, top, toe kick and back are cut.
  expect(s.clipped).toBe(9);
  expect(s.caps).toBeGreaterThanOrEqual(14);
  expect(s.outline).toBe(28);
  const [p] = await planes(page);
  expect(p.normal[0]).toBeCloseTo(1, 9);
  // The left side is cut away: the background shows where it was.
  const after = await pixelAt(page, left);
  expect(Math.abs(after[0] - before[0]) + Math.abs(after[1] - before[1]) + Math.abs(after[2] - before[2])).toBeGreaterThan(20);
  await expect(page.getByTestId("outliner").locator('[data-row="s:1"]')).toContainText("active");

  // Reverse from the Outliner: the left half comes back, the right goes.
  await page.keyboard.press(" ");
  await page.getByTestId("outliner").locator('[data-row="s:1"]').click({ button: "right" });
  await page.getByRole("menuitem", { name: "Reverse" }).click();
  await expect.poll(async () => (await planes(page))[0].normal[0]).toBeLessThan(-0.99);
  await expect.poll(async () => {
    const q = await pixelAt(page, left);
    return Math.abs(q[0] - before[0]) + Math.abs(q[1] - before[1]) + Math.abs(q[2] - before[2]);
  }).toBeLessThan(12);

  // The Styles pane: fill off, cuts off and on again.
  await page.getByRole("tab", { name: "Styles" }).click();
  const style = page.getByTestId("section-style");
  // (The boxes follow the document: each click is an edit, so poll for its effect.)
  const box = (name: string) => style.getByRole("checkbox", { name });
  await box("Section fill").click();
  await expect.poll(async () => (await sections(page)).fill).toBe(false);
  await expect(box("Section fill")).not.toBeChecked();
  await box("Section cuts").click();
  await expect.poll(async () => (await sections(page)).clipped).toBe(0);
  await expect(box("Section fill")).toBeDisabled();
  await box("Section cuts").click();
  await expect(box("Section cuts")).toBeChecked();
  await box("Section fill").click();
  await expect.poll(async () => (await sections(page)).fill).toBe(true);
  await expect.poll(async () => (await sections(page)).clipped).toBe(9);

  // A scene keeps the active plane: deactivate it, then go back to the scene.
  await page.locator(".scenes").getByRole("button", { name: "Add a scene from the current view" }).click();
  await expect(page.locator(".scenes")).toContainText("Scene 1");
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.getByTestId("outliner").locator('[data-row="s:1"]').click({ button: "right" });
  await page.getByRole("menuitem", { name: "Deactivate Cut" }).click();
  await expect.poll(async () => (await sections(page)).active).toBe(0);
  await page.locator(".scenes").getByRole("tab", { name: "Scene 1" }).click();
  await expect.poll(async () => (await sections(page)).active).toBe(1);

  // Create Group from Slice: a group with the cut's faces.
  await page.getByTestId("outliner").locator('[data-row="s:1"]').click({ button: "right" });
  await page.getByRole("menuitem", { name: "Create Group from Slice" }).click();
  await expect(hint(page)).toContainText("Made a group from the slice");
  await expect(page.getByTestId("outliner")).toContainText("Section 1 slice");

  // File > Export > Section slice (SVG): filled caps, 1:1 in millimetres.
  await page.getByTestId("outliner").locator('[data-row="s:1"]').click();
  await page.getByRole("button", { name: "File" }).click();
  await page.getByRole("menuitem", { name: "Export" }).hover();
  const [d] = await Promise.all([page.waitForEvent("download"), page.getByRole("menuitem", { name: "Section slice (SVG)…" }).click()]);
  const svg = readFileSync((await d.path())!, "utf8");
  expect(svg).toContain("Sawhorse section slice (Section 1)");
  expect(svg).toContain('fill-rule="evenodd"');
  expect(svg.match(/Z/g)?.length).toBe(7);

  // Delete it: everything is whole again.
  await page.getByTestId("outliner").locator('[data-row="s:1"]').click();
  await page.keyboard.press("Delete");
  await expect.poll(async () => (await sections(page)).planes).toBe(0);
  await expect.poll(async () => (await sections(page)).clipped).toBe(0);
});
