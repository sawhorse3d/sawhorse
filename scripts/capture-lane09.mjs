#!/usr/bin/env node
// Lane-9 lane-end walk-through: nesting and cutting diagrams, driven like a person at the
// keyboard, mouse, menus and panes, with a screenshot after each stage:
//
//   1. the user guide's bookshelf built from components (as lane 7's walk-through), its
//      back panel named 1/4 birch ply; the Nesting section opened: five 4/4 x 12 boards
//      and one 1/4 in sheet;
//   2. a part hovered in a diagram, lit up in the model;
//   3. the kerf typed as 3/32;
//   4. the tightest layout (maxrects);
//   5. the SVGs, 6. the DXF (drawn from its own polylines) and 7. the PDF booklet saved and
//      rendered (MuPDF via Python's fitz, when it is installed);
//   8. the carcass as 3/4 birch ply (three sheets) and 9. the Stock dialog.
//
//   npx vite --port 5201 --strictPort   (in another terminal)
//   node scripts/capture-lane09.mjs --url http://localhost:5201 --out ../_captures [--files <dir>] [--theme dark] [--prefix <name>]
//
// Uses the installed Edge (PW_CHANNEL=msedge) so nothing is downloaded. Exits non-zero when
// a stage does not produce what it should.
import { chromium } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import DxfParser from "dxf-parser";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const url = args.url ?? "http://localhost:5201";
const outDir = path.resolve(args.out ?? "../_captures");
const files = path.resolve(args.files ?? path.join(os.tmpdir(), "sawhorse-lane09"));
const prefix = args.prefix ?? "2026-10-12-lane09";
// --theme dark re-takes the walk-through in the dark theme (lane 14 uses both for the site).
const theme = args.theme === "dark" ? "dark" : "light";
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
  await page.goto(`${url}/?theme=${theme}&autosave=0&io=basic`);
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
  const row = (name) => page.locator(".cutlist tbody tr", { hasText: name }).first();
  const section = page.getByTestId("nesting");
  const nest = () => page.evaluate(() => window.__sawhorse.nest());
  const summary = async (what) => {
    const n = await nest();
    const line = `${what}: ${n.totals.sheets} sheets, ${n.totals.boards} boards, ${n.totals.placed}/${n.totals.parts_in} placed, yield ${(n.totals.efficiency * 100).toFixed(1)}%, kerf ${(n.settings.kerf / IN).toFixed(5)} in, ${n.algorithm}; ${n.pieces.map((p) => `${p.index}:${p.stock}[${p.placements.map((q) => q.tag).join(",")}] ${(p.efficiency * 100).toFixed(1)}%${p.guillotine ? "" : " (not guillotine)"}`).join(" | ")}${n.unplaced.length ? `; unplaced ${n.unplaced.map((u) => `${u.tag}:${u.reason}`).join(",")}` : ""}`;
    log.push(line);
    return n;
  };
  const noOverlaps = (n) => {
    for (const p of n.pieces)
      for (const [i, a] of p.placements.entries())
        for (const b of p.placements.slice(i + 1)) {
          const apart = a.x + a.dx + p.kerf <= b.x + 1e-9 || b.x + b.dx + p.kerf <= a.x + 1e-9 || a.y + a.dy + p.kerf <= b.y + 1e-9 || b.y + b.dy + p.kerf <= a.y + 1e-9;
          if (!apart) fail(`${a.tag} and ${b.tag} overlap on piece ${p.index}`);
        }
  };
  /** Downloads from one click (several for SVG: one per piece). */
  const saveAll = async (count, click) => {
    const got = [];
    const done = new Promise((resolve) => {
      const on = (d) => {
        got.push(d);
        if (got.length === count) {
          page.off("download", on);
          resolve();
        }
      };
      page.on("download", on);
    });
    await click();
    await done;
    const out = [];
    for (const d of got) {
      const file = path.join(files, d.suggestedFilename());
      await d.saveAs(file);
      log.push(`saved ${d.suggestedFilename()} -> ${file}`);
      out.push(file);
    }
    return out;
  };
  const stockFor = async (names, text) => {
    await row(names[0]).click();
    if (names.length > 1) await row(names[names.length - 1]).click({ modifiers: ["Shift"] });
    await row(names[names.length - 1]).click({ button: "right" });
    await page.getByRole("menuitem", { name: /Stock/ }).click();
    const dialog = page.getByRole("dialog", { name: names.length === 1 ? `Stock for ${names[0]}` : /^Stock for \d+ parts$/ });
    await dialog.getByRole("textbox").fill(text);
    await dialog.getByRole("button", { name: "Set stock" }).click();
    await expectHint(`cut from ${text}`);
  };
  /** The side panel scrolled so the section's top (or `el`) is in view. */
  const showSection = async (el = section) => {
    await el.evaluate((e) => e.scrollIntoView({ block: "start" }));
    await page.waitForTimeout(250);
  };
  /** Render a PDF page with MuPDF when Python has it. */
  const renderPdf = (file, pageNo, name) => {
    const png = path.join(outDir, `${prefix}-${name}.png`);
    const r = spawnSync("python", ["-c", "import fitz,sys; d=fitz.open(sys.argv[1]); d[int(sys.argv[3])].get_pixmap(dpi=100).save(sys.argv[2]); print(d.page_count, d.metadata.get('title'))", file, png, String(pageNo)], { encoding: "utf8" });
    log.push(r.status === 0 ? `[${name}] ${png} (${r.stdout.trim()} pages)` : `[${name}] not rendered (python fitz missing): ${file}`);
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
  await shot("0-bookshelf");


  // 1. The back panel as ply; open the Nesting section.
  await page.getByRole("tab", { name: "Cut list" }).click();
  await page.locator(".cutlist table").waitFor();
  await stockFor(["Back panel"], "1/4 birch ply");
  await escapeAll();
  await page.getByRole("button", { name: "Nesting ›" }).click();
  await section.waitFor();
  await page.waitForFunction(() => document.querySelectorAll('[data-testid="nesting"] g.part').length === 9, null, { timeout: 15_000 });
  let n = await summary("nested");
  if (n.totals.sheets !== 1 || n.totals.boards !== 5 || n.totals.placed !== 9) fail("the bookshelf should take 5 boards and 1 sheet");
  noOverlaps(n);
  log.push(`tiles: ${await page.getByTestId("tile-nest-sheets").textContent()} | ${await page.getByTestId("tile-nest-boards").textContent()} | ${await page.getByTestId("tile-nest-yield").textContent()}`);
  await showSection();
  await shot("1-nesting-boards-and-sheet");

  // 2. A shelf hovered in its diagram: it lights up in the model.
  const shelf = section.locator('g.part[data-tag="4a"]');
  await shelf.scrollIntoViewIfNeeded();
  await shelf.hover();
  await page.waitForTimeout(300);
  await shot("2-hover-part-lights-model", { keepMouse: true });
  await shelf.click();
  await expectHint("Selected 1 piece of 'Shelf'");
  await escapeAll();

  // 3. Kerf 3/32 in.
  await showSection();
  const kerf = section.getByLabel("Kerf");
  await kerf.fill("3/32");
  await kerf.press("Enter");
  await page.waitForFunction(() => document.querySelector('[data-testid="nest-note"]')?.textContent?.includes("Kerf 3/32 in"), null, { timeout: 15_000 });
  n = await summary("kerf 3/32");
  noOverlaps(n);
  await shot("3-kerf-3-32");

  // 4. The tightest layout (maxrects).
  await section.getByLabel("Nesting algorithm").selectOption("maxrects");
  await page.waitForFunction(() => document.querySelector('[data-testid="nest-note"]')?.textContent?.includes("tightest layout"), null, { timeout: 15_000 });
  n = await summary("maxrects");
  noOverlaps(n);
  await shot("4-maxrects");

  // 5. SVG, one per piece; the sheet's drawn by Edge.
  const buttons = section.locator("div.btnrow");
  const svgs = await saveAll(n.pieces.length, () => buttons.getByRole("button", { name: "SVG", exact: true }).click());
  for (const [i, f] of svgs.entries()) {
    const text = readFileSync(f, "utf8");
    const p = n.pieces.find((x) => f.includes(` ${String(x.index).padStart(2, "0")} `));
    if (!p) fail(`no piece for ${f}`);
    for (const q of p.placements) if (!text.includes(`data-tag="${q.tag}"`)) fail(`${q.tag} missing from ${f}`);
    if (i === svgs.length - 1 || p.kind === "sheet") {
      const view = await browser.newPage({ viewport: { width: 1400, height: 900 } });
      await view.setContent(`<html><body style="margin:0;background:#fff">${text.replace(/^<\?xml[^>]*>/, "").replace(/width="[^"]*mm" height="[^"]*mm"/, 'width="1380" height="880"')}</body></html>`);
      await view.screenshot({ path: path.join(outDir, `${prefix}-5-svg-${p.kind}-${p.index}.png`) });
      log.push(`[5-svg-${p.kind}-${p.index}] ${path.join(outDir, `${prefix}-5-svg-${p.kind}-${p.index}.png`)}`);
      await view.close();
    }
  }
  await expectHint("cutting diagrams");

  // 6. DXF, drawn from its own polylines (parsed by dxf-parser).
  const [dxf] = await saveAll(1, () => buttons.getByRole("button", { name: "DXF", exact: true }).click());
  const parsed = new DxfParser().parseSync(readFileSync(dxf, "utf8"));
  const polys = parsed.entities.filter((e) => e.type === "LWPOLYLINE");
  const texts = parsed.entities.filter((e) => e.type === "TEXT");
  for (const p of n.pieces) {
    const layer = `${p.kind.toUpperCase()}-${String(p.index).padStart(2, "0")}`;
    const count = polys.filter((e) => e.layer === layer).length;
    if (count !== p.placements.length) fail(`${layer}: ${count} polylines for ${p.placements.length} parts`);
  }
  const xs = polys.flatMap((e) => e.vertices.map((v) => v.x));
  const ys = polys.flatMap((e) => e.vertices.map((v) => v.y));
  const [minX, maxX, minY, maxY] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  const colour = (l) => (l === "STOCK" ? "#888" : l.startsWith("SHEET") ? "#c0392b" : "#1d4f8f");
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${minX - 60} ${-maxY - 60} ${maxX - minX + 120} ${maxY - minY + 120}" width="1380" height="880">${polys.map((e) => `<polygon points="${e.vertices.map((v) => `${v.x},${-v.y}`).join(" ")}" fill="none" stroke="${colour(e.layer)}" stroke-width="3"/>`).join("")}${texts.map((t) => `<text x="${t.startPoint.x}" y="${-t.startPoint.y}" font-size="${Math.max(t.textHeight, 12)}" text-anchor="middle" fill="#333">${t.text}</text>`).join("")}</svg>`;
  const view = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  await view.setContent(`<html><body style="margin:0;background:#fff">${svg}</body></html>`);
  await view.screenshot({ path: path.join(outDir, `${prefix}-6-dxf.png`) });
  log.push(`[6-dxf] ${path.join(outDir, `${prefix}-6-dxf.png`)} (${polys.length} polylines on ${new Set(polys.map((e) => e.layer)).size} layers, ${texts.length} labels)`);
  await view.close();

  // 7. The PDF booklet: the summary and the sheet's page.
  const [pdf] = await saveAll(1, () => buttons.getByRole("button", { name: "PDF", exact: true }).click());
  await expectHint("a page per sheet or board");
  renderPdf(pdf, 0, "7-pdf-summary");
  const sheet = n.pieces.findIndex((p) => p.kind === "sheet");
  renderPdf(pdf, sheet + 1, "7-pdf-sheet-page");

  // 8. The carcass as 3/4 birch ply: sheets only, back to the table saw.
  await section.getByLabel("Nesting algorithm").selectOption("guillotine");
  await stockFor(["Side", "Shelf"], "3/4 birch ply");
  await escapeAll();
  await page.waitForFunction(() => document.querySelectorAll('[data-testid="nesting"] figure.nest-piece').length === 3, null, { timeout: 15_000 });
  n = await summary("3/4 ply carcass");
  noOverlaps(n);
  if (n.totals.sheets !== 3 || n.totals.boards !== 0) fail("a ply carcass should take three sheets");
  await showSection();
  await shot("8-ply-carcass-sheets");

  // 9. The Stock dialog.
  await section.getByRole("button", { name: "Stock…" }).click();
  await page.getByRole("dialog", { name: "Stock" }).waitFor();
  await shot("9-stock-dialog");
  await page.getByRole("dialog", { name: "Stock" }).getByRole("button", { name: "Done" }).click();
  writeFileSync(path.join(files, "capture-log.txt"), log.join("\n"));
} finally {
  await browser.close();
  console.log(log.join("\n"));
}
