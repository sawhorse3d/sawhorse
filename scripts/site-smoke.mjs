#!/usr/bin/env node
// Smoke test of the assembled site (scripts/build-site.mjs), served the way GitHub Pages
// serves it (scripts/serve-site.mjs), in a real browser:
//
//   1. the landing page loads with no failed requests; "Try it in your browser" goes to app/;
//   2. the demo's kernel (WASM in its worker) loads under the base path; the banner shows;
//   3. a box: R, 36,12, Enter; P, 3/4, Enter -> one watertight solid of 324 cubic inches;
//   4. Ctrl+S saves a .shz (download path, ?io=basic), File > New clears it, and
//      File > Open brings it back (the same handlers run behind the native pickers);
//   5. the banner dismisses and stays dismissed after a reload;
//   6. the guide and the changelog render.
//
//   node scripts/site-smoke.mjs [--dir dist-site] [--port 5214] [--prefix /] [--shots <dir> --shot-prefix <name>]
//
// Local Windows runs use the installed Edge (PW_CHANNEL=msedge); CI uses Playwright's Chromium.
import { chromium } from "@playwright/test";
import { mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { serveSite } from "./serve-site.mjs";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const argv = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 && argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[i + 1] : dflt;
};
const dir = path.resolve(root, opt("dir", "dist-site"));
const shots = opt("shots", "") ? path.resolve(opt("shots", "")) : "";
const shotPrefix = opt("shot-prefix", "site");
if (shots) mkdirSync(shots, { recursive: true });
const IN = 0.0254;

