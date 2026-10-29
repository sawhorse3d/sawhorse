import { expect, test, type Page } from "@playwright/test";
import { curveModels } from "../src/io/corpus/models";

// Lane 23 (#3): a circle, an arc or a polygon drawn by its tool is one curve, as in
// SketchUp. Drawn, clicked (the status line says what it is, in mm), moved by a typed
// distance, scaled from a grip, copied with Ctrl, split by a line into two arcs (each
// selects whole), one arc erased (only it goes), the other exploded (its segments plain
// again); a polygon pushed into a solid and its top rim clicked (the whole rim); the rim's
// Centre offered while hovering it and snapped to; undone step by step and redone; saved as
// .shz and opened again with its curves; a DXF of circles importing as curves; the corpus's
// SketchUp circle, arc and welded curve importing as a circle, an arc and a polyline (lane 25,
// openskp 1.4.0), the circle selecting whole. Positions are inches (the helpers' unit); the
// model's display units are millimetres.
import { IN, at, clickAt, point, selectAt, type, type V3, setUnits } from "./helpers";

const requireKernel = !!process.env.REQUIRE_KERNEL;
const MM = 0.001;

type Curve = { owner: { kind: string; id: number }; curve: number; kind: string; closed: boolean; segments: number; centre?: V3; radius?: number; edges: number };

async function curves(page: Page): Promise<Curve[]> {
  return page.evaluate(() => (window as unknown as { __sawhorse: { curves(): Promise<Curve[]> } }).__sawhorse.curves());
}

async function ready(page: Page) {
  page.on("dialog", (d) => void d.accept());
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  await millimetres(page);
  return vp;
}

/** Millimetres: the status line says lengths in the model's units. */
async function millimetres(page: Page) {
  await setUnits(page, "mm");
  await expect(page.locator(".vcb .units")).toHaveText("mm");
}

async function menu(page: Page, top: string, item: string) {
  await page.getByRole("button", { name: top, exact: true }).click();
  await page.getByRole("menuitem", { name: item, exact: true }).click();
}

async function download(page: Page, action: () => Promise<void>) {
  const [d] = await Promise.all([page.waitForEvent("download"), action()]);
  const fs = await import("node:fs");
  return fs.readFileSync(await d.path());
}

/** Metres to the helpers' inches. */
const inch = (p: [number, number, number]): V3 => [p[0] / IN, p[1] / IN, p[2] / IN];
/** The point at `deg` on a circle (metres in, inches out). */
const onCircle = (c: [number, number], r: number, deg: number, z = 0): V3 => inch([c[0] + r * Math.cos((deg * Math.PI) / 180), c[1] + r * Math.sin((deg * Math.PI) / 180), z]);
/** The middle of the segment from `a` to `b` degrees of an n-gon (metres in, inches out). */
const midSeg = (c: [number, number], r: number, a: number, b: number, z = 0): V3 => {
  const p = onCircle(c, r, a, z);
  const q = onCircle(c, r, b, z);
  return [(p[0] + q[0]) / 2, (p[1] + q[1]) / 2, (p[2] + q[2]) / 2];
};
const byKind = (cs: Curve[]) =>
  cs
    .map((c) => `${c.owner.kind}:${c.kind}`)
    .sort()
    .join(" ");

/** Put the cursor on a grip (inches) and wait until it is the hot one. */
async function grip(page: Page, p: V3) {
  await expect(async () => {
    const [x, y] = await at(page, p);
    await page.mouse.move(x + 3, y + 2);
    await page.mouse.move(x, y, { steps: 2 });
    await expect(page.locator(".vlabel.grip.hot")).toHaveCount(1, { timeout: 800 });
  }).toPass({ timeout: 10_000 });
  return at(page, p);
}

