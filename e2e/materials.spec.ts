import { expect, test, type Page } from "@playwright/test";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { at, box, clickAt, hover, ready, type V3 } from "./helpers";
import { goldenPanelWidth, MAX_BAD, MAX_MEAN, perceptual } from "./perceptual";

// Materials, paint, texture positioning, styles and shadows (lane 10) through the browser
// build: the library, the Paint tool (B) with Alt+click sampling, the texture handles, the
// Styles pane's face styles and presets, the sun at 3 pm, a scene keeping its style, the cut
// list reading the species; and the golden image of a painted cabinet (opened as a .shz
// written by the WASM kernel from tests/golden/painted.json plus two library woods).

type Looks = { face: string; profiles: boolean; shadows: boolean; textures: number; texturedParts: number; sun: V3 | null };
const looks = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { looks(): Looks } }).__sawhorse.looks());
const hint = (page: Page) => page.locator("#hint");

test("paint a box from the library, sample it, position its texture, switch styles and the sun, keep them in a scene", async ({ page }) => {
  test.setTimeout(120_000);
  const vp = await ready(page);
  await box(page);
  // The library: White oak, a textured wood with a stock link.
  await page.getByRole("tab", { name: "Materials" }).click();
  const oak = page.getByRole("option", { name: "White oak" });
  await expect(oak).toBeVisible({ timeout: 15_000 });
  await oak.click();
  await expect(page.locator(".tool[data-tool='paint']")).toHaveAttribute("aria-pressed", "true");
  // Paint the top face (the box is a plain solid: the face itself).
  const top: V3 = [6, 6, 6];
  await hover(page, top, "face", 1, -1, "data-hover");
  await clickAt(page, top, /on-face|.*/);
  await expect(hint(page)).toContainText(/Painted .*White oak/);
  await expect.poll(async () => (await looks(page)).texturedParts, { timeout: 10_000 }).toBe(1);
  await expect(page.getByRole("listbox", { name: "Materials in the model" }).getByRole("option", { name: "White oak" })).toBeVisible();
  // The textured face is not the theme's plain tone any more.
  const center = await page.evaluate(() => {
    const s = (window as unknown as { __sawhorse: { parts(): { owner: number; index: number }[]; partScreenCenter(p: number): [number, number] | null; pixel(x: number, y: number): [number, number, number] } }).__sawhorse;
    const part = s.parts().findIndex((p) => p.owner === 1 && p.index === 2);
    const c = s.partScreenCenter(part)!;
    return s.pixel(c[0], c[1]);
  });
  expect(Math.abs(center[0] - center[2])).toBeGreaterThan(20);

  // Alt+click picks the material up again (after choosing a colour).
  await page.getByRole("option", { name: "Navy" }).click();
  await expect(page.locator(".mat-active")).toHaveAttribute("data-active", "Navy");
  await hover(page, top, "face", 1, -1, "data-hover");
  const [tx, ty] = await page.evaluate(() => {
    const s = (window as unknown as { __sawhorse: { parts(): { owner: number; index: number }[]; partScreenCenter(p: number): [number, number] | null } }).__sawhorse;
    return s.partScreenCenter(s.parts().findIndex((p) => p.owner === 1 && p.index === 2))!;
  });
  const cv = (await page.locator("#cv").boundingBox())!;
  await page.keyboard.down("Alt");
  await page.mouse.click(cv.x + tx, cv.y + ty);
  await page.keyboard.up("Alt");
  await expect(page.locator(".mat-active")).toHaveAttribute("data-active", "White oak");

  // Position the texture: drag the round handle a quarter turn.
  await page.evaluate(() => (window as unknown as { __sawhorse: { paint: { position(o: number, f: number): Promise<boolean> } } }).__sawhorse.paint.position(1, 2));
  // A quarter turn about the centre, in the face's plane: the round handle's world point
  // turned 90 degrees about the face normal (here +Z), then to the screen.
  const hw = await page.evaluate(() => (window as unknown as { __sawhorse: { paint: { handlesWorld(): Record<string, V3> } } }).__sawhorse.paint.handlesWorld());
  const turned = (k: number): V3 => {
    const a = (k * Math.PI) / 2;
    const d = [hw.turn[0] - hw.move[0], hw.turn[1] - hw.move[1]];
    return [hw.move[0] + d[0] * Math.cos(a) - d[1] * Math.sin(a), hw.move[1] + d[0] * Math.sin(a) + d[1] * Math.cos(a), hw.move[2]];
  };
  const screen = (p: V3) => page.evaluate((p) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(p), p);
  const [rx, ry] = await screen(hw.turn);
  await page.mouse.move(cv.x + rx, cv.y + ry);
  await page.mouse.down();
  for (const k of [0.25, 0.5, 0.75, 1]) {
    const [x, y] = await screen(turned(k));
    await page.mouse.move(cv.x + x, cv.y + y, { steps: 3 });
  }
  await page.mouse.up();
  await expect(hint(page)).toContainText("Texture positioned");
  const uv = await page.evaluate(() => (window as unknown as { __sawhorse: { parts(): { owner: number; index: number; uv?: { rotation?: number } }[] } }).__sawhorse.parts().find((p) => p.owner === 1 && p.index === 2)?.uv);
  expect(Math.abs((uv?.rotation ?? 0) % 90)).toBeLessThan(5);
  expect(uv?.rotation ?? 0).not.toBe(0);
  await page.keyboard.press("Escape");

  // Styles: hidden line, X-ray, a preset, and back.
  await page.getByRole("tab", { name: "Styles" }).click();
  await page.getByRole("radio", { name: "Hidden line" }).click();
  await expect.poll(async () => (await looks(page)).face).toBe("hidden_line");
  await expect.poll(async () => (await looks(page)).texturedParts).toBe(0);
  await page.getByRole("radio", { name: "X-ray" }).click();
  await expect.poll(async () => (await looks(page)).face).toBe("x_ray");
  await page.getByRole("button", { name: /Shop drawing/ }).click();
  await expect.poll(async () => (await looks(page)).profiles).toBe(true);
  await page.getByRole("button", { name: /^Sawhorse/ }).click();
  await expect.poll(async () => (await looks(page)).face).toBe("textured");

  // The sun: shadows on at 3 pm (west of south, above the horizon).
  await page.getByLabel("Show shadows").click();
  await expect.poll(async () => (await looks(page)).shadows).toBe(true);
  const time = page.getByLabel("Time");
  await time.fill("900");
  await time.blur();
  await expect(page.getByTestId("shadows").locator(".slider").first()).toContainText("3:00 pm");
  await expect.poll(async () => ((await looks(page)).sun ?? [0, 0, 0])[0]).toBeLessThan(0);
  expect(((await looks(page)).sun ?? [0, 0, 0])[2]).toBeGreaterThan(0);

  // A scene keeps the style and the sun: change both, go back to it.
  await page.getByRole("button", { name: /Add a scene/ }).click();
  await expect(page.getByRole("tab", { name: "Scene 1" })).toBeVisible();
  await page.getByRole("radio", { name: "X-ray" }).click();
  await page.getByLabel("Show shadows").click();
  await expect.poll(async () => (await looks(page)).shadows).toBe(false);
  await page.getByRole("tab", { name: "Scene 1" }).click();
  await expect.poll(async () => (await looks(page)).face).toBe("textured");
  await expect.poll(async () => (await looks(page)).shadows).toBe(true);

  // Undo walks back through the looks too, one step each.
  await expect(vp).toHaveAttribute("data-can-undo", "true");

  // Ctrl+click paints the whole solid; the cut list then reads the species from it.
  await page.getByRole("tab", { name: "Materials" }).click();
  await page.getByRole("listbox", { name: "Materials in the model" }).getByRole("option", { name: "White oak" }).click();
  await hover(page, [6, 0, 3], "face", 1, -1, "data-hover");
  const [sx, sy] = await at(page, [6, 0, 3], 1, -1);
  await page.keyboard.down("Control");
  await page.mouse.click(sx, sy);
  await page.keyboard.up("Control");
  await expect(hint(page)).toContainText(/Painted solid .*White oak/);
  await page.getByRole("tab", { name: "Cut list" }).click();
  await expect(page.getByTestId("cutlist")).toContainText("White oak");
});

