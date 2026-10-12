#!/usr/bin/env node
// Lane-5 lane-end walk-through: a bookshelf built from components, driven like a person at
// the keyboard, mouse, menus and panes, with a screenshot after each stage:
//
//   1. a Side (3/4 x 11 1/4 x 72 in) made a component, a second side by Ctrl+Move
//      (35 1/4 in along red): two instances;
//   2. a Shelf (34 1/2 x 11 1/4 x 3/4 in) made a component, three more by a Move array
//      (Ctrl, 23 3/4 in up, x3): four instances;
//   3. one shelf opened (double-click) and made 1 in thick: all four follow;
//   4. Make Unique on one shelf (Outliner, right-click) and back to 3/4 in: only it changes;
//   5. tags Carcass (the sides) and Shelves (the shelves), with their colours;
//   6. scene Front (front view, Shelves hidden) and scene Iso (everything shown), each
//      switched to from its tab.
//
//   npm run dev -- --port 5195   (in another terminal)
//   node scripts/capture-lane05.mjs --url http://localhost:5195 --out ../_captures
//
// Uses the installed Edge (PW_CHANNEL=msedge) so nothing is downloaded. Exits non-zero when
// a stage does not produce the model it should (the solids are read back and checked).
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const url = args.url ?? "http://localhost:5195";
const outDir = path.resolve(args.out ?? "../_captures");
const prefix = args.prefix ?? "2026-10-11-lane05";
const IN = 0.0254;
mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
const log = [];
const fail = (msg) => {
  throw new Error(msg);
};
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  page.setDefaultTimeout(15_000);
  await page.goto(`${url}/?theme=light&autosave=0`);
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
  /** Click with the Select tool once `want` is under the cursor (`add`: hold Shift). */
  const selectAt = async (p, want, add = false) => {
    const [x, y] = await hover(p, want, 1, -1, "data-hover");
    if (add) await page.keyboard.down("Shift");
    await page.mouse.click(x, y);
    if (add) await page.keyboard.up("Shift");
  };
  const dblAt = async (p, want) => {
    const [x, y] = await hover(p, want, 1, -1, "data-hover");
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
  const view = (name) => page.getByRole("button", { name, exact: true }).click();
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
  const r3 = (v) => Math.round(v * 1000) / 1000;
  const sizes = async () => (await solids()).map((s) => [0, 1, 2].map((k) => r3(s.max[k] - s.min[k])).join(" x "));
  const shot = async (name) => {
    await page.mouse.move(5, 5);
    await page.waitForTimeout(300);
    const file = path.join(outDir, `${prefix}-${name}.png`);
    await page.screenshot({ path: file });
    log.push(`[${name}] ${file} :: ${(await sizes()).join(" | ")}`);
  };
  const edit = async (item) => {
    await page.getByRole("button", { name: "Edit", exact: true }).click();
    await page.getByRole("menuitem", { name: item }).click();
  };
  const component = async (name) => {
    await edit(/Make Component/);
    const field = page.getByRole("dialog", { name: "Make Component" }).getByLabel("Name");
    await field.fill(name);
    await field.press("Enter");
    await expectHint(`Made component ${name}`);
  };
  const escapeAll = async () => {
    for (let k = 0; k < 4; k++) await page.keyboard.press("Escape");
  };

  // 1. The sides.
  await page.keyboard.press("r");
  await type("3/4,11.25");
  await page.keyboard.press("p");
  await type("72");
  await expectHint("Extruded 72 in");
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await component("Side");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt([0, 0, 0], "endpoint", 1, 1);
  await page.keyboard.press("ArrowRight");
  await point([10, 0, 0]);
  await type("35.25");
  await expectHint("Copied component Side");
  await page.keyboard.press("Shift+Z");
  if ((await solids()).length !== 2) fail("two sides expected");
  await shot("1-sides");

  // 2. The shelves: drawn at the origin, moved 3/4 in along red, made a component, arrayed up.
  await escapeAll();
  await page.keyboard.press("r");
  await type("34.5,11.25");
  await page.keyboard.press("p");
  await type("3/4");
  await expectHint("Extruded 3/4 in");
  await page.keyboard.press("Space");
  await escapeAll();
  await dblAt([18, 6, 0.75], "face");
  await expectHint("Selected solid 3");
  await page.keyboard.press("m");
  await clickAt([18, 6, 0.75], /on-face|midpoint/, 0, 0);
  await page.keyboard.press("ArrowRight");
  await point([22, 6, 0.75]);
  await type("3/4");
  await expectHint("Moved solid 3");
  await page.keyboard.press("Space");
  await escapeAll();
  await dblAt([18, 6, 0.75], "face");
  await component("Shelf");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt([0.75, 0, 0], "endpoint", 1, 1);
  await page.keyboard.press("ArrowUp");
  await point([0.75, 0, 10]);
  await type("23.75");
  await expectHint("Copied component Shelf");
  await type("x3");
  await expectHint("Array: 3 copies of component Shelf");
  await page.keyboard.press("Shift+Z");
  if ((await solids()).length !== 6) fail(`six solids expected, ${(await solids()).length}`);
  await shot("2-shelves");

  // 3. Open the second shelf, pull its top up 1/4 in: every shelf is 1 in thick.
  await page.keyboard.press("Space");
  await escapeAll();
  await dblAt([18, 6, 24.5], "node");
  await expectHint("Editing component Shelf");
  await selectAt([18, 6, 24.5], "face");
  await page.keyboard.press("p");
  await type("1/4");
  await page.waitForFunction(() => window.__sawhorse.solids().filter((s) => Math.abs(s.max[2] - s.min[2] - 0.0254) < 1e-9).length === 4, null, { timeout: 10_000 });
  log.push(`after the edit: ${(await sizes()).join(" | ")}`);
  await page.keyboard.press("Space");
  await shot("3-edit-propagates");

  // 4. Make Unique on the top shelf, then back to 3/4 in: only it changes.
  await escapeAll();
  await page.getByRole("tab", { name: "Outliner" }).click();
  const rows = page.locator(".outline li", { hasText: "component · 4 of 4" });
  await rows.first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Make Unique" }).click();
  await expectHint("Made unique");
  await dblAt([18, 6, 72.25], "node");
  await selectAt([18, 6, 72.25], "face");
  await page.keyboard.press("p");
  await type("-1/4");
  await page.waitForFunction(() => window.__sawhorse.solids().filter((s) => Math.abs(s.max[2] - s.min[2] - 0.0254) < 1e-9).length === 3, null, { timeout: 10_000 });
  await page.keyboard.press("Space");
  await shot("4-make-unique");

  // 5. Tags: Carcass on the sides, Shelves on the shelves.
  await escapeAll();
  await page.getByRole("tab", { name: "Tags" }).click();
  for (const name of ["Carcass", "Shelves"]) {
    await page.getByRole("button", { name: /New tag/ }).click();
    const field = page.getByLabel(/New name for tag Tag/);
    await field.fill(name);
    await field.press("Enter");
    await page.locator(`.tree.tags li[data-tag="${name}"]`).waitFor();
  }
  await page.keyboard.press("Space");
  await selectAt([0.4, 0, 36], "node");
  await selectAt([35.6, 0, 36], "node", true);
  await page.getByRole("button", { name: "Put the selection on Carcass" }).click();
  await expectHint("2 items on Carcass");
  await escapeAll();
  await selectAt([18, 0, 0.5], "node");
  for (const z of [24.25, 48, 71.6]) await selectAt([18, 0, z], "node", true);
  await page.getByRole("button", { name: "Put the selection on Shelves" }).click();
  await expectHint("4 items on Shelves");
  await escapeAll();
  await shot("5-tags");

  // 6. Scenes: Front hides the shelves; Iso shows everything.
  await view("Front");
  await page.keyboard.press("Shift+Z");
  await page.getByRole("button", { name: "Hide tag Shelves" }).click();
  await page.getByRole("button", { name: /Add a scene/ }).click();
  await page.getByRole("tab", { name: "Scene 1" }).dblclick();
  let field = page.getByLabel("New name for scene Scene 1");
  await field.fill("Front");
  await field.press("Enter");
  await page.getByRole("button", { name: "Show tag Shelves" }).click();
  await view("Iso");
  await page.keyboard.press("Shift+Z");
  await page.getByRole("button", { name: /Add a scene/ }).click();
  await page.getByRole("tab", { name: "Scene 2" }).dblclick();
  field = page.getByLabel("New name for scene Scene 2");
  await field.fill("Iso");
  await field.press("Enter");
  await page.getByRole("tab", { name: "Front" }).click();
  await expectHint("Scene Front: hides Shelves");
  const front = await vp.getAttribute("data-triangles");
  await shot("6-scene-front");
  await page.getByRole("tab", { name: "Iso" }).click();
  await expectHint("Scene Iso");
  const iso = await vp.getAttribute("data-triangles");
  if (Number(front) >= Number(iso)) fail(`Front should hide the shelves (${front} vs ${iso} triangles)`);
  await page.getByRole("tab", { name: "Outliner" }).click();
  await shot("7-scene-iso");
  // The model as plain numbers for the report.
  const graph = await page.evaluate(() => window.__sawhorse.graph?.());
  log.push(`definitions: ${JSON.stringify(graph?.definitions?.map((d) => [d.name, d.instances]))}`);
} finally {
  await browser.close();
  console.log(log.join("\n"));
}