test("a circle is one curve: select, move, scale, copy, split, erase, explode; a pushed polygon's rim, its Centre; undo, redo, .shz", async ({ page }) => {
  test.setTimeout(120_000);
  const vp = await ready(page);
  const hint = page.locator("#hint");

  // 1. A circle of 300 mm at the origin, its first point along red.
  await page.keyboard.press("c");
  await clickAt(page, [0, 0, 0], /grid|endpoint|axis/);
  await point(page, [20, 0, 0]);
  await type(page, "300");
  await expect(hint).toContainText("Circle r 300 mm, 24 segments");
  let cs = await curves(page);
  expect(cs).toHaveLength(1);
  expect(cs[0]).toMatchObject({ owner: { kind: "sketch" }, kind: "circle", closed: true, segments: 24, edges: 24 });
  expect(cs[0].radius).toBeCloseTo(0.3, 9);

  // 2. A click on the rim selects the whole circle; the status line says what it is.
  await page.keyboard.press("Space");
  await selectAt(page, midSeg([0, 0], 0.3, 0, 15), "curve");
  await expect(hint).toContainText("Selected a circle (24 segments, radius 300 mm). Delete erases it; Edit > Explode Curve makes its edges plain.");
  await expect(vp).toHaveAttribute("data-selection", "1");

  // 3. Moved 100 mm along red, typed: still the circle (the same id), its centre moved.
  await page.keyboard.press("m");
  await clickAt(page, onCircle([0, 0], 0.3, 0), "endpoint");
  await point(page, onCircle([0, 0], 0.3, 0, 0).map((x, k) => (k === 0 ? x + 8 : x)) as V3);
  await expect(vp).toHaveAttribute("data-inference", "axis-x");
  await type(page, "100");
  await expect(hint).toContainText("Moved a circle 100 mm along +red");
  cs = await curves(page);
  expect(cs).toHaveLength(1);
  expect(cs[0].kind).toBe("circle");
  expect(cs[0].centre![0]).toBeCloseTo(0.1, 9);
  expect(cs[0].centre![1]).toBeCloseTo(0, 9);
  await expect(vp).toHaveAttribute("data-selection", "1");

  // 4. Scaled x2 from its top-right grip (about the opposite one): a circle of 600 mm.
  await page.keyboard.press("s");
  await expect(page.locator(".vlabel.grip")).toHaveCount(8);
  // Lane 23b: a factor has no unit: no "mm" chip beside the Scale box.
  await expect(page.locator("#vcbName")).toHaveText("Scale");
  await expect(page.locator(".vcb button.units")).toHaveCount(0);
  await expect(page.getByTestId("vcb-unit")).toHaveText("");
  const [gx, gy] = await grip(page, inch([0.4, 0.3, 0]));
  await page.mouse.click(gx, gy);
  await page.mouse.move(gx + 40, gy - 30, { steps: 3 });
  await type(page, "2");
  await expect(hint).toContainText("Scaled a circle ×2: radius 600 mm.");
  cs = await curves(page);
  expect(cs[0].kind).toBe("circle");
  expect(cs[0].radius).toBeCloseTo(0.6, 9);
  expect(cs[0].centre![0]).toBeCloseTo(0.4, 9);
  expect(cs[0].centre![1]).toBeCloseTo(0.3, 9);
  const C: [number, number] = [0.4, 0.3];
  const R = 0.6;

  // 5. Copied with Ctrl 1500 mm along green: a second circle, a new curve.
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt(page, onCircle(C, R, 0), "endpoint");
  await point(page, onCircle(C, R, 0).map((x, k) => (k === 1 ? x + 20 : x)) as V3);
  await expect(vp).toHaveAttribute("data-inference", "axis-y");
  await type(page, "1500");
  await expect(hint).toContainText("Copied a circle 1500 mm along +green");
  cs = await curves(page);
  expect(byKind(cs)).toBe("sketch:circle sketch:circle");
  expect(new Set(cs.map((c) => c.curve)).size).toBe(2);
  const copy = cs.find((c) => c.centre![1] > 1)!;
  expect(copy.centre![1]).toBeCloseTo(1.8, 9);

  // 6. A line across the first circle (y = 14 in): two arcs, each selected whole.
  await page.keyboard.press("Escape");
  await page.keyboard.press("l");
  await clickAt(page, [-20, 14, 0], "grid");
  // Locked to red (other points around would offer "from point" alignments).
  await page.keyboard.press("ArrowRight");
  await point(page, [0, 14, 0]);
  await type(page, "2000");
  await expect(hint).toContainText("Line 2000 mm");
  await page.keyboard.press("Escape");
  cs = await curves(page);
  expect(byKind(cs)).toBe("sketch:arc sketch:arc sketch:circle");
  await page.keyboard.press("Space");
  await selectAt(page, midSeg(C, R, 90, 105), "curve");
  await expect(hint).toContainText("Selected an arc (");
  await expect(vp).toHaveAttribute("data-selection", "1");
  await selectAt(page, midSeg(C, R, 270, 285), "curve");
  await expect(hint).toContainText("Selected an arc (");

  // 7. The Eraser on the top arc: only that arc goes (the line and the other arc stay).
  await page.keyboard.press("e");
  await selectAt(page, midSeg(C, R, 90, 105), "curve");
  await expect(hint).toContainText("Erased 1 item");
  cs = await curves(page);
  expect(byKind(cs)).toBe("sketch:arc sketch:circle");

  // 8. Explode the other arc: its segments are plain edges; a click takes one segment.
  await page.keyboard.press("Space");
  await selectAt(page, midSeg(C, R, 270, 285), "curve");
  await menu(page, "Edit", "Explode Curve");
  await expect(hint).toContainText("Exploded the curve");
  cs = await curves(page);
  expect(byKind(cs)).toBe("sketch:circle");
  await selectAt(page, midSeg(C, R, 270, 285), "edge");
  await expect(hint).toContainText("Selected a sketch edge. Delete erases it.");

  // 9. A polygon (6 sides, 200 mm) pulled 100 mm: its top rim selects whole.
  await page.keyboard.press("c");
  await page.keyboard.press("c");
  await expect(hint).toContainText("Polygon (6 sides)");
  await clickAt(page, [-40, 0, 0], /grid|axis/);
  await point(page, [-30, 0, 0]);
  await type(page, "200");
  await expect(hint).toContainText("Polygon r 200 mm, 6 sides");
  // Its centre where the click landed (the metric grid snaps it to -1 m).
  const poly = (await curves(page)).find((c) => c.kind === "polygon")!;
  const P: [number, number] = [poly.centre![0], poly.centre![1]];
  expect(poly.radius).toBeCloseTo(0.2, 9);
  // Push/Pull takes the region the polygon just made, typed.
  await page.keyboard.press("p");
  await type(page, "100");
  await expect(hint).toContainText("Extruded");
  cs = await curves(page);
  expect(byKind(cs)).toBe("object:polygon object:polygon sketch:circle");
  for (const c of cs.filter((x) => x.owner.kind === "object")) expect(c.edges).toBe(6);
  await page.keyboard.press("Space");
  await selectAt(page, midSeg(P, 0.2, 0, 60, 0.1), "curve");
  await expect(hint).toContainText("Selected a polygon (6 sides, radius 200 mm), a rim of solid 1. Edit > Explode Curve makes its edges plain.");
  await expect(vp).toHaveAttribute("data-selection", "1");

  // 10. Hovering the rim with the Line tool offers its Centre; moving there snaps to it.
  await page.keyboard.press("Escape");
  await page.keyboard.press("l");
  const centre = `${P[0].toFixed(6)},${(0).toFixed(6)},${(0.1).toFixed(6)}`;
  await expect(async () => {
    const [x, y] = await at(page, midSeg(P, 0.2, 0, 60, 0.1), 1, -1);
    await page.mouse.move(x + 2, y + 1);
    await page.mouse.move(x, y, { steps: 2 });
    await expect(vp).toHaveAttribute("data-centre", centre, { timeout: 800 });
  }).toPass({ timeout: 10_000 });
  await expect(page.getByTestId("centre-hint")).toHaveText("Centre");
  await expect(async () => {
    const [x, y] = await at(page, inch([P[0], 0, 0.1]));
    await page.mouse.move(x + 2, y + 1);
    await page.mouse.move(x, y, { steps: 2 });
    await expect(vp).toHaveAttribute("data-inference", "centre", { timeout: 800 });
  }).toPass({ timeout: 10_000 });
  await expect(page.locator("#tip")).toContainText("Centre");
  await page.keyboard.press("Escape");

  // 11. Undo through all of it, one step at a time, then redo it all.
  const want = [
    "sketch:circle sketch:polygon", // the extrusion undone: the polygon is a curve again
    "sketch:circle", // the polygon
    "sketch:arc sketch:circle", // the explode
    "sketch:arc sketch:arc sketch:circle", // the erase
    "sketch:circle sketch:circle", // the line: one circle again
    "sketch:circle", // the copy
    "sketch:circle", // the scale
    "sketch:circle", // the move
    "", // the circle
  ];
  const final = byKind(await curves(page));
  for (const w of want) {
    await page.keyboard.press("Control+z");
    await expect.poll(async () => byKind(await curves(page))).toBe(w);
  }
  expect((await curves(page)).length).toBe(0);
  for (let i = 0; i < want.length; i++) await page.keyboard.press("Control+y");
  await expect.poll(async () => byKind(await curves(page))).toBe(final);
  await expect(vp).toHaveAttribute("data-can-redo", "false");

  // 12. Saved and opened again: the same model, curves and all.
  const hash = await vp.getAttribute("data-hash");
  const before = await curves(page);
  const saved = await download(page, () => page.keyboard.press("Control+s"));
  await menu(page, "File", "New");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await page.getByTestId("file-input").setInputFiles({ name: "curves.shz", mimeType: "application/octet-stream", buffer: saved });
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(vp).toHaveAttribute("data-hash", hash!);
  expect((await curves(page)).map((c) => [c.owner.kind, c.curve, c.kind, c.edges])).toEqual(before.map((c) => [c.owner.kind, c.curve, c.kind, c.edges]));
  await page.keyboard.press("Space");
  await selectAt(page, midSeg(P, 0.2, 0, 60, 0.1), "curve");
  await expect(hint).toContainText("Selected a polygon (6 sides, radius 200 mm)");
});

