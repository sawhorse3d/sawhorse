#!/usr/bin/env node
// Lane-12 lane-end walk-through: section planes and drawing sheets, driven like a person at
// the mouse and keyboard, with a screenshot after each stage:
//
//   1. the component bookshelf; 2. cut down the middle with the Section Plane tool (locked
//      square to red): the left half gone, the cut filled and outlined;
//   3. the plane reversed from the Outliner; 4. the Right view in the Shop drawing style
//      (hidden line) with the section fill, saved as the scene "Section A";
//   5. a Letter landscape sheet: the front elevation at 1:12 (72 in tall is 9 in at 1:8,
//      more than the 8.5 in paper), the plan at 1:8, the section at 1:4 slid to the toe
//      kick and the bottom shelf, the overall width, the height and a shelf spacing
//      dimensioned, a label with a leader, the cut list as a table, the title block filled;
//   6. the section viewport selected (its properties in the Sheet pane);
//   7. the sheet exported as PDF, SVG, DXF and a 300 dpi PNG (saved next to the shots);
//   8. the same sheet in the dark theme (the paper stays paper).
//
//   npx vite --port 5221 --strictPort   (in another terminal; npm run wasm first)
//   node scripts/capture-lane12.mjs --url http://localhost:5221 --out ../_captures [--prefix 2026-10-12-lane12]
//
// Uses the installed Edge (PW_CHANNEL=msedge). Exits non-zero when a stage does not produce
// what it should.
import { chromium } from "@playwright/test";
import { copyFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const args = Object.fromEntries(process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []));
const url = args.url ?? "http://localhost:5221";
const outDir = path.resolve(args.out ?? "../_captures");
const prefix = args.prefix ?? "2026-10-12-lane12";
const IN = 0.0254;
mkdirSync(outDir, { recursive: true });
const fail = (msg) => {
  throw new Error(msg);
};

// The component bookshelf (the cut list golden, without its hidden shelf and mirrored copy),
// written as a .shz by the same WASM kernel the app runs.
const pkg = path.join("src", "wasm", "pkg");
if (!existsSync(path.join(pkg, "sawhorse_wasm.js"))) fail("build the kernel first: npm run wasm");
const wasm = await import(pathToFileURL(path.resolve(pkg, "sawhorse_wasm.js")).href);
wasm.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
const golden = JSON.parse(readFileSync(path.join("crates", "kernel", "tests", "golden", "cutlist.json"), "utf8"));
const ops = golden.ops.filter((o) => o.op !== "set_hidden" && !(o.op === "copy" && o.xform.linear[0][0] < 0));
const doc = wasm.document_replay(JSON.stringify(ops));
const shelf = Buffer.from(doc.save_shz("frac-in", new Uint8Array()));
doc.free();

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
const shots = [];

async function open(theme, shz, name) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, acceptDownloads: true });
  page.setDefaultTimeout(15_000);
  await page.goto(`${url}/?theme=${theme}&autosave=0&io=basic`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
  await page.getByTestId("file-input").setInputFiles({ name, mimeType: "application/vnd.sawhorse.shz", buffer: shz });
  await page.waitForSelector('main.vp[data-objects="9"]', { timeout: 30_000 });
  return page;
}

function helpers(page, theme) {
  const hint = page.locator("#hint");
  const shot = async (name) => {
    const file = path.join(outDir, `${prefix}-${name}${theme === "dark" ? "-dark" : ""}.png`);
    await page.waitForTimeout(300);
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
      const [x, y] = await at(p, 1 + (i % 3), -1 - (i % 2));
      await page.mouse.move(x + 3, y + 2);
      await page.mouse.move(x, y, { steps: 2 });
      await page.waitForTimeout(120);
      if ((await page.locator("main.vp").getAttribute("data-inference")) === kind) return [x, y];
    }
    fail(`no ${kind} inference at ${p}`);
  };
  const sections = () => page.evaluate(() => window.__sawhorse.sections());
  const render = () => page.evaluate(() => window.__sawhorse.sheet?.render() ?? null);
  const client = async (p) => {
    const c = await page.evaluate((q) => window.__sawhorse.sheet.toClient(q), p);
    if (!c) fail("no sheet canvas");
    return c;
  };
  const drag = async (a, b, mods = []) => {
    const [p, q] = [await client(a), await client(b)];
    for (const m of mods) await page.keyboard.down(m);
    await page.mouse.move(p[0], p[1]);
    await page.mouse.down();
    await page.mouse.move((p[0] + q[0]) / 2, (p[1] + q[1]) / 2, { steps: 5 });
    await page.mouse.move(q[0], q[1], { steps: 5 });
    await page.mouse.up();
    for (const m of mods) await page.keyboard.up(m);
  };
  const until = async (what, test, tries = 60) => {
    for (let i = 0; i < tries; i++) {
      if (await test()) return;
      await page.waitForTimeout(150);
    }
    fail(`timed out waiting for ${what}: ${(await hint.textContent())?.trim()}`);
  };
  const tool = (id) => page.locator(`.tool[data-sheet-tool="${id}"]`).click();
  return { hint, shot, hover, sections, render, client, drag, until, tool };
}

