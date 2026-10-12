import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";

// The cut list (lane 7), driven like a person would: the bookshelf of the user guide built
// from components with the keyboard and the mouse (Rect, Push/Pull, Move with typed
// distances, Make Component, Ctrl+Move copies and an array), then the Cut list pane read
// row by row, grain and stock set from its right-click menu, a hidden shelf and Visible
// only, a row clicked to select its pieces, and every export saved and read back.
import { at, clickAt, IN, point, selectAt, solids, type, type V3 } from "./helpers";
import { csvCellText, parseCsv } from "../src/cutlist/csv";

const requireKernel = !!process.env.REQUIRE_KERNEL;
const hint = (page: Page) => page.locator("#hint");

async function ready(page: Page) {
  // io=basic: saving is a download Playwright can catch (no native file picker).
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

async function dblAt(page: Page, p: V3, kind: string) {
  await selectAt(page, p, kind);
  const [x, y] = await at(page, p, 1, -1);
  await page.mouse.dblclick(x, y);
}

/** Zoom in toward a point (the wheel zooms toward the cursor), so thin faces are easy to hit. */
async function zoomTo(page: Page, p: V3) {
  await page.keyboard.press("Shift+Z");
  const [x, y] = await at(page, p);
  await page.mouse.move(x, y);
  for (let k = 0; k < 4; k++) await page.mouse.wheel(0, -240);
  await page.waitForTimeout(100);
}

async function escapeAll(page: Page) {
  await page.keyboard.press("Space");
  for (let k = 0; k < 4; k++) await page.keyboard.press("Escape");
}

async function component(page: Page, name: string) {
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: /Make Component/ }).click();
  const field = page.getByRole("dialog", { name: "Make Component" }).getByLabel("Name");
  await field.fill(name);
  await field.press("Enter");
  await expect(hint(page)).toContainText(`Made component '${name}'`);
}

/** A board at the origin: a typed rectangle on the ground, pulled up. */
async function board(page: Page, size: string, h: string) {
  await escapeAll(page);
  await page.keyboard.press("r");
  await type(page, size);
  await expect(hint(page)).toContainText("on the ground");
  await page.keyboard.press("p");
  await type(page, h);
  await expect(hint(page)).toContainText(`Extruded ${h} in`);
  await page.keyboard.press("Shift+Z");
}

/** Select the solid with a face through `p` (double-click), then move it by a typed
 * distance along an arrow-locked axis, the cursor showing the way. */
async function nudge(page: Page, p: V3, arrow: "ArrowRight" | "ArrowLeft" | "ArrowUp", toward: V3, dist: string) {
  await escapeAll(page);
  await zoomTo(page, p);
  await dblAt(page, p, "face");
  await expect(hint(page)).toContainText("Selected solid");
  await page.keyboard.press("m");
  // Any snap will do: the move is the typed distance along the locked axis.
  await clickAt(page, p, /.+/, 0, 0);
  await page.keyboard.press(arrow);
  await point(page, toward);
  await type(page, dist);
  await expect(hint(page)).toContainText("Moved solid");
}

/**
 * The bookshelf of the user guide, as components: Side x2 (3/4 x 11 1/4 x 71 1/4), Top
 * (3/4 x 11 1/4 x 36) on the sides, Toe kick (3/4 x 3 1/2 x 34 1/2) 2 in back, Shelf x4
 * (3/4 x 11 x 34 1/2) from 3 1/2 in up every 18 in, and a 1/4 in Back panel behind the
 * shelves (built first, while there is room at the origin).
 */
