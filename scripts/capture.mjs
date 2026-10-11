#!/usr/bin/env node
// Lane-end screenshot of the running app (ADR build rules: every lane ends with a capture).
//
//   node scripts/capture.mjs --out ../_captures/2026-10-11-lane01-shell.png [--url http://localhost:5173]
//        [--theme light|dark] [--tool p] [--type 3/4] [--hover endpoint] [--size 1440x900]
//
// Uses the installed Edge by default (PW_CHANNEL=msedge) so nothing is downloaded.
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const url = args.url ?? "http://localhost:5173";
const out = path.resolve(args.out ?? "capture.png");
const [w, h] = (args.size ?? "1440x900").split("x").map(Number);
const theme = args.theme ?? "light";

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
try {
  const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
  await page.goto(`${url}/?theme=${theme}`);
  await page.waitForSelector('main.vp[data-kernel="ready"], main.vp[data-kernel="missing"]', { timeout: 20_000 });
  if (args.tool) await page.keyboard.press(args.tool);
  if (args.type) {
    await page.keyboard.type(args.type);
    await page.keyboard.press("Enter");
    await page.waitForFunction((t) => document.querySelector(".hud")?.textContent?.includes(t), args.type.split(",")[0], { timeout: 10_000 });
  }
  if (args.hover) {
    // Hover a visible box corner so the coarse inference shows its tip (Endpoint).
    const pts = await page.evaluate(() => window.__sawhorse?.visibleEndpoints() ?? []);
    const box = await page.locator("#cv").boundingBox();
    // Prefer the corner nearest the top of the view (the cap), like a user reaching for it.
    pts.sort((a, b) => a[1] - b[1]);
    if (pts.length && box) {
      const [x, y] = pts[Math.min(1, pts.length - 1)];
      await page.mouse.move(box.x + x + 2, box.y + y + 2);
      await page.waitForSelector("#tip", { timeout: 5_000 }).catch(() => console.warn("capture: no hover tip"));
    }
  }
  await page.waitForTimeout(300);
  mkdirSync(path.dirname(out), { recursive: true });
  await page.screenshot({ path: out });
  const state = await page.evaluate(() => ({
    kernel: document.querySelector("main.vp")?.getAttribute("data-kernel"),
    triangles: document.querySelector("main.vp")?.getAttribute("data-triangles"),
    hash: document.querySelector("main.vp")?.getAttribute("data-hash"),
    hud: document.querySelector(".hud")?.textContent,
    hint: document.getElementById("hint")?.textContent,
    tip: document.getElementById("tip")?.textContent ?? null,
  }));
  console.log(JSON.stringify({ out, ...state }, null, 2));
} finally {
  await browser.close();
}
