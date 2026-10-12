import { expect, test, type Page } from "@playwright/test";
import { readFileSync, writeFileSync } from "node:fs";
import { readPdf } from "../src/cutlist/pdf";
import { pngDpi } from "../src/sheets/exports";
import { goldenShz, openShz } from "./shz";

// Drawing sheets (lane 12) through the browser build: the sheet golden laid out by the
// kernel on the canvas; then a sheet built from nothing on the bookshelf with every tool
// (viewport, dimension, label, table, line, rectangle), moved, resized, deleted and undone,
// the paper and title block changed, and the sheet saved as SVG.

type P2 = [number, number];
type V3 = [number, number, number];
type Info = { id: number; rect: [number, number, number, number]; factor: number; centre: P2; frame: { right: V3; up: V3 } };
type Render = { id: number; name: string; size: P2; items: { id: number; kind: string; bounds: [number, number, number, number]; text?: string }[]; viewports: Info[]; prims: { p: string; text?: string }[] };
type W = { __sawhorse: { sheet: { render(): Render | null; selection(): number[]; tool(): string; toClient(p: P2): P2 | null } } };

const IN = 0.0254;
const render = (page: Page) => page.evaluate(() => (window as unknown as W).__sawhorse.sheet.render());
const selection = (page: Page) => page.evaluate(() => (window as unknown as W).__sawhorse.sheet.selection());
const hint = (page: Page) => page.locator("#hint");
const texts = (r: Render | null) => (r?.prims ?? []).filter((p) => p.p === "text").map((p) => p.text);

/** Page coordinates of a paper point (metres). */
async function client(page: Page, p: P2): Promise<P2> {
  const c = await page.evaluate((q) => (window as unknown as W).__sawhorse.sheet.toClient(q), p);
  if (!c) throw new Error("no sheet canvas");
  return c;
}

/** Page coordinates of a model point seen in viewport `v`. */
async function modelClient(page: Page, v: Info, m: V3): Promise<P2> {
  const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const d: P2 = [dot(m, v.frame.right), dot(m, v.frame.up)];
  const cx = v.rect[0] + v.rect[2] / 2;
  const cy = v.rect[1] + v.rect[3] / 2;
  return client(page, [cx + (d[0] - v.centre[0]) * v.factor, cy - (d[1] - v.centre[1]) * v.factor]);
}

async function drag(page: Page, a: P2, b: P2) {
  const [p, q] = [await client(page, a), await client(page, b)];
  await page.mouse.move(p[0], p[1]);
  await page.mouse.down();
  await page.mouse.move((p[0] + q[0]) / 2, (p[1] + q[1]) / 2, { steps: 4 });
  await page.mouse.move(q[0], q[1], { steps: 4 });
  await page.mouse.up();
}

async function sheetsMode(page: Page) {
  await page.locator('.scenes:not(.sheet-tabs) .mode-switch [data-mode="sheets"]').click();
  await expect(page.locator(".sheet-area")).toBeVisible();
}

test("the sheet golden lays out on the canvas as the kernel draws it", async ({ page }) => {
  test.setTimeout(60_000);
  await openShz(page, await goldenShz("sheet"), 9, "sheet.shz");
  await sheetsMode(page);
  await expect(page.locator('.sheet-tabs [role="tab"][data-sheet="Bookshelf"]')).toHaveAttribute("aria-selected", "true");
  await expect.poll(async () => (await render(page))?.items.length).toBe(8);
  const r = (await render(page))!;
  const golden = JSON.parse(readFileSync("crates/kernel/tests/golden/sheet.json", "utf8")) as { renders: string[] };
  expect((r as unknown as { hash: string }).hash).toBe(golden.renders[0]);
  expect(r.items.filter((i) => i.kind === "dimension").map((i) => i.text)).toEqual(['36"', '72"', '17 1/4"']);
  // The canvas draws the kernel's primitives: the dimension strings and the title block.
  const svg = page.getByTestId("sheet-svg");
  for (const t of ['36"', '72"', "17 1/4\"", "CUT LIST", "FRONT ELEVATION 1:12", "1 of 1", "As noted"]) await expect(svg.locator("text", { hasText: t }).first()).toBeAttached();
  await page.screenshot({ path: "test-results/sheets-golden.png" });
  // Back to the model: the 3D view was there all along.
  await page.locator('.sheet-tabs .mode-switch [data-mode="model"]').click();
  await expect(page.locator("main.vp")).toBeVisible();
  await expect(page.locator(".sheet-area")).toHaveCount(0);
});