export async function buildBookshelf(page: Page) {
  const vp = page.locator("main.vp");
  // Back panel first, while the model is empty: a 34 1/2 x 71 1/4 slab 1/4 in thick, turned
  // upright about the red axis, then moved behind where the shelves go (3/4 in along red,
  // to 11 in back along green).
  await board(page, "34 1/2, 71 1/4", "1/4");
  await escapeAll(page);
  await zoomTo(page, [17, 35, 0.25]);
  await dblAt(page, [17, 35, 0.25], "face");
  await page.keyboard.press("q");
  await page.keyboard.press("ArrowRight");
  await clickAt(page, [0, 0, 0], "endpoint", -1, 1);
  await clickAt(page, [0, 30, 0], /on-edge|midpoint|endpoint/, -1, 1);
  await type(page, "90");
  await expect(hint(page)).toContainText("Turned solid 1 90°");
  let slab = (await solids(page))[0];
  if (slab.max[2] < 1) {
    await type(page, "-90");
    await expect.poll(async () => (await solids(page))[0].max[2]).toBeGreaterThan(70);
    slab = (await solids(page))[0];
  }
  const y0 = slab.min[1];
  await page.keyboard.press("Shift+Z");
  await nudge(page, [17, y0, 35], "ArrowRight", [25, y0, 35], "3/4");
  await nudge(page, [17.75, y0, 35], "ArrowLeft", [17.75, y0 + 6, 35], `${11 - y0}`);
  await escapeAll(page);
  await zoomTo(page, [17.75, 11, 35]);
  await dblAt(page, [17.75, 11, 35], "face");
  await component(page, "Back panel");
  // Sides.
  await board(page, "3/4, 11 1/4", "71 1/4");
  await escapeAll(page);
  await page.keyboard.press("Shift+Z");
  await dblAt(page, [0, 5.6, 35], "face");
  await component(page, "Side");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt(page, [0, 0, 0], "endpoint", 1, -1);
  await page.keyboard.press("ArrowRight");
  await point(page, [10, 0, 0]);
  await type(page, "35 1/4");
  await expect(hint(page)).toContainText("Copied component 'Side'");
  await page.keyboard.press("Shift+Z");
  // Top: drawn at the origin, lifted onto the sides.
  await board(page, "36, 11 1/4", "3/4");
  await nudge(page, [18, 6, 0.75], "ArrowUp", [18, 6, 20], "71 1/4");
  await escapeAll(page);
  await zoomTo(page, [18, 6, 72]);
  await dblAt(page, [18, 6, 72], "face");
  await component(page, "Top");
  // Toe kick: 3/4 in along red, 2 in back along green.
  await board(page, "34 1/2, 3/4", "3 1/2");
  await nudge(page, [18, 0, 1.75], "ArrowRight", [24, 0, 1.75], "3/4");
  await nudge(page, [18.75, 0, 1.75], "ArrowLeft", [18.75, 6, 1.75], "2");
  await escapeAll(page);
  await zoomTo(page, [18.75, 2, 1.75]);
  await dblAt(page, [18.75, 2, 1.75], "face");
  await component(page, "Toe kick");
  // Shelves: the bottom one on the toe kick, three more every 18 in.
  await board(page, "34 1/2, 11", "3/4");
  // (Points well back on the shelf: the toe kick stands in front of its front half.)
  await nudge(page, [18, 10, 0.75], "ArrowRight", [24, 10, 0.75], "3/4");
  await nudge(page, [18.75, 10, 0.75], "ArrowUp", [18.75, 10, 10], "3 1/2");
  await escapeAll(page);
  await zoomTo(page, [18.75, 10, 4.25]);
  await dblAt(page, [18.75, 10, 4.25], "face");
  await component(page, "Shelf");
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt(page, [35.25, 0, 4.25], "endpoint", -1, -1);
  await page.keyboard.press("ArrowUp");
  await point(page, [35.25, 0, 14]);
  await type(page, "18");
  await expect(hint(page)).toContainText("Copied component 'Shelf'");
  await type(page, "x3");
  await expect(hint(page)).toContainText("Array: 3 copies of component 'Shelf'");
  await escapeAll(page);
  await page.keyboard.press("Shift+Z");
  await expect(vp).toHaveAttribute("data-objects", "9");
  // Every part where it belongs.
  const boxes: [string, V3, V3][] = [
    ["left side", [0, 0, 0], [0.75, 11.25, 71.25]],
    ["right side", [35.25, 0, 0], [36, 11.25, 71.25]],
    ["top", [0, 0, 71.25], [36, 11.25, 72]],
    ["toe kick", [0.75, 2, 0], [35.25, 2.75, 3.5]],
    ...[3.5, 21.5, 39.5, 57.5].map((z, i): [string, V3, V3] => [`shelf ${i + 1}`, [0.75, 0, z], [35.25, 11, z + 0.75]]),
    ["back panel", [0.75, 11, 0], [35.25, 11.25, 71.25]],
  ];
  const got = await solids(page);
  const near = (a: V3, b: V3) => a.every((x, k) => Math.abs(x - b[k]) < 1e-4);
  for (const [name, lo, hi] of boxes) {
    expect(
      got.some((s) => near(s.min, lo) && near(s.max, hi)),
      `${name}: nothing spans ${JSON.stringify(lo)} to ${JSON.stringify(hi)}; ${JSON.stringify(got.map((s) => [s.min, s.max]))}`,
    ).toBe(true);
  }
}

