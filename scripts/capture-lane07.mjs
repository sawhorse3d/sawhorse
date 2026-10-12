#!/usr/bin/env node
// Lane-7 lane-end walk-through: the cut list, driven like a person at the keyboard, mouse,
// menus and panes, with a screenshot after each stage:
//
//   1. the user guide's bookshelf built from components (Back panel, Side x2, Top,
//      Toe kick, Shelf x4), every part checked where it belongs;
//   2. the Cut list pane: the table, the tiles, grain arrows in the view;
//   3. stock named from the right-click menu (White oak); the top's grain set across its
//      width (the arrow turns);
//   4. the back panel named 1/4 birch ply and marked sheet goods;
//   5. CSV saved (and read back);
//   6. print preview (the print stylesheet: the report alone);
//   7. the label sheet PDF and 8. the report PDF, saved and rendered page 1 (MuPDF via
//      Python's fitz, when it is installed).
//
//   npm run dev -- --port 5197 --strictPort   (in another terminal)
//   node scripts/capture-lane07.mjs --url http://localhost:5197 --out ../_captures [--files <dir>]
//
// Uses the installed Edge (PW_CHANNEL=msedge) so nothing is downloaded. Exits non-zero when
// a stage does not produce what it should.
import { chromium } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const url = args.url ?? "http://localhost:5197";
const outDir = path.resolve(args.out ?? "../_captures");
const files = path.resolve(args.files ?? path.join(os.tmpdir(), "sawhorse-lane07"));
const prefix = args.prefix ?? "2026-10-12-lane07";
const IN = 0.0254;
mkdirSync(outDir, { recursive: true });
mkdirSync(files, { recursive: true });

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
const log = [];
const fail = (msg) => {
  throw new Error(msg);
};
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, acceptDownloads: true });
  page.setDefaultTimeout(15_000);
  await page.goto(`${url}/?theme=light&autosave=0&io=basic`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
  const vp = page.locator("main.vp");
  const hint = page.locator("#hint");

  const at = async (p, dx = 0, dy = 0) => {
    const box = await page.locator("#cv").boundingBox();
    const xy = await page.evaluate((m) => window.__sawhorse.worldToScreen(m), p.map((v) => v * IN));
    return [box.x + xy[0] + dx, box.y + xy[1] + dy];
  };
  const waitAttr = async (name, want, timeout = 800) => {
    const re = want instanceof RegExp ? want : new RegExp(`^${want}$`);
    const t0 = Date.now();
    while (Date.now() - t0 < timeout) {
      if (re.test((await vp.getAttribute(name)) ?? "")) return true;
      await page.waitForTimeout(40);
    }
    return false;
  };
  const hover = async (p, want, dx = 1, dy = -1, attr = "data-inference") => {
    for (let k = 0; k < 15; k++) {
      const [x, y] = await at(p, dx, dy);
      await page.mouse.move(x + 3, y + 2);
      await page.mouse.move(x, y, { steps: 2 });
      if (await waitAttr(attr, want)) return [x, y];
    }
    fail(`no ${want} at ${p} (${attr} = ${await vp.getAttribute(attr)})`);
  };
  const clickAt = async (p, want, dx = 1, dy = -1) => {
    const [x, y] = await hover(p, want, dx, dy);
    await page.mouse.click(x, y);
  };
  const dblAt = async (p) => {
    const [x, y] = await hover(p, "face", 1, -1, "data-hover");
    await page.mouse.click(x, y);
    await page.mouse.dblclick(x, y);
  };
  const point = async (p) => {
    const [x, y] = await at(p);
    await page.mouse.move(x, y, { steps: 4 });
  };
  const type = async (text) => {
    await page.keyboard.type(text);
    await page.keyboard.press("Enter");
  };
  const expectHint = async (text) => {
    try {
      await page.waitForFunction((t) => document.getElementById("hint")?.textContent?.includes(t), text, { timeout: 15_000 });
    } catch {
      fail(`expected "${text}" in the status line, found "${(await hint.textContent())?.trim()}"`);
    }
    log.push((await hint.textContent()).trim());
  };
  const solids = async () =>
    (await page.evaluate(() => window.__sawhorse.solids())).map((s) => ({ ...s, min: s.min.map((v) => v / IN), max: s.max.map((v) => v / IN) }));
  const shot = async (name, opts = {}) => {
    if (!opts.keepMouse) await page.mouse.move(5, 5);
    await page.waitForTimeout(350);
    const file = path.join(outDir, `${prefix}-${name}.png`);
    await page.screenshot({ path: file });
    log.push(`[${name}] ${file}`);
  };
  const escapeAll = async () => {
    await page.keyboard.press("Space");
    for (let k = 0; k < 4; k++) await page.keyboard.press("Escape");
  };
  const zoomTo = async (p) => {
    await page.keyboard.press("Shift+Z");
    const [x, y] = await at(p);
    await page.mouse.move(x, y);
    for (let k = 0; k < 4; k++) await page.mouse.wheel(0, -240);
    await page.waitForTimeout(120);
  };
  const component = async (name) => {
    await page.getByRole("button", { name: "Edit", exact: true }).click();
    await page.getByRole("menuitem", { name: /Make Component/ }).click();
    const field = page.getByRole("dialog", { name: "Make Component" }).getByLabel("Name");
    await field.fill(name);
    await field.press("Enter");
    await expectHint(`Made component '${name}'`);
  };
  const board = async (size, h) => {
    await escapeAll();
    await page.keyboard.press("r");
    await type(size);
    await expectHint("on the ground");
    await page.keyboard.press("p");
    await type(h);
    await expectHint(`Extruded ${h} in`);
    await page.keyboard.press("Shift+Z");
  };
  const nudge = async (p, arrow, toward, dist) => {
    await escapeAll();
    await zoomTo(p);
    await dblAt(p);
    await expectHint("Selected solid");
    await page.keyboard.press("m");
    await clickAt(p, /.+/, 0, 0);
    await page.keyboard.press(arrow);
    await point(toward);
    await type(dist);
    await expectHint("Moved solid");
  };
  const pick = async (p) => {
    await escapeAll();
    await zoomTo(p);
    await dblAt(p);
  };
  const rows = () =>
    page.locator(".cutlist table tbody tr").evaluateAll((trs) => trs.map((tr) => Array.from(tr.querySelectorAll("td")).map((td) => (td.querySelector(".name") ?? td).textContent)));
  const save = async (button, as) => {
    const [d] = await Promise.all([page.waitForEvent("download"), page.locator(".cutlist .btnrow").getByRole("button", { name: button, exact: true }).click()]);
    const file = path.join(files, as ?? d.suggestedFilename());
    await d.saveAs(file);
    log.push(`saved ${d.suggestedFilename()} -> ${file}`);
    return file;
  };
  const row = (name) => page.locator(".cutlist tbody tr", { hasText: name }).first();
  const stock = async (name, text, kind = "auto") => {
    await row(name).click({ button: "right" });
    await page.getByRole("menuitem", { name: /Stock/ }).click();
    const dialog = page.getByRole("dialog", { name: `Stock for ${name}` });
    await dialog.getByRole("textbox").fill(text);
    await dialog.getByLabel("Counted as").selectOption(kind);
    await dialog.getByRole("button", { name: "Set stock" }).click();
    await expectHint(`'${name}' is cut from ${text}`);
  };

  // 1. The bookshelf from components.
  await board("34 1/2, 71 1/4", "1/4");
  await pick([17, 35, 0.25]);
  await page.keyboard.press("q");
  await page.keyboard.press("ArrowRight");
  await clickAt([0, 0, 0], "endpoint", -1, 1);
  await clickAt([0, 30, 0], /on-edge|midpoint|endpoint/, -1, 1);
  await type("90");
  await expectHint("Turned solid 1 90°");
  let slab = (await solids())[0];
  if (slab.max[2] < 1) {
    await type("-90");
    await page.waitForFunction(() => window.__sawhorse.solids()[0].max[2] > 1.7);
    slab = (await solids())[0];
  }
  const y0 = slab.min[1];
  await page.keyboard.press("Shift+Z");
  await nudge([17, y0, 35], "ArrowRight", [25, y0, 35], "3/4");
  await nudge([17.75, y0, 35], "ArrowLeft", [17.75, y0 + 6, 35], `${11 - y0}`);
  await pick([17.75, 11, 35]);
  await component("Back panel");
  await board("3/4, 11 1/4", "71 1/4");
  await escapeAll();
  await page.keyboard.press("Shift+Z");
  await dblAt([0, 5.6, 35]);
  await component("Side");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt([0, 0, 0], "endpoint", 1, -1);
  await page.keyboard.press("ArrowRight");
  await point([10, 0, 0]);
  await type("35 1/4");
  await expectHint("Copied component 'Side'");
  await board("36, 11 1/4", "3/4");
  await nudge([18, 6, 0.75], "ArrowUp", [18, 6, 20], "71 1/4");
  await pick([18, 6, 72]);
  await component("Top");
  await board("34 1/2, 3/4", "3 1/2");
  await nudge([18, 0, 1.75], "ArrowRight", [24, 0, 1.75], "3/4");
  await nudge([18.75, 0, 1.75], "ArrowLeft", [18.75, 6, 1.75], "2");
  await pick([18.75, 2, 1.75]);
  await component("Toe kick");
  await board("34 1/2, 11", "3/4");
  await nudge([18, 10, 0.75], "ArrowRight", [24, 10, 0.75], "3/4");
  await nudge([18.75, 10, 0.75], "ArrowUp", [18.75, 10, 10], "3 1/2");
  await pick([18.75, 10, 4.25]);
  await component("Shelf");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt([35.25, 0, 4.25], "endpoint", -1, -1);
  await page.keyboard.press("ArrowUp");
  await point([35.25, 0, 14]);
  await type("18");
  await expectHint("Copied component 'Shelf'");
  await type("x3");
  await expectHint("Array: 3 copies of component 'Shelf'");
  await escapeAll();
  await page.keyboard.press("Shift+Z");
  const got = await solids();
  if (got.length !== 9) fail(`nine solids expected, ${got.length}`);
  log.push(`solids (in): ${got.map((s) => `[${s.min.map((v) => +v.toFixed(3))}]..[${s.max.map((v) => +v.toFixed(3))}]`).join(" ")}`);
  await shot("1-bookshelf");

  // 2. The Cut list pane, with grain arrows in the view.
  await page.getByRole("tab", { name: "Cut list" }).click();
  await page.locator(".cutlist table").waitFor();
  await page.waitForTimeout(300);
  log.push(`rows: ${JSON.stringify(await rows())}`);
  const want = [
    ["Side", "2", "3/4", "11 1/4", "71 1/4", "—"],
    ["Top", "1", "3/4", "11 1/4", "36", "—"],
    ["Toe kick", "1", "3/4", "3 1/2", "34 1/2", "—"],
    ["Shelf", "4", "3/4", "11", "34 1/2", "—"],
    ["Back panel", "1", "1/4", "34 1/2", "71 1/4", "—"],
  ];
  if (JSON.stringify(await rows()) !== JSON.stringify(want)) fail(`rows ${JSON.stringify(await rows())}`);
  log.push(`tiles: ${await page.getByTestId("tile-board-feet").textContent()} | ${await page.getByTestId("tile-sheets").textContent()}`);
  await shot("2-cutlist");

  // 3. Stock, and the top's grain across its width.
  for (const name of ["Side", "Top", "Toe kick", "Shelf"]) await stock(name, "White oak", "lumber");
  await row("Top").click({ button: "right" });
  await page.getByRole("menuitemradio", { name: /across/ }).click();
  await expectHint("Grain of 'Top': along W (across)");
  await shot("3-grain-top-across");

  // 4. The back panel as 1/4 birch ply, sheet goods.
  await stock("Back panel", "1/4 birch ply", "sheet");
  await row("Back panel").click();
  await expectHint("Selected 1 piece of 'Back panel'");
  log.push(`tiles: ${await page.getByTestId("tile-board-feet").textContent()} | ${await page.getByTestId("tile-sheets").textContent()}`);
  await shot("4-back-panel-ply", { keepMouse: true });
  await escapeAll();

  // 5. CSV.
  const csv = await save("CSV");
  const text = readFileSync(csv, "utf8").replace(/^﻿/, "");
  log.push(`CSV:\n${text.trimEnd()}`);
  if (!text.includes("Back panel,1,1/4,34 1/2,71 1/4,1/4 birch ply,along L,Sheet,,17.07")) fail("the CSV does not list the back panel as ply");
  await expectHint("Saved Untitled cut list.csv");
  await shot("5-csv-saved");

  // 6. Print preview: the print stylesheet shows the report alone.
  await page.evaluate(() => {
    window.print = () => {
      window.__printed = document.body.classList.contains("print-cutlist");
    };
  });
  await page.locator(".cutlist .btnrow").getByRole("button", { name: "Print", exact: true }).click();
  if (!(await page.evaluate(() => window.__printed))) fail("Print did not switch to the report");
  await page.emulateMedia({ media: "print" });
  await page.setViewportSize({ width: 816, height: 1056 }); // Letter at 96 dpi
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, `${prefix}-6-print-preview.png`), fullPage: true });
  log.push(`[6-print-preview] ${path.join(outDir, `${prefix}-6-print-preview.png`)}`);
  const printPdf = path.join(files, "print-preview.pdf");
  await page.pdf({ path: printPdf, format: "Letter", printBackground: true }).catch(() => log.push("page.pdf is not available in this browser"));
  await page.emulateMedia({ media: "screen" });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.evaluate(() => document.body.classList.remove("print-cutlist"));

  // 7-8. Labels and the report as PDFs, rendered with MuPDF when Python has it.
  const labels = await save("Labels");
  const report = await save("PDF");
  for (const [file, name] of [
    [labels, "7-labels-pdf"],
    [report, "8-report-pdf"],
  ]) {
    const png = path.join(outDir, `${prefix}-${name}.png`);
    const r = spawnSync("python", ["-c", "import fitz,sys; d=fitz.open(sys.argv[1]); d[0].get_pixmap(dpi=110).save(sys.argv[2]); print(d.page_count, d.metadata.get('title'))", file, png], { encoding: "utf8" });
    log.push(r.status === 0 ? `[${name}] ${png} (${r.stdout.trim()})` : `[${name}] not rendered (python fitz missing): ${file}`);
  }
  const kernel = await page.evaluate(async () => (await window.__sawhorse.cutList()).totals);
  log.push(`kernel totals (1/16 in): ${JSON.stringify(kernel)}`);
  writeFileSync(path.join(files, "capture-log.txt"), log.join("\n"));
} finally {
  await browser.close();
  console.log(log.join("\n"));
}
