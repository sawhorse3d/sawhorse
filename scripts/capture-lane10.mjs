#!/usr/bin/env node
// Lane-10 lane-end walk-through: materials, paint, texture positioning, styles, shadows and
// a scene, driven like a person at the mouse and the panes, with a screenshot after each
// stage:
//
//   1. the user guide's bookshelf (components), opened as a .shz the WASM kernel writes from
//      crates/kernel/tests/golden/cutlist.json (without its "Oak" paint);
//   2. painted White oak (every part selected, one click), the back Baltic birch ply (clicked
//      where it shows between the shelves), the top's texture turned and slid with the handles;
//   3. hidden line, 4. X-ray;
//   5. shadows on at 3 pm (with the frame time of the lit, textured bookshelf);
//   6. a scene saved, 7. the style changed and the scene bringing it back;
//   8. the cut list reading the species; 9. the Materials pane.
//
//   npm run dev -- --port 5203 --strictPort   (in another terminal)
//   node scripts/capture-lane10.mjs --url http://localhost:5203 --out ../_captures
//
// Uses the installed Edge (PW_CHANNEL=msedge). Exits non-zero when a stage does not produce
// what it should.
import { chromium } from "@playwright/test";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const url = args.url ?? "http://localhost:5203";
const outDir = path.resolve(args.out ?? "../_captures");
const prefix = args.prefix ?? "2026-10-12-lane10";
mkdirSync(outDir, { recursive: true });
const fail = (msg) => {
  throw new Error(msg);
};