async function cutRows(page: Page): Promise<string[][]> {
  return page.locator(".cutlist table tbody tr").evaluateAll((trs) => trs.map((tr) => Array.from(tr.querySelectorAll("td")).map((td) => (td.querySelector(".name") ?? td).textContent ?? "")));
}

async function save(page: Page, button: string): Promise<{ name: string; bytes: Buffer }> {
  const [d] = await Promise.all([page.waitForEvent("download"), page.locator(".cutlist .btnrow").getByRole("button", { name: button, exact: true }).click()]);
  return { name: d.suggestedFilename(), bytes: readFileSync((await d.path())!) };
}

test("the bookshelf's cut list: rows, totals, grain and stock, hidden parts, selection, CSV, PDF, labels and print", async ({ page }) => {
  test.setTimeout(240_000);
  await ready(page);
  await buildBookshelf(page);

  await page.getByRole("tab", { name: "Cut list" }).click();
  await expect(page.locator(".cutlist table")).toBeVisible();
  await expect.poll(() => cutRows(page)).toEqual([
    ["Side", "2", "3/4", "11 1/4", "71 1/4", "—"],
    ["Top", "1", "3/4", "11 1/4", "36", "—"],
    ["Toe kick", "1", "3/4", "3 1/2", "34 1/2", "—"],
    ["Shelf", "4", "3/4", "11", "34 1/2", "—"],
    ["Back panel", "1", "1/4", "34 1/2", "71 1/4", "—"],
  ]);
  // 2735.15625 cubic inches of lumber: 18.99 board feet; the back panel fits one sheet.
  await expect(page.getByTestId("tile-board-feet")).toHaveText("18.99board feet");
  await expect(page.getByTestId("tile-sheets")).toHaveText("1sheet 4×8, 1/4");
  const kernel = await page.evaluate(async () => (await (window as unknown as { __sawhorse: { cutList(): Promise<{ totals: { board_feet: number } }> } }).__sawhorse.cutList()).totals.board_feet);
  expect(kernel).toBe(2735.15625 / 144);
  // Rounding is display only: every rounding (so every unit system) gives the same totals.
  const totals = await page.evaluate(async () => {
    const api = (window as unknown as { __sawhorse: { cutList(o: { rounding: string }): Promise<{ totals: { board_feet: number; sheet_count: number }; rows: { name: string; sheet: boolean }[] }> } }).__sawhorse;
    const out: [number, number, string[]][] = [];
    for (const rounding of ["sixteenth", "thirty_second", "half_mm"]) {
      const l = await api.cutList({ rounding });
      out.push([l.totals.board_feet, l.totals.sheet_count, l.rows.filter((r) => r.sheet).map((r) => r.name)]);
    }
    return out;
  });
  expect(totals).toEqual([0, 1, 2].map(() => [18.994140625, 1, ["Back panel"]]));

  // Stock from the right-click menu, for many rows at once: Side, then Shift+click Shelf
  // picks the carcass and shelves (one range of four rows); one Stock sets them all, as one
  // undo step. The back is ply.
  const tr = (name: string) => page.locator(".cutlist tbody tr", { hasText: name }).first();
  const undoDepth = async () => Number(/undo (\d+)/.exec(await page.locator(".vp .hud").innerText())?.[1]);
  const before = await undoDepth();
  await tr("Side").click();
  await tr("Shelf").click({ modifiers: ["Shift"] });
  await expect(hint(page)).toContainText("Selected 8 pieces of 4 parts");
  for (const name of ["Side", "Top", "Toe kick", "Shelf"]) await expect(tr(name)).toHaveAttribute("aria-selected", "true");
  await expect(tr("Back panel")).toHaveAttribute("aria-selected", "false");
  await tr("Toe kick").click({ button: "right" });
  await expect(page.getByRole("menu").locator(".ctxtitle")).toHaveText("4 parts: Side, Top, Toe kick, Shelf");
  await page.getByRole("menuitem", { name: /Stock/ }).click();
  const many = page.getByRole("dialog", { name: "Stock for 4 parts" });
  await many.getByRole("textbox").fill("White oak");
  await many.getByRole("button", { name: "Set stock" }).click();
  await expect(hint(page)).toContainText("4 parts are cut from White oak");
  expect(await undoDepth()).toBe(before + 1);
  await page.locator(".cutlist tbody tr", { hasText: "Back panel" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: /Stock/ }).click();
  const dialog = page.getByRole("dialog", { name: "Stock for Back panel" });
  await dialog.getByRole("textbox").fill("1/4 birch ply");
  await dialog.getByLabel("Counted as").selectOption("sheet");
  await dialog.getByRole("button", { name: "Set stock" }).click();
  // The top's grain across its width (a plain click picks it alone first).
  await tr("Top").click();
  await page.locator(".cutlist tbody tr", { hasText: "Top" }).click({ button: "right" });
  await page.getByRole("menuitemradio", { name: /across/ }).click();
  await expect(hint(page)).toContainText("Grain of 'Top': across W");
  await expect(page.locator(".cutlist tbody tr", { hasText: "Top" }).locator("td").nth(3).locator("i.grain")).toHaveCount(1);
  // Ctrl+Z and Ctrl+Y work while the pane's controls have focus (the select, the checkbox).
  await page.getByLabel("Round sizes to").focus();
  await page.keyboard.press("Control+z");
  await expect(hint(page)).toContainText("Undone.");
  await expect(page.locator(".cutlist tbody tr", { hasText: "Top" }).locator("td").nth(4).locator("i.grain")).toHaveCount(1);
  await page.getByLabel("Visible only").focus();
  await page.keyboard.press("Control+y");
  await expect(hint(page)).toContainText("Redone.");
  await expect(page.locator(".cutlist tbody tr", { hasText: "Top" }).locator("td").nth(3).locator("i.grain")).toHaveCount(1);
  await expect(page.getByLabel("Visible only")).not.toBeChecked();
  await expect.poll(async () => (await cutRows(page)).map((r) => r[5])).toEqual(["White oak", "White oak", "White oak", "White oak", "1/4 birch ply"]);
  await expect(page.getByTestId("tile-board-feet")).toHaveText("18.99board feet, White oak");
  await expect(page.getByTestId("tile-sheets")).toHaveText("1sheet 4×8 1/4 birch ply, 1/4");

  // A row click selects its pieces in the model.
  await page.locator(".cutlist tbody tr", { hasText: "Shelf" }).click();
  await expect(hint(page)).toContainText("Selected 4 pieces of 'Shelf'");
  await expect(page.locator(".vp .hud")).toContainText("Selection: 4");
  await expect(page.locator(".cutlist tbody tr", { hasText: "Shelf" })).toHaveAttribute("aria-selected", "true");

  // CSV: every row and the totals, read back.
  const csv = await save(page, "CSV");
  expect(csv.name).toBe("Untitled cut list.csv");
  const text = csv.bytes.toString("utf8").replace(/^﻿/, "");
  const lines = parseCsv(text).map((l) => l.map(csvCellText));
  expect(lines[0]).toEqual(["Part", "Qty", "T", "W", "L", "T (in)", "W (in)", "L (in)", "Material", "Grain", "Stock", "Board feet", "Area (sq ft)", "Notes", "Summary"]);
  // Fractions reach a spreadsheet as text (="3/4"), never as a date.
  expect(text).toContain('"=""3/4"""');
  expect(lines.slice(1).map((l) => [...l.slice(0, 13), l[14]])).toEqual([
    ["Side", "2", "3/4", "11 1/4", "71 1/4", "0.75", "11.25", "71.25", "White oak", "along L", "Lumber", "8.35", "", ""],
    ["Top", "1", "3/4", "11 1/4", "36", "0.75", "11.25", "36", "White oak", "across W", "Lumber", "2.11", "", ""],
    ["Toe kick", "1", "3/4", "3 1/2", "34 1/2", "0.75", "3.5", "34.5", "White oak", "along L", "Lumber", "0.63", "", ""],
    ["Shelf", "4", "3/4", "11", "34 1/2", "0.75", "11", "34.5", "White oak", "along L", "Lumber", "7.91", "", ""],
    ["Back panel", "1", "1/4", "34 1/2", "71 1/4", "0.25", "34.5", "71.25", "1/4 birch ply", "along L", "Sheet", "", "17.07", ""],
    ["Total", "9", "", "", "", "", "", "", "", "", "", "18.99", "17.07", "1 sheet 4x8"],
  ]);

  // The report and the labels are PDFs.
  const pdf = await save(page, "PDF");
  expect(pdf.name).toBe("Untitled cut list.pdf");
  expect(pdf.bytes.subarray(0, 8).toString("latin1")).toBe("%PDF-1.4");
  expect(pdf.bytes.toString("latin1")).toContain("(Cut list: Untitled) Tj");
  const labels = await save(page, "Labels");
  expect(labels.name).toBe("Untitled labels.pdf");
  expect(labels.bytes.toString("latin1")).toContain("(3/4 \\327 11 1/4 \\327 71 1/4 in) Tj");

  // Print: the report alone, in the print stylesheet.
  await page.evaluate(() => {
    (window as unknown as { print: () => void }).print = () => {
      (window as unknown as { __printed: boolean }).__printed = document.body.classList.contains("print-cutlist");
    };
  });
  await page.locator(".cutlist .btnrow").getByRole("button", { name: "Print", exact: true }).click();
  expect(await page.evaluate(() => (window as unknown as { __printed?: boolean }).__printed)).toBe(true);
  await page.emulateMedia({ media: "print" });
  await expect(page.getByTestId("print-report")).toBeVisible();
  await expect(page.locator("#root")).toBeHidden();
  await expect(page.getByTestId("print-report").locator("tbody tr")).toHaveCount(5);
  await page.emulateMedia({ media: "screen" });
  await page.evaluate(() => document.body.classList.remove("print-cutlist"));

  // Hide a shelf: still listed, flagged; Visible only leaves it out.
  await escapeAll(page);
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.getByRole("button", { name: "Hide Shelf" }).last().click();
  await page.getByRole("tab", { name: "Cut list" }).click();
  await expect(page.locator(".cutlist tbody tr", { hasText: "Shelf" })).toContainText("1 hidden");
  await page.getByLabel("Visible only").check();
  await expect.poll(async () => (await cutRows(page)).find((r) => r[0] === "Shelf")?.[1]).toBe("3");
  await expect(page.getByTestId("tile-board-feet")).toHaveText("17.02board feet, White oak");
  // 1/32 in rounding changes nothing here (every size is in sixteenths); metric reads mm.
  await page.getByLabel("Round sizes to").selectOption("half_mm");
  await expect.poll(async () => (await cutRows(page))[0].slice(2, 5)).toEqual(["19", "286", "1810"]);
  await page.screenshot({ path: test.info().outputPath("cutlist.png") });
});