const server = await serveSite({ dir, port: Number(opt("port", 5214)), prefix: opt("prefix", "/") });
const browser = await chromium.launch({ channel: process.env.PW_CHANNEL || undefined });
const steps = [];
const ok = (msg) => {
  steps.push(msg);
  console.log(`[smoke] ok: ${msg}`);
};
const fail = (msg) => {
  throw new Error(msg);
};
const shot = async (page, name) => {
  if (!shots) return;
  const file = path.join(shots, `${shotPrefix}-${name}.png`);
  await page.screenshot({ path: file });
  console.log(`[smoke] screenshot ${file}`);
};

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, acceptDownloads: true, colorScheme: "light" });
  const page = await context.newPage();
  page.setDefaultTimeout(15_000);
  const errors = [];
  page.on("pageerror", (e) => errors.push(`page error: ${e.message}`));
  page.on("response", (r) => {
    if (r.url().startsWith(server.url.replace(/\/[^/]*\/?$/, "")) && r.status() >= 400) errors.push(`HTTP ${r.status()} ${r.url()}`);
  });

  // 1. landing page
  await page.goto(server.url);
  if ((await page.title()) !== "Sawhorse") fail(`home title is "${await page.title()}"`);
  const pitch = await page.locator(".pitch").textContent();
  if (!pitch?.includes("Free, local-first, open-source SketchUp-style modeling for woodworkers and makers")) fail(`pitch: ${pitch}`);
  await page.waitForLoadState("networkidle");
  await shot(page, "home-light");
  ok("landing page");

  // 2. into the demo
  await page.getByRole("link", { name: "Try it in your browser" }).first().click();
  await page.waitForURL(/\/app\/$/);
  const vp = page.locator("main.vp");
  await page.waitForSelector('main.vp:not([data-kernel="loading"])', { timeout: 30_000 });
  const kernel = await vp.getAttribute("data-kernel");
  if (kernel !== "ready") fail(`kernel is "${kernel}" under ${page.url()}`);
  const banner = page.getByRole("note", { name: "About this demo" });
  if (!(await banner.isVisible())) fail("no demo banner");
  const dl = await banner.getByRole("link", { name: "Download the desktop app" }).getAttribute("href");
  if (!dl?.startsWith("https://github.com/sawhorse3d/sawhorse/releases")) fail(`banner download link ${dl}`);
  ok(`kernel ready at ${new URL(page.url()).pathname}, banner shown`);

  // 3. a box
  const typeLine = async (text) => {
    await page.keyboard.type(text);
    await page.keyboard.press("Enter");
  };
  // A typed box, as the e2e shell spec makes it. The pointer goes off the model first: a typed
  // rectangle and a typed push follow what is under the pointer, and after the click on "Try it
  // in your browser" it rests wherever that link was, which depends on the fonts (a different
  // place on the CI runner than on Windows). The push waits for the rectangle.
  const box = async () => {
    await page.mouse.move(1, 1);
    await page.keyboard.press("r");
    await typeLine("36,12");
    await page.waitForSelector('main.vp[data-triangles="2"]');
    await page.keyboard.press("p");
    await typeLine("3/4");
    await page.locator("#hint").filter({ hasText: "Extruded" }).waitFor();
  };
  await box();
  const solids = async () => page.evaluate(() => window.__sawhorse.solids());
  let s = await solids();
  const volume = (x) => x.volume / IN ** 3;
  if (s.length !== 1 || Math.abs(volume(s[0]) - 324) > 1e-6) fail(`after the box: ${JSON.stringify(s)}`);
  if ((await vp.getAttribute("data-watertight")) !== "true") fail("box is not watertight");
  await page.waitForTimeout(300);
  await shot(page, "app-box");
  ok("box extruded: one solid, 36 x 12 x 3/4 in = 324 cu in");

  // 4. save, new, open (download and file-input path; same handlers as the pickers)
  await page.goto(`${server.url}app/?io=basic&autosave=0`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
  await box();
  // Menu items' names include their shortcut ("Open… Ctrl+O"), so match the start.
  const menu = async (top, item) => {
    await page.getByRole("button", { name: top, exact: true }).click();
    await page.getByRole("menuitem", { name: new RegExp(`^${item}`) }).first().click();
  };
  const [download] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Control+s")]);
  const saved = readFileSync(await download.path());
  if (!download.suggestedFilename().endsWith(".shz") || saved.length < 100) fail(`save gave ${download.suggestedFilename()} (${saved.length} bytes)`);
  await menu("File", "New");
  await page.waitForFunction(() => window.__sawhorse.solids().length === 0);
  const [chooser] = await Promise.all([page.waitForEvent("filechooser"), menu("File", "Open")]);
  await chooser.setFiles({ name: download.suggestedFilename(), mimeType: "application/octet-stream", buffer: saved });
  await page.waitForFunction(() => window.__sawhorse.solids().length === 1);
  s = await solids();
  if (Math.abs(volume(s[0]) - 324) > 1e-6) fail(`reopened: ${JSON.stringify(s)}`);
  ok(`Ctrl+S saved ${download.suggestedFilename()} (${saved.length} bytes); New; Open read it back`);

  // 5. banner dismissal sticks
  await page.getByRole("button", { name: "Dismiss" }).click();
  await page.reload();
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
  if (await page.getByRole("note", { name: "About this demo" }).count()) fail("banner came back after Dismiss");
  ok("banner dismissed and stays dismissed");

  // 6. documents
  await page.goto(`${server.url}guide/`);
  if (!(await page.locator("h1").first().textContent())?.includes("Sawhorse user guide")) fail("guide has no title");
  if ((await page.locator(".toc a").count()) < 5) fail("guide has no table of contents");
  await shot(page, "guide");
  await page.goto(`${server.url}changelog/`);
  if (!(await page.locator("h1").first().textContent())?.includes("Changelog")) fail("changelog has no title");
  ok("guide and changelog render");

  // the dark landing page, for the record
  if (shots) {
    const dark = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, colorScheme: "dark" });
    await dark.goto(server.url);
    await dark.waitForLoadState("networkidle");
    await shot(dark, "home-dark");
    await dark.close();
  }

  if (errors.length) fail(`errors while browsing:\n  ${errors.join("\n  ")}`);
  ok("no page errors, no failed requests");
  console.log(`[smoke] passed (${steps.length} checks) at ${server.url}`);
} catch (e) {
  console.error(`[smoke] FAILED: ${e.message}`);
  process.exitCode = 1;
} finally {
  await browser.close();
  await server.close();
}
