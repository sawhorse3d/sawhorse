import { expect, test, type Page } from "@playwright/test";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { at, box, hover, ready, type V3 } from "./helpers";
import { MAX_BAD, MAX_MEAN, perceptual } from "./perceptual";

// Soft, smooth and hidden edges (lane 15) through the browser build: the Eraser's modifiers
// exactly as its hint says (Ctrl softens and smooths, Shift hides, Ctrl+Shift unsoftens),
// one undo step each; the Styles pane's Soften control on a selected 24-sided cylinder; the
// "Soft and hidden edges" switch; and the golden image of that softened cylinder.

type Counts = { hard: number; soft: number; hidden: number; smoothNormals: boolean };
const counts = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { edgeCounts(): Counts } }).__sawhorse.edgeCounts());
const hint = (page: Page) => page.locator("#hint");
const IN = 0.0254;

/** A 24-sided prism (radius 2 in, 3 in tall) as one import op, built by the app's kernel. */
function cylinderOps(soften: boolean): unknown[] {
  const positions: number[][] = [];
  for (const z of [0, 3])
    for (let k = 0; k < 24; k++) {
      const a = (2 * Math.PI * k) / 24;
      positions.push([2 * IN * Math.cos(a), 2 * IN * Math.sin(a), z * IN]);
    }
  const faces: number[][][] = [[[...Array(24).keys()].reverse()], [[...Array(24).keys()].map((k) => k + 24)]];
  for (let k = 0; k < 24; k++) faces.push([[k, (k + 1) % 24, 24 + ((k + 1) % 24), 24 + k]]);
  const ops: unknown[] = [{ op: "import", items: [{ name: "Dowel", positions, faces, merge_coplanar: false }] }];
  if (soften) ops.push({ op: "soften_edges", objects: [1], angle: 20, soft: true, smooth: true });
  return ops;
}

async function openOps(page: Page, ops: unknown[], name: string) {
  const pkg = path.join("src", "wasm", "pkg");
  const mod = await import(pathToFileURL(path.resolve(pkg, "sawhorse_wasm.js")).href);
  mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  const doc = mod.document_replay(JSON.stringify(ops));
  const shz: Uint8Array = doc.save_shz("frac-in", new Uint8Array());
  doc.free();
  await page.getByTestId("file-input").setInputFiles({ name, mimeType: "application/vnd.sawhorse.shz", buffer: Buffer.from(shz) });
  await expect(page.locator("main.vp")).toHaveAttribute("data-objects", "1", { timeout: 15_000 });
}

test("the Eraser softens, hides and unsoftens edges, as its hint says, one undo step each", async ({ page }) => {
  test.setTimeout(90_000);
  await ready(page);
  await box(page);
  expect(await counts(page)).toEqual({ hard: 12, soft: 0, hidden: 0, smoothNormals: false });
  await page.keyboard.press("e");
  await expect(hint(page)).toContainText("Ctrl softens and smooths, Shift hides, Ctrl+Shift unsoftens");
  // The box's front right upright edge, x = 12 in, y = 0, half way up.
  const upright: V3 = [12, 0, 3];
  const click = async (mods: string[]) => {
    const [x, y] = await at(page, upright, 0, 0);
    for (const m of mods) await page.keyboard.down(m);
    await page.mouse.move(x + 2, y + 1);
    await page.mouse.move(x, y, { steps: 2 });
    await page.waitForTimeout(150);
    await page.mouse.click(x, y);
    for (const m of mods.reverse()) await page.keyboard.up(m);
  };
  await hover(page, upright, "edge", 0, 0, "data-hover");
  await click(["Control"]);
  await expect(hint(page)).toContainText("Softened and smoothed 1 edge.");
  await expect.poll(() => counts(page)).toEqual({ hard: 11, soft: 1, hidden: 0, smoothNormals: true });
  // A soft edge is out of reach of a plain click: the face behind it is under the cursor.
  await page.mouse.move(10, 10);
  // Ctrl+Shift reaches it and unsoftens it.
  await click(["Control", "Shift"]);
  await expect(hint(page)).toContainText("Unsoftened 1 edge.");
  await expect.poll(() => counts(page)).toEqual({ hard: 12, soft: 0, hidden: 0, smoothNormals: false });
  // Shift hides it; Ctrl+Z brings it back in one step.
  await hover(page, upright, "edge", 0, 0, "data-hover");
  await click(["Shift"]);
  await expect(hint(page)).toContainText("Hid 1 edge.");
  await expect.poll(() => counts(page)).toEqual({ hard: 11, soft: 0, hidden: 1, smoothNormals: false });
  await page.keyboard.press("Control+z");
  await expect.poll(() => counts(page)).toEqual({ hard: 12, soft: 0, hidden: 0, smoothNormals: false });
  // Nothing was erased along the way: still one closed box.
  await expect(page.locator("main.vp")).toHaveAttribute("data-objects", "1");
  await expect(page.locator("main.vp")).toHaveAttribute("data-watertight", "true");
});