test("build a sheet of the bookshelf with every tool, edit it, undo, and save it as SVG", async ({ page }) => {
  test.setTimeout(120_000);
  const shz = await goldenShz("cutlist", [], (o) => o.op !== "set_hidden" && !(o.op === "copy" && (o as unknown as { xform: { linear: number[][] } }).xform.linear[0][0] < 0));
  await openShz(page, shz, 9, "bookshelf.shz");
  await sheetsMode(page);
  await expect(page.locator(".sheet-empty")).toContainText("No drawing sheets yet");
  await page.getByRole("button", { name: "Add a sheet", exact: true }).first().click();
  await expect(page.locator('.sheet-tabs [role="tab"][data-sheet="Sheet 1"]')).toHaveAttribute("aria-selected", "true");
  await expect.poll(async () => (await render(page))?.size[0]).toBeCloseTo(11 * IN, 9);
  // The View tool is in hand; the bookshelf's front, dragged on the grid.
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__sawhorse.sheet.tool())).toBe("viewport");
  await page.getByRole("combobox", { name: "Shows" }).selectOption("std:front");
  await drag(page, [0.75 * IN, 0.625 * IN], [4.75 * IN, 7.625 * IN]);
  await expect(hint(page)).toContainText("Viewport: Front at 1:12");
  let r = (await render(page))!;
  expect(r.viewports).toHaveLength(1);
  const v = r.viewports[0];
  expect(v.factor).toBeCloseTo(1 / 12, 12);
  expect(v.rect.map((x) => x / IN)).toEqual([0.75, 0.625, 4, 7].map((x) => expect.closeTo(x, 9)));
  expect(texts(r)).toContain("FRONT ELEVATION 1:12");

  // Dimensions: hover a corner (the kernel snaps to the drawing), click, the other, then where.
  await page.locator('.tool[data-sheet-tool="dimension"]').click();
  const dimension = async (kind: string, a: V3, b: V3, at: P2, want: string) => {
    await page.getByRole("radio", { name: kind, exact: true }).click();
    for (const m of [a, b]) {
      const c = await modelClient(page, v, m.map((x) => x * IN) as V3);
      await page.mouse.move(c[0] + 2, c[1] - 2, { steps: 3 });
      await expect(page.locator(".sh-snap")).toBeAttached();
      await page.mouse.click(c[0] + 2, c[1] - 2);
    }
    await expect(hint(page)).toContainText("where the dimension line goes");
    const c = await client(page, at);
    await page.mouse.click(c[0], c[1]);
    await expect.poll(async () => (await render(page))?.items.filter((i) => i.kind === "dimension").map((i) => i.text)).toContain(want);
  };
  const cx = v.rect[0] + v.rect[2] / 2;
  await dimension("Horizontal", [0, 0, 0], [36, 0, 0], [cx, 7.3 * IN], '36"');
  await dimension("Vertical", [36, 0, 0], [36, 0, 72], [4.6 * IN, 4 * IN], '72"');
  await dimension("Vertical", [0.75, 0, 4.25], [0.75, 0, 21.5], [0.65 * IN, 5 * IN], '17 1/4"');
  r = (await render(page))!;
  // At 1:12, 36 in is 3 in of paper between the dimension's ends.
  const horiz = r.items.find((i) => i.text === '36"')!;
  expect(horiz.bounds[2] / IN).toBeGreaterThan(2.999);
  expect(horiz.bounds[2] / IN).toBeLessThan(3.2);

  // A label pointing at the middle of a shelf, its text off to the side.
  await page.locator('.tool[data-sheet-tool="label"]').click();
  const shelf = await modelClient(page, v, [18 * IN, 0, 21.5 * IN]);
  await page.mouse.move(shelf[0] + 1, shelf[1] + 1, { steps: 3 });
  await expect(page.locator(".sh-snap")).toBeAttached();
  await page.mouse.click(shelf[0] + 1, shelf[1] + 1);
  const textAt = await client(page, [5.25 * IN, 7.25 * IN]);
  await page.mouse.click(textAt[0], textAt[1]);
  await page.getByRole("textbox", { name: "Label text" }).fill("Walnut, oil finish");
  await page.keyboard.press("Enter");
  await expect.poll(async () => texts(await render(page))).toContain("Walnut, oil finish");

  // The cut list as a table, a line and a rectangle.
  await page.locator('.tool[data-sheet-tool="table"]').click();
  await drag(page, [5.25 * IN, 3.5 * IN], [10.375 * IN, 6.5 * IN]);
  await expect.poll(async () => texts(await render(page))).toContain("CUT LIST");
  expect(texts(await render(page))).toContain("Side");
  await page.locator('.tool[data-sheet-tool="line"]').click();
  await drag(page, [5.25 * IN, 1 * IN], [8 * IN, 1 * IN]);
  await page.locator('.tool[data-sheet-tool="rect"]').click();
  await drag(page, [8.5 * IN, 0.75 * IN], [10 * IN, 2.5 * IN]);
  await expect.poll(async () => (await render(page))?.items.map((i) => i.kind)).toEqual(["viewport", "dimension", "dimension", "dimension", "label", "table", "line", "rect"]);
  const rectId = (await selection(page))[0];

  // Move the rectangle half an inch left (on the grid), resize the viewport from its corner.
  const rb = (await render(page))!.items.find((i) => i.id === rectId)!.bounds;
  await drag(page, [rb[0] + rb[2] / 2, rb[1] + rb[3] / 2], [rb[0] + rb[2] / 2 - 0.5 * IN, rb[1] + rb[3] / 2 + 0.003]);
  await expect.poll(async () => (await render(page))!.items.find((i) => i.id === rectId)!.bounds[0] / IN).toBeCloseTo(8.0, 2);
  const vp = (await render(page))!.items[0];
  await page.mouse.click(...(await client(page, [vp.bounds[0] + 0.3 * IN, vp.bounds[1] + 0.3 * IN])));
  expect(await selection(page)).toEqual([vp.id]);
  await drag(page, [4.75 * IN, 7.625 * IN], [5 * IN, 7.625 * IN]);
  await expect.poll(async () => (await render(page))!.viewports[0].rect[2] / IN).toBeCloseTo(4.25, 6);
  // The dimensions followed (they are on the model, not the paper).
  expect((await render(page))!.items.filter((i) => i.kind === "dimension").map((i) => i.text)).toEqual(['36"', '72"', '17 1/4"']);

  // Delete the label, then undo it back.
  const label = (await render(page))!.items.find((i) => i.kind === "label")!;
  await page.mouse.click(...(await client(page, [label.bounds[0] + label.bounds[2] - 0.1 * IN, label.bounds[1] + label.bounds[3] / 2])));
  expect(await selection(page)).toEqual([label.id]);
  await page.keyboard.press("Delete");
  await expect.poll(async () => (await render(page))!.items.length).toBe(7);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await render(page))!.items.length).toBe(8);

  // The paper and the title block.
  await page.getByRole("combobox", { name: "Paper size" }).selectOption("tabloid");
  await expect.poll(async () => (await render(page))!.size.map((x) => Math.round((x / IN) * 100) / 100)).toEqual([17, 11]);
  const title = page.getByRole("textbox", { name: "Title", exact: true });
  await title.fill("Bookshelf, walnut");
  await title.press("Enter");
  await expect.poll(async () => texts(await render(page))).toContain("Bookshelf, walnut");
  await page.screenshot({ path: "test-results/sheets-built.png" });

  // Saved as SVG at the paper's size, the dimension strings in it.
  const download = page.waitForEvent("download");
  await page.locator('[data-testid="sheet-export"]').getByRole("button", { name: "SVG", exact: true }).click();
  const file = await (await download).path();
  const svg = readFileSync(file, "utf8");
  expect(svg).toContain('width="17in" height="11in"');
  expect(svg).toContain('viewBox="0 0 1224 792"');
  for (const t of ["36&quot;", "72&quot;", "17 1/4&quot;", "CUT LIST", "Walnut, oil finish", "Bookshelf, walnut"]) expect(svg).toContain(`>${t}<`);
});

