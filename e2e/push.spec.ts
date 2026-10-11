import { expect, test, type Page } from "@playwright/test";

// Push/Pull feel (lane 3): press-drag-release with a live preview, click-move-click, a typed
// distance mid-gesture, double-click to repeat, Ctrl for a new face, push-through on the
// boolean path, typed refusals in the status line.
const requireKernel = !!process.env.REQUIRE_KERNEL;
type V3 = [number, number, number];
const IN = 0.0254;

async function ready(page: Page) {
  await page.goto("/");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

async function at(page: Page, p: V3, dx = 0, dy = 0): Promise<[number, number]> {
  const box = await page.locator("#cv").boundingBox();
  const xy = await page.evaluate((p) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(p), p);
  return [box!.x + xy[0] + dx, box!.y + xy[1] + dy];
}

const volume = async (page: Page) => Number(await page.locator("main.vp").getAttribute("data-volume")) / IN ** 3;

/** A 24 x 16 x 12 in box, Push/Pull active, Front view (the top edge and front face visible). */
async function box(page: Page) {
  const vp = await ready(page);
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type("12");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  return vp;
}

test("press on the top, drag up, let go: the preview follows and the release commits", async ({ page }) => {
  const vp = await box(page);
  const [x0, y0] = await at(page, [12 * IN, 8 * IN, 12 * IN]);
  const [x1, y1] = await at(page, [12 * IN, 8 * IN, 18 * IN]);
  await page.mouse.move(x0, y0);
  await page.mouse.down();
  await page.mouse.move(x1, y1, { steps: 10 });
  // Live: the value box and the label show the distance while dragging.
  await expect(page.locator("#vcb")).toHaveValue("6");
  await expect(page.locator(".vp .vlabel.live")).toContainText("6");
  await page.mouse.up();
  await expect(page.locator("#hint")).toContainText("Pulled 6 in");
  expect(await volume(page)).toBeCloseTo(24 * 16 * 18, 4);
  await expect(vp).toHaveAttribute("data-watertight", "true");
});

test("click, move, click; typed distance mid-gesture; double-click repeats; Ctrl pulls a new face", async ({ page }) => {
  const vp = await box(page);
  await page.getByRole("button", { name: "Front", exact: true }).click();
  // Click the front face, move out (toward the eye, -y), click: 4 in.
  const [x0, y0] = await at(page, [12 * IN, 0, 6 * IN]);
  await page.mouse.click(x0, y0);
  await expect(page.locator("#hint")).toContainText("Pushing face");
  // In Front view the normal points at the eye: type instead.
  await page.keyboard.type("4");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Pulled 4 in");
  expect(await volume(page)).toBeCloseTo(24 * 20 * 12, 4);
  // Double-click the top: the last distance again (4 in).
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  const [tx, ty] = await at(page, [12 * IN, 8 * IN, 12 * IN]);
  await page.mouse.dblclick(tx, ty);
  await expect(page.locator("#hint")).toContainText("Repeated 4 in");
  expect(await volume(page)).toBeCloseTo(24 * 20 * 16, 4);
  const faces = Number(await vp.getAttribute("data-parts"));
  // Ctrl held on release: a new face, the old edge loop stays as a seam (+4 faces).
  const [ux, uy] = await at(page, [12 * IN, 8 * IN, 16 * IN]);
  const [vx, vy] = await at(page, [12 * IN, 8 * IN, 18 * IN]);
  await page.mouse.move(ux, uy);
  await page.mouse.down();
  await page.mouse.move(vx, vy, { steps: 8 });
  await page.keyboard.down("Control");
  await page.mouse.up();
  await page.keyboard.up("Control");
  await expect(page.locator("#hint")).toContainText("Pulled a new face 2 in");
  await expect(vp).toHaveAttribute("data-parts", String(faces + 4));
});

test("pushing through goes the boolean path; refusals show typed in the status line", async ({ page }) => {
  const vp = await box(page);
  // An imprint on the top, pushed 12 in down: through the bottom (genus 1).
  await page.keyboard.press("r");
  const a = await at(page, [8 * IN, 5 * IN, 12 * IN]);
  await page.mouse.click(a[0], a[1]);
  await page.keyboard.type("8,6");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("imprinted");
  await page.keyboard.press("p");
  await page.keyboard.type("-12");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("genus 1");
  expect(await volume(page)).toBeCloseTo(24 * 16 * 12 - 8 * 6 * 12, 4);
  // Zero is refused, typed, and nothing changes.
  const hash = await vp.getAttribute("data-hash");
  const [tx, ty] = await at(page, [3 * IN, 3 * IN, 12 * IN]);
  await page.mouse.click(tx, ty);
  await page.keyboard.type("0");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toHaveClass(/err/);
  await expect(vp).toHaveAttribute("data-hash", hash ?? "");
});
