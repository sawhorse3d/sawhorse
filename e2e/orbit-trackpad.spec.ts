import { expect, test, type Page } from "@playwright/test";
import { IN, at, box, clickAt, hover, ready, type V3 } from "./helpers";

// Lane 30 (issue #36): a MacBook's trackpad has no middle button, so nothing orbited. The
// Orbit tool (O) takes a plain left drag, Alt (Option) + left drag orbits in every tool but
// Paint and Section Plane, and Alt+Shift pans. The drags are the real mouse through
// Playwright (the left button, the Alt key held), not synthetic events.

type Hooks = { worldToScreen(p: V3): [number, number]; camera(): { eye: V3; target: V3 } };
const screen = (page: Page, p: V3) => page.evaluate((p) => (window as unknown as { __sawhorse: Hooks }).__sawhorse.worldToScreen(p), p);
const eye = async (page: Page) => (await page.evaluate(() => (window as unknown as { __sawhorse: Hooks }).__sawhorse.camera())).eye;
const tool = (page: Page, id: string) => page.locator(`.tool[data-tool='${id}']`);
const viewButtons = (page: Page) => page.getByRole("group", { name: "Views" }).locator("button[aria-pressed='true']");

/**
 * Drag with the left button from the screen position of `probe` (offset by `d`) through
 * `steps` moves of (dx, dy) each, the given keys held, and return the worst distance
 * between `probe` on screen and where it was at the press, after each move.
 */
async function leftDrag(page: Page, probe: V3, step: [number, number], steps: number, opts: { keys?: string[]; hold?: boolean } = {}) {
  const cv = (await page.locator("#cv").boundingBox())!;
  const s0 = await screen(page, probe);
  const at: [number, number] = [s0[0] + 1, s0[1] - 1];
  for (const k of opts.keys ?? []) await page.keyboard.down(k);
  await page.mouse.move(cv.x + at[0], cv.y + at[1]);
  await page.mouse.down();
  let drift = 0;
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(cv.x + at[0] + step[0] * i, cv.y + at[1] + step[1] * i);
    const s = await screen(page, probe);
    drift = Math.max(drift, Math.hypot(s[0] - s0[0], s[1] - s0[1]));
  }
  if (!opts.hold) {
    await page.mouse.up();
    for (const k of [...(opts.keys ?? [])].reverse()) await page.keyboard.up(k);
  }
  return { drift, end: [at[0] + step[0] * steps, at[1] + step[1] * steps] as [number, number] };
}

test("Alt+drag with the left button orbits about the corner under the cursor, leaves the standard view, and selects nothing", async ({ page }) => {
  const vp = await ready(page);
  await box(page); // 12 x 12 x 6 in at the origin
  await page.keyboard.press("Space");
  await expect(tool(page, "select")).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await expect(vp).toHaveAttribute("data-view", "iso");
  await expect(page.getByRole("button", { name: "Iso", exact: true })).toHaveAttribute("aria-pressed", "true");
  const hash = await vp.getAttribute("data-hash");
  const before = await eye(page);

  // Alt held, the left button down on the box's top corner, 160 px across and 80 down.
  const corner: V3 = [12 * IN, 0, 6 * IN];
  const held = await leftDrag(page, corner, [8, 4], 20, { keys: ["Alt"], hold: true });
  // The pivot is the corner: pinned under the cursor's press (the cursor is 1 px off it).
  expect(held.drift).toBeLessThan(1);
  await expect(vp).toHaveAttribute("data-nav", "orbit");
  await expect(page.getByTestId("orbit-pivot")).toBeVisible();
  await page.mouse.up();
  await page.keyboard.up("Alt");
  await expect(vp).toHaveAttribute("data-nav", "");
  await expect(page.getByTestId("orbit-pivot")).toHaveCount(0);
  // It turned the view: the standard view is left and no view button stays lit.
  await expect(vp).toHaveAttribute("data-view", "custom");
  await expect(viewButtons(page)).toHaveCount(0);
  const after = await eye(page);
  expect(Math.hypot(after[0] - before[0], after[1] - before[1], after[2] - before[2])).toBeGreaterThan(0.05);
  // Select never saw the press: no window box, nothing selected, the model unchanged, Select
  // still the tool.
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".selbox")).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-hash", hash!);
  await expect(tool(page, "select")).toHaveAttribute("aria-pressed", "true");

  // Alt+Shift+drag pans: the corner grabbed stays under the cursor.
  const pan = await leftDrag(page, corner, [-10, 6], 10, { keys: ["Alt", "Shift"], hold: true });
  await expect(vp).toHaveAttribute("data-nav", "pan");
  const s = await screen(page, corner);
  expect(Math.hypot(s[0] - pan.end[0], s[1] - pan.end[1])).toBeLessThan(1.5);
  await page.mouse.up();
  await page.keyboard.up("Shift");
  await page.keyboard.up("Alt");
  await expect(vp).toHaveAttribute("data-selection", "0");

  // In Move, holding Alt for the orbit does not switch autofold (an Alt tap still does).
  await page.keyboard.press("m");
  await expect(tool(page, "move")).toHaveAttribute("aria-pressed", "true");
  await leftDrag(page, corner, [6, 0], 10, { keys: ["Alt"] });
  await expect(page.locator("#hint")).not.toContainText("Autofold");
  await page.keyboard.press("Alt");
  await expect(page.locator("#hint")).toContainText("Autofold on");
});

