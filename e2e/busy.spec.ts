import { expect, test, type Page } from "@playwright/test";

// #144: nothing on screen said a long edit was running. Once the kernel has worked on an edit
// for about a second, the status line says "Working… N s" and the cursor over the model shows
// busy; both go when it answers, and the step's own words come back. Nothing is blocked. The
// long edit is made with the test hook (?kerneltest=1): the worker is busy for a set time
// before it runs the next edit, as it is during a large one.
import { box, clickAt, point, type } from "./helpers";

type Hook = { __sawhorse: { kernelTest: { stallNextEdit(ms: number): void } }; __hints: { at: number; text: string }[] };

async function open(page: Page) {
  await page.goto("/?kerneltest=1&autosave=0");
  const vp = page.locator("main.vp");
  await expect(vp).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  // Every text the status line shows, in order, with when it showed.
  await page.evaluate(() => {
    const w = window as unknown as Hook;
    w.__hints = [];
    const el = document.getElementById("hint")!;
    new MutationObserver(() => w.__hints.push({ at: Date.now(), text: el.textContent ?? "" })).observe(el, { childList: true, subtree: true, characterData: true });
  });
  return vp;
}

const hints = (page: Page) => page.evaluate(() => (window as unknown as Hook).__hints);
const cursor = (page: Page) => page.locator("#cv").evaluate((c) => (c as HTMLCanvasElement).style.cursor);

test("a long edit says Working and how long, and shows a busy cursor, until it answers (#144)", async ({ page }) => {
  const vp = await open(page);
  const hint = page.locator("#hint");
  await box(page);
  await page.evaluate(() => (window as unknown as Hook).__sawhorse.kernelTest.stallNextEdit(2_600));
  // A rectangle on the ground, whose edit the kernel takes 2.6 s over.
  await page.keyboard.press("r");
  await clickAt(page, [30, 0, 0], "grid", 1, 1);
  await point(page, [36, -6, 0]);
  const sent = await page.evaluate(() => Date.now());
  await type(page, "6,6");
  await expect(hint).toHaveText("Rect. Working… 1 s");
  await expect(vp).toHaveAttribute("data-busy", "true");
  expect(await cursor(page)).toBe("progress");
  await expect(hint).toHaveText("Rect. Working… 2 s");
  // A move over the model while it runs keeps the busy cursor (the tool's own waits).
  await point(page, [6, 6, 6]);
  expect(await cursor(page)).toBe("progress");
  // It answers: the step's own words, the cursor and the mark gone, the rectangle there.
  await expect(hint).toContainText(/Rectangle 6 \S 6 in on the ground/);
  await expect(vp).not.toHaveAttribute("data-busy");
  expect(await cursor(page)).not.toBe("progress");
  await expect(vp).toHaveAttribute("data-parts", "7");
  // Nothing was said before about a second had passed (the edit was sent after Enter).
  const first = (await hints(page)).find((h) => h.text.includes("Working"))!;
  expect(first.text).toBe("Rect. Working… 1 s");
  expect(first.at - sent).toBeGreaterThanOrEqual(1_000);
});

test("an edit that answers within a second shows no mark at all (#144)", async ({ page }) => {
  const vp = await open(page);
  const hint = page.locator("#hint");
  await box(page);
  await page.keyboard.press("r");
  await clickAt(page, [30, 0, 0], "grid", 1, 1);
  await point(page, [36, -6, 0]);
  await type(page, "6,6");
  await expect(hint).toContainText(/Rectangle 6 \S 6 in on the ground/);
  await expect(vp).toHaveAttribute("data-parts", "7");
  expect((await hints(page)).filter((h) => h.text.includes("Working"))).toEqual([]);
  await expect(vp).not.toHaveAttribute("data-busy");
});
