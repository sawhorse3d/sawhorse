import { expect, test, type Page } from "@playwright/test";

// The Value Control Box (lane 3): typed values mid-gesture with fractional inches, feet and
// inches, metric suffixes; Enter commits, Esc cancels; Ctrl+Z works while it has focus; array
// syntax follows a Move or Rotate copy; an untouched box does not swallow tool keys.
const requireKernel = !!process.env.REQUIRE_KERNEL;

async function ready(page: Page) {
  await page.goto("/");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

test("typed lengths in every woodworker form", async ({ page }) => {
  const vp = await ready(page);
  const vcb = page.locator("#vcb");
  await page.keyboard.press("r");
  await page.keyboard.type("1'6\",11 1/4");
  await expect(vcb).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Rectangle 18 × 11 1/4 in");
  await expect(vcb).not.toBeFocused();
  await page.keyboard.press("p");
  await page.keyboard.type("3/4");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Extruded 3/4 in");
  // Metric typed into an imperial document: 457.2 mm is exactly 18 in.
  await page.keyboard.type("457.2mm");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Pulled 18 in");
  const v = Number(await vp.getAttribute("data-volume"));
  expect(v / 0.0254 ** 3).toBeCloseTo(18 * 11.25 * 18.75, 6);
});

test("Esc cancels typing, Ctrl+Z works mid-entry, arrays follow a copy, tool keys pass an untouched box", async ({ page }) => {
  const vp = await ready(page);
  const vcb = page.locator("#vcb");
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-triangles", "2");
  await page.keyboard.press("p");
  await expect(vcb).toHaveValue("12");
  // Start typing, then Esc: the box goes back to its default and lets go of the keyboard.
  await page.keyboard.type("5");
  await expect(vcb).toHaveValue("5");
  await page.keyboard.press("Escape");
  await expect(vcb).toHaveValue("12");
  await expect(vcb).not.toBeFocused();
  // Start typing, then Ctrl+Z: the document steps back (the rectangle goes).
  await page.keyboard.type("7");
  await expect(vcb).toBeFocused();
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-triangles", "0");
  await page.keyboard.press("Control+y");
  await expect(vp).toHaveAttribute("data-triangles", "2");
  await page.keyboard.press("Escape");
  // Array syntax belongs to Move and Rotate copies.
  await page.keyboard.type("x3");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("follow a copy");
  await expect(vp).toHaveAttribute("data-triangles", "2");
  // A box that was clicked into but not typed in does not swallow tool keys.
  await vcb.click();
  await expect(vcb).toBeFocused();
  await page.keyboard.press("r");
  await expect(page.locator('.tool[data-tool="rect"]')).toHaveAttribute("aria-pressed", "true");
  // Garbage is explained, nothing changes.
  const hash = await vp.getAttribute("data-hash");
  await page.keyboard.press("p");
  await page.keyboard.type("3/0");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toHaveClass(/err/);
  await expect(vp).toHaveAttribute("data-hash", hash ?? "");
});

test("the measurements box clears after undo and redo", async ({ page }) => {
  const vp = await ready(page);
  const vcb = page.locator("#vcb");
  const IN = 0.0254;
  const click = async (p: [number, number, number]) => {
    const box = await page.locator("#cv").boundingBox();
    const xy = await page.evaluate((q) => (window as unknown as { __sawhorse: { worldToScreen(p: number[]): [number, number] } }).__sawhorse.worldToScreen(q), p);
    await page.mouse.move(box!.x + xy[0], box!.y + xy[1], { steps: 2 });
    await page.mouse.click(box!.x + xy[0], box!.y + xy[1]);
  };
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type("12");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  // The tape shows what it measured...
  await page.keyboard.press("t");
  await click([24 * IN, 0, 12 * IN]);
  await click([24 * IN, 16 * IN, 12 * IN]);
  await expect(page.locator("#hint")).toContainText("Distance 16 in");
  await expect(vcb).toHaveValue("16");
  // ...until the document steps back: then the box is empty again (the measurement was of
  // geometry that is gone), and after redo too.
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-triangles", "2");
  await expect(vcb).toHaveValue("");
  await click([0, 0, 0]);
  await page.keyboard.press("Control+y");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  await expect(vcb).toHaveValue("");
});
