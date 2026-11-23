import { expect, test, type Locator, type Page } from "@playwright/test";
import { at, clickAt, ready, type V3 } from "./helpers";

// #130: a draw tool's click with no point under it (the Front view of an empty model) and a
// drag (press, move, release) draw nothing, and the status line says why; a first corner
// already placed survives the drag. #129: a dimension in the model takes a click on its
// number and on its extension lines, for Erase and for Select, and an Eraser drag across it
// takes it with the rest of the drag in one undo step. The window is 1000 px wide, the
// narrowest the end-to-end tests use, and every message is checked to fit the status line.
test.use({ viewport: { width: 1000, height: 720 } });

const hint = (page: Page) => page.locator("#hint");
const hash = (page: Page) => page.locator("main.vp").getAttribute("data-hash");
const dims = (page: Page) => page.locator(".vp .vlabel.dim");
type Region = { rings: V3[][] };
const regions = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { regions(): Promise<Region[]> } }).__sawhorse.regions());

async function view(page: Page, name: string) {
  await page.getByRole("button", { name, exact: true }).click();
  await expect(page.locator("main.vp")).toHaveAttribute("data-view", name.toLowerCase());
}

/**
 * The words fit the status line: at this width the line is about 360 to 410 px wide (the
 * side pane takes the rest) and the tools' own hints already take two lines there, so the
 * words (the bold lead and the text, not the buttons after them) take at most two.
 */
async function fits(el: Locator) {
  const lines = await el.evaluate((n) => {
    const r = document.createRange();
    r.setStart(n, 0);
    const button = n.querySelector("button");
    if (button) r.setEndBefore(button);
    else r.setEnd(n, n.childNodes.length);
    return new Set(Array.from(r.getClientRects()).filter((x) => x.width > 0).map((x) => Math.round(x.top))).size;
  });
  expect(lines, (await el.textContent()) ?? "").toBeLessThanOrEqual(2);
}

/** The point the last pointer event was resolved to (the inference engine's answer). */
const lastPoint = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { lastInference: { point: V3 } } }).__sawhorse.lastInference.point);

/** A point in the canvas, as a fraction of its width and height. */
async function canvasAt(page: Page, fx: number, fy: number): Promise<[number, number]> {
  const b = (await page.locator("#cv").boundingBox())!;
  return [b.x + b.width * fx, b.y + b.height * fy];
}

/** Press, move 60 px in small steps, release: a drag, not a click. */
async function drag(page: Page, [x, y]: [number, number]) {
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 60, y + 30, { steps: 8 });
  await page.mouse.up();
}