test("a DXF's circles and arc, and a SketchUp file's circle, arc and welded curve, come in as curves", async ({ page }) => {
  const vp = await ready(page);
  const circle = (x: number, y: number, r: number) => `0\nCIRCLE\n8\n0\n10\n${x}\n20\n${y}\n30\n0\n40\n${r}\n`;
  const dxf = `0\nSECTION\n2\nENTITIES\n${circle(0, 0, 50)}${circle(200, 0, 30)}0\nARC\n8\n0\n10\n400\n20\n0\n30\n0\n40\n40\n50\n0\n51\n135\n0\nENDSEC\n0\nEOF\n`;
  await page.getByTestId("file-input").setInputFiles({ name: "rounds.dxf", mimeType: "application/dxf", buffer: Buffer.from(dxf) });
  const report = page.getByTestId("import-report");
  await expect(report).toContainText("3 curves came in as curves (2 circles, 1 arc)");
  const cs = await curves(page);
  expect(byKind(cs)).toBe("sketch:arc sketch:circle sketch:circle");
  expect(cs.find((c) => c.kind === "arc")!.radius).toBeCloseTo(40 * MM, 9);

  // Lane 25: the corpus's three SketchUp curves (openskp 1.4.0 says which edges make up each,
  // and an arc's exact circle): a disc whose 24 edges share one CArcCurve, a quarter arc of
  // 12, a closed welded curve of 6 with no arc record. Each lies at the top of its model, so
  // it goes into a sketch as one curve (the disc's face with it, not an amber mesh), and the
  // report counts it.
  const want: [string, string, number][] = [
    ["201-curve-circle", "circle", 24],
    ["202-curve-arc", "arc", 12],
    ["203-curve-polygon", "polyline", 6],
  ];
  const skps = curveModels();
  expect(skps.map((s) => s.name)).toEqual(want.map((w) => w[0]));
  for (const [k, skp] of skps.entries()) {
    await menu(page, "File", "New");
    await expect(vp).toHaveAttribute("data-objects", "0");
    await page.getByTestId("file-input").setInputFiles({ name: `${skp.name}.skp`, mimeType: "application/octet-stream", buffer: Buffer.from(skp.build()) });
    await expect(report).toContainText(`Imported ${skp.name}.skp`);
    await expect(report).toContainText("1 curve came in as curve");
    await expect(report).not.toContainText("plain edges");
    await expect(report).not.toContainText("did not match");
    await expect(report).not.toContainText("not solid");
    const got = await curves(page);
    expect(got.map((c) => [c.owner.kind, c.kind, c.segments]), skp.name).toEqual([["sketch", want[k][1], want[k][2]]]);
    if (want[k][1] !== "polyline") expect(got[0].radius!, skp.name).toBeCloseTo(6 * IN, 9);
    await expect(vp).toHaveAttribute("data-objects", "1");
  }
  // The circle selects whole, as one drawn with the Circle tool does.
  await menu(page, "File", "New");
  await page.getByTestId("file-input").setInputFiles({ name: "201-curve-circle.skp", mimeType: "application/octet-stream", buffer: Buffer.from(skps[0].build()) });
  await expect(report).toContainText("1 curve came in as curve");
  await page.keyboard.press("Space");
  await selectAt(page, midSeg([0, 0], 6 * IN, 0, 15), "curve");
  await expect(page.locator("#hint")).toContainText("Selected a circle (24 segments, radius 152.4 mm)");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
});