test("Soften selected in the Styles pane rounds a 24-sided cylinder; soft edges can be shown dashed", async ({ page }) => {
  test.setTimeout(90_000);
  test.skip(!existsSync(path.join("src", "wasm", "pkg", "sawhorse_wasm.js")), "kernel WASM not built");
  await page.goto("/?io=basic&theme=light&autosave=0");
  await expect(page.locator("main.vp")).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  await openOps(page, cylinderOps(false), "dowel.shz");
  expect(await counts(page)).toEqual({ hard: 72, soft: 0, hidden: 0, smoothNormals: false });
  await page.keyboard.press("Control+a");
  await page.getByRole("tab", { name: "Styles" }).click();
  await expect(page.getByTestId("soften")).toContainText("1 solid selected");
  await page.getByRole("button", { name: "Soften selected" }).click();
  await expect(hint(page)).toContainText("24 edges changed");
  await expect.poll(() => counts(page)).toEqual({ hard: 48, soft: 24, hidden: 0, smoothNormals: true });
  // One undo step.
  await page.keyboard.press("Control+z");
  await expect.poll(() => counts(page)).toEqual({ hard: 72, soft: 0, hidden: 0, smoothNormals: false });
  await page.keyboard.press("Control+y");
  await expect.poll(() => counts(page)).toEqual({ hard: 48, soft: 24, hidden: 0, smoothNormals: true });
  // The style's switch shows them dashed (part of the style: saved, one undo step).
  await page.getByLabel("Soft and hidden edges").click();
  await expect(page.getByLabel("Soft and hidden edges")).toBeChecked();
});

test("a softened cylinder renders like its golden image", async ({ page }) => {
  test.setTimeout(90_000);
  test.skip(!existsSync(path.join("src", "wasm", "pkg", "sawhorse_wasm.js")), "kernel WASM not built");
  await page.setViewportSize({ width: 1000, height: 720 });
  await page.goto("/?io=basic&theme=light&autosave=0");
  await expect(page.locator("main.vp")).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  await page.addStyleTag({ content: ".vp .hud,.vp .tip,.vp .axes,.vp .legend,.import-report,.drop-hint{visibility:hidden!important}.scenes{display:none!important}" });
  await openOps(page, cylinderOps(true), "dowel.shz");
  await expect.poll(() => counts(page)).toEqual({ hard: 48, soft: 24, hidden: 0, smoothNormals: true });
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  const png = await page.locator("#cv").screenshot({ animations: "disabled" });
  mkdirSync(path.join("test-results", "corpus"), { recursive: true });
  writeFileSync(path.join("test-results", "corpus", "softened-cylinder.png"), png);
  const ref = path.join("tests", "corpus", "golden", "softened-cylinder.png");
  if (process.env.UPDATE_GOLDEN === "1" || !existsSync(ref)) {
    if (process.env.UPDATE_GOLDEN !== "1") throw new Error(`missing golden image ${ref}: run UPDATE_GOLDEN=1 npx playwright test e2e/edges.spec.ts`);
    writeFileSync(ref, png);
    return;
  }
  const d = await perceptual(page, png, readFileSync(ref));
  expect(d.ink, "the render shows the model").toBeGreaterThan(0.002);
  expect(d.mean, `mean cell difference ${d.mean.toFixed(4)}`).toBeLessThanOrEqual(MAX_MEAN);
  expect(d.bad, `share of very different cells ${d.bad.toFixed(4)}`).toBeLessThanOrEqual(MAX_BAD);
});