test("the O tool: a plain left drag orbits, Shift+drag pans, Esc goes back to the tool before; H pans", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("l");
  await expect(tool(page, "line")).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("o");
  await expect(tool(page, "orbit")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#hint")).toContainText("Drag to orbit around the point under the cursor.");
  await page.getByRole("button", { name: "Front", exact: true }).click();
  await expect(vp).toHaveAttribute("data-view", "front");
  const hash = await vp.getAttribute("data-hash");

  // A plain drag orbits about the front top corner.
  const corner: V3 = [12 * IN, 0, 6 * IN];
  const orbit = await leftDrag(page, corner, [0, 6], 20, { hold: true });
  expect(orbit.drift).toBeLessThan(1);
  await expect(vp).toHaveAttribute("data-nav", "orbit");
  await page.mouse.up();
  await expect(vp).toHaveAttribute("data-view", "custom");
  await expect(viewButtons(page)).toHaveCount(0);
  // Line never saw it: no edge drawn, the model as it was.
  await expect(vp).toHaveAttribute("data-hash", hash!);

  // Shift+drag pans under the Orbit tool.
  const pan = await leftDrag(page, corner, [-8, -4], 10, { keys: ["Shift"], hold: true });
  await expect(vp).toHaveAttribute("data-nav", "pan");
  const s = await screen(page, corner);
  expect(Math.hypot(s[0] - pan.end[0], s[1] - pan.end[1])).toBeLessThan(1.5);
  await page.mouse.up();
  await page.keyboard.up("Shift");

  // H: the Pan tool, a plain drag pans.
  await page.keyboard.press("h");
  await expect(tool(page, "pan")).toHaveAttribute("aria-pressed", "true");
  const pan2 = await leftDrag(page, corner, [10, 5], 10, { hold: true });
  await expect(vp).toHaveAttribute("data-nav", "pan");
  const s2 = await screen(page, corner);
  expect(Math.hypot(s2[0] - pan2.end[0], s2[1] - pan2.end[1])).toBeLessThan(1.5);
  await page.mouse.up();

  // Esc goes back to Line, the tool before Orbit and Pan; then from Select, to Select.
  await page.keyboard.press("Escape");
  await expect(tool(page, "line")).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Space");
  await page.getByRole("button", { name: "View", exact: true }).click();
  await page.getByRole("menuitemcheckbox", { name: /Orbit\s*O/ }).click();
  await expect(tool(page, "orbit")).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect(tool(page, "select")).toHaveAttribute("aria-pressed", "true");
  await expect(vp).toHaveAttribute("data-hash", hash!);
});

test("in Paint, Alt+click still picks a material up and does not orbit", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.getByRole("tab", { name: "Materials" }).click();
  const oak = page.getByRole("option", { name: "White oak" });
  await expect(oak).toBeVisible({ timeout: 15_000 });
  await oak.click();
  await expect(tool(page, "paint")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#hint")).toContainText(/press O to orbit|White oak/);
  const top: V3 = [6, 6, 6];
  await hover(page, top, "face", 1, -1, "data-hover");
  await clickAt(page, top, /on-face|.*/);
  await expect(page.locator("#hint")).toContainText(/Painted .*White oak/);
  await page.getByRole("option", { name: "Navy" }).click();
  await expect(page.locator(".mat-active")).toHaveAttribute("data-active", "Navy");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await expect(vp).toHaveAttribute("data-view", "iso");
  const before = await eye(page);

  // Alt held, a click on the painted top, and a drag across it: the view stays put, the
  // click samples White oak, and Paint is still the tool.
  await hover(page, top, "face", 1, -1, "data-hover");
  const [tx, ty] = await screen(page, [6 * IN, 6 * IN, 6 * IN]);
  const cv = (await page.locator("#cv").boundingBox())!;
  await page.keyboard.down("Alt");
  await page.mouse.click(cv.x + tx, cv.y + ty);
  await expect(page.locator(".mat-active")).toHaveAttribute("data-active", "White oak");
  await page.mouse.move(cv.x + tx, cv.y + ty);
  await page.mouse.down();
  await page.mouse.move(cv.x + tx + 120, cv.y + ty + 40, { steps: 8 });
  await expect(vp).toHaveAttribute("data-nav", "");
  await page.mouse.up();
  await page.keyboard.up("Alt");
  await expect(vp).toHaveAttribute("data-view", "iso");
  expect(await eye(page)).toEqual(before);
  await expect(tool(page, "paint")).toHaveAttribute("aria-pressed", "true");
});