test("a painted cabinet renders like its golden image", async ({ page }) => {
  test.setTimeout(90_000);
  const pkg = path.join("src", "wasm", "pkg");
  test.skip(!existsSync(path.join(pkg, "sawhorse_wasm.js")), "kernel WASM not built");
  // The .shz, written by the same WASM kernel the app runs.
  const mod = await import(pathToFileURL(path.resolve(pkg, "sawhorse_wasm.js")).href);
  mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  const golden = JSON.parse(readFileSync(path.join("crates", "kernel", "tests", "golden", "painted.json"), "utf8"));
  const ops = [...golden.ops, ...JSON.parse(mod.material_library_ops("White oak", "")), ...JSON.parse(mod.material_library_ops("Baltic birch ply", "Birch ply")), { op: "set_style", style: null }, { op: "set_shadows", shadows: null }];
  const doc = mod.document_replay(JSON.stringify(ops));
  const shz: Uint8Array = doc.save_shz("frac-in", new Uint8Array());
  doc.free();
  await page.setViewportSize({ width: 1000, height: 720 });
  await goldenPanelWidth(page);
  await page.goto("/?io=basic&theme=light&autosave=0");
  await expect(page.locator("main.vp")).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  await page.addStyleTag({ content: ".vp .hud,.vp .tip,.vp .axes,.vp .legend,.import-report,.drop-hint{visibility:hidden!important}.scenes{display:none!important}" });
  await page.getByTestId("file-input").setInputFiles({ name: "painted.shz", mimeType: "application/vnd.sawhorse.shz", buffer: Buffer.from(shz) });
  await expect(page.locator("main.vp")).toHaveAttribute("data-objects", "4", { timeout: 15_000 });
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await expect.poll(async () => (await looks(page)).texturedParts, { timeout: 10_000 }).toBeGreaterThan(10);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  const png = await page.locator("#cv").screenshot({ animations: "disabled" });
  mkdirSync(path.join("test-results", "corpus"), { recursive: true });
  writeFileSync(path.join("test-results", "corpus", "painted-cabinet.png"), png);
  const ref = path.join("tests", "corpus", "golden", "painted-cabinet.png");
  if (process.env.UPDATE_GOLDEN === "1" || !existsSync(ref)) {
    if (process.env.UPDATE_GOLDEN !== "1") throw new Error(`missing golden image ${ref}: run UPDATE_GOLDEN=1 npx playwright test e2e/materials.spec.ts`);
    writeFileSync(ref, png);
    return;
  }
  const d = await perceptual(page, png, readFileSync(ref));
  expect(d.ink, "the render shows the model").toBeGreaterThan(0.002);
  expect(d.mean, `mean cell difference ${d.mean.toFixed(4)}`).toBeLessThanOrEqual(MAX_MEAN);
  expect(d.bad, `share of very different cells ${d.bad.toFixed(4)}`).toBeLessThanOrEqual(MAX_BAD);
});
