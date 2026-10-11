import { expect, test, type Page } from "@playwright/test";

// Selection and erasing (lane 3): hover highlight, click / Shift / Ctrl, window and crossing
// boxes, Esc, Delete through the kernel (validated), the Erase tool, Ctrl+A.
const requireKernel = !!process.env.REQUIRE_KERNEL;
type V3 = [number, number, number];

async function ready(page: Page) {
  await page.goto("/");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** Page coordinates of a world point. */
async function at(page: Page, p: V3): Promise<[number, number]> {
  const box = await page.locator("#cv").boundingBox();
  const xy = await page.evaluate((p) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(p), p);
  return [box!.x + xy[0], box!.y + xy[1]];
}

const IN = 0.0254;

/** A 24 x 16 x 12 in box with a 8 x 6 in imprint centred on its top. */
async function boxWithImprint(page: Page) {
  const vp = await ready(page);
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-triangles", "2");
  await page.keyboard.press("p");
  await page.keyboard.type("12");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  await expect(vp).toHaveAttribute("data-parts", "6");
  await page.keyboard.press("r");
  const a = await at(page, [8 * IN, 5 * IN, 12 * IN]);
  const b = await at(page, [16 * IN, 11 * IN, 12 * IN]);
  await page.mouse.click(a[0], a[1]);
  await page.mouse.click(b[0], b[1]);
  await expect(vp).toHaveAttribute("data-parts", "7");
  await page.keyboard.press("Space");
  return vp;
}

test("hover, click, Shift adds, Ctrl toggles, Esc clears", async ({ page }) => {
  const vp = await boxWithImprint(page);
  const top = await at(page, [3 * IN, 8 * IN, 12 * IN]);
  const imprint = await at(page, [12 * IN, 8 * IN, 12 * IN]);
  await page.mouse.move(top[0], top[1]);
  await expect(vp).toHaveAttribute("data-hover", "face");
  await page.mouse.click(top[0], top[1]);
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(page.locator("#hint")).toContainText("Selected face 2 of solid 1");
  await page.keyboard.down("Shift");
  await page.mouse.click(imprint[0], imprint[1]);
  await page.keyboard.up("Shift");
  await expect(vp).toHaveAttribute("data-selection", "2");
  await page.keyboard.down("Control");
  await page.mouse.click(top[0], top[1]);
  await page.keyboard.up("Control");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(page.locator("#hint")).toContainText("Selected face 2"); // last message was the toggle
  await page.keyboard.press("Escape");
  await expect(vp).toHaveAttribute("data-selection", "0");
  // An edge and a vertex.
  const edge = await at(page, [12 * IN, 0, 12 * IN]);
  await page.mouse.move(edge[0], edge[1]);
  await expect(vp).toHaveAttribute("data-hover", "edge");
  const corner = await at(page, [24 * IN, 0, 12 * IN]);
  await page.mouse.move(corner[0], corner[1]);
  await expect(vp).toHaveAttribute("data-hover", "vertex");
  await page.mouse.click(corner[0], corner[1]);
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(page.locator("#hint")).toContainText("Selected a point");
});

test("window selects what is inside, crossing selects what it touches; double-click selects the solid", async ({ page }) => {
  const vp = await boxWithImprint(page);
  const box = (await page.locator("#cv").boundingBox())!;
  // Window around everything (left to right).
  await page.mouse.move(box.x + 20, box.y + 20);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 20, box.y + box.height - 20, { steps: 8 });
  await expect(page.locator(".vp .selbox")).toBeVisible();
  await expect(page.locator(".vp .selbox")).not.toHaveClass(/crossing/);
  await page.mouse.up();
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(page.locator("#hint")).toContainText("Window selection: 1 item");
  await page.keyboard.press("Escape");
  // A small window over part of the box selects nothing...
  const c = await at(page, [12 * IN, 8 * IN, 12 * IN]);
  await page.mouse.move(c[0] - 30, c[1] - 10);
  await page.mouse.down();
  await page.mouse.move(c[0] + 30, c[1] + 10, { steps: 5 });
  await page.mouse.up();
  await expect(page.locator("#hint")).toContainText("Window selection: 0 items");
  // ...but the same box dragged right to left (crossing) selects the solid.
  await page.mouse.move(c[0] + 30, c[1] - 10);
  await page.mouse.down();
  await page.mouse.move(c[0] - 30, c[1] + 10, { steps: 5 });
  await expect(page.locator(".vp .selbox")).toHaveClass(/crossing/);
  await page.mouse.up();
  await expect(vp).toHaveAttribute("data-selection", "1");
  await expect(page.locator("#hint")).toContainText("Crossing selection: 1 item");
  await page.keyboard.press("Escape");
  await page.mouse.dblclick(c[0], c[1]);
  await expect(page.locator("#hint")).toContainText("Selected solid 1");
});

test("Delete erases through the kernel; structural edges are refused; Erase tool; Ctrl+A", async ({ page }) => {
  const vp = await boxWithImprint(page);
  const imprint = await at(page, [12 * IN, 8 * IN, 12 * IN]);
  // Select the imprinted face and delete it: it merges back into the top.
  await page.mouse.click(imprint[0], imprint[1]);
  await page.keyboard.press("Delete");
  await expect(vp).toHaveAttribute("data-parts", "6");
  await expect(page.locator("#hint")).toContainText("Erased 1 item");
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-parts", "7");

  // The Erase tool: a box edge would open the solid (refused, nothing changes).
  const hash = await vp.getAttribute("data-hash");
  await page.keyboard.press("e");
  const edge = await at(page, [12 * IN, 0, 12 * IN]);
  await page.mouse.click(edge[0], edge[1]);
  await expect(page.locator("#hint")).toHaveClass(/err/);
  await expect(page.locator("#hint")).toContainText("would open the solid");
  await expect(vp).toHaveAttribute("data-hash", hash ?? "");
  // One edge of the imprint: the imprint goes.
  const imprintEdge = await at(page, [12 * IN, 5 * IN, 12 * IN]);
  await page.mouse.click(imprintEdge[0], imprintEdge[1]);
  await expect(vp).toHaveAttribute("data-parts", "6");

  // Ctrl+A selects everything; Delete removes it; Ctrl+Z brings it back.
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await page.keyboard.press("Delete");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-objects", "1");
});