// The bookshelf as a .shz, from the cut list golden (components: sides, shelves, top, toe
// kick, back panel).
const pkg = path.join("src", "wasm", "pkg");
const mod = await import(pathToFileURL(path.resolve(pkg, "sawhorse_wasm.js")).href);
mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
const golden = JSON.parse(readFileSync(path.join("crates", "kernel", "tests", "golden", "cutlist.json"), "utf8"));
// Unpainted: the golden paints its solids "Oak", and a solid's own material would keep the
// instance paint below from showing (as it should).
const ops = golden.ops.filter((o) => o.op !== "define_material" && o.op !== "set_material");
const doc = mod.document_replay(JSON.stringify(ops));
const shz = Buffer.from(doc.save_shz("frac-in", new Uint8Array()));
doc.free();

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
const log = [];
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  page.setDefaultTimeout(15_000);
  await page.goto(`${url}/?theme=light&autosave=0&io=basic`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
  const vp = page.locator("main.vp");
  const hint = page.locator("#hint");
  const shot = async (n, name, what) => {
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    const file = path.join(outDir, `${prefix}-${String(n).padStart(2, "0")}-${name}.png`);
    await page.screenshot({ path: file });
    log.push(`${n}. ${what} -> ${path.basename(file)}`);
    console.log(log.at(-1));
  };
  const looks = () => page.evaluate(() => window.__sawhorse.looks());
  const waitFor = async (fn, what, timeout = 10_000) => {
    const t0 = Date.now();
    while (Date.now() - t0 < timeout) {
      if (await fn()) return;
      await page.waitForTimeout(100);
    }
    fail(`timed out: ${what}`);
  };
  const cv = async () => page.locator("#cv").boundingBox();
  const solids = () => page.evaluate(() => window.__sawhorse.solids());
  const parts = () => page.evaluate(() => window.__sawhorse.parts());
  // The screen centre of the largest face of `object` whose normal points along `dir`.
  const faceAt = async (object, dir) => {
    const ps = await parts();
    const i = ps.findIndex((p) => p.kind === "face" && p.owner === object && p.normal[0] * dir[0] + p.normal[1] * dir[1] + p.normal[2] * dir[2] > 0.9);
    if (i < 0) fail(`no face of ${object} along ${dir}`);
    const c = await page.evaluate((i) => window.__sawhorse.partScreenCenter(i), i);
    const b = await cv();
    return { part: i, face: ps[i].index, x: b.x + c[0], y: b.y + c[1] };
  };

  // 1. Open the bookshelf.
  await page.getByTestId("file-input").setInputFiles({ name: "bookshelf.shz", mimeType: "application/vnd.sawhorse.shz", buffer: shz });
  await waitFor(async () => Number(await vp.getAttribute("data-objects")) >= 9, "the bookshelf opens");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await shot(1, "bookshelf", "the component bookshelf, opened");

  // 2. White oak on every part: select all, choose White oak, click a selected part.
  await page.getByRole("tab", { name: "Materials" }).click();
  await page.waitForSelector('[role="option"]:has-text("White oak")');
  await page.keyboard.press(" ");
  await page.locator("#cv").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("Control+a");
  await page.getByRole("option", { name: "White oak" }).click();
  const all = await solids();
  const side = all.reduce((a, b) => (b.max[2] - b.min[2] > a.max[2] - a.min[2] ? b : a));
  const sideFace = await faceAt(side.id, [-1, 0, 0]);
  await page.mouse.move(sideFace.x, sideFace.y, { steps: 3 });
  await page.mouse.click(sideFace.x, sideFace.y);
  await waitFor(async () => /Painted .*White oak/.test((await hint.textContent()) ?? ""), "the parts are painted");
  await waitFor(async () => (await looks()).texturedParts > 20, "the oak texture shows");
  // The back panel: Baltic birch ply, clicked where it shows between the shelves.
  await page.keyboard.press(" ");
  await page.keyboard.press("Escape");
  await page.getByRole("option", { name: "Baltic birch ply" }).click();
  const b0 = await cv();
  const back = all.find((s) => s.max[1] - s.min[1] < 0.008 && s.max[2] - s.min[2] > 1);
  if (!back) fail("no back panel");
  // A point of its front face showing between the shelves: try a grid over the face until
  // the pick lands on the panel itself.
  let backFace = null;
  for (let gz = 1; gz < 16 && !backFace; gz++) {
    for (let gx = 1; gx < 6 && !backFace; gx++) {
      const w = [back.min[0] + ((back.max[0] - back.min[0]) * gx) / 6, back.min[1] - 1e-4, back.min[2] + ((back.max[2] - back.min[2]) * gz) / 16];
      const [sx, sy] = await page.evaluate((p) => window.__sawhorse.worldToScreen(p), w);
      const hit = await page.evaluate(([x, y]) => window.__sawhorse.pick(x, y), [sx, sy]);
      if ((await parts())[hit?.part ?? -1]?.owner === back.id) backFace = { x: b0.x + sx, y: b0.y + sy };
    }
  }
  if (!backFace) fail("could not see the back panel");
  await page.mouse.move(backFace.x, backFace.y, { steps: 3 });
  await page.mouse.click(backFace.x, backFace.y);
  await waitFor(async () => /Painted .*Baltic birch ply/.test((await hint.textContent()) ?? ""), "the back is painted");
  await shot(2, "back-birch-ply", "the back panel painted Baltic birch ply");
  // Position the top's texture: open the top component, drag the round handle and the centre.
  // From above, so the handles and the grain are easy to see.
  await page.getByRole("button", { name: "Top", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  const top = all.reduce((a, b) => (b.max[2] > a.max[2] ? b : a));
  const above = await faceAt(top.id, [0, 0, 1]);
  await page.mouse.move(above.x, above.y);
  for (let k = 0; k < 6; k++) {
    await page.mouse.wheel(0, -240);
    await page.waitForTimeout(60);
  }
  const topFace = await faceAt(top.id, [0, 0, 1]);
  await page.keyboard.press(" ");
  await page.mouse.dblclick(topFace.x, topFace.y);
  await waitFor(async () => /Editing/.test((await hint.textContent()) ?? ""), "the top component opens");
  const ok = await page.evaluate(([o, f]) => window.__sawhorse.paint.position(o, f), [top.id, topFace.face]);
  if (!ok) fail("positioning did not start");
  const hs = await page.evaluate(() => window.__sawhorse.paint.handles());
  const b1 = await cv();
  const [mx, my] = hs.move;
  const [rx, ry] = hs.turn;
  await page.mouse.move(b1.x + rx, b1.y + ry);
  await page.mouse.down();
  const ang = (30 * Math.PI) / 180;
  const dx = rx - mx;
  const dy = ry - my;
  await page.mouse.move(b1.x + mx + dx * Math.cos(ang) - dy * Math.sin(ang), b1.y + my + dx * Math.sin(ang) + dy * Math.cos(ang), { steps: 10 });
  await page.mouse.up();
  await waitFor(async () => /Texture positioned/.test((await hint.textContent()) ?? ""), "the texture turns");
  await page.mouse.move(b1.x + mx, b1.y + my);
  await page.mouse.down();
  await page.mouse.move(b1.x + mx + 40, b1.y + my + 10, { steps: 8 });
  await page.mouse.up();
  await shot(3, "texture-positioned", "the top's grain turned 30 degrees and slid with the handles");
  // Esc ends positioning, Esc again closes the top component.
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await shot(4, "painted", "White oak bookshelf with a birch ply back");

  // 3-4. Hidden line, X-ray.
  await page.getByRole("tab", { name: "Styles" }).click();
  await page.getByRole("radio", { name: "Hidden line" }).click();
  await waitFor(async () => (await looks()).face === "hidden_line", "hidden line");
  await shot(5, "hidden-line", "Styles: hidden line");
  await page.getByRole("radio", { name: "X-ray" }).click();
  await waitFor(async () => (await looks()).face === "x_ray", "x-ray");
  await shot(6, "x-ray", "Styles: X-ray");
  await page.getByRole("button", { name: /Shop drawing/ }).click();
  await waitFor(async () => (await looks()).profiles, "profiles");
  await shot(7, "shop-drawing", "Styles: the Shop drawing preset (hidden line, profiles, extensions)");
  await page.getByRole("button", { name: /^Sawhorse/ }).click();
  await waitFor(async () => (await looks()).face === "textured", "back to textured");

  // 5. Shadows at 3 pm.
  await page.getByLabel("Show shadows").click();
  await waitFor(async () => (await looks()).shadows, "shadows on");
  const time = page.getByLabel("Time");
  await time.fill("900");
  await time.blur();
  await waitFor(async () => ((await looks()).sun ?? [0])[0] < 0, "the sun in the west");
  const ms = await page.evaluate(() => window.__sawhorse.frameTime(120));
  log.push(`   frame time, lit and textured bookshelf: ${ms.toFixed(2)} ms (${(1000 / ms).toFixed(0)} fps)`);
  console.log(log.at(-1));
  await shot(8, "shadows-3pm", "shadows on, 3:00 pm");

  // 6-7. A scene keeps the style and the sun.
  await page.getByRole("button", { name: /Add a scene/ }).click();
  await page.getByRole("tab", { name: "Scene 1" }).waitFor();
  await shot(9, "scene-saved", "scene saved with the style and the sun");
  await page.getByRole("radio", { name: "X-ray" }).click();
  await page.getByLabel("Show shadows").click();
  await waitFor(async () => !(await looks()).shadows, "shadows off");
  await page.getByRole("tab", { name: "Scene 1" }).click();
  await waitFor(async () => (await looks()).shadows && (await looks()).face === "textured", "the scene brings the style back");
  await shot(10, "scene-restored", "X-ray and no shadows, then the scene: textures and 3 pm shadows again");

  // 8. The cut list reads the species.
  await page.getByRole("tab", { name: "Cut list" }).click();
  await waitFor(async () => /White oak/.test((await page.getByTestId("cutlist").textContent()) ?? ""), "the cut list says White oak");
  await shot(11, "cutlist-species", "the cut list: White oak lumber, Baltic birch ply sheet");
  await page.getByRole("tab", { name: "Materials" }).click();
  await shot(12, "materials-pane", "the Materials pane: the model's materials and the library");
  writeFileSync(path.join(outDir, `${prefix}-log.txt`), log.join("\n") + "\n");
} finally {
  await browser.close();
}
