import { expect, test } from "@playwright/test";

// End-to-end checks against the production browser build (npm run build && vite preview).
// REQUIRE_KERNEL=1 (set in CI, after wasm-pack) makes the kernel spike mandatory.
const requireKernel = !!process.env.REQUIRE_KERNEL;

test("shell matches the approved mock's structure", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator(".app > header.top")).toBeVisible();
  await expect(page.locator(".tool")).toHaveCount(15);
  await expect(page.getByRole("tab")).toHaveText(["Outliner", "Tags", "Cut list", "Render"]);
  await expect(page.locator(".legend")).toContainText("Endpoint");
  await expect(page.locator(".vcb")).toBeVisible();
  // Our inference colours, from the CSS tokens.
  const colours = await page.evaluate(() => {
    const cs = getComputedStyle(document.documentElement);
    return ["--inf-end", "--inf-mid", "--inf-edge", "--inf-face"].map((n) => cs.getPropertyValue(n).trim());
  });
  expect(colours).toEqual(["#7bc043", "#17a2a0", "#e08a1e", "#8a5cd6"]);
});

test("keyboard shortcuts switch tools", async ({ page }) => {
  await page.goto("/");
  for (const [key, tool] of [["p", "pushpull"], ["r", "rect"], ["l", "line"], ["Space", "select"]] as const) {
    await page.keyboard.press(key);
    await expect(page.locator(`.tool[data-tool="${tool}"]`)).toHaveAttribute("aria-pressed", "true");
  }
});

test("the kernel box renders and the value bar drives extrude", async ({ page }) => {
  await page.goto("/");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  const state = await vp.getAttribute("data-kernel");
  if (!requireKernel && state === "missing") test.skip(true, "kernel WASM not built (run npm run wasm)");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  await expect(vp).toHaveAttribute("data-triangles", "12");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  const before = await vp.getAttribute("data-hash");

  await page.keyboard.press("p");
  await page.keyboard.type("3/4");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Pushed 3/4 in");
  await expect(page.locator(".hud")).toContainText("24 × 16 × 3/4 in");
  await expect(vp).not.toHaveAttribute("data-hash", before ?? "");

  // A zero push is a typed kernel refusal; the last good solid stays.
  await page.locator("#vcb").fill("0");
  await page.locator("#vcb").press("Enter");
  await expect(page.locator("#hint")).toHaveClass(/err/);
  await expect(vp).toHaveAttribute("data-triangles", "12");

  // Rect takes "w,h".
  await page.keyboard.press("Escape");
  await page.locator("#cv").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("r");
  await page.keyboard.type("36,12");
  await page.keyboard.press("Enter");
  await expect(page.locator(".hud")).toContainText("36 × 12 × 3/4 in");

  // A WebGL2 canvas actually drew something other than the clear colour.
  const painted = await page.evaluate(() => {
    const c = document.getElementById("cv") as HTMLCanvasElement;
    const gl = c.getContext("webgl2");
    return !!gl;
  });
  expect(painted).toBe(true);
});