type P2 = [number, number];
type SheetRender = { items: { id: number; kind: string; prims: [number, number] }[]; viewports: { factor: number }[]; prims: { p: string; a?: P2; b?: P2; dash?: P2 }[] };
type Region = { sketch: number; region: number };

async function regions(page: Page): Promise<Region[]> {
  return page.evaluate(() => (window as unknown as { __sawhorse: { regions(): Promise<Region[]> } }).__sawhorse.regions());
}

/** A press at `a`, a drag through to `b` (inches), and the release there. */
async function sweep(page: Page, a: V3, b: V3) {
  const [x0, y0] = await at(page, a);
  const [x1, y1] = await at(page, b);
  await page.mouse.move(x0, y0);
  await page.mouse.down();
  await page.mouse.move(x1, y1, { steps: 60 });
  await page.mouse.up();
}

/** The Circle tool in its circle variant (C again toggles Polygon, and it remembers). */
async function circleTool(page: Page) {
  const hint = page.locator("#hint");
  await page.keyboard.press("c");
  await expect(hint).toContainText(/Circle \(|Polygon \(/);
  if ((await hint.textContent())?.includes("Polygon (")) await page.keyboard.press("c");
  await expect(hint).toContainText("Circle (");
}

test("lane 23b: the Eraser across a circle or a polygon, two circles moved, a pulled circle's front elevation", async ({ page }) => {
  test.setTimeout(120_000);
  const vp = await ready(page);
  const hint = page.locator("#hint");

  // 1. A circle of 300 mm, then an Eraser drag from outside it, across it, to outside it:
  // the circle and its face go, one undo step (it used to say "no curve that one there").
  await page.keyboard.press("c");
  await clickAt(page, [0, 0, 0], /grid|endpoint|axis/);
  await point(page, [20, 0, 0]);
  await type(page, "300");
  await expect(hint).toContainText("Circle r 300 mm, 24 segments");
  expect(await curves(page)).toHaveLength(1);
  expect(await regions(page)).toHaveLength(1);
  await page.keyboard.press("e");
  await sweep(page, inch([-0.5, 0.02, 0]), inch([0.5, 0.03, 0]));
  await expect(hint).toContainText("Erased 1 item.");
  await expect(hint).not.toContainText("Can't do that");
  await expect.poll(async () => (await curves(page)).length).toBe(0);
  expect(await regions(page)).toHaveLength(0);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await curves(page)).length).toBe(1);
  expect(await regions(page)).toHaveLength(1);
  await page.keyboard.press("Control+y");
  await expect.poll(async () => (await curves(page)).length).toBe(0);

  // 2. A polygon (6 sides, 200 mm), swept diagonally corner to corner: the same.
  await page.keyboard.press("c");
  await page.keyboard.press("c");
  await expect(hint).toContainText("Polygon (6 sides)");
  await clickAt(page, [0, 0, 0], /grid|endpoint|axis/);
  await point(page, [10, 0, 0]);
  await type(page, "200");
  await expect(hint).toContainText("Polygon r 200 mm, 6 sides");
  expect(byKind(await curves(page))).toBe("sketch:polygon");
  expect(await regions(page)).toHaveLength(1);
  await page.keyboard.press("e");
  await sweep(page, inch([-0.3, -0.27, 0]), inch([0.31, 0.28, 0]));
  await expect(hint).toContainText("Erased 1 item.");
  await expect.poll(async () => (await curves(page)).length).toBe(0);
  expect(await regions(page)).toHaveLength(0);
  await page.keyboard.press("Control+z");
  await expect.poll(async () => byKind(await curves(page))).toBe("sketch:polygon");
  expect(await regions(page)).toHaveLength(1);

  // 3. Two circles with their faces, window-selected and moved: "2 circles", not 3 items.
  await menu(page, "File", "New");
  await expect.poll(async () => (await curves(page)).length).toBe(0);
  await millimetres(page);
  // At the origin and 1 m along red (the metric grid's points).
  for (const cx of [0, 1 / IN]) {
    await circleTool(page);
    await clickAt(page, [cx, 0, 0], /grid|endpoint|axis|from-point/);
    await point(page, [cx + 10, 0, 0]);
    await type(page, "200");
    await expect(hint).toContainText("Circle r 200 mm, 24 segments");
    await page.keyboard.press("Escape");
  }
  expect(await regions(page)).toHaveLength(2);
  const xs = (cs: Curve[]) => cs.map((c) => c.centre![0]).sort((a, b) => a - b);
  const was = xs(await curves(page));
  // A crossing box (right to left) over both: each circle and its face.
  await page.keyboard.press("Space");
  await sweep(page, [1 / IN + 12, -12, 0], [-12, 12, 0]);
  await expect(vp).toHaveAttribute("data-selection", "4");
  await page.keyboard.press("m");
  await clickAt(page, onCircle([0, 0], 0.2, 0), "endpoint");
  await point(page, onCircle([0, 0], 0.2, 0).map((x, k) => (k === 0 ? x + 8 : x)) as V3);
  await expect(vp).toHaveAttribute("data-inference", "axis-x");
  await type(page, "100");
  await expect(hint).toContainText("Moved 2 circles 100 mm along +red");
  const moved = await curves(page);
  expect(xs(moved)).toEqual(was.map((x) => expect.closeTo(x + 0.1, 9)));
  expect(byKind(moved)).toBe("sketch:circle sketch:circle");

  // 4. A circle pulled 400 mm up, drawn on a sheet from the front: its outline (two
  // upright lines at its sides), not its 24 soft seams (13 upright lines in front).
  await menu(page, "File", "New");
  await expect.poll(async () => (await curves(page)).length).toBe(0);
  await millimetres(page);
  await circleTool(page);
  await clickAt(page, [0, 0, 0], /grid|endpoint|axis|from-point/);
  await point(page, [20, 0, 0]);
  await type(page, "300");
  await expect(hint).toContainText("Circle r 300 mm, 24 segments");
  await page.keyboard.press("p");
  await type(page, "400");
  await expect(hint).toContainText("Extruded");
  await page.locator('.scenes:not(.sheet-tabs) .mode-switch [data-mode="sheets"]').click();
  await expect(page.locator(".sheet-area")).toBeVisible();
  await page.getByRole("button", { name: "Add a sheet", exact: true }).first().click();
  const render = () => page.evaluate(() => (window as unknown as { __sawhorse: { sheet: { render(): SheetRender | null } } }).__sawhorse.sheet.render());
  await expect.poll(() => page.evaluate(() => (window as unknown as { __sawhorse: { sheet: { tool(): string } } }).__sawhorse.sheet.tool())).toBe("viewport");
  await page.getByRole("combobox", { name: "Shows" }).selectOption("std:front");
  const client = (p: P2) => page.evaluate((q) => (window as unknown as { __sawhorse: { sheet: { toClient(p: P2): P2 | null } } }).__sawhorse.sheet.toClient(q), p);
  const [p0, p1] = [await client([0.75 * IN, 0.625 * IN]), await client([4.75 * IN, 7.625 * IN])];
  await page.mouse.move(p0![0], p0![1]);
  await page.mouse.down();
  await page.mouse.move(p1![0], p1![1], { steps: 8 });
  await page.mouse.up();
  await expect(hint).toContainText("Viewport: Front");
  await expect.poll(async () => (await render())?.viewports.length).toBe(1);
  const r = (await render())!;
  const item = r.items.find((i) => i.kind === "viewport")!;
  const f = r.viewports[0].factor;
  const upright = r.prims.slice(item.prims[0], item.prims[1]).filter((q) => q.p === "line" && !q.dash && Math.abs(q.a![0] - q.b![0]) < 1e-9 && Math.abs(Math.abs(q.a![1] - q.b![1]) - 0.4 * f) < 1e-6);
  expect(upright, JSON.stringify(upright)).toHaveLength(2);
  expect(Math.abs(upright[0].a![0] - upright[1].a![0])).toBeCloseTo(0.6 * f, 6);
});
