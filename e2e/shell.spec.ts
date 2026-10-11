import { expect, test, type Page } from "@playwright/test";

// End-to-end checks against the production browser build (npm run build && vite preview).
// REQUIRE_KERNEL=1 (set in CI, after wasm-pack) makes the kernel tests mandatory.
const requireKernel = !!process.env.REQUIRE_KERNEL;

type V3 = [number, number, number];
interface Part {
  kind: "face" | "region";
  owner: number;
  index: number;
  origin: V3;
  normal: V3;
  u: V3;
  v: V3;
}

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

async function kernelReady(page: Page) {
  await page.goto("/");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  const state = await vp.getAttribute("data-kernel");
  if (!requireKernel && state === "missing") test.skip(true, "kernel WASM not built (run npm run wasm)");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** Screen point (page px) of a point on part `part`, at frame offsets (du, dv) metres from its centroid. */
async function onPart(page: Page, pick: (p: Part) => boolean, du = 0, dv = 0): Promise<[number, number]> {
  const box = await page.locator("#cv").boundingBox();
  const xy = await page.evaluate(
    ([du, dv, src]) => {
      const hooks = (window as unknown as { __sawhorse: { parts(): Part[]; worldToScreen(p: V3): [number, number] } }).__sawhorse;
      const test = new Function("p", `return (${src})(p)`) as (p: Part) => boolean;
      const part = hooks.parts().find(test);
      if (!part) return null;
      const p: V3 = [0, 1, 2].map((k) => part.origin[k] + part.u[k] * du + part.v[k] * dv) as V3;
      return hooks.worldToScreen(p);
    },
    [du, dv, pick.toString()] as const,
  );
  if (!xy || !box) throw new Error("part not on screen");
  return [box.x + xy[0], box.y + xy[1]];
}

test("rect and push/pull edit the persistent document; Ctrl+Z / Ctrl+Y step through it", async ({ page }) => {
  const vp = await kernelReady(page);
  await expect(vp).toHaveAttribute("data-triangles", "0");

  // A 24 x 16 in rectangle on the ground (typed), then pull it up 12 in.
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-triangles", "2");
  await page.keyboard.press("p");
  await page.keyboard.type("12");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Extruded 12 in");
  await expect(vp).toHaveAttribute("data-triangles", "12");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  const box = await vp.getAttribute("data-hash");

  // A zero push is a typed refusal; nothing changes.
  await page.locator("#vcb").fill("0");
  await page.locator("#vcb").press("Enter");
  await expect(page.locator("#hint")).toHaveClass(/err/);
  await expect(vp).toHaveAttribute("data-hash", box ?? "");

  // Draw a second rectangle on the top face by clicking two corners: an imprint.
  await page.keyboard.press("Escape");
  await page.keyboard.press("r");
  const isTop = (p: Part) => p.kind === "face" && p.normal[2] > 0.5;
  const a = await onPart(page, isTop, -0.1, -0.06);
  const b = await onPart(page, isTop, 0.1, 0.06);
  await page.mouse.click(a[0], a[1]);
  await expect(page.locator("#hint")).toContainText("First corner set on face");
  await page.mouse.click(b[0], b[1]);
  await expect(page.locator("#hint")).toContainText("imprinted");
  await expect(vp).toHaveAttribute("data-parts", "7");

  // Pull the imprint up 3 in (side walls), then push it 20 in down through the far side.
  await page.keyboard.press("p");
  await page.keyboard.type("3");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Pulled 3 in");
  await expect(vp).toHaveAttribute("data-parts", "11");
  const boss = await vp.getAttribute("data-hash");
  await page.keyboard.type("-20");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("genus 1");

  // Undo twice (back to the plain imprint), redo once (the boss again).
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-hash", boss ?? "");
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-parts", "7");
  await page.keyboard.press("Control+y");
  await expect(vp).toHaveAttribute("data-hash", boss ?? "");
  await expect(vp).toHaveAttribute("data-can-redo", "true");
});