test("in Section Plane, Alt+click places the plane reversed and does not orbit; a tap of Alt reverses it too", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  const planes = () => page.evaluate(() => (window as unknown as { __sawhorse: { graph(): { sections?: { normal: V3 }[] } } }).__sawhorse.graph().sections ?? []);
  const top: V3 = [6, 6, 6];
  const place = async (alt: "held" | "tapped" | "none") => {
    await page.getByRole("button", { name: "Tools" }).click();
    await page.getByRole("menuitemcheckbox", { name: "Section Plane" }).click();
    await expect(tool(page, "section")).toHaveAttribute("aria-pressed", "true");
    await hover(page, top, "on-face");
    if (alt === "tapped") {
      await page.keyboard.press("Alt");
      await expect(page.locator("#hint")).toContainText("reversed");
    }
    const [x, y] = await at(page, top, 1, -1);
    if (alt === "held") await page.keyboard.down("Alt");
    await page.mouse.click(x, y);
    if (alt === "held") await page.keyboard.up("Alt");
    await expect(page.locator("#hint")).toContainText("Section plane placed");
    const all = await planes();
    return all[all.length - 1].normal;
  };
  // A plain click lays the plane on the top face; Alt held at the click turns it round.
  const plain = await place("none");
  expect(Math.abs(plain[2])).toBeCloseTo(1, 9);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await planes()).length).toBe(0);
  const held = await place("held");
  expect(held[2]).toBeCloseTo(-plain[2], 9);
  await expect(vp).toHaveAttribute("data-view", "iso");
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await planes()).length).toBe(0);
  // A tap of Alt before a plain click turns it round as well.
  const tapped = await place("tapped");
  expect(tapped[2]).toBeCloseTo(-plain[2], 9);
});

test("macOS: Control+drag is the right button and pans in every tool, through Control let go; Select sees nothing, Move's copying is untouched, Cmd+click toggles", async ({ page }) => {
  // The browser here may not be on a Mac: say it is (the platform is all the app reads).
  await page.addInitScript(() => Object.defineProperty(Navigator.prototype, "platform", { get: () => "MacIntel" }));
  const vp = await ready(page);
  await box(page); // 12 x 12 x 6 in at the origin
  await page.keyboard.press("Space");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await expect(vp).toHaveAttribute("data-view", "iso");
  const hash = await vp.getAttribute("data-hash");
  const corner: V3 = [12 * IN, 0, 6 * IN];

  // Select: Control held, the left button down on the corner; Control let go halfway.
  const half = await leftDrag(page, corner, [-8, 5], 8, { keys: ["Control"], hold: true });
  await expect(vp).toHaveAttribute("data-nav", "pan");
  await page.keyboard.up("Control");
  const cv = (await page.locator("#cv").boundingBox())!;
  const end: [number, number] = [half.end[0] - 64, half.end[1] + 40];
  await page.mouse.move(cv.x + end[0], cv.y + end[1], { steps: 8 });
  await expect(vp).toHaveAttribute("data-nav", "pan");
  const s = await screen(page, corner);
  expect(Math.hypot(s[0] - end[0], s[1] - end[1])).toBeLessThan(1.5);
  await page.mouse.up();
  await expect(vp).toHaveAttribute("data-nav", "");
  // A pan keeps the standard view; Select never saw the press.
  await expect(vp).toHaveAttribute("data-view", "iso");
  await expect(vp).toHaveAttribute("data-selection", "0");
  await expect(page.locator(".selbox")).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-hash", hash!);

  // Cmd+click is the tools' Ctrl there: it toggles the top face into the selection.
  await hover(page, [6, 6, 6], "face", 1, -1, "data-hover");
  const top = await at(page, [6, 6, 6], 1, -1);
  await page.keyboard.down("Meta");
  await page.mouse.click(top[0], top[1]);
  await page.keyboard.up("Meta");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("Escape");

  // Under the Orbit tool Control+drag still pans (a right-drag pans in every tool).
  await page.keyboard.press("o");
  await expect(tool(page, "orbit")).toHaveAttribute("aria-pressed", "true");
  await leftDrag(page, corner, [-6, 4], 6, { keys: ["Control"], hold: true });
  await expect(vp).toHaveAttribute("data-nav", "pan");
  await page.mouse.up();
  await page.keyboard.up("Control");
  await page.keyboard.press("Escape");

  // In Move, holding Control for the pan does not switch copying (a Control tap still does).
  await page.keyboard.press("m");
  await expect(tool(page, "move")).toHaveAttribute("aria-pressed", "true");
  await leftDrag(page, corner, [6, 0], 10, { keys: ["Control"] });
  await expect(page.locator("#hint")).not.toContainText("Copying");
  await page.keyboard.press("Control");
  await expect(page.locator("#hint")).toContainText("Copying");
});
