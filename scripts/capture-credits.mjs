#!/usr/bin/env node
// Lane-8 end to end against a REAL render service (render-service/, MockProvider + PGlite):
// connect, buy a pack through the service's simulated checkout (a signed webhook through the
// real verification path), model a small bookshelf carcass, render it, see the credit go,
// open the history, then move the credits to a second "device" with an emailed claim code.
// Screenshots go to --out-dir; a JSON summary is printed.
//
//   (render-service)  PORT=8787 RENDER_PROVIDER=mock node src/main.ts > service.log
//   (app)             npx vite --port 5178 --strictPort
//   (the script saves the service address, --service or http://127.0.0.1:8787, in Settings'
//   storage of each browser context: since lane 12c a development build has no default)
//   node scripts/capture-credits.mjs --url http://localhost:5178 --service-log path/to/service.log \
//        --out-dir ../_captures --prefix 2026-10-11-lane08
//
// Uses the installed Edge (PW_CHANNEL=msedge by default) so nothing is downloaded.
import { chromium } from "@playwright/test";
import { mkdirSync, readFileSync } from "node:fs";
import path from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const SERVICE_URL = (args.service || "http://127.0.0.1:8787").replace(/\/+$/, "");
/** Runs in the page before the app: the address Edit > Settings saves (src/credits/gate.ts). */
function useService(url) {
  try {
    localStorage.setItem("sawhorse.renderServiceUrl", url);
  } catch {
    /* storage blocked */
  }
}

const url = args.url ?? "http://localhost:5178";
const outDir = path.resolve(args["out-dir"] ?? "../_captures");
const prefix = args.prefix ?? "lane08";
const serviceLog = args["service-log"];
const theme = args.theme ?? "light";
mkdirSync(outDir, { recursive: true });
const shots = [];
const summary = {};

async function shot(page, name) {
  const file = path.join(outDir, `${prefix}-${name}.png`);
  await page.screenshot({ path: file });
  shots.push(file);
}

