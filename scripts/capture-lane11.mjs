#!/usr/bin/env node
// Lane-11 lane-end walk-through: the plugin API, through the three sample plugins, driven
// like a person at the mouse and keyboard, with a screenshot after each stage:
//
//   1. the Plugins pane: the samples listed, off, with their permissions; the permission
//      dialog for Cabinet generator;
//   2. a 30 x 72 x 12 in cabinet with three shelves from its panel;
//   3. the Outliner showing the named parts; 4. the Cut list listing them;
//   5. Shelf pin holes: the Right side opened from the Outliner, a column previewed on its
//      inside face, then two columns drilled (each one undo step; the mirrored Left side gets
//      them too);
//   6. Model report: the live counts, size, board feet, no warnings; its Markdown and CSV
//      exports read back;
//   7. the same in the dark theme.
//
//   npx vite --port 5211 --strictPort   (in another terminal)
//   node scripts/capture-lane11.mjs --url http://localhost:5211 --out ../_captures [--prefix 2026-10-12-lane11]
//
// Uses the installed Edge (PW_CHANNEL=msedge). Exits non-zero when a stage does not produce
// what it should.
import { chromium } from "@playwright/test";
import { mkdirSync, readFileSync } from "node:fs";
import path from "node:path";

const args = Object.fromEntries(process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []));
const url = args.url ?? "http://localhost:5211";
const outDir = path.resolve(args.out ?? "../_captures");
const prefix = args.prefix ?? "2026-10-12-lane11";
const IN = 0.0254;
const CABINET = "com.sawhorse3d.cabinet-generator";
const PINS = "com.sawhorse3d.shelf-pin-holes";
const REPORT = "com.sawhorse3d.model-report";
mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
const fail = (msg) => {
  throw new Error(msg);
};
const shots = [];