test("a click with nothing under it, and a drag, draw nothing and say why; a first corner survives a drag", async ({ page }) => {
  const vp = await ready(page);
  const h0 = await hash(page);
  await view(page, "Front");
  await page.keyboard.press("r");
  await expect(hint(page)).toContainText("Rect.");
  const q1 = await canvasAt(page, 0.4, 0.3);
  const q2 = await canvasAt(page, 0.6, 0.45);
  // Hovering says nothing: only the click does.
  await page.mouse.move(q1[0] + 20, q1[1] + 10, { steps: 4 });
  await page.mouse.move(q1[0], q1[1], { steps: 4 });
  await expect(vp).toHaveAttribute("data-inference", "");
  await expect(hint(page)).not.toContainText("Nothing to draw");
  await page.mouse.click(q1[0], q1[1]);
  await expect(hint(page)).toHaveText("Can't do that. Nothing to draw on there: click the ground or a face, or use Top or Iso.");
  await expect(hint(page)).toHaveClass(/err/);
  await fits(hint(page));
  await page.mouse.click(q2[0], q2[1]);
  await expect(hint(page)).toContainText("Nothing to draw on there");
  expect(await regions(page)).toEqual([]);
  expect(await hash(page)).toBe(h0);

  // In Top the same clicks draw a rectangle, and the status line says what each did.
  await view(page, "Top");
  await page.mouse.move(q1[0], q1[1], { steps: 2 });
  await page.mouse.click(q1[0], q1[1]);
  await expect(hint(page)).toContainText("First corner set on the ground. Click the opposite corner, or type width, depth.");
  await expect(hint(page)).not.toHaveClass(/err/);
  await page.mouse.move(q2[0], q2[1], { steps: 4 });
  await page.mouse.click(q2[0], q2[1]);
  await expect(hint(page)).toContainText(/^Rect\. Rectangle .* on the ground/);
  expect(await regions(page)).toHaveLength(1);
  const h1 = await hash(page);

  // A drag with Rectangle, then with Line: nothing drawn, and the words say so.
  const q = await canvasAt(page, 0.25, 0.75);
  await drag(page, q);
  await expect(hint(page)).toHaveText("Can't do that. Click, move, then click again. A drag does not draw.");
  await fits(hint(page));
  await page.keyboard.press("l");
  await drag(page, q);
  await expect(hint(page)).toHaveText("Can't do that. Click, move, then click again. A drag does not draw.");
  // The edit queue has settled (regions() waits for it): nothing was drawn by either.
  expect(await regions(page)).toHaveLength(1);
  expect(await hash(page)).toBe(h1);

  // A first corner, then a drag: the corner stays, and so does what to do next.
  await page.keyboard.press("r");
  const c1 = await canvasAt(page, 0.15, 0.6);
  const c2 = await canvasAt(page, 0.3, 0.85);
  await page.mouse.move(c1[0], c1[1], { steps: 2 });
  await page.mouse.click(c1[0], c1[1]);
  await expect(hint(page)).toContainText("First corner set");
  // Where that click landed: the last point the pointer was resolved to is the click's.
  const p1 = await lastPoint(page);
  await drag(page, await canvasAt(page, 0.5, 0.65));
  await expect(hint(page)).toHaveText("Can't do that. A drag does not draw. Click the opposite corner, or type width, depth.");
  await fits(hint(page));
  expect(await hash(page)).toBe(h1);
  await page.mouse.move(c2[0], c2[1], { steps: 4 });
  await page.mouse.click(c2[0], c2[1]);
  await expect(hint(page)).toContainText(/Rectangle .* on the ground/);
  const p2 = await lastPoint(page);
  const rs = await regions(page);
  expect(rs).toHaveLength(2);
  // The new rectangle's corners are where the two clicks landed, not where the drag went.
  const has = (q: V3) => rs.some((r) => r.rings[0].some((c) => Math.hypot(c[0] - q[0], c[1] - q[1], c[2] - q[2]) < 1e-9));
  expect(has(p1), JSON.stringify(p1)).toBe(true);
  expect(has(p2), JSON.stringify(p2)).toBe(true);
});

test("a dimension is erased by its number and its extension lines, selected by its number, and swept by an Eraser drag in one undo step", async ({ page }) => {
  const vp = await ready(page);
  await view(page, "Top");
  await page.keyboard.press("r");
  await page.keyboard.type("24,12");
  await page.keyboard.press("Enter");
  await expect(hint(page)).toContainText("Rectangle 24 × 12 in");
  // A dimension along the rectangle's front edge, 6 in in front of it.
  await page.keyboard.press("d");
  await clickAt(page, [0, 0, 0], "endpoint");
  await clickAt(page, [24, 0, 0], "endpoint");
  await clickAt(page, [12, -6, 0], /grid|free|/);
  await expect(hint(page)).toContainText("Dimension 24 in");
  await expect(dims(page)).toHaveCount(1);
  const hd = await hash(page);

  /** A point inside the number's box but 6 px or more off the dimension line it sits on. */
  const onNumber = async (): Promise<[number, number]> => {
    const b = (await dims(page).boundingBox())!;
    const [, ly] = await at(page, [12, -6, 0]);
    const y = b.y + b.height - 2;
    expect(y - ly).toBeGreaterThanOrEqual(6);
    return [b.x + b.width - 3, y];
  };
  /** The left extension line, halfway along it. */
  const onExtension = () => at(page, [0, -3, 0]);
  /**
   * Point at a part of the dimension and click once the tool shows the dimension under the
   * cursor. The view may refit after an edit or an undo, so the place is read again and the
   * cursor nudged until it does.
   */
  const clickDim = async (where: () => Promise<[number, number]>) => {
    let xy: [number, number] = [0, 0];
    await expect(async () => {
      xy = await where();
      await page.mouse.move(xy[0] + 3, xy[1] + 2);
      await page.mouse.move(xy[0], xy[1], { steps: 2 });
      await expect(vp).toHaveAttribute("data-hover", "dimension", { timeout: 800 });
    }).toPass({ timeout: 10_000 });
    await page.mouse.click(xy[0], xy[1]);
  };
  const undo = async () => {
    await page.keyboard.press("Control+z");
    await expect(dims(page)).toHaveCount(1);
    await expect(vp).toHaveAttribute("data-hash", hd!);
  };

  // The Eraser's hint names what it takes.
  await page.keyboard.press("e");
  await expect(hint(page)).toContainText("dimensions");
  await fits(hint(page));

  // Erase: a click on the number.
  await clickDim(onNumber);
  await expect(dims(page)).toHaveCount(0);
  await expect(hint(page)).toContainText("Erased 1 item");
  await undo();

  // Erase: a click on the left extension line.
  await clickDim(onExtension);
  await expect(dims(page)).toHaveCount(0);
  await expect(hint(page)).toContainText("Erased 1 item");
  await undo();

  // Select: a click on the number selects the dimension; Delete erases it.
  await page.keyboard.press("Space");
  await clickDim(onNumber);
  await expect(hint(page)).toContainText("Selected a dimension. Delete erases it.");
  await page.keyboard.press("Delete");
  await expect(dims(page)).toHaveCount(0);
  await undo();
  await page.keyboard.press("Escape");

  // An Eraser drag from in front of the dimension, across its line and the rectangle's front
  // edge: both go, in one step.
  await page.keyboard.press("e");
  let [x, y] = await at(page, [6, -9, 0]);
  await page.mouse.move(x, y, { steps: 2 });
  await page.mouse.down();
  for (const [p, kind] of [
    [[6, -6, 0], "dimension"],
    [[6, 0, 0], "edge"],
  ] as [V3, string][]) {
    [x, y] = await at(page, p);
    await page.mouse.move(x, y, { steps: 6 });
    await expect(vp).toHaveAttribute("data-hover", kind);
  }
  await page.mouse.up();
  await expect(dims(page)).toHaveCount(0);
  await expect(hint(page)).toContainText(/Erased [2-9] items/);
  expect(await regions(page)).toEqual([]);
  await undo();
  expect(await regions(page)).toHaveLength(1);
});