// ---- the model: cut, reversed, a section scene ------------------------------------------
const page = await open("light", shelf, "bookshelf.shz");
const h = helpers(page, "light");
await page.getByRole("button", { name: "Iso", exact: true }).click();
await page.keyboard.press("Shift+Z");
await h.shot("01-bookshelf");

await page.getByRole("button", { name: "Tools" }).click();
await page.getByRole("menuitemcheckbox", { name: "Section Plane" }).click();
await h.hover([18, 5.5, 58.25], "on-face");
await page.keyboard.press("ArrowRight");
const [sx, sy] = await h.hover([18, 5.5, 58.25], "on-face");
await page.mouse.click(sx, sy);
await h.until("the section plane", async () => (await h.sections()).active === 1);
await page.mouse.move(40, 860);
await h.shot("02-section-cut");

await page.keyboard.press(" ");
await page.getByTestId("outliner").locator('[data-row="s:1"]').click({ button: "right" });
await page.getByRole("menuitem", { name: "Reverse" }).click();
await h.until("the reversed plane", async () => (await page.evaluate(() => window.__sawhorse.graph().sections[0].normal[0])) < -0.99);
await page.mouse.move(40, 860);
await h.shot("03-section-reversed");

// The Right view looks at the kept half's cut face; the Shop drawing style (hidden line)
// keeps the section fill. Saved as the scene the sheet's section viewport shows.
await page.getByRole("button", { name: "Right", exact: true }).click();
await page.keyboard.press("Shift+Z");
await page.getByRole("tab", { name: "Styles" }).click();
await page.locator(".presets button", { hasText: "Shop drawing" }).click();
await page.locator('.presets button[aria-pressed="true"]', { hasText: "Shop drawing" }).waitFor();
await page.locator(".scenes:not(.sheet-tabs)").getByRole("button", { name: "Add a scene from the current view" }).click();
await page.locator('.scenes:not(.sheet-tabs) [role="tab"]', { hasText: "Scene 1" }).dblclick();
const rename = page.getByRole("textbox", { name: "New name for scene Scene 1" });
await rename.fill("Section A");
await rename.press("Enter");
await h.until("the scene Section A", async () => (await page.locator('.scenes:not(.sheet-tabs) [data-scene="Section A"]').count()) === 1);
const sec = await h.sections();
if (!sec.fill || sec.caps < 1) fail(`the section is not filled: ${JSON.stringify(sec)}`);
await h.shot("04-hidden-line-section-scene");

// Whole again for the elevations (the scene keeps the plane active for its viewport).
await page.getByRole("tab", { name: "Outliner" }).click();
await page.getByTestId("outliner").locator('[data-row="s:1"]').click({ button: "right" });
await page.getByRole("menuitem", { name: "Deactivate Cut" }).click();
await h.until("the cut off", async () => (await h.sections()).active === 0);

// ---- the sheet -----------------------------------------------------------------------------
await page.locator('.scenes:not(.sheet-tabs) .mode-switch [data-mode="sheets"]').click();
await page.getByRole("button", { name: "Add a sheet", exact: true }).first().click();
await h.until("Sheet 1", async () => (await h.render())?.name === "Sheet 1");