async function kernelReady(page) {
  await page.goto(`${url}/?theme=${theme}`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
}

async function onPart(page, pick, du, dv) {
  const box = await page.locator("#cv").boundingBox();
  const xy = await page.evaluate(
    ([du, dv, src]) => {
      const hooks = window.__sawhorse;
      const test = new Function("p", `return (${src})(p)`);
      const part = hooks.parts().find(test);
      if (!part) return null;
      const p = [0, 1, 2].map((k) => part.origin[k] + part.u[k] * du + part.v[k] * dv);
      return hooks.worldToScreen(p);
    },
    [du, dv, pick.toString()],
  );
  if (!xy || !box) throw new Error("part not on screen");
  return [box.x + xy[0], box.y + xy[1]];
}

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  await context.addInitScript(useService, SERVICE_URL);
  const page = await context.newPage();
  await kernelReady(page);
  const chip = page.locator("header.top .chip");
  await page.waitForFunction(() => document.querySelector("header.top .chip")?.getAttribute("data-state") === "connected", null, { timeout: 15_000 });
  summary.startCredits = await page.locator("#credits").textContent();
  await chip.click();
  await page.waitForSelector("#pane-render:not([hidden])");
  await shot(page, "01-connected");

  // Buy the $10 pack: the service's simulated checkout opens in a new window.
  const popupP = page.waitForEvent("popup");
  await page.getByRole("button", { name: /\$10/ }).click();
  const checkout = await popupP;
  await checkout.waitForLoadState();
  await checkout.setViewportSize({ width: 900, height: 700 });
  await checkout.waitForSelector("#pay");
  await checkout.fill("#email", "maker@example.com");
  await shot(checkout, "02-simulated-checkout");
  await checkout.click("#pay");
  await checkout.waitForSelector("text=Payment received");
  summary.paidPage = (await checkout.locator("main").innerText()).replace(/\s+/g, " ").trim();
  await shot(checkout, "03-paid");
  await checkout.close();
  await page.bringToFront();
  await page.waitForFunction(() => document.getElementById("credits")?.textContent === "100", null, { timeout: 20_000 });
  summary.afterPurchase = await page.locator("#credits").textContent();
  await shot(page, "04-credits-arrived");

  // A bookshelf carcass: 36 x 12 in footprint, 48 in tall, a 9 in deep opening in the front.
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press("r");
  await page.keyboard.type("36,12");
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type("48");
  await page.keyboard.press("Enter");
  try {
    await page.waitForFunction(() => document.querySelector("main.vp")?.getAttribute("data-objects") === "1", null, { timeout: 10_000 });
  } catch (err) {
    await shot(page, "debug-no-box");
    console.error(await page.evaluate(() => ({ hint: document.getElementById("hint")?.textContent, active: document.activeElement?.outerHTML?.slice(0, 120), objects: document.querySelector("main.vp")?.getAttribute("data-objects") })));
    throw err;
  }
  await page.keyboard.press("Escape");
  await page.keyboard.press("r");
  const front = (p) => p.kind === "face" && p.normal[1] < -0.5;
  const a = await onPart(page, front, -0.38, -0.5);
  const b = await onPart(page, front, 0.38, 0.5);
  await page.mouse.click(a[0], a[1]);
  await page.mouse.click(b[0], b[1]);
  await page.waitForFunction(() => /imprinted/.test(document.getElementById("hint")?.textContent ?? ""), null, { timeout: 10_000 });
  await page.keyboard.press("p");
  await page.keyboard.type("-9");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(500);
  summary.model = await page.evaluate(() => ({ hint: document.getElementById("hint")?.textContent, watertight: document.querySelector("main.vp")?.getAttribute("data-watertight"), objects: document.querySelector("main.vp")?.getAttribute("data-objects") }));
  await page.keyboard.press("Escape");
  await page.keyboard.press("Space");

  // Render it (Workshop preset), catching the in-progress state with the depth preview.
  await page.getByRole("button", { name: /Workshop/ }).click();
  await page.locator("#renderBtn").click();
  await page.waitForSelector('#result[data-state="rendering"] img.preview', { timeout: 15_000 });
  await shot(page, "05-rendering");
  await page.waitForSelector('#result[data-state="done"] #renderImg', { timeout: 60_000 });
  await page.waitForFunction(() => document.getElementById("renderImg")?.complete && document.getElementById("renderImg").naturalWidth > 0);
  summary.afterRender = await page.locator("#credits").textContent();
  summary.renderImage = await page.locator("#renderImg").evaluate((img) => ({ src: img.src.replace(/sig=[^&]+/, "sig=…"), w: img.naturalWidth, h: img.naturalHeight }));
  await shot(page, "06-rendered");
  await page.getByRole("button", { name: "History" }).click();
  await page.waitForSelector(".history img");
  await shot(page, "07-history");
  summary.installA = await page.evaluate(() => document.querySelector("header.top .chip")?.getAttribute("title"));

  // A second device (fresh browser profile = new device key) claims the credits by email.
  if (serviceLog) {
    const other = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
    await other.addInitScript(useService, SERVICE_URL);
    const p2 = await other.newPage();
    await kernelReady(p2);
    await p2.waitForFunction(() => document.querySelector("header.top .chip")?.getAttribute("data-state") === "connected", null, { timeout: 15_000 });
    await p2.locator("header.top .chip").click();
    await p2.getByRole("button", { name: "Move credits" }).click();
    await p2.getByLabel("Email used at checkout").fill("maker@example.com");
    await p2.getByRole("button", { name: "Email me a code" }).click();
    await p2.waitForSelector("text=a code is on its way");
    // The service has no Resend key locally, so the email is printed to its log.
    let code = null;
    for (let i = 0; i < 20 && !code; i++) {
      const log = readFileSync(serviceLog, "utf8");
      const all = [...log.matchAll(/Claim code: ([A-Z0-9]{4}-[A-Z0-9]{4})/g)];
      code = all.at(-1)?.[1] ?? null;
      if (!code) await p2.waitForTimeout(250);
    }
    if (!code) throw new Error("no claim code in the service log");
    await p2.getByLabel("Claim code").fill(code);
    await shot(p2, "08-claim-code");
    await p2.getByRole("button", { name: "Move credits here" }).click();
    await p2.waitForSelector("text=Credits moved to this device");
    summary.claimed = { code: "printed to the service log", credits: await p2.locator("#credits").textContent() };
    await shot(p2, "09-claimed-on-second-device");
    await page.bringToFront();
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await page.waitForTimeout(800);
    summary.firstDeviceAfterClaim = await page.locator("#credits").textContent();
    await other.close();
  }
  summary.screenshots = shots;
  console.log(JSON.stringify(summary, null, 2));
} finally {
  await browser.close();
}
