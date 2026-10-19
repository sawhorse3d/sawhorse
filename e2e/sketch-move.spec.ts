import { expect, test, type Page } from "@playwright/test";

// Moving and turning sketch geometry (lane 22, issue #2): a circle moved by a typed distance
// and by inference, turned 45 degrees, copied with Ctrl, every step one undo; one corner of
// a rectangle moved (the rectangle stays flat, the edges beside the corner follow) and the
// region pushed into a solid of the right volume; a whole rectangle lifted off the ground
// and pushed there. Positions are inches; regions are read back from the kernel through
// window.__sawhorse.regions() (world rings and the sketch's plane).
import { IN, clickAt, near, point, ready, selectAt, solids, type, type V3 } from "./helpers";

type Region = { sketch: number; region: number; rings: V3[][]; normal: V3 | null; origin: V3 | null };

async function regions(page: Page): Promise<Region[]> {
  const all = await page.evaluate(() => (window as unknown as { __sawhorse: { regions(): Promise<Region[]> } }).__sawhorse.regions());
  return all.map((r) => ({ ...r, rings: r.rings.map((ring) => ring.map((p) => p.map((x) => x / IN) as V3)), origin: r.origin ? (r.origin.map((x) => x / IN) as V3) : null }));
}

function centroid(ring: V3[]): V3 {
  const s = ring.reduce((a, p) => [a[0] + p[0], a[1] + p[1], a[2] + p[2]] as V3, [0, 0, 0] as V3);
  return s.map((x) => x / ring.length) as V3;
}

/** Area of a planar ring (half the length of its Newell normal), square inches. */
function area(ring: V3[]): number {
  const n = [0, 0, 0];
  for (let i = 0; i < ring.length; i++) {
    const a = ring[i];
    const b = ring[(i + 1) % ring.length];
    n[0] += (a[1] - b[1]) * (a[2] + b[2]);
    n[1] += (a[2] - b[2]) * (a[0] + b[0]);
    n[2] += (a[0] - b[0]) * (a[1] + b[1]);
  }
  return Math.hypot(n[0], n[1], n[2]) / 2;
}

/** The centres of the regions' outer rings, sorted by x. */
async function centres(page: Page): Promise<V3[]> {
  return (await regions(page)).map((r) => centroid(r.rings[0])).sort((a, b) => a[0] - b[0]);
}

test("a circle moves by a typed distance and by inference, turns 45 degrees and copies; undo takes each back", async ({ page }) => {
  const vp = await ready(page);
  const hint = page.locator("#hint");
  // A line to drop the circle onto: (48, 12) to (60, 12).
  await page.keyboard.press("l");
  await clickAt(page, [48, 12, 0], /grid|endpoint/);
  await point(page, [56, 12, 0]);
  await expect(vp).toHaveAttribute("data-inference", "axis-x");
  await type(page, "12");
  await expect(page.locator("#hint")).toContainText("Line 12 in on the ground");
  await page.keyboard.press("Escape");
  // The circle: centre at the origin, its first point at (6, 0), 24 segments.
  await page.keyboard.press("c");
  await clickAt(page, [0, 0, 0], /grid|endpoint|axis/);
  await point(page, [6, 0, 0]);
  await type(page, "6");
  await expect(hint).toContainText("Circle r 6 in");
  await expect(vp).toHaveAttribute("data-parts", "1");
  const before = await regions(page);
  expect(before).toHaveLength(1);
  const a0 = area(before[0].rings[0]);
  expect(a0).toBeCloseTo(0.5 * 24 * 36 * Math.sin(Math.PI / 12), 6);

  // Select it and move it 24 in along red, typed.
  await page.keyboard.press("Space");
  await selectAt(page, [1, 1, 0], "region");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("m");
  await clickAt(page, [6, 0, 0], "endpoint");
  await point(page, [20, 0, 0]);
  await expect(vp).toHaveAttribute("data-inference", "axis-x");
  await type(page, "24");
  await expect(hint).toContainText("Moved region 1 24 in along +red");
  await expect(hint).not.toContainText("don't move");
  near((await centres(page))[0], [24, 0, 0]);
  expect(area((await regions(page))[0].rings[0])).toBeCloseTo(a0, 6);
  await expect(vp).toHaveAttribute("data-selection", "1");

  // By inference: its point at (30, 0) onto the line's end at (48, 12).
  await clickAt(page, [30, 0, 0], "endpoint");
  await clickAt(page, [48, 12, 0], "endpoint");
  await expect(hint).toContainText("Moved region 1");
  near((await centres(page))[0], [42, 12, 0]);

  // Turn it 45 degrees about that point, on the ground's plane.
  await page.keyboard.press("q");
  await clickAt(page, [48, 12, 0], "endpoint");
  await expect(hint).toContainText("about the blue axis");
  await clickAt(page, [60, 12, 0], "endpoint");
  await type(page, "45");
  await expect(hint).toContainText("Turned region 1 45° about the blue axis");
  const h = 6 * Math.SQRT1_2;
  near((await centres(page))[0], [48 - h, 12 - h, 0]);
  for (const p of (await regions(page))[0].rings[0]) expect(Math.abs(p[2])).toBeLessThan(1e-9);

  // Ctrl: a copy 24 in along red; the original stays.
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt(page, [48, 12, 0], "endpoint");
  await page.keyboard.press("ArrowRight");
  await point(page, [56, 12, 0]);
  await type(page, "24");
  await expect(hint).toContainText("Copied region 1 24 in along +red");
  let c = await centres(page);
  expect(c).toHaveLength(2);
  near(c[0], [48 - h, 12 - h, 0]);
  near(c[1], [72 - h, 12 - h, 0]);
  for (const r of await regions(page)) expect(area(r.rings[0])).toBeCloseTo(a0, 6);

  // One undo step each: the copy, the turn, the inferred move, the typed move.
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await centres(page)).length).toBe(1);
  near((await centres(page))[0], [48 - h, 12 - h, 0]);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await centres(page))[0][1]).toBeCloseTo(12, 6);
  near((await centres(page))[0], [42, 12, 0]);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await centres(page))[0][0]).toBeCloseTo(24, 6);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await centres(page))[0][0]).toBeCloseTo(0, 6);
  near((await centres(page))[0], [0, 0, 0]);
});