const place = async (source, a, b) => {
  await h.tool("viewport");
  await page.getByRole("combobox", { name: "Shows" }).selectOption(source);
  const before = (await h.render()).viewports.length;
  await h.drag(a, b);
  await h.until(`the ${source} viewport`, async () => (await h.render()).viewports.length === before + 1);
  return (await h.render()).viewports[before];
};
const front = await place("std:front", [0.625 * IN, 0.625 * IN], [4.625 * IN, 7.625 * IN]);
if (Math.abs(front.factor - 1 / 12) > 1e-12) fail(`the front elevation is not at 1:12 (${front.factor})`);
const scaleTo = async (index, label, factor) => {
  await page.getByTestId("item-props").getByRole("combobox", { name: "Scale" }).selectOption(label);
  await h.until(`viewport ${index} at ${label}`, async () => Math.abs((await h.render()).viewports[index].factor - factor) < 1e-12);
};
await place("std:top", [5 * IN, 0.625 * IN], [10.5 * IN, 2.5 * IN]);
await scaleTo(1, "1:8", 1 / 8);
const section = await place("scene:Section A", [5 * IN, 2.75 * IN], [8.5 * IN, 4.5 * IN]);
await scaleTo(2, "1:4", 1 / 4);
// At 1:4 the viewport holds 7 in of the section: slid down to the toe kick and the bottom
// shelf (Alt+drag slides the drawing; a few strokes, each at most 1 1/2 in on paper).
const sr = section.rect;
const target = 3.25 * IN;
for (let i = 0; i < 8; i++) {
  const v = (await h.render()).viewports[2];
  const off = (v.centre[1] - target) * v.factor;
  if (Math.abs(off) < 0.02 * IN) break;
  const step = Math.sign(off) * Math.min(Math.abs(off), 1.5 * IN);
  const from = [sr[0] + sr[2] / 2, sr[1] + sr[3] / 2 + (step > 0 ? 0.6 * IN : -0.6 * IN)];
  const before = v.centre[1];
  await h.drag(from, [from[0], from[1] - step], ["Alt"]);
  await h.until("the section to slide", async () => (await h.render()).viewports[2].centre[1] !== before);
}
if (Math.abs((await h.render()).viewports[2].centre[1] - target) > 0.25 * IN) fail("the section did not slide to the bottom shelf");

// Dimensions: the overall width under the elevation, the height to its right, a shelf
// spacing to its left (clicked on the drawing: the kernel snaps to its lines).
const modelClient = async (v, m) => {
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const d = [dot(m, v.frame.right), dot(m, v.frame.up)];
  const cx = v.rect[0] + v.rect[2] / 2;
  const cy = v.rect[1] + v.rect[3] / 2;
  return h.client([cx + (d[0] - v.centre[0]) * v.factor, cy - (d[1] - v.centre[1]) * v.factor]);
};
await h.tool("dimension");
const dimension = async (kind, a, b, at, want) => {
  await page.getByRole("radio", { name: kind, exact: true }).click();
  const v = (await h.render()).viewports[0];
  for (const m of [a, b]) {
    const c = await modelClient(v, m.map((x) => x * IN));
    await page.mouse.move(c[0] + 2, c[1] - 2, { steps: 3 });
    await page.locator(".sh-snap").waitFor({ state: "attached" });
    await page.mouse.click(c[0] + 2, c[1] - 2);
  }
  const c = await h.client(at);
  await page.mouse.move(c[0], c[1], { steps: 4 });
  await page.mouse.click(c[0], c[1]);
  await h.until(`the ${want} dimension`, async () => (await h.render()).items.some((i) => i.kind === "dimension" && i.text === want));
};
await dimension("Horizontal", [0, 0, 0], [36, 0, 0], [2.625 * IN, 7.25 * IN], '36"');
await dimension("Vertical", [36, 0, 0], [36, 0, 72], [4.45 * IN, 4 * IN], '72"');
await dimension("Vertical", [0.75, 0, 4.25], [0.75, 0, 21.5], [0.8 * IN, 5 * IN], '17 1/4"');