test("a press and release 4 px apart is a click, 5 px apart a drag", async ({ page }) => {
  await ready(page);
  await view(page, "Top");
  await page.keyboard.press("r");
  const [x, y] = await canvasAt(page, 0.4, 0.4);
  for (const [dx, dy, click] of [
    [4, 0, true],
    [5, 0, false],
    [0, 4, true],
    [3, 4, false],
  ] as [number, number, boolean][]) {
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x + dx, y + dy);
    await page.mouse.up();
    if (click) {
      await expect(hint(page), `${dx},${dy}`).toContainText("First corner set");
      await page.keyboard.press("Escape");
      await expect(hint(page)).not.toContainText("First corner set");
    } else {
      await expect(hint(page), `${dx},${dy}`).toHaveText("Can't do that. Click, move, then click again. A drag does not draw.");
    }
  }
  expect(await regions(page)).toEqual([]);
});

test("an Eraser sweep let go outside the view erases nothing on a later drag", async ({ page }) => {
  const vp = await ready(page);
  await view(page, "Top");
  await page.keyboard.press("r");
  await page.keyboard.type("24,12");
  await page.keyboard.press("Enter");
  await expect(hint(page)).toContainText("Rectangle 24 × 12 in");
  await page.keyboard.press("d");
  await clickAt(page, [0, 0, 0], "endpoint");
  await clickAt(page, [24, 0, 0], "endpoint");
  await clickAt(page, [12, -6, 0], /grid|free|/);
  await expect(dims(page)).toHaveCount(1);
  // The model as it is once every queued edit has landed (regions() waits for them).
  await regions(page);
  const hd = await hash(page);
  await page.keyboard.press("e");
  // Press in front of the dimension and sweep across it.
  let [x, y] = await at(page, [6, -9, 0]);
  await page.mouse.move(x, y, { steps: 2 });
  await page.mouse.down();
  [x, y] = await at(page, [6, -6, 0]);
  await page.mouse.move(x, y, { steps: 6 });
  await expect(vp).toHaveAttribute("data-hover", "dimension");
  // Leave the view and let go there: the view never gets the release.
  const cv = (await page.locator("#cv").boundingBox())!;
  await page.mouse.move(cv.x + cv.width + 60, cv.y + cv.height / 2, { steps: 6 });
  await page.mouse.up();
  // Back in, far from everything, and a small drag over nothing.
  const [bx, by] = [cv.x + cv.width * 0.85, cv.y + cv.height * 0.15];
  await page.mouse.move(bx, by, { steps: 6 });
  await page.mouse.down();
  await page.mouse.move(bx + 20, by + 10, { steps: 4 });
  await page.mouse.up();
  // The edit queue has settled (regions() waits for it).
  expect(await regions(page)).toHaveLength(1);
  await expect(dims(page)).toHaveCount(1);
  await expect(vp).toHaveAttribute("data-hash", hd!);
  await expect(page.locator(".vp .hud")).toContainText("undo 2 ");
});
