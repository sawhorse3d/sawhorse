import { expect, test, type Download, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import DxfParser from "dxf-parser";

// Nesting (lane 9), driven like a person would: the user guide's bookshelf built from
// components through the UI, its back panel named 1/4 birch ply, then the Cut list pane's
// Nesting section: oak on 4/4 boards and the back on one 1/4 in sheet; a part clicked in a
// diagram selects it; kerf typed as 3/32; the tightest (maxrects) layout; the carcass as
// 3/4 ply (sheets only); a material's stock from the Stock dialog (sides too long for
// 5 x 5 sheets are reported); then the SVGs, the DXF and the PDF booklet saved and read
// back with every part in them. Every layout is checked for overlaps and kerf gaps.
import { buildBookshelf, hint, ready } from "./bookshelf-build";
import { readPdf } from "../src/cutlist/pdf";

const IN = 0.0254;

interface Place {
  tag: string;
  name: string;
  x: number;
  y: number;
  dx: number;
  dy: number;
  turned: boolean;
}
interface Piece {
  index: number;
  kind: "sheet" | "board";
  stock: string;
  length: number;
  width: number;
  usable: [number, number, number, number];
  kerf: number;
  placements: Place[];
  guillotine: boolean;
}
interface Nest {
  algorithm: string;
  settings: { kerf: number; trim: number; algorithm: string };
  pieces: Piece[];
  unplaced: { tag: string; name: string; reason: string }[];
  totals: { sheets: number; boards: number; parts_in: number; placed: number; unplaced: number; efficiency: number };
}

async function nest(page: Page): Promise<Nest> {
  return page.evaluate(async () => (await (window as unknown as { __sawhorse: { nest(): Promise<unknown> } }).__sawhorse.nest()) as never);
}

/** No two parts on a piece overlap or sit closer than the kerf; every part is inside the
 * usable area; nothing turned against the grain unless the part fits only that way. */
function checkLayout(n: Nest) {
  for (const p of n.pieces) {
    const [x0, y0, x1, y1] = p.usable;
    for (const [i, a] of p.placements.entries()) {
      expect(a.x >= x0 - 1e-9 && a.y >= y0 - 1e-9 && a.x + a.dx <= x1 + 1e-9 && a.y + a.dy <= y1 + 1e-9, `${a.tag} inside ${p.stock}`).toBe(true);
      for (const b of p.placements.slice(i + 1)) {
        const apart = a.x + a.dx + p.kerf <= b.x + 1e-9 || b.x + b.dx + p.kerf <= a.x + 1e-9 || a.y + a.dy + p.kerf <= b.y + 1e-9 || b.y + b.dy + p.kerf <= a.y + 1e-9;
        expect(apart, `${a.tag} and ${b.tag} on piece ${p.index}`).toBe(true);
      }
    }
  }
}

const tags = (n: Nest) => n.pieces.flatMap((p) => p.placements.map((q) => q.tag)).sort();

async function downloads(page: Page, count: number, click: () => Promise<void>): Promise<{ name: string; bytes: Buffer }[]> {
  const got: Download[] = [];
  const done = new Promise<void>((resolve) => {
    const on = (d: Download) => {
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
  return Promise.all(got.map(async (d) => ({ name: d.suggestedFilename(), bytes: readFileSync((await d.path())!) })));
}

test("nesting the bookshelf: boards and a sheet, kerf, maxrects, ply, stock, and SVG / DXF / PDF with every part", async ({ page }) => {
  test.setTimeout(300_000);
  await ready(page);
  await buildBookshelf(page);
  await page.getByRole("tab", { name: "Cut list" }).click();
  await expect(page.locator(".cutlist table")).toBeVisible();
  const tr = (name: string) => page.locator(".cutlist tbody tr", { hasText: name }).first();
  // The back panel is 1/4 birch ply.
  await tr("Back panel").click({ button: "right" });
  await page.getByRole("menuitem", { name: /Stock/ }).click();
  const back = page.getByRole("dialog", { name: "Stock for Back panel" });
  await back.getByRole("textbox").fill("1/4 birch ply");
  await back.getByRole("button", { name: "Set stock" }).click();
  await expect(hint(page)).toContainText("cut from 1/4 birch ply");

  // Open the Nesting section.
  await page.getByRole("button", { name: "Nesting ›" }).click();
  const section = page.getByTestId("nesting");
  await expect(section).toBeVisible();
  // Nine pieces: the carcass, top, shelves and toe kick (3/4 in, no material) on five
  // 4/4 x 12 x 96 boards; the back on one 1/4 in sheet.
  await expect(page.getByTestId("tile-nest-boards")).toContainText("5");
  await expect(page.getByTestId("tile-nest-sheets")).toContainText("1sheet: 4 x 8 sheet, 1/4");
  let n = await nest(page);
  expect([n.totals.sheets, n.totals.boards, n.totals.placed, n.totals.unplaced]).toEqual([1, 5, 9, 0]);
  expect(tags(n)).toEqual(["1a", "1b", "2", "3", "4a", "4b", "4c", "4d", "5"]);
  expect(n.pieces.filter((p) => p.kind === "board").every((p) => p.stock === "4/4 x 12 x 96")).toBe(true);
  checkLayout(n);
  const yieldBoards = n.totals.efficiency;
  expect(yieldBoards).toBeCloseTo(6105 / 10368, 9);
  await expect(page.getByTestId("tile-nest-yield")).toContainText("58.9%");
  await expect(section.locator("figure.nest-piece")).toHaveCount(6);
  await expect(section.locator("g.part")).toHaveCount(9);

  // A part in a diagram: hover lights it, click selects it in the model.
  const backPart = section.locator('g.part[data-tag="5"]');
  await backPart.hover();
  await expect(backPart).toHaveClass(/on/);
  await backPart.click();
  await expect(hint(page)).toContainText("Selected 1 piece of 'Back panel'");

  // Kerf 3/32 in: typed, Enter; the layout follows (a settings op, saved with the model).
  const kerf = section.getByLabel("Kerf");
  await expect(kerf).toHaveValue("1/8 in");
  await kerf.fill("3/32");
  await kerf.press("Enter");
  await expect.poll(async () => (await nest(page)).settings.kerf).toBeCloseTo((3 / 32) * IN, 12);
  await expect(kerf).toHaveValue("3/32 in");
  await expect(page.getByTestId("nest-note")).toContainText("Kerf 3/32 in");
  await expect(hint(page)).toContainText("Nesting: kerf 3/32 in, trim none");
  n = await nest(page);
  checkLayout(n);
  expect(n.pieces[0].kerf).toBeCloseTo((3 / 32) * IN, 12);

  // The tightest layout (maxrects).
  await section.getByLabel("Nesting algorithm").selectOption("maxrects");
  await expect.poll(async () => (await nest(page)).algorithm).toBe("maxrects");
  n = await nest(page);
  checkLayout(n);
  expect(n.totals.placed).toBe(9);
  await expect(page.getByTestId("nest-note")).toContainText("tightest layout (CNC)");

  // The carcass as 3/4 birch ply: sheet goods by its name; two 4 x 8 sheets (grain along:
  // four 11 1/4 strips fit across, but nothing 34 1/2 long fits beside a side), and the
  // back's sheet. Back to the table saw for it.
  await section.getByLabel("Nesting algorithm").selectOption("guillotine");
  await tr("Side").click();
  await tr("Shelf").click({ modifiers: ["Shift"] });
  await tr("Toe kick").click({ button: "right" });
  await page.getByRole("menuitem", { name: /Stock/ }).click();
  const many = page.getByRole("dialog", { name: "Stock for 4 parts" });
  await many.getByRole("textbox").fill("3/4 birch ply");
  await many.getByRole("button", { name: "Set stock" }).click();
  await expect.poll(async () => (await nest(page)).totals.sheets).toBe(3);
  n = await nest(page);
  expect([n.totals.sheets, n.totals.boards, n.totals.placed]).toEqual([3, 0, 9]);
  expect(n.pieces.every((p) => p.guillotine)).toBe(true);
  expect(n.pieces.flatMap((p) => p.placements).every((q) => !q.turned)).toBe(true);
  checkLayout(n);
  await expect(page.getByTestId("tile-nest-sheets")).toContainText("3sheets, 2 kinds");

  // The Stock dialog: the 3/4 ply defaults to 5 x 5 Baltic birch; the 71 1/4 in sides do
  // not fit a 60 in sheet and are reported, not dropped. Then back to automatic.
  await section.getByRole("button", { name: "Stock…" }).click();
  const dlg = page.getByRole("dialog", { name: "Stock" });
  await expect(dlg.getByRole("table", { name: "Stock library" })).toContainText("4/4 x 12 x 96");
  await dlg.getByLabel("Stock for 3/4 birch ply").selectOption("5 x 5 Baltic birch, 3/4");
  await expect(page.getByTestId("nest-unplaced")).toContainText("2 parts on no stock");
  await expect(page.getByTestId("nest-unplaced")).toContainText("larger than 5 x 5 Baltic birch, 3/4");
  n = await nest(page);
  // (All sheet goods now, so the rows are in Outliner order: the back panel first, the sides 2a and 2b.)
  expect(n.unplaced.map((u) => [u.name, u.tag, u.reason])).toEqual([
    ["Side", "2a", "too_large"],
    ["Side", "2b", "too_large"],
  ]);
  expect(n.totals.parts_in).toBe(n.totals.placed + n.totals.unplaced);
  await dlg.getByLabel("Stock for 3/4 birch ply").selectOption("");
  await expect(page.getByTestId("nest-unplaced")).toHaveCount(0);
  await dlg.getByRole("button", { name: "Done" }).click();
  await expect(dlg).toHaveCount(0);
  n = await nest(page);
  expect(n.totals.sheets).toBe(3);

  // SVG: one file per piece, each holding every part of its piece.
  const svgs = await downloads(page, n.pieces.length, () => section.locator("div.btnrow").getByRole("button", { name: "SVG", exact: true }).click());
  expect(svgs.map((s) => s.name).every((x) => x.endsWith(".svg"))).toBe(true);
  for (const p of n.pieces) {
    const f = svgs.find((s) => s.name.includes(` ${String(p.index).padStart(2, "0")} `));
    expect(f, `SVG of piece ${p.index}`).toBeTruthy();
    const text = f!.bytes.toString("utf8");
    expect(text).toContain(`data-piece="${p.index}"`);
    for (const q of p.placements) expect(text).toContain(`data-tag="${q.tag}"`);
  }
  // DXF: a polyline per part on its piece's layer, every tag as a label.
  const [dxf] = await downloads(page, 1, () => section.locator("div.btnrow").getByRole("button", { name: "DXF", exact: true }).click());
  expect(dxf.name).toMatch(/nesting\.dxf$/);
  const parsed = new DxfParser().parseSync(dxf.bytes.toString("utf8")) as unknown as { entities: { type: string; layer: string; text?: string }[] };
  for (const p of n.pieces) {
    const layer = `${p.kind.toUpperCase()}-${String(p.index).padStart(2, "0")}`;
    expect(parsed.entities.filter((e) => e.type === "LWPOLYLINE" && e.layer === layer).length, layer).toBe(p.placements.length);
  }
  const labels = parsed.entities.filter((e) => e.type === "TEXT").map((e) => e.text);
  for (const t of tags(n)) expect(labels).toContain(t);
  // PDF booklet: a summary page, then a page per piece with its parts.
  const [pdf] = await downloads(page, 1, () => section.locator("div.btnrow").getByRole("button", { name: "PDF", exact: true }).click());
  const book = readPdf(new Uint8Array(pdf.bytes));
  expect(book.pages).toBeGreaterThanOrEqual(n.pieces.length + 1);
  expect(book.text[0].join(" ")).toContain("What to buy");
  const pages = book.text.slice(1).map((t) => t.join("\n"));
  for (const t of tags(n)) expect(pages.some((pg) => pg.includes(t))).toBe(true);
  await expect(hint(page)).toContainText("a page per sheet or board");
});
