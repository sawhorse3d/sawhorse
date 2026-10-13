#!/usr/bin/env node
// Lane 15's walk-through: drives a running dev server like a person and saves a screenshot
// per stage, failing when a stage does not show what it should.
//
//   npx vite --port 5215 --strictPort                      (in another terminal)
//   node scripts/capture-lane15.mjs --url http://localhost:5215 --out ../_captures [--benches <path to benches.skp>]
//
// 1. A first run: no Credits chip, no Render tab; Edit > Settings says why in one line.
// 2. benches.skp (all loose faces): Make parts from loose faces, the flagged parts in the
//    Cut list, then in Nesting (default: tests/fixtures/skp-ccby/benches.skp).
// 3. A 24-sided cylinder (drawn with the Circle tool, pulled up), its side edges softened
//    with the Eraser and Ctrl, one by one, then the rest with Soften selected.
// 4. The Shop drawing style on the dark theme, on the bookshelf.
import { mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "@playwright/test";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const argv = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : dflt;
};
const url = opt("url", "http://localhost:5215").replace(/\/+$/, "");
const out = path.resolve(opt("out", path.join(root, "..", "_captures")));
const prefix = opt("prefix", "2026-10-12-lane15");
const benches = path.resolve(opt("benches", path.join(root, "tests", "fixtures", "skp-ccby", "benches.skp")));
mkdirSync(out, { recursive: true });
const fail = (m) => {
  throw new Error(m);
};

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
const log = [];
try {
  const open = async (theme = "light") => {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
    page.setDefaultTimeout(20_000);
    page.on("dialog", (d) => void d.accept());
    await page.goto(`${url}/?theme=${theme}&autosave=0&io=basic`);
    await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
    return page;
  };
  const shot = async (page, name, what) => {
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    const file = path.join(out, `${prefix}-${name}.png`);
    await page.screenshot({ path: file });
    log.push(`${name}: ${what} -> ${path.basename(file)}`);
    console.log(log.at(-1));
  };
  const cutList = (page) => page.evaluate(() => window.__sawhorse.cutList());
  const counts = (page) => page.evaluate(() => window.__sawhorse.edgeCounts());

  // 1. First run: nothing about credits. A dev build asks the local render service (none
  // runs here), so renders stay off and Settings says so.
  {
    const page = await open();
    await page.waitForTimeout(1500);
    if (await page.locator("header.top .chip").count()) fail("a Credits chip on the first run");
    if (await page.getByRole("tab", { name: "Render" }).count()) fail("a Render tab on the first run");
    await page.getByRole("button", { name: "Edit" }).click();
    await page.getByRole("menuitem", { name: /Settings/ }).click();
    await page.waitForFunction(() => /off|unreachable/.test(document.querySelector('[data-testid="render-gate"]')?.getAttribute("data-status") ?? ""));
    const line = await page.getByTestId("render-gate").textContent();
    if (!/^Renders are off/.test(line ?? "")) fail(`the Settings line: ${line}`);
    await shot(page, "01-first-run-no-credits", `no chip, no Render tab; Settings: "${line}"`);
    await page.close();
  }

  // 2. benches.skp: flagged parts in the Cut list and in Nesting.
  {
    const page = await open();
    await page.getByTestId("file-input").setInputFiles(benches);
    await page.getByTestId("import-report").waitFor();
    await page.locator(".import-report button").first().click().catch(() => {});
    await page.getByRole("button", { name: "Iso", exact: true }).click();
    await page.keyboard.press("Shift+Z");
    await page.getByRole("tab", { name: "Cut list" }).click();
    await page.getByRole("button", { name: "Make parts from loose faces" }).click();
    await page.waitForFunction(async () => (await window.__sawhorse.cutList()).totals.parts > 0);
    const cut = await cutList(page);
    if (!cut.totals.not_solid) fail("no flagged part from benches.skp");
    await page.locator('.cutlist tbody tr[data-not-solid="true"]').first().click();
    await shot(page, "02-benches-cutlist-flagged", `${cut.totals.parts} parts, ${cut.totals.not_solid} flagged "not solid (sizes from bounds)"`);
    await page.getByRole("button", { name: /Nesting ›/ }).click();
    await page.waitForSelector('[data-testid="nest-note"]');
    const nest = await page.evaluate(() => window.__sawhorse.nest());
    await page.locator('[data-testid="nesting"]').scrollIntoViewIfNeeded();
    await shot(page, "03-benches-nesting", `in Nesting like any other part: ${nest.totals.placed} placed, ${nest.totals.unplaced} on no stock (${(nest.unplaced ?? []).map((u) => u.reason?.kind ?? u.reason).join(", ")})`);
    await page.close();
  }

  // 3. A cylinder softened with the Eraser.
  {
    const page = await open();
    const type = async (t) => {
      await page.keyboard.type(t);
      await page.keyboard.press("Enter");
    };
    const cv = await page.locator("#cv").boundingBox();
    await page.keyboard.press("c");
    await page.mouse.move(cv.x + cv.width / 2, cv.y + cv.height / 2 + 60, { steps: 4 });
    await page.mouse.click(cv.x + cv.width / 2, cv.y + cv.height / 2 + 60);
    await type("6");
    await page.keyboard.press("p");
    await page.mouse.move(cv.x + cv.width / 2, cv.y + cv.height / 2 + 60, { steps: 4 });
    await page.mouse.click(cv.x + cv.width / 2, cv.y + cv.height / 2 + 60);
    await page.waitForTimeout(300);
    await type("10");
    await page.waitForFunction(() => document.querySelector("main.vp")?.getAttribute("data-objects") === "1");
    await page.keyboard.press(" ");
    await page.getByRole("button", { name: "Iso", exact: true }).click();
    await page.keyboard.press("Shift+Z");
    const before = await counts(page);
    // The Eraser with Ctrl on a few side edges: their screen points from the hook.
    await page.keyboard.press("e");
    const edges = await page.evaluate(() => {
      const s = window.__sawhorse.solids()[0];
      return { min: s.min, max: s.max };
    });
    const c = [(edges.min[0] + edges.max[0]) / 2, (edges.min[1] + edges.max[1]) / 2];
    const r = (edges.max[0] - edges.min[0]) / 2;
    let softened = 0;
    for (const k of [20, 21, 22, 23, 0, 1]) {
      const a = (2 * Math.PI * k) / 24;
      const p = [c[0] + r * Math.cos(a), c[1] + r * Math.sin(a), (edges.min[2] + edges.max[2]) / 2];
      const [sx, sy] = await page.evaluate((q) => window.__sawhorse.worldToScreen(q), p);
      await page.mouse.move(cv.x + sx + 1, cv.y + sy, { steps: 2 });
      await page.waitForTimeout(120);
      if ((await page.locator("main.vp").getAttribute("data-hover")) !== "edge") continue;
      await page.keyboard.down("Control");
      await page.mouse.click(cv.x + sx + 1, cv.y + sy);
      await page.keyboard.up("Control");
      await page.waitForTimeout(150);
      softened++;
    }
    const mid = await counts(page);
    if (mid.soft < 1) fail(`the Eraser softened nothing (${softened} clicks; ${JSON.stringify(mid)})`);
    await shot(page, "04-cylinder-eraser-soft", `Eraser + Ctrl: ${mid.soft} of the side edges soft and smooth (from ${before.hard} hard edges)`);
    // The rest with Soften selected.
    await page.keyboard.press(" ");
    await page.keyboard.press("Control+a");
    await page.getByRole("tab", { name: "Styles" }).click();
    await page.getByRole("button", { name: "Soften selected" }).click();
    await page.waitForFunction(() => window.__sawhorse.edgeCounts().soft >= 24);
    await page.keyboard.press("Escape");
    const after = await counts(page);
    await shot(page, "05-cylinder-softened", `Soften selected: ${after.soft} soft, ${after.hard} hard (the rims), smooth shading ${after.smoothNormals}`);
    await page.close();
  }

  // 4. Shop drawing on the dark theme, on the bookshelf.
  {
    const pkg = path.join(root, "src", "wasm", "pkg");
    const mod = await import(pathToFileURL(path.join(pkg, "sawhorse_wasm.js")).href);
    mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
    const golden = JSON.parse(readFileSync(path.join(root, "crates", "kernel", "tests", "golden", "cutlist.json"), "utf8"));
    const doc = mod.document_replay(JSON.stringify(golden.ops.filter((o, i) => i < 23 || o.op === "set_stock")));
    const shz = Buffer.from(doc.save_shz("frac-in", new Uint8Array()));
    doc.free();
    const page = await open("dark");
    await page.getByTestId("file-input").setInputFiles({ name: "bookshelf.shz", mimeType: "application/vnd.sawhorse.shz", buffer: shz });
    await page.waitForFunction(() => Number(document.querySelector("main.vp")?.getAttribute("data-objects")) >= 9);
    await page.getByRole("button", { name: "Iso", exact: true }).click();
    await page.keyboard.press("Shift+Z");
    await page.getByRole("tab", { name: "Styles" }).click();
    await page.getByRole("button", { name: "Shop drawing" }).click();
    await page.waitForFunction(() => window.__sawhorse.looks().face === "hidden_line");
    const grid = await page.evaluate(() => window.__sawhorse.gridColours());
    await shot(page, "06-shop-drawing-dark", `Shop drawing on the dark theme; the grid follows the paper background (${grid.minor}, ${grid.major})`);
    await page.close();
  }
} finally {
  await browser.close();
}
console.log(`\n${log.length} captures in ${out}`);