async function run(theme) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, acceptDownloads: true });
  page.setDefaultTimeout(15_000);
  await page.goto(`${url}/?theme=${theme}&autosave=0&io=basic`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
  const hint = page.locator("#hint");
  const shot = async (name) => {
    const file = path.join(outDir, `${prefix}-${name}${theme === "dark" ? "-dark" : ""}.png`);
    await page.waitForTimeout(250);
    await page.screenshot({ path: file });
    shots.push(file);
    console.log(`  saved ${path.basename(file)}: ${(await hint.textContent())?.trim()}`);
  };
  const at = async (p, dx = 0, dy = 0) => {
    const box = await page.locator("#cv").boundingBox();
    const xy = await page.evaluate((m) => window.__sawhorse.worldToScreen(m), p.map((v) => v * IN));
    return [box.x + xy[0] + dx, box.y + xy[1] + dy];
  };
  const hover = async (p, kind) => {
    for (let i = 0; i < 40; i++) {
      const [x, y] = await at(p, i % 3, -(i % 2));
      await page.mouse.move(x + 3, y + 2);
      await page.mouse.move(x, y, { steps: 2 });
      await page.waitForTimeout(120);
      if ((await page.locator("main.vp").getAttribute("data-inference")) === kind) return [x, y];
    }
    fail(`no ${kind} inference at ${p}`);
  };
  const row = (id) => page.locator(`.plugin-row[data-plugin="${id}"]`);
  const enable = async (id, name, take) => {
    await page.getByRole("tab", { name: "Plugins" }).click();
    await row(id).locator('[data-do="enable"]').click();
    const dialog = page.getByRole("dialog", { name: `Enable ${name}` });
    await dialog.waitFor();
    if (take) await shot(take);
    await dialog.getByRole("button", { name: "Enable" }).click();
    await page.locator(`.plugin-row[data-plugin="${id}"][data-status="running"]`).waitFor();
  };
  const field = async (panel, id, value) => {
    const input = page.locator(`[data-panel="${panel}"] [data-field="${id}"]`);
    await input.fill(value);
    await input.press("Enter");
  };

  // 1. The pane and the permission dialog.
  await page.getByRole("tab", { name: "Plugins" }).click();
  for (const id of [CABINET, PINS, REPORT]) if ((await row(id).getAttribute("data-status")) !== "disabled") fail(`${id} should start off`);
  if (theme === "light") await shot("pane");
  await enable(CABINET, "Cabinet generator", theme === "light" ? "permissions" : null);

  // 2. A cabinet from the panel.
  const panel = `${CABINET}/cabinet`;
  await field(panel, "width", "30");
  await field(panel, "height", "72");
  await field(panel, "depth", "12");
  await field(panel, "shelves", "3");
  await page.locator(`[data-panel="${panel}"] [data-action="generate"]`).click();
  await hint.filter({ hasText: "Made a cabinet" }).waitFor();
  const solids = await page.evaluate(() => window.__sawhorse.solids().length);
  if (solids !== 9) fail(`the cabinet has ${solids} parts, not 9`);
  await page.keyboard.press("Shift+Z");
  await shot("cabinet");

  // 3. The Outliner: the cabinet unfolded, its parts named.
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.getByRole("button", { name: "Unfold Cabinet" }).click();
  for (const n of ["Left side", "Right side", "Top", "Bottom", "Shelf 1", "Shelf 2", "Shelf 3", "Back", "Toe kick"])
    if ((await page.locator(".outline li .label", { hasText: new RegExp(`^${n}$`) }).count()) !== 1) fail(`no ${n} in the Outliner`);
  await shot("outliner");

  // 4. The Cut list.
  await page.getByRole("tab", { name: "Cut list" }).click();
  await page.locator(".cutlist table").waitFor();
  const cut = await page.evaluate(async () => (await window.__sawhorse.cutList()).rows.map((r) => `${r.name} x${r.qty}`));
  for (const want of ["Side x2", "Top x1", "Bottom x1", "Shelf x3", "Back x1", "Toe kick x1"]) if (!cut.includes(want)) fail(`the cut list lacks ${want}: ${cut}`);
  await shot("cutlist");

  // 5. Shelf pin holes in the Right side, opened from the Outliner.
  await enable(PINS, "Shelf pin holes", null);
  await page.getByRole("tab", { name: "Outliner" }).click();
  const right = page.locator(".outline li", { has: page.locator(".label", { hasText: /^Right side$/ }) });
  await right.click();
  await right.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Edit inside" }).click();
  await page.locator(".vp .hud", { hasText: "Right side" }).waitFor();
  await page.locator(`.rail [data-group="plugins"] button[data-tool="plugin:${PINS}/drill"]`).click();
  const x = 30 - 0.75;
  // Zoom in on the inside of the side (the wheel zooms toward the cursor).
  {
    const [zx, zy] = await at([x, 6, 28]);
    await page.mouse.move(zx, zy);
    for (let i = 0; i < 6; i++) {
      await page.mouse.wheel(0, -240);
      await page.waitForTimeout(60);
    }
  }
  await hover([x, 2, 24], "on-face");
  await page.waitForTimeout(300);
  await shot("pins-preview");
  const before = await page.evaluate(() => window.__sawhorse.solids().reduce((v, s) => v + s.volume, 0));
  for (const y of [2, 9.75]) {
    const [cx, cy] = await hover([x, y, 24], "on-face");
    await page.mouse.click(cx, cy);
    await hint.filter({ hasText: "Drilled 8 holes" }).waitFor();
  }
  const after = await page.evaluate(() => window.__sawhorse.solids().reduce((v, s) => v + s.volume, 0));
  // Two columns of eight in each side (the Left side is the same component, mirrored).
  const hole = 0.5 * 16 * 0.0025 ** 2 * Math.sin((2 * Math.PI) / 16) * 0.375 * IN;
  if (Math.abs(before - after - 2 * 2 * 8 * hole) > 1e-9) fail(`drilled ${(before - after) / hole} holes' worth, not 32`);
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await shot("pins-drilled");

  // 6. The model report.
  await enable(REPORT, "Model report", null);
  const report = page.locator(`[data-panel="${REPORT}/report"]`);
  await page.locator(`[data-panel="${CABINET}/cabinet"]`).evaluate((el) => el.removeAttribute("open"));
  await report.locator('table[data-table="counts"] tbody tr').first().waitFor();
  await page.waitForTimeout(500);
  const counts = await report.locator('table[data-table="counts"] tbody tr').evaluateAll((trs) => Object.fromEntries(trs.map((tr) => [tr.children[0].textContent, Number(tr.children[1].textContent)])));
  if (counts.Solids !== 9 || counts.Instances !== 10 || counts.Components !== 7) fail(`report counts ${JSON.stringify(counts)}`);
  // The whole cabinet in view, the report at the top of the pane.
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.locator("#pane-plugins").evaluate((el) => (el.scrollTop = 0));
  await shot("report");
  if (theme === "light") {
    const files = [];
    page.on("download", async (d) => files.push(readFileSync(await d.path(), "utf8")));
    await report.locator('[data-action="export"]').click();
    await hint.filter({ hasText: "Saved" }).waitFor();
    for (let i = 0; i < 40 && files.length < 2; i++) await page.waitForTimeout(100);
    const md = files.find((f) => f.startsWith("# Model report"));
    if (!md || !md.includes("| Solids | 9 |")) fail("the Markdown report is missing or wrong");
    console.log(`  report.md: ${md.split("\n").length} lines; report.csv: ${files.find((f) => f.includes("Section,Item,Value"))?.split("\r\n").length} lines`);
  }
  await page.close();
}

try {
  console.log("light theme");
  await run("light");
  console.log("dark theme");
  await run("dark");
  console.log(`lane 11 walk-through: ${shots.length} screenshots in ${outDir}`);
} catch (e) {
  console.error(`FAILED: ${e instanceof Error ? e.message : String(e)}`);
  process.exitCode = 1;
} finally {
  await browser.close();
}