test("the cut list fits the 300 px pane at every rounding, unit and grain, with long names", async ({ page }) => {
  test.setTimeout(120_000);
  await ready(page);
  // One awkward board: thirty-seconds near 100 in, a long name and a long stock.
  await escapeAll(page);
  await page.keyboard.press("r");
  await type(page, "11 15/32, 99 31/32");
  await expect(hint(page)).toContainText("on the ground");
  await page.keyboard.press("p");
  await type(page, "1 15/32");
  await expect(hint(page)).toContainText("Extruded");
  await escapeAll(page);
  await page.keyboard.press("Control+a");
  await component(page, "Adjustable shelf, left-hand carcass, upper bay");
  await page.getByRole("tab", { name: "Cut list" }).click();
  const row = page.locator(".cutlist tbody tr").first();
  await expect(row).toContainText("Adjustable shelf");
  await row.click({ button: "right" });
  // The menu is titled with the row and the row stays marked while it is open.
  await expect(page.getByRole("menu").locator(".ctxtitle")).toHaveText("Adjustable shelf, left-hand carcass, upper bay");
  await expect(row).toHaveClass(/(^| )menu( |$)/);
  await page.getByRole("menuitem", { name: /Stock/ }).click();
  const dialog = page.getByRole("dialog", { name: /^Stock for Adjustable/ });
  await dialog.getByRole("textbox").fill("Quartersawn white oak, select and better, kiln dried");
  await dialog.getByRole("button", { name: "Set stock" }).click();
  await expect(row.locator("td.mat")).toHaveAttribute("title", "Quartersawn white oak, select and better, kiln dried");

  const side = await page.locator(".side").boundingBox();
  expect(Math.round(side!.width)).toBe(300);
  const fits = () =>
    page.evaluate(() => {
      const els = [".cutlist", ".cutlist .tablewrap", "#pane-cutlist"].map((q) => document.querySelector(q) as HTMLElement);
      const over = els.filter((e) => e.scrollWidth > e.clientWidth + 0.5).map((e) => `${e.className || e.id}: ${e.scrollWidth} > ${e.clientWidth}`);
      const t = document.querySelector(".cutlist table") as HTMLElement;
      const w = document.querySelector(".cutlist .tablewrap") as HTMLElement;
      if (t.getBoundingClientRect().width > w.getBoundingClientRect().width + 0.5) over.push(`table ${t.getBoundingClientRect().width} > ${w.getBoundingClientRect().width}`);
      return over;
    });
  const grains = [/along the length/, /across/, /No grain/];
  for (const rounding of ["follow", "sixteenth", "thirty_second", "half_mm"]) {
    await page.getByLabel("Round sizes to").selectOption(rounding);
    for (const g of grains) {
      await row.click({ button: "right" });
      await page.getByRole("menuitemradio", { name: g }).click();
      await expect(page.getByRole("menu")).toHaveCount(0);
      await page.waitForTimeout(50);
      expect(await fits(), `${rounding} ${g}`).toEqual([]);
    }
  }
  await page.getByLabel("Round sizes to").selectOption("thirty_second");
  await expect(row.locator("td").nth(4)).toContainText("99 31/32");
  // Every display unit (Follow units): fractions, decimal inches, ft-in decimals, mm.
  await page.getByLabel("Round sizes to").selectOption("follow");
  const seen: string[] = [];
  for (let k = 0; k < 6; k++) {
    await page.locator("button.units").click();
    await page.waitForTimeout(50);
    seen.push((await row.locator("td").nth(3).innerText()).trim());
    expect(await fits(), `units ${k}`).toEqual([]);
  }
  // decimal in, ft-in (decimal inches), mm, cm, m (both mm), then fractional again.
  // W = 11 15/32 in: 11 1/2 at 1/16 in; 291.5 mm at 0.5 mm.
  expect(seen).toEqual(["11.5", "11.5", "291.5", "291.5", "291.5", "11 1/2"]);
  await page.screenshot({ path: test.info().outputPath("cutlist-fit.png") });
});

test("the cut list of an empty model says how parts come about", async ({ page }) => {
  await ready(page);
  await page.getByRole("tab", { name: "Cut list" }).click();
  await expect(page.getByTestId("cutlist-empty")).toContainText("Make Component");
  await expect(page.locator(".cutlist .btnrow").getByRole("button", { name: "CSV" })).toBeDisabled();
  void IN;
});