// A label on a fixed shelf's edge, its text off to the side.
await h.tool("label");
{
  const v = (await h.render()).viewports[0];
  const c = await modelClient(v, [24 * IN, 0, 21.5 * IN]);
  await page.mouse.move(c[0] + 1, c[1] + 1, { steps: 3 });
  await page.locator(".sh-snap").waitFor({ state: "attached" });
  await page.mouse.click(c[0] + 1, c[1] + 1);
  const t = await h.client([5 * IN, 7.375 * IN]);
  await page.mouse.click(t[0], t[1]);
  await page.getByRole("textbox", { name: "Label text" }).fill("Fixed shelf, 3/4 oak");
  await page.keyboard.press("Enter");
  await h.until("the label", async () => (await h.render()).items.some((i) => i.kind === "label"));
}

// The cut list as a table.
await h.tool("table");
await h.drag([5 * IN, 4.875 * IN], [10.5 * IN, 6.5 * IN]);
await h.until("the cut list", async () => (await h.render()).prims.some((p) => p.p === "text" && p.text === "CUT LIST"));

// The title block.
const field = async (label, value) => {
  const f = page.getByTestId("title-block").getByRole("textbox", { name: label, exact: true });
  await f.fill(value);
  await f.press("Enter");
};
await page.mouse.click(...(await h.client([9.5 * IN, 3.625 * IN])));
await field("Title", "Bookshelf");
await field("Project", "Shop drawings");
await field("Revision", "A");
await page.getByTestId("title-block").getByRole("button", { name: "Today's date" }).click();
await h.until("the title block", async () => (await h.render()).prims.some((p) => p.p === "text" && p.text === "Shop drawings"));
await page.mouse.move(1100, 860);
await h.shot("05-sheet");

// The section viewport selected: its properties in the Sheet pane.
await page.mouse.click(...(await h.client([sr[0] + 0.2 * IN, sr[1] + 0.2 * IN])));
await page.getByTestId("item-props").waitFor();
await h.shot("06-sheet-section-selected");
await page.keyboard.press("Escape");

// ---- exports -------------------------------------------------------------------------------
const saveAs = async (kind, file) => {
  const download = page.waitForEvent("download");
  await page.locator(`[data-testid="sheet-export"] [data-export="${kind}"]`).click();
  const d = await download;
  const to = path.join(outDir, `${prefix}-${file}`);
  copyFileSync(await d.path(), to);
  console.log(`  saved ${path.basename(to)} (${d.suggestedFilename()})`);
  return to;
};
const pdf = readFileSync(await saveAs("pdf", "sheet.pdf"), "latin1");
for (const t of ["(36\") Tj", "(72\") Tj", "(17 1/4\") Tj", "(CUT LIST) Tj", "(Bookshelf) Tj", "(FRONT ELEVATION 1:12) Tj", "(PLAN 1:8) Tj", "(SECTION A 1:4) Tj"]) if (!pdf.includes(t)) fail(`the PDF has no ${t}`);
if (!pdf.includes("/MediaBox [0 0 792 612]")) fail("the PDF is not Letter landscape");
const svg = readFileSync(await saveAs("svg", "sheet.svg"), "utf8");
if (!svg.includes('width="11in" height="8.5in"')) fail("the SVG is not 11 x 8.5 in");
await saveAs("dxf", "sheet.dxf");
await saveAs("png300", "sheet-300dpi.png");

// Save the document for the dark run.
const download = page.waitForEvent("download");
await page.keyboard.press("Control+s");
const saved = readFileSync(await (await download).path());

// ---- the dark theme --------------------------------------------------------------------------
const dark = await open("dark", saved, "bookshelf sheets.shz");
const d = helpers(dark, "dark");
await dark.locator('.scenes:not(.sheet-tabs) .mode-switch [data-mode="sheets"]').click();
await d.until("the sheet", async () => (await d.render())?.items.length === 8);
await dark.mouse.move(1100, 860);
await d.shot("07-sheet");
// Back to the model: the scene Section A with its cut.
await dark.locator('.sheet-tabs .mode-switch [data-mode="model"]').click();
await dark.locator('.scenes:not(.sheet-tabs) [data-scene="Section A"]').click();
await d.until("the scene's cut", async () => (await d.sections()).active === 1);
await d.shot("08-section-scene");

await browser.close();
console.log(`${shots.length} screenshots in ${outDir}`);