test("one corner of a rectangle moves: it stays flat, its edges follow; pushed, its volume is exact", async ({ page }) => {
  const vp = await ready(page);
  const hint = page.locator("#hint");
  await page.keyboard.press("r");
  await type(page, "12,8");
  await expect(vp).toHaveAttribute("data-parts", "1");
  await page.keyboard.press("Space");
  await selectAt(page, [12, 8, 0], "vertex");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("m");
  await clickAt(page, [12, 8, 0], "endpoint");
  await page.keyboard.press("ArrowRight");
  await point(page, [20, 8, 0]);
  await type(page, "4");
  await expect(hint).toContainText("Moved a sketch point 4 in along +red");
  const rs = await regions(page);
  expect(rs).toHaveLength(1);
  const ring = rs[0].rings[0];
  // The same four corners, one moved: the two edges at it followed, nothing else changed.
  expect(ring).toHaveLength(4);
  const has = (p: V3) => ring.some((q) => Math.hypot(q[0] - p[0], q[1] - p[1], q[2] - p[2]) < 1e-6);
  for (const p of [[0, 0, 0], [12, 0, 0], [16, 8, 0], [0, 8, 0]] as V3[]) expect(has(p), JSON.stringify(ring)).toBe(true);
  for (const p of ring) expect(Math.abs(p[2])).toBeLessThan(1e-9);
  expect(area(ring)).toBeCloseTo(112, 6);
  // The point stays selected where it went.
  await expect(vp).toHaveAttribute("data-selection", "1");

  // Pushed 3 in: (12 + 16) / 2 x 8 x 3 = 336 cubic inches.
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await page.keyboard.press("p");
  await clickAt(page, [6, 4, 0], /on-face|grid/);
  await type(page, "3");
  await expect(vp).toHaveAttribute("data-objects", "1");
  let s = await solids(page);
  expect(s[0].volume).toBeCloseTo(336, 6);
  near(s[0].min, [0, 0, 0]);
  near(s[0].max, [16, 8, 3]);

  // Undo the push, then the move: the rectangle as it was.
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await page.keyboard.press("Control+z");
  await expect.poll(async () => area((await regions(page))[0].rings[0])).toBeCloseTo(96, 6);
  s = await solids(page);
  expect(s).toHaveLength(0);
});

test("a whole rectangle lifted off the ground goes onto its new plane and pushes there", async ({ page }) => {
  const vp = await ready(page);
  const hint = page.locator("#hint");
  await page.keyboard.press("r");
  await type(page, "10,10");
  await page.keyboard.press("Space");
  await selectAt(page, [5, 5, 0], "region");
  await page.keyboard.press("m");
  await clickAt(page, [0, 0, 0], "endpoint");
  await page.keyboard.press("ArrowUp");
  await point(page, [0, 0, 10]);
  await type(page, "6");
  await expect(hint).toContainText("Moved region 1 6 in along +blue");
  const rs = await regions(page);
  expect(rs).toHaveLength(1);
  near(rs[0].origin!, [0, 0, 6]);
  for (const p of rs[0].rings[0]) expect(p[2]).toBeCloseTo(6, 9);
  expect(area(rs[0].rings[0])).toBeCloseTo(100, 6);
  // Still selected (as the region it became up there), and Push/Pull takes it next.
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("p");
  await type(page, "2");
  await expect(vp).toHaveAttribute("data-objects", "1");
  const s = await solids(page);
  near(s[0].min, [0, 0, 6]);
  near(s[0].max, [10, 10, 8]);
  expect(s[0].volume).toBeCloseTo(200, 6);
});