test("the sheet golden out as PDF (its dimension strings are text in it), DXF, PNG and print", async ({ page }) => {
  test.setTimeout(90_000);
  await openShz(page, await goldenShz("sheet"), 9, "sheet.shz");
  await sheetsMode(page);
  await expect.poll(async () => (await render(page))?.items.length).toBe(8);
  const exportButton = (kind: string) => page.locator(`[data-testid="sheet-export"] [data-export="${kind}"]`);
  const save = async (kind: string) => {
    const download = page.waitForEvent("download");
    await exportButton(kind).click();
    const d = await download;
    return { name: d.suggestedFilename(), bytes: readFileSync((await d.path())!) };
  };

  // The golden PDF text check: one Letter landscape page, every dimension string as text.
  const pdf = await save("pdf");
  expect(pdf.name).toBe("sheet - Bookshelf.pdf");
  const check = readPdf(new Uint8Array(pdf.bytes));
  expect(check.pages).toBe(1);
  for (const t of ['36"', '72"', '17 1/4"', "FRONT ELEVATION 1:12", "PLAN 1:12", "SECTION A 1:4", "CUT LIST", "Back panel 1/4 ply in a rabbet", "Bookshelf", "1 of 1", "As noted"]) expect(check.text[0]).toContain(t);
  expect(pdf.bytes.toString("latin1")).toContain("/MediaBox [0 0 792 612]");
  writeFileSync("test-results/sheets-golden.pdf", pdf.bytes);

  // DXF: paper inches, text for the dimensions.
  const dxf = (await save("dxf")).bytes.toString("utf8");
  expect(dxf).toContain("$INSUNITS\n 70\n1\n");
  expect(dxf).toContain('\n36"\n');
  expect(dxf).not.toContain("DIMENSION\n");

  // PNG at 150 dpi: 11 x 8.5 in is 1650 x 1275 pixels, and says so.
  const png = (await save("png150")).bytes;
  expect(png.readUInt32BE(16)).toBe(1650);
  expect(png.readUInt32BE(20)).toBe(1275);
  expect(pngDpi(new Uint8Array(png))).toBe(150);
  writeFileSync("test-results/sheets-golden-150dpi.png", png);

  // Print: the sheet alone, at its real size, through the print dialog.
  await page.evaluate(() => {
    const w = window as unknown as { printed?: string[] };
    w.printed = [];
    window.print = () => {
      w.printed!.push(document.querySelector(".print-sheets")?.innerHTML.slice(0, 400) ?? "");
      w.printed!.push(document.querySelector("style[data-print-sheets]")?.textContent ?? "");
    };
  });
  await exportButton("print").click();
  await expect.poll(() => page.evaluate(() => (window as unknown as { printed: string[] }).printed.length)).toBe(2);
  const printed = await page.evaluate(() => (window as unknown as { printed: string[] }).printed);
  expect(printed[0]).toContain('width="11in" height="8.5in"');
  expect(printed[1]).toContain("@page { size: 11in 8.5in; margin: 0 }");
});
