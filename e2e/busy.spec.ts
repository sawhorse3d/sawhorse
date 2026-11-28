import { expect, test, type Page } from "@playwright/test";

// #144: nothing on screen said a long edit was running. Once a request has waited about a
// second for the kernel's answer, the status line says "Working… N s" and the cursor over the
// model shows busy; both go when it answers, and the step's own words come back. Nothing is
// blocked. The long edit is made with the test hook (?kerneltest=1): the worker is busy for a
// set time before it runs the next edit, as it is during a large one.
import { box, clickAt, point, type } from "./helpers";

type Hook = {
  __sawhorse: { kernelTest: { stallNextEdit(ms: number): void; busySince(): number | null } };
  __hints: { at: number; text: string }[];
};

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
const stall = (page: Page, ms: number) => page.evaluate((ms) => (window as unknown as Hook).__sawhorse.kernelTest.stallNextEdit(ms), ms);

/** A rectangle on the ground at (30, 0), typed 6,6: one edit. */
async function rectangle(page: Page) {
  await page.keyboard.press("r");
  await clickAt(page, [30, 0, 0], "grid", 1, 1);
  await point(page, [36, -6, 0]);
  await type(page, "6,6");
}

test("a long edit says Working and how long, and shows a busy cursor, until it answers (#144)", async ({ page }) => {
  const vp = await open(page);
  const hint = page.locator("#hint");
  await box(page);
  await stall(page, 2_600);
  await rectangle(page);
  await expect(hint).toHaveText("Rect. Working… 1 s");
  // When the edit's wait began, as the client has it.
  const since = (await page.evaluate(() => (window as unknown as Hook).__sawhorse.kernelTest.busySince()))!;
  await expect(vp).toHaveAttribute("data-busy", "true");
  expect(await cursor(page)).toBe("progress");
  await expect(hint).toHaveText("Rect. Working… 2 s");
  // It answers: the step's own words, the cursor and the mark gone, the rectangle there.
  await expect(hint).toContainText(/Rectangle 6 \S 6 in on the ground/);
  await expect(vp).not.toHaveAttribute("data-busy");
  expect(await cursor(page)).not.toBe("progress");
  await expect(vp).toHaveAttribute("data-parts", "7");
  // Nothing was said before a second of waiting.
  const first = (await hints(page)).find((h) => h.text.includes("Working"))!;
  expect(first.text).toBe("Rect. Working… 1 s");
  expect(first.at - since).toBeGreaterThanOrEqual(1_000);
});

test("a move over the model during a long edit: the count goes on, and the step's words follow it (#144)", async ({ page }) => {
  const vp = await open(page);
  const hint = page.locator("#hint");
  await box(page);
  await stall(page, 3_000);
  await rectangle(page);
  await expect(hint).toHaveText("Rect. Working… 1 s");
  // Moves over the box send picks, which wait behind the edit.
  for (const p of [[6, 6, 6], [3, 3, 6], [9, 3, 6], [6, 0, 3]] as [number, number, number][]) await point(page, p);
  expect(await cursor(page)).toBe("progress");
  await expect(hint).toContainText(/Rectangle 6 \S 6 in on the ground/);
  await expect(vp).not.toHaveAttribute("data-busy");
  const seen = (await hints(page)).map((h) => h.text);
  const counts = seen.filter((t) => t.includes("Working")).map((t) => Number(/(\d+) s$/.exec(t)![1]));
  // Up, never back (a pick that waited behind the edit is not counted from when it was sent).
  expect(counts).toEqual([...counts].sort((a, b) => a - b));
  expect(counts.at(-1)).toBeGreaterThanOrEqual(2);
  // After the last Working, the step's own words, and no Working again.
  const last = seen.findLastIndex((t) => t.includes("Working"));
  expect(seen[last + 1]).toMatch(/Rectangle 6 \S 6 in on the ground/);
});

test("an edit that answers within a second shows no mark at all (#144)", async ({ page }) => {
  const vp = await open(page);
  const hint = page.locator("#hint");
  await box(page);
  await rectangle(page);
  await expect(hint).toContainText(/Rectangle 6 \S 6 in on the ground/);
  await expect(vp).toHaveAttribute("data-parts", "7");
  expect((await hints(page)).filter((h) => h.text.includes("Working"))).toEqual([]);
  await expect(vp).not.toHaveAttribute("data-busy");
});
