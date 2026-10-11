#!/usr/bin/env node
// Lane 2 "verify by looking": drive the running app (npm run dev) through the kernel flow
// with real clicks and keys, and screenshot every state:
//
//   1 rectangle drawn on the ground (two clicks)      5 a face pushed through the far side
//   2 pulled up into a box (click region, type 12)    6 undo (back to the boss)
//   3 rectangle drawn on the top face (imprint)       7 undo (back to the imprint)
//   4 imprint pulled up 4 in (side walls)             8 redo (the boss again)
//
//   node scripts/capture-lane02.mjs [--url http://localhost:5173] [--out ../_captures] [--theme light]
//
// Uses the installed Edge (PW_CHANNEL=msedge by default) so nothing is downloaded. Prints
// the kernel state after each step and exits non-zero if any step did not land.
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const url = args.url ?? "http://localhost:5173";
const outDir = path.resolve(args.out ?? "../_captures");
const theme = args.theme ?? "light";
const IN = 0.0254;
mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
let failed = false;
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  await page.goto(`${url}/?theme=${theme}`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
  const canvas = await page.locator("#cv").boundingBox();
  const vp = page.locator("main.vp");

  const screen = async (p) => {
    const xy = await page.evaluate((p) => window.__sawhorse.worldToScreen(p), p);
    return [canvas.x + xy[0], canvas.y + xy[1]];
  };
  const clickWorld = async (p) => {
    const [x, y] = await screen(p);
    await page.mouse.move(x - 8, y - 8);
    await page.mouse.move(x, y, { steps: 4 });
    await page.mouse.click(x, y);
  };
  const partCenter = async (pred) => {
    const xy = await page.evaluate((src) => {
      const h = window.__sawhorse;
      const test = new Function("p", `return (${src})(p)`);
      const i = h.parts().findIndex(test);
      return i < 0 ? null : h.partScreenCenter(i);
    }, pred.toString());
    if (!xy) throw new Error(`no part matches ${pred}`);
    return [canvas.x + xy[0], canvas.y + xy[1]];
  };
  const type = async (text) => {
    await page.keyboard.type(text);
    await page.keyboard.press("Enter");
  };
  const state = () =>
    page.evaluate(() => {
      const v = document.querySelector("main.vp");
      return {
        triangles: v.getAttribute("data-triangles"),
        parts: v.getAttribute("data-parts"),
        hash: v.getAttribute("data-hash"),
        watertight: v.getAttribute("data-watertight"),
        hud: [...document.querySelectorAll(".hud span")].map((s) => s.textContent),
        hint: document.getElementById("hint")?.textContent,
      };
    });
  let n = 0;
  const shot = async (name, expect) => {
    // Park the pointer in an empty corner so the hover tip does not cover the model.
    await page.mouse.move(canvas.x + canvas.width - 20, canvas.y + canvas.height - 20);
    await page.waitForTimeout(250);
    const file = path.join(outDir, `2026-10-11-lane02-${String(++n).padStart(2, "0")}-${name}.png`);
    await page.screenshot({ path: file });
    const s = await state();
    const ok = expect(s);
    if (!ok) failed = true;
    console.log(JSON.stringify({ step: n, name, ok, file, ...s }));
  };

  // 1. Rectangle on the ground: Rect tool, two clicks.
  await page.keyboard.press("r");
  await clickWorld([0, 0, 0]);
  await clickWorld([24 * IN, 16 * IN, 0]);
  await vp.locator('xpath=.').evaluate(() => 0);
  await page.waitForFunction(() => document.querySelector("main.vp").getAttribute("data-parts") === "1");
  await shot("rectangle", (s) => s.parts === "1" && s.hint.includes("on the ground"));

  // 2. Pull it into a box: Push/Pull, click the region, type 12.
  await page.keyboard.press("p");
  const region = await partCenter((p) => p.kind === "region");
  await page.mouse.click(region[0], region[1]);
  await type("12");
  await page.waitForFunction(() => document.querySelector("main.vp").getAttribute("data-triangles") === "12");
  await shot("box", (s) => s.triangles === "12" && s.watertight === "true");

  // 3. Rectangle on the box's top face: an imprint (sub-face + twin hole).
  await page.keyboard.press("r");
  await clickWorld([6 * IN, 4 * IN, 12 * IN]);
  await clickWorld([18 * IN, 12 * IN, 12 * IN]);
  await page.waitForFunction(() => document.querySelector("main.vp").getAttribute("data-parts") === "7");
  await shot("imprint-on-top", (s) => s.parts === "7" && s.hint.includes("imprinted"));

  // 4. Pull the imprint up 4 in: side walls.
  await page.keyboard.press("p");
  await type("4");
  await page.waitForFunction(() => document.querySelector("main.vp").getAttribute("data-parts") === "11");
  await shot("boss-side-walls", (s) => s.parts === "11" && s.hint.includes("Pulled 4 in"));

  // 5. Push the boss's top down through the far side of the box: the boolean path.
  await type("-20");
  await page.waitForFunction(() => document.getElementById("hint")?.textContent?.includes("genus 1"));
  await shot("push-through-boolean", (s) => s.hint.includes("genus 1") && s.watertight === "true");

  // 6-7. Undo twice; 8. redo once.
  await page.keyboard.press("Control+z");
  await page.waitForFunction(() => document.querySelector("main.vp").getAttribute("data-parts") === "11");
  await shot("undo-1-boss", (s) => s.parts === "11");
  await page.keyboard.press("Control+z");
  await page.waitForFunction(() => document.querySelector("main.vp").getAttribute("data-parts") === "7");
  await shot("undo-2-imprint", (s) => s.parts === "7");
  await page.keyboard.press("Control+y");
  await page.waitForFunction(() => document.querySelector("main.vp").getAttribute("data-parts") === "11");
  await shot("redo-boss", (s) => s.parts === "11");
} finally {
  await browser.close();
}
process.exitCode = failed ? 1 : 0;
